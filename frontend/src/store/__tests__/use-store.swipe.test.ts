import { beforeEach, describe, expect, it } from 'vitest';
import { useStore } from '../use-store';
import type { Label, Reminder } from '../../types';

const seedLabel = (overrides: Partial<Label> = {}): Label => ({
  id: 'work',
  name: 'Work',
  color: '#6366F1',
  bgColor: '#EEF2FF',
  ...overrides,
});

const seedReminder = (overrides: Partial<Reminder> = {}): Reminder => ({
  id: 'r1',
  title: 'Ship swipe row',
  description: 'Implement shared swipe logic',
  priority: 'high',
  labels: ['work'],
  createdAt: '2026-04-06T10:00:00.000Z',
  archivedAt: null,
  ...overrides,
});

describe('useStore swipe actions', () => {
  beforeEach(() => {
    useStore.getState().reset();
    useStore.setState({
      labels: [seedLabel(), seedLabel({ id: 'personal', name: 'Personal', color: '#16A34A', bgColor: '#F0FDF4' })],
      reminders: [seedReminder(), seedReminder({ id: 'r2', labels: ['work', 'personal'] })],
    });
  });

  it('archives a reminder instead of removing it', () => {
    useStore.getState().archiveReminder('r1');
    const archived = useStore.getState().reminders.find((item) => item.id === 'r1');
    expect(archived?.archivedAt).toBeTruthy();
  });

  it('restores an archived reminder', () => {
    useStore.getState().archiveReminder('r1');
    useStore.getState().restoreReminder('r1');
    const restored = useStore.getState().reminders.find((item) => item.id === 'r1');
    expect(restored?.archivedAt).toBeNull();
  });

  it('deletes a label and strips it from reminders and the active filter', () => {
    useStore.getState().setFilter({ label: 'work' });
    useStore.getState().deleteLabel('work');
    expect(useStore.getState().labels.map((item) => item.id)).not.toContain('work');
    expect(useStore.getState().filter.label).toBeNull();
    expect(useStore.getState().reminders.every((item) => !item.labels.includes('work'))).toBe(true);
  });

  it('updates reminder fields inline', () => {
    useStore.getState().updateReminderInline('r1', {
      title: 'Ship swipe row v2',
      description: 'Keep the patch minimal',
      dueTime: '18:30',
    });
    expect(useStore.getState().reminders.find((item) => item.id === 'r1')?.title).toBe('Ship swipe row v2');
  });

  it('updates label fields inline', () => {
    useStore.getState().updateLabelInline('work', {
      name: 'Deep Work',
      color: '#4338CA',
      bgColor: '#E0E7FF',
    });
    expect(useStore.getState().labels.find((item) => item.id === 'work')?.name).toBe('Deep Work');
  });
});
