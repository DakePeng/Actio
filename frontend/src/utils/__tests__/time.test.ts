import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { formatTimeShort } from '../time';

/** Pins the rendering branches in `formatTimeShort` (ISSUES.md #71).
 *  After the calendar-aware rewrite in #72, the function uses true
 *  calendar-day arithmetic for "today" / "Tomorrow" / "{Weekday}"
 *  bucketing instead of `floor(diffMs / 1d)`. Tests assert the
 *  corrected behaviour throughout.
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

  // ── Tomorrow (next calendar day, regardless of hour-distance) ───────
  it('next calendar day renders as "Tomorrow at h:mm AM/PM"', () => {
    // Even when the target is < 24h ahead by clock time (e.g. Tue
    // 09:00 viewed Mon 14:00 = 19h), the calendar-day diff is 1 so
    // it correctly says "Tomorrow", not "today".
    expect(formatTimeShort(iso(1, 9, 0))).toBe('Tomorrow at 9:00 AM');
    expect(formatTimeShort(iso(1, 0, 0))).toBe('Tomorrow at 12:00 AM'); // midnight edge
    expect(formatTimeShort(iso(1, 14, 0))).toBe('Tomorrow at 2:00 PM');
    expect(formatTimeShort(iso(1, 18, 0))).toBe('Tomorrow at 6:00 PM');
  });

  // ── This week (2-6 calendar days out) ───────────────────────────────
  it('2-6 calendar days ahead shows "{weekday} at h:mm AM/PM"', () => {
    expect(formatTimeShort(iso(2, 14, 0))).toBe('Wednesday at 2:00 PM');
    expect(formatTimeShort(iso(3, 14, 0))).toBe('Thursday at 2:00 PM');
    // The previously-buggy case: +6 cal days from Mon = Sun. Now
    // resolves to Sunday correctly because we use target.getDay().
    expect(formatTimeShort(iso(6, 11, 30))).toBe('Sunday at 11:30 AM');
  });

  // ── Further out (> 6 diffDays) ──────────────────────────────────────
  it('further than 6 diffDays falls to en-US "MMM D"', () => {
    // +14 days → 2026-05-11 → "May 11"
    expect(formatTimeShort(iso(14, 9, 0))).toBe('May 11');
    expect(formatTimeShort(iso(30, 9, 0))).toBe('May 27');
  });

  // ── Past, within the last week ──────────────────────────────────────
  it('past dates within a week show "Due {weekday}"', () => {
    // 3 days ago from Mon = Friday. Was previously rendering
    // "Due undefined" due to a (now-fixed) negative-modulo bug.
    expect(formatTimeShort(iso(-3, 10))).toBe('Due Friday');
    expect(formatTimeShort(iso(-1, 10))).toBe('Due Sunday');
  });

  it('past dates older than a week show "Due {MMM D}"', () => {
    expect(formatTimeShort(iso(-14, 9, 0))).toBe('Due Apr 13');
  });

  // ── Minute padding ──────────────────────────────────────────────────
  it('zero-pads single-digit minutes', () => {
    // Use the Tomorrow branch which exercises the timeStr formatting
    // path with a non-zero minute and outside the same-day suffix.
    expect(formatTimeShort(iso(1, 14, 5))).toBe('Tomorrow at 2:05 PM');
    expect(formatTimeShort(iso(1, 14, 0))).toBe('Tomorrow at 2:00 PM');
  });
});
