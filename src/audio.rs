use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::HostId;
use magnum::container::ogg::OpusSourceOgg;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::time::Duration;

struct FadingSink {
    id: String,
    sink: Sink,
    start_volume: f32,
    elapsed: Duration,
    total_duration: Duration,
}

struct MagnumOggWrapper<R: std::io::Read + std::io::Seek>(OpusSourceOgg<R>);

impl<R: std::io::Read + std::io::Seek> Iterator for MagnumOggWrapper<R> {
    type Item = f32;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

impl<R: std::io::Read + std::io::Seek> Source for MagnumOggWrapper<R> {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        2
    }

    fn sample_rate(&self) -> u32 {
        48000
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

// Helper to spin up the correct decoder based on extension.
// Extracted from play() to decouple formatting and flatten nesting.
fn create_decoder_from_path(file_path: &str) -> anyhow::Result<Box<dyn Source<Item = f32> + Send>> {
    log::debug!("Creating decoder for: {}", file_path);

    let file = File::open(file_path)
        .context(format!("Failed to open sound file: {}", file_path))?;

    let is_opus = file_path.to_lowercase().ends_with(".opus")
        || file_path.to_lowercase().ends_with(".webm");

    if is_opus {
        log::info!("Attempting to use Magnum (Opus) decoder for: {}", file_path);
        let file_for_opus = file.try_clone().context("Failed to clone file handle for Opus")?;

        if let Ok(decoder) = OpusSourceOgg::new(BufReader::new(file_for_opus)) {
            log::info!("Magnum decoder created successfully.");
            return Ok(Box::new(MagnumOggWrapper(decoder)));
        }
        log::warn!("Magnum decoder failed, falling back to Rodio.");
    }

    // Catch potential rodio panics to isolate crashes from the main TUI thread
    let decoder_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        Decoder::new(BufReader::new(file))
    }));

    match decoder_result {
        Ok(Ok(d)) => Ok(Box::new(d.convert_samples())),
        Ok(Err(e)) => Err(anyhow::anyhow!("Rodio decoder error: {}", e)),
        Err(_) => Err(anyhow::anyhow!("Rodio decoder panicked.")),
    }
}

// Stream chunks from disk and preload the next decoder cycle ahead of time
// to avoid boundary glitches without caching the whole PCM into RAM.
struct GaplessLoopingSource {
    file_path: String,
    current_decoder: Box<dyn Source<Item = f32> + Send>,
    next_decoder: Option<Box<dyn Source<Item = f32> + Send>>, // hot standby
}

impl Iterator for GaplessLoopingSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(sample) = self.current_decoder.next() {
            // Trigger preloading lazily when vacant
            if self.next_decoder.is_none() {
                if let Ok(pre_loaded) = create_decoder_from_path(&self.file_path) {
                    self.next_decoder = Some(pre_loaded);
                }
            }
            Some(sample)
        } else {
            // Un seamless switch to the preloaded standby decoder
            if let Some(mut ready_decoder) = self.next_decoder.take() {
                let first_sample = ready_decoder.next();
                self.current_decoder = ready_decoder;
                first_sample
            } else {
                // Blocking fallback if preloading haven't finished yet
                if let Ok(fallback) = create_decoder_from_path(&self.file_path) {
                    self.current_decoder = fallback;
                    self.current_decoder.next()
                } else {
                    None
                }
            }
        }
    }
}

impl Source for GaplessLoopingSource {
    fn current_frame_len(&self) -> Option<usize> { self.current_decoder.current_frame_len() }
    fn channels(&self) -> u16 { self.current_decoder.channels() }
    fn sample_rate(&self) -> u32 { self.current_decoder.sample_rate() }
    fn total_duration(&self) -> Option<Duration> { None } // Infinite loop
}

pub struct AudioEngine {
    _stream: OutputStream,
    stream_handle: OutputStreamHandle,
    sinks: HashMap<String, Sink>,
    fading_sinks: Vec<FadingSink>,
    master_volume: f32,
    sound_volumes: HashMap<String, f32>,
    fade_duration: Duration,
}

