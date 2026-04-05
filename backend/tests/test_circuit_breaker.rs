use actio_asr::engine::circuit_breaker::{CircuitBreaker, CircuitState};

#[test]
fn test_full_cycle() {
    let mut cb = CircuitBreaker::new();

    // Normal operation
    cb.record_success();
    assert_eq!(cb.state(), CircuitState::Closed);
    assert!(cb.allow_local());

    // Failures accumulate
    cb.record_failure();
    cb.record_failure();
    assert!(cb.allow_local()); // not yet threshold

    cb.record_failure(); // 3rd failure
    assert!(!cb.allow_local()); // open

    // Success resets nothing while open
    cb.record_success(); // only matters in half-open
    assert!(!cb.allow_local()); // still open
}

#[test]
fn test_half_open_transition() {
    let mut cb = CircuitBreaker::new();

    // Force to open
    for _ in 0..3 {
        cb.record_failure();
    }
    assert!(!cb.allow_local());

    // Record a success while open (no effect)
    cb.record_success();
    assert!(!cb.allow_local());
}
