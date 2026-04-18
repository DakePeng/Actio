use std::sync::Mutex;
use tracing::info;

/// State of the dictation session.
#[derive(Debug, Default)]
enum DictationState {
    #[default]
    Idle,
    Listening,
}

/// Manages an on-demand dictation session (separate from the always-on ASR pipeline).
pub struct DictationService {
    state: Mutex<DictationState>,
}

impl DictationService {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(DictationState::Idle),
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(*self.state.lock().unwrap(), DictationState::Listening)
    }

    /// Start a dictation session. Returns Ok if session started, Err if already active.
    pub fn start(&self) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap();
        if matches!(*state, DictationState::Listening) {
            return Err(anyhow::anyhow!("dictation already active"));
        }
        *state = DictationState::Listening;
        info!("Dictation started");
        Ok(())
    }

    /// Stop the dictation session. Returns a placeholder transcript.
    pub fn stop(&self) -> Option<String> {
        let mut state = self.state.lock().unwrap();
        if matches!(*state, DictationState::Listening) {
            *state = DictationState::Idle;
            info!("Dictation stopped");
            Some(String::new())
        } else {
            None
        }
    }
}

impl Default for DictationService {
    fn default() -> Self {
        Self::new()
    }
}
