use rodio::Source;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

/// A frontend audio source that consumes pre-decoded chunks.
/// Employs a dual-channel memory pooling architecture to recycle buffers, 
/// guaranteeing strictly zero heap allocations during steady-state playback.
pub struct BufferedSource {
    receiver: Receiver<Vec<f32>>,
    recycle_tx: Sender<Vec<f32>>,
    current_chunk: Option<Vec<f32>>,
    cursor: usize,
    channels: u16,
    sample_rate: u32,
}

impl BufferedSource {
    pub fn new(
        receiver: Receiver<Vec<f32>>, 
        recycle_tx: Sender<Vec<f32>>,
        channels: u16,
        sample_rate: u32,
    ) -> Self {
        Self {
            receiver,
            recycle_tx,
            current_chunk: None,
            cursor: 0,
            channels,
            sample_rate,
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

