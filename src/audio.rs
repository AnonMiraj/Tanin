use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::HostId;
use magnum::container::ogg::OpusSourceOgg;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::time::Duration;
use std::sync::mpsc::Sender;
use crate::buffered::{self, DecodeTask};

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
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { 2 }
    fn sample_rate(&self) -> u32 { 48000 }
    fn total_duration(&self) -> Option<Duration> { None }
}

fn create_decoder_from_path(file_path: &str) -> Result<Box<dyn Source<Item = f32> + Send>> {
    let file = File::open(file_path)
        .context(format!("Failed to open sound file: {}", file_path))?;

    let is_opus = file_path.to_lowercase().ends_with(".opus")
        || file_path.to_lowercase().ends_with(".webm");

    if is_opus {
        let file_for_opus = file.try_clone().context("Failed to clone file handle for Opus")?;
        if let Ok(decoder) = OpusSourceOgg::new(BufReader::new(file_for_opus)) {
            return Ok(Box::new(MagnumOggWrapper(decoder)));
        }
    }

    let decoder = Decoder::new(BufReader::new(file))?;
    Ok(Box::new(decoder.convert_samples()))
}

pub struct AudioEngine {
    _stream: OutputStream,
    stream_handle: OutputStreamHandle,
    sinks: HashMap<String, Sink>,
    fading_sinks: Vec<FadingSink>,
    master_volume: f32,
    sound_volumes: HashMap<String, f32>,
    fade_duration: Duration,
    task_dispatcher: Sender<DecodeTask>,
}

impl AudioEngine {
    pub fn new() -> Result<Self> {
        let available_hosts = cpal::available_hosts();
        log::info!("Available audio hosts: {:?}", available_hosts);

        let mut device = None;
        let mut host_name = "Default";

        #[allow(unused_mut)]
        let mut priority_hosts: Vec<(HostId, &'static str)> = Vec::new();

        #[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd"))]
        {
            #[cfg(feature = "jack")]
            priority_hosts.push((HostId::Jack, "JACK"));
            priority_hosts.push((HostId::Alsa, "ALSA"));
        }

        for &(host_id, name_str) in &priority_hosts {
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
                        host_name = name_str;
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

        // init bufferd source worker pool
        let task_dispatcher = buffered::init_worker_pool();

        log::info!("Audio engine initialized successfully using {}", host_name);

        Ok(Self {
            _stream,
            stream_handle,
            sinks: HashMap::new(),
            fading_sinks: Vec::new(),
            master_volume: 1.0,
            sound_volumes: HashMap::new(),
            fade_duration: Duration::from_secs(2),
            task_dispatcher,
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

        // Clone the string to move it into the factory closure safely.
        let path_clone = file_path.to_string();
        let base_source = buffered::spawn_stream(&self.task_dispatcher, move || {
            create_decoder_from_path(&path_clone)
        })?;
        let final_source = base_source.fade_in(self.fade_duration);

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
