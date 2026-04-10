import { describe, expect, it } from 'vitest';
import { mapBackendLabel, mapBackendReminder } from '../actio-api';

describe('actio api mappers', () => {
  it('maps backend reminders into the frontend reminder model', () => {
    const reminder = mapBackendReminder({
      id: 'rem-1',
      session_id: null,
      tenant_id: 'tenant-1',
      speaker_id: null,
      assigned_to: null,
      title: 'Ship integration',
      description: 'Connect the board to persisted data',
      status: 'archived',
      priority: 'high',
      due_time: '2026-04-09T18:30:00.000Z',
      archived_at: '2026-04-09T19:00:00.000Z',
      transcript_excerpt: 'Finish wiring the API layer',
      context: 'Captured from a note',
      source_time: '2026-04-09T17:00:00.000Z',
      labels: ['label-1', 'label-2'],
      created_at: '2026-04-09T16:00:00.000Z',
      updated_at: '2026-04-09T16:30:00.000Z',
    });

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