impl AudioEngine {
    pub fn new() -> Result<Self> {
        let available_hosts = cpal::available_hosts();
        log::info!("Available audio hosts: {:?}", available_hosts);

        let mut device = None;
        let mut host_name = "Default";

        let mut priority_hosts = Vec::new();

        #[cfg(all(
            any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd"),
            feature = "jack"
        ))]
        priority_hosts.push(HostId::Jack);

        #[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd"))]
        priority_hosts.push(HostId::Alsa);

        for &host_id in &priority_hosts {
            if available_hosts.contains(&host_id) {
                log::debug!("Attempting to use audio host: {:?}", host_id);
                if let Ok(host) = cpal::host_from_id(host_id) {
                    if let Some(d) = host.default_output_device() {
                        log::info!(
                            "Selected audio device from host {:?}: {}",
                            host_id,
                            d.name().unwrap_or_else(|_| "Unknown".to_string())
                        );
                        device = Some(d);
                        host_name = match host_id {
                            #[cfg(all(
                                any(
                                    target_os = "linux",
                                    target_os = "dragonfly",
                                    target_os = "freebsd"
                                ),
                                feature = "jack"
                            ))]
                            HostId::Jack => "JACK",
                            #[cfg(any(
                                target_os = "linux",
                                target_os = "dragonfly",
                                target_os = "freebsd"
                            ))]
                            HostId::Alsa => "ALSA",
                            #[allow(unreachable_patterns)]
                            _ => "Unknown",
                        };

                        break;
                    }
                }
            }
        }

        let (_stream, stream_handle) = if let Some(d) = device {
            OutputStream::try_from_device(&d)
                .map_err(|e| anyhow::anyhow!("Failed to create output stream from device: {}", e))?
        } else {
            log::warn!("No preferred audio host found. Falling back to default.");
            OutputStream::try_default().context("No audio output device available")?
        };

        log::info!("Audio engine initialized successfully using {}", host_name);

        Ok(Self {
            _stream,
            stream_handle,
            sinks: HashMap::new(),
            fading_sinks: Vec::new(),
            master_volume: 1.0,
            sound_volumes: HashMap::new(),
            fade_duration: Duration::from_secs(2),
        })
    }

    pub fn update(&mut self, dt: Duration) {
        let mut finished_indices = Vec::new();

        for (i, fading) in self.fading_sinks.iter_mut().enumerate() {
            fading.elapsed += dt;
            if fading.elapsed >= fading.total_duration {
                fading.sink.set_volume(0.0);
                fading.sink.stop();
                finished_indices.push(i);
            } else {
                let progress = fading.elapsed.as_secs_f32() / fading.total_duration.as_secs_f32();
                let remaining_vol = fading.start_volume * (1.0 - progress);
                fading.sink.set_volume(remaining_vol);
            }
        }

        for i in finished_indices.into_iter().rev() {
            self.fading_sinks.swap_remove(i);
        }
    }

    pub fn play(&mut self, id: &str, file_path: &str, volume: f32) -> Result<()> {
        log::info!("Attempting to play sound '{}' from '{}'", id, file_path);
        if self.sinks.contains_key(id) {
            log::debug!("Sound '{}' is already playing", id);
            return Ok(());
        }

        // Check if there's a fading out version of this sound and remove it to prevent overlap
        if let Some(pos) = self.fading_sinks.iter().position(|f| f.id == id) {
            log::debug!("Stopping fading sink for '{}' to restart", id);
            let fading = self.fading_sinks.remove(pos);
            fading.sink.stop();
        }

        let initial_decoder = create_decoder_from_path(file_path)?;
        let looping_source = GaplessLoopingSource {
            file_path: file_path.to_string(),
            current_decoder: initial_decoder,
            next_decoder: None,
        };
        let final_source = looping_source.fade_in(self.fade_duration);

        log::debug!("Creating sink for: {}", id);

        let sink = Sink::try_new(&self.stream_handle)?;
        sink.append(final_source);

        self.sound_volumes.insert(id.to_string(), volume);
        let effective_vol = volume * self.master_volume;
        sink.set_volume(effective_vol);

        self.sinks.insert(id.to_string(), sink);
        log::info!("Started playing '{}'", id);
        Ok(())
    }

    pub fn stop(&mut self, id: &str) {
        if let Some(sink) = self.sinks.remove(id) {
            let start_vol = sink.volume();

            self.fading_sinks.push(FadingSink {
                id: id.to_string(),
                sink,
                start_volume: start_vol,
                elapsed: Duration::ZERO,
                total_duration: self.fade_duration,
            });
        }
    }

    pub fn set_volume(&mut self, id: &str, volume: f32) {
        self.sound_volumes.insert(id.to_string(), volume);
        if let Some(sink) = self.sinks.get(id) {
            let effective_vol = volume * self.master_volume;
            sink.set_volume(effective_vol);
        }
    }

    pub fn set_master_volume(&mut self, volume: f32) {
        self.master_volume = volume;
        for (id, sink) in &self.sinks {
            if let Some(&vol) = self.sound_volumes.get(id) {
                sink.set_volume(vol * self.master_volume);
            }
        }
    }

    pub fn is_playing(&self, id: &str) -> bool {
        self.sinks.contains_key(id)
    }

    pub fn stop_all(&mut self) {
        self.sinks.clear();
        self.fading_sinks.clear();
    }
}
