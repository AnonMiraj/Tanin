use anyhow::Result;
use rodio::Source;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::thread;

use crate::buffered::source::BufferedSource;

const WORKER_COUNT: usize = 2;
const PREFETCH_COUNT: usize = 3;

pub fn init_worker_pool() -> Sender<DecodeTask> {
    let (task_tx, task_rx) = channel::<DecodeTask>();
    let shared_rx = Arc::new(Mutex::new(task_rx));

    for i in 0..WORKER_COUNT {
        let worker_rx = Arc::clone(&shared_rx);
        thread::Builder::new()
            .name(format!("AudioWorker-{}", i))
            .spawn(move || {
                loop {
                    let task_result = {
                        let lock = worker_rx.lock().unwrap();
                        lock.recv()
                    };
                    match task_result {
                        Ok(task) => task.process_chunk(),
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
    let chunk_size = std::cmp::max((sample_rate as usize) * (channels as usize), 1024);

    // prime_chunk_size = 100 milliseconds of audio for instant startup.
    let prime_chunk_size = chunk_size / 10;

    let (reply_tx, reply_rx) = channel::<Vec<f32>>();
    let (recycle_tx, recycle_rx) = channel::<Vec<f32>>();
    let suspended_task = Arc::new(Mutex::new(None));

    for _ in 0..PREFETCH_COUNT {
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
        suspended_task: Arc::downgrade(&suspended_task),
        global_task_tx: dispatcher.clone(),
        next_buffer: None,
    };

    dispatcher.send(task).unwrap();

    Ok(BufferedSource::new(
        reply_rx,
        recycle_tx,
        channels,
        sample_rate,
        suspended_task,
        dispatcher.clone(),
    ))
}

pub struct DecodeTask {
    pub decoder: Box<dyn Source<Item = f32> + Send>,
    pub factory: Box<dyn Fn() -> Result<Box<dyn Source<Item = f32> + Send>> + Send>,
    pub reply_tx: Sender<Vec<f32>>,
    pub recycle_rx: Receiver<Vec<f32>>,
    pub chunk_size: usize,
    pub suspended_task: Weak<Mutex<Option<DecodeTask>>>,
    pub global_task_tx: Sender<DecodeTask>,
    pub next_buffer: Option<Vec<f32>>,
}

impl DecodeTask {
    pub fn process_chunk(mut self) {
        let mut chunk = if let Some(buf) = self.next_buffer.take() {
            buf
        } else {
            self.recycle_rx
                .try_recv()
                .expect("State machine violation: Task woke up but no buffer is available!")
        };
        
        chunk.clear(); 
        
        let decode_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            chunk.extend(self.decoder.by_ref().take(self.chunk_size));
        }));

        if decode_result.is_err() {
            log::error!("Decoder panicked mid-stream. Terminating.");
            return;
        }

        if chunk.len() < self.chunk_size {
            match (self.factory)() {
                Ok(new_decoder) => {
                    self.decoder = new_decoder;
                    let remaining = self.chunk_size - chunk.len();
                    let loop_decode = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        chunk.extend(self.decoder.by_ref().take(remaining));
                    }));
                    if loop_decode.is_err() {
                        log::error!("Decoder panicked during gapless loop. Terminating.");
                        return;
                    }
                }
                Err(e) => {
                    log::error!("Gapless loop factory failed: {}. Terminating stream.", e);
                    if !chunk.is_empty() {
                        let _ = self.reply_tx.send(chunk);
                    }
                    return;
                }
            }
        }

        if chunk.is_empty() { return; }
        if self.reply_tx.send(chunk).is_err() {
            return;
        }

        if let Some(suspended_arc) = self.suspended_task.upgrade() {
            let mut suspended = suspended_arc.lock().unwrap();
            match self.recycle_rx.try_recv() {
                Ok(next_buf) => {
                    self.next_buffer = Some(next_buf);
                    drop(suspended);
                    let _ = self.global_task_tx.clone().send(self);
                }
                Err(_) => {
                    log::trace!("Task {:p} buffer full, suspending.", self.suspended_task.as_ptr());
                    *suspended = Some(self);
                }
            }
        }
    }
}

