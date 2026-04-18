use serde::Serialize;
use std::sync::atomic::{AtomicU32, AtomicU64};
use std::time::Instant;

pub struct Metrics {
    pub active_sessions: AtomicU32,
    pub total_chunks_received: AtomicU64,
    pub unknown_speaker_count: AtomicU64,
    pub local_route_count: AtomicU64,
    pub worker_error_count: AtomicU64,
    pub transcript_push_count: AtomicU64,
    pub start_time: Instant,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            active_sessions: AtomicU32::new(0),
            total_chunks_received: AtomicU64::new(0),
            unknown_speaker_count: AtomicU64::new(0),
            local_route_count: AtomicU64::new(0),
            worker_error_count: AtomicU64::new(0),
            transcript_push_count: AtomicU64::new(0),
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
    pub worker_state: String,
    pub local_route_count: u64,
    pub worker_error_count: u64,
    pub unknown_speaker_count: u64,
}
