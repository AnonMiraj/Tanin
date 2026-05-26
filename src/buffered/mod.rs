//! Streaming audio architecture with gapless loop prefetching.

pub mod source;
pub mod worker;

pub use worker::{init_worker_pool, spawn_stream, DecodeTask};

