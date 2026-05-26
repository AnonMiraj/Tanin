use anyhow::Result;
use rodio::Source;
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::buffered::source::BufferedSource;

const WORKER_COUNT: usize = 2;
const PREFETCH_COUNT: usize = 3;

pub fn init_worker_pool() -> Sender<DecodeTask> {
    let (task_tx, task_rx) = channel::<DecodeTask>();
    let shared_rx = Arc::new(Mutex::new(task_rx));

    for i in 0..WORKER_COUNT {
        let worker_rx = Arc::clone(&shared_rx);
        let worker_tx = task_tx.clone();
        
        thread::Builder::new()
            .name(format!("AudioWorker-{}", i))
            .spawn(move || {
                loop {
                    let task_result = {
                        let lock = worker_rx.lock().unwrap();
                        lock.recv()
                    };
                    match task_result {
                        Ok(task) => task.process_chunk(&worker_tx),
                        Err(_) => break, 
                    }
                }
            }).expect("Failed to spawn audio worker thread");
    }

    task_tx
}

pub fn spawn_stream<F>(
    dispatcher: &Sender<DecodeTask>,
    decoder_factory: F,
) -> Result<BufferedSource>
where
    F: Fn() -> Result<Box<dyn Source<Item = f32> + Send>> + Send + 'static,
{
    let mut initial_decoder = decoder_factory()?;
    
    // Extract metadata from the opened file.
    let channels = initial_decoder.channels();
    let sample_rate = initial_decoder.sample_rate();

    // Calculate chunk sizes based on actual file properties.
    // chunk_size = 1 second of audio.
    let chunk_size = (sample_rate as usize) * (channels as usize);
    // prime_chunk_size = 100 milliseconds of audio for instant startup.
    let prime_chunk_size = chunk_size / 10;

    let (reply_tx, reply_rx) = sync_channel::<Vec<f32>>(PREFETCH_COUNT);
    let (recycle_tx, recycle_rx) = channel::<Vec<f32>>();

    // Pre-allocate enough chunks to completely saturate the pipeline:
    // PREFETCH_COUNT (in queue) + 1 (playing in frontend) + 1 (being decoded in worker)
    for _ in 0..(PREFETCH_COUNT + 2) {
        recycle_tx.send(Vec::with_capacity(chunk_size)).unwrap();
    }

    let mut prime_chunk = recycle_rx.recv().unwrap();
    prime_chunk.extend(initial_decoder.by_ref().take(prime_chunk_size));
    let _ = reply_tx.send(prime_chunk);

    let task = DecodeTask {
        decoder: initial_decoder,
        factory: Box::new(decoder_factory),
        reply_tx,
        recycle_rx,
        chunk_size, // Store the calculated size for background execution
    };

    let _ = dispatcher.send(task);

    Ok(BufferedSource::new(reply_rx, recycle_tx, channels, sample_rate))
}

pub struct DecodeTask {
    pub decoder: Box<dyn Source<Item = f32> + Send>,
    pub factory: Box<dyn Fn() -> Result<Box<dyn Source<Item = f32> + Send>> + Send>,
    pub reply_tx: SyncSender<Vec<f32>>,
    pub recycle_rx: Receiver<Vec<f32>>,
    pub chunk_size: usize,
}

impl DecodeTask {
    pub fn process_chunk(mut self, task_tx: &Sender<DecodeTask>) {
        let mut chunk = self.recycle_rx.try_recv().unwrap_or_else(|_| Vec::with_capacity(self.chunk_size));
        
        chunk.clear(); 
        chunk.extend(self.decoder.by_ref().take(self.chunk_size));

        if chunk.len() < self.chunk_size {
            if let Ok(new_decoder) = (self.factory)() {
                self.decoder = new_decoder;
                let remaining = self.chunk_size - chunk.len();
                chunk.extend(self.decoder.by_ref().take(remaining));
            }
        }

        if self.reply_tx.send(chunk).is_ok() {
            let _ = task_tx.send(self);
        }
    }
}

