import { describe, expect, it } from 'vitest';
import { mapBackendLabel, mapBackendReminder } from '../actio-api';
import type { BackendReminderDto } from '../../types';

/** Builds a BackendReminderDto with safe defaults; spread `over` to vary
 *  individual fields per test. The defaults cover the all-required fields
 *  shape; nullable fields default to null so each test sets only what it
 *  cares about. */
function dto(over: Partial<BackendReminderDto> = {}): BackendReminderDto {
  return {
    id: 'rem-1',
    session_id: null,
    tenant_id: 'tenant-1',
    speaker_id: null,
    assigned_to: null,
    title: 'Ship integration',
    description: 'Connect the board to persisted data',
    status: 'open',
    priority: 'high',
    due_time: null,
    archived_at: null,
    transcript_excerpt: null,
    context: null,
    source_time: null,
    source_window_id: null,
    labels: [],
    created_at: '2026-04-09T16:00:00.000Z',
    updated_at: '2026-04-09T16:30:00.000Z',
    ...over,
  };
}

describe('actio api mappers', () => {
  it('maps backend reminders into the frontend reminder model', () => {
    const reminder = mapBackendReminder(
      dto({
        status: 'archived',
        due_time: '2026-04-09T18:30:00.000Z',
        archived_at: '2026-04-09T19:00:00.000Z',
        transcript_excerpt: 'Finish wiring the API layer',
        context: 'Captured from a note',
        source_time: '2026-04-09T17:00:00.000Z',
        labels: ['label-1', 'label-2'],
      }),
    );

    expect(reminder).toMatchObject({
      id: 'rem-1',
      title: 'Ship integration',
      description: 'Connect the board to persisted data',
      priority: 'high',
      dueTime: '2026-04-09T18:30:00.000Z',
      labels: ['label-1', 'label-2'],
      transcript: 'Finish wiring the API layer',
      context: 'Captured from a note',
      sourceTime: '2026-04-09T17:00:00.000Z',
      createdAt: '2026-04-09T16:00:00.000Z',
      archivedAt: '2026-04-09T19:00:00.000Z',
    });
  });

  // ── ISSUES.md #65: null-fallback branches ─────────────────────────────

  it('null title falls back to description', () => {
    const reminder = mapBackendReminder(
      dto({ title: null, description: 'Connect the board' }),
    );
    expect(reminder.title).toBe('Connect the board');
  });

  it('null priority defaults to medium', () => {
    const reminder = mapBackendReminder(dto({ priority: null }));
    expect(reminder.priority).toBe('medium');
  });

  it('nullable string fields become undefined (not null) on the Reminder side', () => {
    const reminder = mapBackendReminder(
      dto({
        speaker_id: null,
        due_time: null,
        transcript_excerpt: null,
        context: null,
        source_time: null,
        source_window_id: null,
      }),
    );
    expect(reminder.speakerId).toBeUndefined();
    expect(reminder.dueTime).toBeUndefined();
    expect(reminder.transcript).toBeUndefined();
    expect(reminder.context).toBeUndefined();
    expect(reminder.sourceTime).toBeUndefined();
    expect(reminder.sourceWindowId).toBeUndefined();
  });

  it('archived status with null archived_at falls back to updated_at (legacy-row support)', () => {
    const reminder = mapBackendReminder(
      dto({
        status: 'archived',
        archived_at: null,
        updated_at: '2026-04-26T05:00:00.000Z',
      }),
    );
    expect(reminder.archivedAt).toBe('2026-04-26T05:00:00.000Z');
  });

  it('open status with non-null archived_at still produces archivedAt:null (status wins)', () => {
    const reminder = mapBackendReminder(
      dto({
        status: 'open',
        archived_at: '2026-04-26T04:00:00.000Z', // stale DB value
      }),
    );
    expect(reminder.archivedAt).toBeNull();
  });

  it('maps backend labels into the frontend label model', () => {
    expect(
      mapBackendLabel({
        id: 'label-1',
        tenant_id: 'tenant-1',
        name: 'Work',
        color: '#6366F1',
        bg_color: '#EEF2FF',
        created_at: '2026-04-09T16:00:00.000Z',
      }),
    ).toEqual({
      id: 'label-1',
      name: 'Work',
      color: '#6366F1',
      bgColor: '#EEF2FF',
    });
  });
});
