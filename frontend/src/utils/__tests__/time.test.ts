import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { formatTimeShort } from '../time';

/** Pins the rendering branches in `formatTimeShort` (ISSUES.md #71).
 *  The function uses time-based bucketing — `diffDays = floor(diffMs / 1d)`
 *  — NOT calendar-day boundaries, which produces some surprising
 *  behaviour at day boundaries. Tests pin actual current behaviour,
 *  including the surprises (flagged inline) so a future cleanup is a
 *  deliberate change rather than an accidental break.
 *
 *  All cases are anchored at "now = 2026-04-27 14:00 (Mon, local time)". */
const NOW_MS = new Date('2026-04-27T14:00:00').getTime();

function iso(daysFromNow: number, hour: number, minute = 0): string {
  // Build a local-time ISO string so getHours() matches the literal
  // numbers the test asserts. Using a UTC-Z suffix would shift the
  // hour on a non-UTC machine.
  const d = new Date(NOW_MS);
  d.setDate(d.getDate() + daysFromNow);
  d.setHours(hour, minute, 0, 0);
  return d.toISOString();
}

describe('formatTimeShort (ISSUES.md #71)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(NOW_MS));
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  // ── Past, same day (< 24h ago) ──────────────────────────────────────
  it('past time same day shows just the time string (no "today" suffix)', () => {
    expect(formatTimeShort(iso(0, 9, 30))).toBe('9:30 AM');
    // 12-hour edge: noon and 1pm of "earlier today"
    expect(formatTimeShort(iso(0, 12, 0))).toBe('12:00 PM');
    expect(formatTimeShort(iso(0, 13, 0))).toBe('1:00 PM');
    expect(formatTimeShort(iso(0, 11, 0))).toBe('11:00 AM');
  });

  // ── Within an hour ahead ────────────────────────────────────────────
  it('within an hour shows "In N min"', () => {
    expect(formatTimeShort(iso(0, 14, 30))).toBe('In 30 min');
    expect(formatTimeShort(iso(0, 14, 1))).toBe('In 1 min');
    expect(formatTimeShort(iso(0, 14, 59))).toBe('In 59 min');
  });

  // ── Later today (1h - 24h ahead) ────────────────────────────────────
  it('1h-24h ahead shows "h:mm AM/PM today"', () => {
    expect(formatTimeShort(iso(0, 18, 30))).toBe('6:30 PM today');
    expect(formatTimeShort(iso(0, 23, 0))).toBe('11:00 PM today');
  });

  // ── Surprise: < 24h ahead but next calendar day still says "today" ─
  it('next calendar morning < 24h out shows as "today" (time-based bucketing)', () => {
    // now=Mon 14:00, target=Tue 09:00 → 19h ahead, diffMin=1140, < 1440
    // → renders as "9:00 AM today" even though it's tomorrow's date.
    // This is current behaviour; a calendar-aware fix is a separate
    // ticket if anyone wants it.
    expect(formatTimeShort(iso(1, 9, 0))).toBe('9:00 AM today');
    expect(formatTimeShort(iso(1, 0, 0))).toBe('12:00 AM today'); // midnight edge
  });

  // ── Tomorrow (>= 24h ahead, < 48h) ──────────────────────────────────
  it('24-48h ahead shows "Tomorrow at h:mm AM/PM"', () => {
    // now=Mon 14:00, target=Tue 14:00 = exactly 24h → diffDays=1
    expect(formatTimeShort(iso(1, 14, 0))).toBe('Tomorrow at 2:00 PM');
    expect(formatTimeShort(iso(1, 18, 0))).toBe('Tomorrow at 6:00 PM');
  });

  // ── This week (2-6 diffDays out, time-based) ───────────────────────
  it('2-6 diffDays ahead shows "{weekday} at h:mm AM/PM"', () => {
    // now=Mon 14:00, target=Wed 14:00 → 48h, diffDays=2 → Wednesday
    expect(formatTimeShort(iso(2, 14, 0))).toBe('Wednesday at 2:00 PM');
    expect(formatTimeShort(iso(3, 14, 0))).toBe('Thursday at 2:00 PM');
  });

  it('weekday name lags by one when target time-of-day is earlier (time-based bucketing)', () => {
    // now=Mon 14:00, target=Sun 11:30 (+6 cal days) → 5d 21h 30m
    // → diffMin=8490, diffDays=floor(8490/1440)=5 → Saturday, not Sunday.
    // Pinning this surprising-but-current behaviour; same root cause as
    // the "next calendar day shows as today" surprise above.
    expect(formatTimeShort(iso(6, 11, 30))).toBe('Saturday at 11:30 AM');
  });

  // ── Further out (> 6 diffDays) ──────────────────────────────────────
  it('further than 6 diffDays falls to en-US "MMM D"', () => {
    // +14 days → 2026-05-11 → "May 11"
    expect(formatTimeShort(iso(14, 9, 0))).toBe('May 11');
    expect(formatTimeShort(iso(30, 9, 0))).toBe('May 27');
  });

  // ── Past, older than 24h (BUG: dayLabel returns undefined) ──────────
  it('past older than 24h has a "Due undefined" bug for negative diffDays', () => {
    // now=Mon 14:00, target=Fri 10:00 (3 days ago) → diff = -77h, diffMin=-4620
    // diffDays = floor(-4620/1440) = -4. dayLabel: diffDays <= 6 yes,
    // returns dayNames[(1 + -4) % 7] = dayNames[-3] (JS: -3 % 7 = -3)
    // → undefined. Pinning the buggy output as "current state" so a
    // future fix is a deliberate change. The fix is to use
    // ((diffDays % 7) + 7) % 7 — but that's a separate ticket.
    expect(formatTimeShort(iso(-3, 10))).toBe('Due undefined');
  });

  // ── Minute padding ──────────────────────────────────────────────────
  it('zero-pads single-digit minutes', () => {
    // Use the Tomorrow branch which exercises the timeStr formatting
    // path with a non-zero minute and outside the same-day suffix.
    expect(formatTimeShort(iso(1, 14, 5))).toBe('Tomorrow at 2:05 PM');
    expect(formatTimeShort(iso(1, 14, 0))).toBe('Tomorrow at 2:00 PM');
  });
});
