use std::time::Instant;
use std::sync::atomic::{AtomicU32, AtomicU64};
use serde::Serialize;

pub struct Metrics {
    pub active_sessions: AtomicU32,
    pub total_chunks_received: AtomicU64,
    pub unknown_speaker_count: AtomicU64,
    pub start_time: Instant,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            active_sessions: AtomicU32::new(0),
            total_chunks_received: AtomicU64::new(0),
            unknown_speaker_count: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}

#[derive(Serialize)]
pub struct HealthSummary {
    pub active_sessions: u32,
    pub uptime_secs: u64,
}
