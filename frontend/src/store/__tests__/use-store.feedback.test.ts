import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../use-store';

/** Pins the lifetime branch in `pushFeedback`:
 *
 *   - plain feedback (no `action`) auto-dismisses at 2 200 ms
 *   - actionable feedback (with `action`) gets a 5 000 ms grace window
 *     so the user has time to hit Undo
 *
 *  See ISSUES.md #54 (introduces the branch) and #64 (this test). */
describe('useStore — pushFeedback lifetime (ISSUES.md #64)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useStore.getState().clearFeedback();
  });
  afterEach(() => {
    useStore.getState().clearFeedback();
    vi.useRealTimers();
  });

  it('plain toast auto-dismisses at 2200 ms', () => {
    useStore.getState().setFeedback('feedback.reminderConfirmed', 'success');
    expect(useStore.getState().ui.feedback?.message).toBe(
      'feedback.reminderConfirmed',
    );

    vi.advanceTimersByTime(2_199);
    expect(useStore.getState().ui.feedback).not.toBeNull();

    vi.advanceTimersByTime(1);
    expect(useStore.getState().ui.feedback).toBeNull();
  });

  it('actionable toast survives until 5000 ms', () => {
    const onAction = vi.fn();
    useStore.getState().setFeedback(
      'feedback.reminderDismissed',
      'neutral',
      undefined,
      { labelKey: 'feedback.undo', onAction },
    );

    // Plain-toast deadline: still alive because action extended the window.
    vi.advanceTimersByTime(2_200);
    expect(useStore.getState().ui.feedback?.action).toBeDefined();

    // Just before the actionable deadline.
    vi.advanceTimersByTime(2_799);
    expect(useStore.getState().ui.feedback).not.toBeNull();

    // Hit the 5 000 ms boundary exactly.
    vi.advanceTimersByTime(1);
    expect(useStore.getState().ui.feedback).toBeNull();
    // The Undo callback is the user's choice; auto-dismiss must not fire it.
    expect(onAction).not.toHaveBeenCalled();
  });

  it('a follow-up setFeedback cancels the prior timer (no double-clear)', () => {
    useStore.getState().setFeedback('feedback.reminderConfirmed', 'success');
    vi.advanceTimersByTime(2_000);
    // Replace before the first timer fires.
    useStore.getState().setFeedback('feedback.reminderArchived', 'neutral');

    // First message's deadline (200 ms more) would fire here if the
    // first timer wasn't cancelled — assert it was.
    vi.advanceTimersByTime(200);
    expect(useStore.getState().ui.feedback?.message).toBe(
      'feedback.reminderArchived',
    );

    // The second message's full 2 200 ms window applies; it dismisses
    // 2 200 ms after the *replace*, not 2 200 ms after the first set.
    vi.advanceTimersByTime(1_999);
    expect(useStore.getState().ui.feedback).not.toBeNull();
    vi.advanceTimersByTime(1);
    expect(useStore.getState().ui.feedback).toBeNull();
  });

  it('clearFeedback nulls the toast and prevents stale-timer firing', () => {
    useStore.getState().setFeedback('feedback.reminderConfirmed', 'success');
    expect(useStore.getState().ui.feedback).not.toBeNull();

    useStore.getState().clearFeedback();
    expect(useStore.getState().ui.feedback).toBeNull();

    // After the original 2 200 ms would have fired, state stays null —
    // a stale callback didn't trigger.
    vi.advanceTimersByTime(3_000);
    expect(useStore.getState().ui.feedback).toBeNull();
  });
});
