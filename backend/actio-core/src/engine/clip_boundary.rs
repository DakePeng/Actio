//! Pure state machine that decides when to close an audio clip.
//!
//! Reads VAD events as they arrive (speech segments + silence ticks) and
//! emits a single `Decision::CloseClip` whenever any of:
//!   * the active clip has been open ≥ `target_secs` AND we observed
//!     ≥ `silence_close_ms` of contiguous silence,
//!   * the active clip has been open ≥ `max_secs` (hard cap, mid-utterance),
//!   * the user toggled mute (capture is stopping).
//!
//! Owns no I/O; the caller plumbs cpal/VAD events in and turns decisions
//! into manifest writes. Reset between clips with `reset_after_close`.

#[derive(Debug, Clone, Copy)]
pub struct BoundaryConfig {
    pub target_secs: u32,
    pub max_secs: u32,
    pub silence_close_ms: u32,
}

#[derive(Debug, Clone)]
pub enum BoundaryEvent {
    /// A finalized VAD speech segment.
    Speech { start_ms: i64, end_ms: i64 },
    /// "We are still in silence at this monotonic timestamp." The watcher
    /// uses this to advance the silence-duration counter when no speech
    /// events would otherwise arrive (idle mic).
    SilenceTick { now_ms: i64 },
    /// User muted — close immediately, even if shorter than `target_secs`.
    Mute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Continue,
    CloseClip,
}

pub struct BoundaryWatcher {
    cfg: BoundaryConfig,
    clip_started_ms: Option<i64>,
    last_speech_end_ms: Option<i64>,
}

impl BoundaryWatcher {
    pub fn new(cfg: BoundaryConfig) -> Self {
        Self {
            cfg,
            clip_started_ms: None,
            last_speech_end_ms: None,
        }
    }

    /// Process one event. Caller must follow up `CloseClip` with a fresh
    /// watcher (or call `reset_after_close`) so the next clip starts fresh.
    pub fn observe(&mut self, ev: BoundaryEvent) -> Decision {
        match ev {
            BoundaryEvent::Speech { start_ms, end_ms } => {
                if self.clip_started_ms.is_none() {
                    self.clip_started_ms = Some(start_ms);
                }
                self.last_speech_end_ms = Some(end_ms);
                self.check_close(end_ms)
            }
            BoundaryEvent::SilenceTick { now_ms } => self.check_close(now_ms),
            BoundaryEvent::Mute => {
                if self.clip_started_ms.is_some() {
                    Decision::CloseClip
                } else {
                    Decision::Continue
                }
            }
        }
    }

    pub fn reset_after_close(&mut self) {
        self.clip_started_ms = None;
        self.last_speech_end_ms = None;
    }

    fn check_close(&self, now_ms: i64) -> Decision {
        let started = match self.clip_started_ms {
            Some(v) => v,
            None => return Decision::Continue,
        };
        let elapsed_ms = now_ms - started;
        if elapsed_ms >= self.cfg.max_secs as i64 * 1_000 {
            return Decision::CloseClip;
        }
        if elapsed_ms >= self.cfg.target_secs as i64 * 1_000 {
            let silence = match self.last_speech_end_ms {
                Some(end) => now_ms - end,
                None => elapsed_ms,
            };
            if silence >= self.cfg.silence_close_ms as i64 {
                return Decision::CloseClip;
            }
        }
        Decision::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> BoundaryConfig {
        BoundaryConfig {
            target_secs: 300,
            max_secs: 360,
            silence_close_ms: 1500,
        }
    }

    #[test]
    fn no_speech_yet_no_close() {
        let mut w = BoundaryWatcher::new(cfg());
        assert_eq!(
            w.observe(BoundaryEvent::SilenceTick { now_ms: 60_000 }),
            Decision::Continue
        );
    }

    #[test]
    fn closes_after_target_plus_long_silence() {
        let mut w = BoundaryWatcher::new(cfg());
        w.observe(BoundaryEvent::Speech {
            start_ms: 0,
            end_ms: 2_000,
        });
        // Speech ending at 4:50 (290_000), silence tick at 5:01.5 (301_500)
        // → elapsed 301.5s ≥ target 300s, silence 11.5s ≥ 1.5s.
        w.observe(BoundaryEvent::Speech {
            start_ms: 280_000,
            end_ms: 290_000,
        });
        assert_eq!(
            w.observe(BoundaryEvent::SilenceTick { now_ms: 301_500 }),
            Decision::CloseClip
        );
    }

    #[test]
    fn does_not_close_at_target_when_speech_is_continuing() {
        let mut w = BoundaryWatcher::new(cfg());
        w.observe(BoundaryEvent::Speech {
            start_ms: 0,
            end_ms: 2_000,
        });
        w.observe(BoundaryEvent::Speech {
            start_ms: 295_000,
            end_ms: 300_500,
        });
        assert_eq!(
            w.observe(BoundaryEvent::SilenceTick { now_ms: 300_700 }),
            Decision::Continue
        );
    }

    #[test]
    fn force_closes_at_max_when_speech_runs_through() {
        let mut w = BoundaryWatcher::new(cfg());
        w.observe(BoundaryEvent::Speech {
            start_ms: 0,
            end_ms: 2_000,
        });
        let d = w.observe(BoundaryEvent::Speech {
            start_ms: 300_000,
            end_ms: 360_500,
        });
        assert_eq!(d, Decision::CloseClip);
    }

    #[test]
    fn mute_closes_immediately_if_clip_open() {
        let mut w = BoundaryWatcher::new(cfg());
        w.observe(BoundaryEvent::Speech {
            start_ms: 0,
            end_ms: 1_000,
        });
        assert_eq!(w.observe(BoundaryEvent::Mute), Decision::CloseClip);
    }

    #[test]
    fn mute_no_op_if_no_clip_open() {
        let mut w = BoundaryWatcher::new(cfg());
        assert_eq!(w.observe(BoundaryEvent::Mute), Decision::Continue);
    }

    #[test]
    fn reset_after_close_starts_fresh_clip_on_next_speech() {
        let mut w = BoundaryWatcher::new(cfg());
        w.observe(BoundaryEvent::Speech {
            start_ms: 0,
            end_ms: 2_000,
        });
        w.observe(BoundaryEvent::Mute);
        w.reset_after_close();

        w.observe(BoundaryEvent::Speech {
            start_ms: 1_000_000,
            end_ms: 1_002_000,
        });
        let d = w.observe(BoundaryEvent::SilenceTick { now_ms: 1_303_000 });
        assert_eq!(d, Decision::CloseClip);
    }

    #[test]
    fn silence_window_resets_when_new_speech_arrives() {
        let mut w = BoundaryWatcher::new(cfg());
        w.observe(BoundaryEvent::Speech {
            start_ms: 0,
            end_ms: 2_000,
        });
        // At 5:00, would-have-closed silence window starts.
        w.observe(BoundaryEvent::Speech {
            start_ms: 200_000,
            end_ms: 299_000,
        });
        // But more speech arrives 500 ms later, before the gap reaches 1.5s.
        let d = w.observe(BoundaryEvent::Speech {
            start_ms: 299_500,
            end_ms: 305_000,
        });
        assert_eq!(d, Decision::Continue);
        // Now genuine silence past 1.5s → close.
        let d = w.observe(BoundaryEvent::SilenceTick { now_ms: 307_000 });
        assert_eq!(d, Decision::CloseClip);
    }
}
