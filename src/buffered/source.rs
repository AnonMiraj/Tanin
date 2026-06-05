use rodio::Source;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::buffered::worker::DecodeTask;

///A frontend audio source that consumes pre-decoded chunks.
/// Employs a dual-channel memory pooling architecture to recycle buffers,
/// guaranteeing strictly zero heap allocations during steady-state playback.
pub struct BufferedSource {
    /// The channel used to receive fully decoded audio chunks (filled buckets)
    /// from the background workers.
    receiver: Receiver<Vec<f32>>,

    /// The channel used to send consumed, empty buffers (empty buckets) back
    /// to the background workers for zero-allocation recycling.
    recycle_tx: Sender<Vec<f32>>,

    /// The audio buffer currently being played/consumed by the frontend audio sink.
    current_chunk: Option<Vec<f32>>,

    /// The current read position (index) within the `current_chunk`.
    cursor: usize,

    /// The number of audio channels (e.g., 1 for mono, 2 for stereo) extracted
    /// from the original source to ensure correct playback.
    channels: u16,

    /// The sample rate of the audio (e.g., 44100, 48000) extracted from the original source.
    sample_rate: u32,

    /// The "waiting room" for the background task.
    /// If the prefetch buffer reaches its capacity, the worker will park the `DecodeTask`
    /// here to prevent thread blocking and free up the worker for other streams.
    suspended_task: Arc<Mutex<Option<DecodeTask>>>,

    /// The global dispatcher channel of the thread pool.
    /// Used by this frontend consumer to wake up the suspended task and push it back
    /// into the worker pool's queue whenever an empty buffer is returned.
    global_task_tx: Sender<DecodeTask>,
}

impl BufferedSource {
    pub fn new(
        receiver: Receiver<Vec<f32>>, 
        recycle_tx: Sender<Vec<f32>>,
        channels: u16,
        sample_rate: u32,
        suspended_task: Arc<Mutex<Option<DecodeTask>>>,
        global_task_tx: Sender<DecodeTask>,
    ) -> Self {
        Self {
            receiver, recycle_tx, current_chunk: None,
            cursor: 0, channels, sample_rate,
            suspended_task, global_task_tx,
        }
    }
}

impl Iterator for BufferedSource {
    type Item = f32;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(chunk) = &self.current_chunk {
            if self.cursor < chunk.len() {
                let sample = chunk[self.cursor];
                self.cursor += 1;
                return Some(sample);
            } else {
                if let Some(old_chunk) = self.current_chunk.take() {
                    let _ = self.recycle_tx.send(old_chunk);
                    let mut suspended = self.suspended_task.lock().unwrap();
                    if let Some(task) = suspended.take() {
                        log::trace!("Waking up suspended task!");
                        let _ = self.global_task_tx.send(task);
                    }
                }
            }
        }
        if let Ok(next_chunk) = self.receiver.recv() {
            self.current_chunk = Some(next_chunk);
            self.cursor = 0;
            self.next()
        } else {
            None
        }
    }
}

impl Source for BufferedSource {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { self.channels }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn total_duration(&self) -> Option<Duration> { None } 
}

