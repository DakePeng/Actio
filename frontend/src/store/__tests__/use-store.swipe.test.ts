import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../use-store';
import type { Label, Reminder } from '../../types';

const { mockClient } = vi.hoisted(() => ({
  mockClient: {
    listReminders: vi.fn(),
    createReminder: vi.fn(),
    updateReminder: vi.fn(),
    deleteReminder: vi.fn(),
    listLabels: vi.fn(),
    createLabel: vi.fn(),
    updateLabel: vi.fn(),
    deleteLabel: vi.fn(),
  },
}));

vi.mock('../../api/actio-api', () => ({
  createActioApiClient: () => mockClient,
}));

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
    Object.values(mockClient).forEach((fn) => fn.mockReset());
    useStore.setState({
      labels: [seedLabel(), seedLabel({ id: 'personal', name: 'Personal', color: '#16A34A', bgColor: '#F0FDF4' })],
      reminders: [seedReminder(), seedReminder({ id: 'r2', labels: ['work', 'personal'] })],
    });
  });

  it('archives a reminder instead of removing it', async () => {
    mockClient.updateReminder.mockResolvedValue(seedReminder({ archivedAt: '2026-04-06T12:00:00.000Z' }));
    await useStore.getState().archiveReminder('r1');
    const archived = useStore.getState().reminders.find((item) => item.id === 'r1');
    expect(archived?.archivedAt).toBeTruthy();
  });

  it('restores an archived reminder', async () => {
    mockClient.updateReminder
      .mockResolvedValueOnce(seedReminder({ archivedAt: '2026-04-06T12:00:00.000Z' }))
      .mockResolvedValueOnce(seedReminder({ archivedAt: null }));
    await useStore.getState().archiveReminder('r1');
    await useStore.getState().restoreReminder('r1');
    const restored = useStore.getState().reminders.find((item) => item.id === 'r1');
    expect(restored?.archivedAt).toBeNull();
  });

  it('deletes a label and strips it from reminders and the active filter', async () => {
    mockClient.deleteLabel.mockResolvedValue(undefined);
    useStore.getState().setFilter({ label: 'work' });
    await useStore.getState().deleteLabel('work');
    expect(useStore.getState().labels.map((item) => item.id)).not.toContain('work');
    expect(useStore.getState().filter.label).toBeNull();
    expect(useStore.getState().reminders.every((item) => !item.labels.includes('work'))).toBe(true);
  });

  it('updates reminder fields inline', async () => {
    mockClient.updateReminder.mockResolvedValue(seedReminder({
      title: 'Ship swipe row v2',
      description: 'Keep the patch minimal',
      dueTime: '2026-04-06T18:30:00.000Z',
    }));
    await useStore.getState().updateReminderInline('r1', {
      title: 'Ship swipe row v2',
      description: 'Keep the patch minimal',
      dueTime: '2026-04-06T18:30:00.000Z',
    });
    expect(useStore.getState().reminders.find((item) => item.id === 'r1')?.title).toBe('Ship swipe row v2');
  });

  it('updates label fields inline', async () => {
    mockClient.updateLabel.mockResolvedValue(seedLabel({
      name: 'Deep Work',
      color: '#4338CA',
      bgColor: '#E0E7FF',
    }));
    await useStore.getState().updateLabelInline('work', {
      name: 'Deep Work',
      color: '#4338CA',
      bgColor: '#E0E7FF',
    });
    expect(useStore.getState().labels.find((item) => item.id === 'work')?.name).toBe('Deep Work');
  });
});
