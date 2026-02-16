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

        log::debug!("Opening file: {}", file_path);
        let file =
            File::open(file_path).context(format!("Failed to open sound file: {}", file_path))?;

        log::debug!("Creating decoder for: {}", file_path);

        let file_for_closure = file.try_clone().context("Failed to clone file handle")?;

        let is_opus = file_path.to_lowercase().ends_with(".opus")
            || file_path.to_lowercase().ends_with(".webm");

        let source: Box<dyn Source<Item = f32> + Send> = if is_opus {
            log::info!("Attempting to use Magnum (Opus) decoder for: {}", file_path);
            match OpusSourceOgg::new(BufReader::new(file_for_closure)) {
                Ok(decoder) => {
                    log::info!("Magnum decoder created successfully.");
                    Box::new(MagnumOggWrapper(decoder))
                }
                Err(e) => {
                    log::error!("Magnum decoder failed: {:?}. Falling back to Rodio.", e);
                    let file_fallback = file
                        .try_clone()
                        .context("Failed to clone file for fallback")?;
                    let decoder_result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                            Decoder::new(BufReader::new(file_fallback))
                        }));
                    match decoder_result {
                        Ok(Ok(d)) => Box::new(d.convert_samples()),
                        Ok(Err(e)) => return Err(anyhow::anyhow!("Rodio decoder error: {}", e)),
                        Err(_) => return Err(anyhow::anyhow!("Rodio decoder panicked.")),
                    }
                }
            }
        } else {
            let decoder_result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    Decoder::new(BufReader::new(file_for_closure))
                }));

            match decoder_result {
                Ok(result) => match result {
                    Ok(d) => Box::new(d.convert_samples()),
                    Err(e) => {
                        log::error!("Failed to create decoder for '{}': {}", file_path, e);
                        return Err(anyhow::anyhow!("Decoder error: {}", e));
                    }
                },
                Err(_) => {
                    log::error!("Decoder PANICKED for '{}'.", file_path);
                    return Err(anyhow::anyhow!("Decoder panicked."));
                }
            }
        };

        let source = source.repeat_infinite().fade_in(self.fade_duration);

        log::debug!("Creating sink for: {}", id);

        let sink = Sink::try_new(&self.stream_handle)?;
        sink.append(source);

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
