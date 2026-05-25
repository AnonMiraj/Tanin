use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::HostId;
use magnum::container::ogg::OpusSourceOgg;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::time::Duration;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

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

// Internal payload for the I/O worker thread
struct PreloadTask {
    file_path: String,
    reply_tx: Sender<Box<dyn Source<Item = f32> + Send>>,
}

// Helper to spin up the correct decoder based on extension.
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

    let decoder_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        Decoder::new(BufReader::new(file))
    }));

    match decoder_result {
        Ok(Ok(d)) => Ok(Box::new(d.convert_samples())),
        Ok(Err(e)) => Err(anyhow::anyhow!("Rodio decoder error: {}", e)),
        Err(_) => Err(anyhow::anyhow!("Rodio decoder panicked.")),
    }
}

// Stream chunks from disk and async preload the next cycle
// to achieve gapless looping without RAM bloating.
struct GaplessLoopingSource {
    file_path: String,
    current_decoder: Box<dyn Source<Item = f32> + Send>,
    next_decoder_rx: Option<Receiver<Box<dyn Source<Item = f32> + Send>>>,
    io_worker_tx: Sender<PreloadTask>,
}

impl GaplessLoopingSource {
    pub fn new(
        file_path: String,
        initial_decoder: Box<dyn Source<Item = f32> + Send>,
        io_worker_tx: Sender<PreloadTask>
    ) -> Self {
        let mut source = Self {
            file_path,
            current_decoder: initial_decoder,
            next_decoder_rx: None,
            io_worker_tx,
        };
        source.trigger_preload();
        source
    }

    fn trigger_preload(&mut self) {
        let (tx, rx) = channel();
        let task = PreloadTask {
            file_path: self.file_path.clone(),
            reply_tx: tx,
        };

        // Fire and forget.
        let _ = self.io_worker_tx.send(task);
        self.next_decoder_rx = Some(rx);
    }
}

impl Iterator for GaplessLoopingSource {
    type Item = f32;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(sample) = self.current_decoder.next() {
            Some(sample)
        } else {
            // Drain the channel for the hot standby.
            // Blocks here if I/O worker is lagging behind, acting as a graceful fallback.
            if let Some(rx) = self.next_decoder_rx.take() {
                match rx.recv() {
                    Ok(mut ready_decoder) => {
                        let first_sample = ready_decoder.next();
                        self.current_decoder = ready_decoder;
                        self.trigger_preload(); 
                        return first_sample;
                    }
                    Err(_) => {
                        log::error!("I/O worker disconnected while preloading '{}'. Stopping stream.", self.file_path);
                    }
                }
            }
            // Let the stream die naturally if I/O worker panicked or channel dropped
            None
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
    io_worker_tx: Sender<PreloadTask>,
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

        // Spawn a dedicated, long-lived worker for decoder initialization
        // to keep audio threads free from blocking I/O calls.
        let (task_tx, task_rx) = channel::<PreloadTask>();
        thread::spawn(move || {
            while let Ok(task) = task_rx.recv() {
                if let Ok(decoder) = create_decoder_from_path(&task.file_path) {
                    // Send back to the specific source instance.
                    // Ignore errors if the sound was stopped by user in the meantime.
                    let _ = task.reply_tx.send(decoder);
                }
            }
        });

        log::info!("Audio engine initialized successfully using {}", host_name);

        Ok(Self {
            _stream,
            stream_handle,
            sinks: HashMap::new(),
            fading_sinks: Vec::new(),
            master_volume: 1.0,
            sound_volumes: HashMap::new(),
            fade_duration: Duration::from_secs(2),
            io_worker_tx: task_tx, // Store the dispatcher
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
        let looping_source = GaplessLoopingSource::new(
            file_path.to_string(),
            initial_decoder,
            self.io_worker_tx.clone() // Hand over a copy of the dispatcher
        );
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
