use std::time::{Duration, Instant};
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

pub struct CircuitBreaker {
    state: CircuitState,
    failure_count: u32,
    opened_at: Option<Instant>,
    open_duration: Duration,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            opened_at: None,
            open_duration: Duration::from_secs(30),
        }
    }

    pub fn allow_local(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let Some(opened) = self.opened_at else {
                    // Safety: should never happen, treat as closed
                    return true;
                };
                if opened.elapsed() >= self.open_duration {
                    info!("Circuit breaker: Open -> HalfOpen");
                    self.state = CircuitState::HalfOpen;
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    pub fn record_success(&mut self) {
        self.failure_count = 0;
        if self.state == CircuitState::HalfOpen {
            info!("Circuit breaker: HalfOpen -> Closed");
            self.state = CircuitState::Closed;
            self.opened_at = None;
        }
    }

    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        if self.state == CircuitState::HalfOpen {
            info!("Circuit breaker: HalfOpen -> Open (failure)");
            self.state = CircuitState::Open;
            self.opened_at = Some(Instant::now());
        } else if self.failure_count >= 3 {
            info!("Circuit breaker: Closed -> Open ({} failures)", self.failure_count);
            self.state = CircuitState::Open;
            self.opened_at = Some(Instant::now());
        }
    }

    pub fn state(&self) -> CircuitState {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_allows() {
        let mut cb = CircuitBreaker::new();
        assert!(cb.allow_local());
    }

    #[test]
    fn opens_after_3_failures() {
        let mut cb = CircuitBreaker::new();
        cb.record_failure();
        cb.record_failure();
        assert!(cb.allow_local());
        cb.record_failure();
        assert!(!cb.allow_local());
    }

    #[test]
    fn resets_on_success() {
        let mut cb = CircuitBreaker::new();
        cb.record_failure();
        cb.record_failure();
        cb.record_success();
        cb.record_failure();
        cb.record_failure();
        assert!(cb.allow_local());
    }
}
