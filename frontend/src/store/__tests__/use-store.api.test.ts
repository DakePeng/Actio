import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../use-store';

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

describe('useStore backend integration actions', () => {
  beforeEach(() => {
    useStore.getState().reset();
    localStorage.clear();
    Object.values(mockClient).forEach((fn) => fn.mockReset());
  });

  it('loads reminders and labels from the backend client', async () => {
    mockClient.listLabels.mockResolvedValue([
      { id: 'label-1', name: 'Work', color: '#6366F1', bgColor: '#EEF2FF' },
    ]);
    mockClient.listReminders.mockResolvedValue([
      {
        id: 'rem-1',
        title: 'Persist board',
        description: 'Replace the mock API',
        priority: 'high',
        labels: ['label-1'],
        createdAt: '2026-04-09T16:00:00.000Z',
        archivedAt: null,
      },
    ]);

    await useStore.getState().loadBoard();

    expect(mockClient.listLabels).toHaveBeenCalledOnce();
    expect(mockClient.listReminders).toHaveBeenCalledOnce();
    expect(useStore.getState().labels).toEqual([
      { id: 'label-1', name: 'Work', color: '#6366F1', bgColor: '#EEF2FF' },
    ]);
    expect(useStore.getState().reminders.map((item) => item.id)).toEqual(['rem-1']);
  });

  it('creates, archives, restores, and deletes reminders through the backend client', async () => {
    useStore.setState({
      labels: [{ id: 'label-1', name: 'Work', color: '#6366F1', bgColor: '#EEF2FF' }],
      reminders: [
        {
          id: 'rem-1',
          title: 'Persist board',
          description: 'Replace the mock API',
          priority: 'high',
          labels: ['label-1'],
          createdAt: '2026-04-09T16:00:00.000Z',
          archivedAt: null,
        },
      ],
    });

    mockClient.createReminder.mockResolvedValue({
      id: 'rem-2',
      title: 'Create reminder',
      description: 'Round-trip through backend',
      priority: 'medium',
      labels: [],
      createdAt: '2026-04-09T17:00:00.000Z',
      archivedAt: null,
    });
    mockClient.updateReminder
      .mockResolvedValueOnce({
        id: 'rem-1',
        title: 'Persist board',
        description: 'Replace the mock API',
        priority: 'high',
        labels: ['label-1'],
        createdAt: '2026-04-09T16:00:00.000Z',
        archivedAt: '2026-04-09T18:00:00.000Z',
      })
      .mockResolvedValueOnce({
        id: 'rem-1',
        title: 'Persist board',
        description: 'Replace the mock API',
        priority: 'high',
        labels: ['label-1'],
        createdAt: '2026-04-09T16:00:00.000Z',
        archivedAt: null,
      });
    mockClient.deleteReminder.mockResolvedValue(undefined);

    await useStore.getState().addReminder({
      title: 'Create reminder',
      description: 'Round-trip through backend',
      priority: 'medium',
      labels: [],
      createdAt: '2026-04-09T17:00:00.000Z',
      archivedAt: null,
    });
    await useStore.getState().archiveReminder('rem-1');
    await useStore.getState().restoreReminder('rem-1');
    await useStore.getState().deleteReminder('rem-2');

    expect(mockClient.createReminder).toHaveBeenCalledOnce();
    expect(mockClient.updateReminder).toHaveBeenNthCalledWith(1, 'rem-1', { status: 'archived' });
    expect(mockClient.updateReminder).toHaveBeenNthCalledWith(2, 'rem-1', { status: 'open' });
    expect(mockClient.deleteReminder).toHaveBeenCalledWith('rem-2');
    expect(useStore.getState().reminders.map((item) => item.id)).toEqual(['rem-1']);
    expect(useStore.getState().reminders[0]?.archivedAt).toBeNull();
  });

  it('persists label creation, deletion, and reminder label assignment through the backend client', async () => {
    useStore.setState({
      labels: [{ id: 'label-1', name: 'Work', color: '#6366F1', bgColor: '#EEF2FF' }],
      reminders: [
        {
          id: 'rem-1',
          title: 'Persist board',
          description: 'Replace the mock API',
          priority: 'high',
          labels: ['label-1'],
          createdAt: '2026-04-09T16:00:00.000Z',
          archivedAt: null,
        },
      ],
    });

    mockClient.createLabel.mockResolvedValue({
      id: 'label-2',
      name: 'Personal',
      color: '#16A34A',
      bgColor: '#F0FDF4',
    });
    mockClient.updateReminder.mockResolvedValue({
      id: 'rem-1',
      title: 'Persist board',
      description: 'Replace the mock API',
      priority: 'high',
      labels: ['label-1', 'label-2'],
      createdAt: '2026-04-09T16:00:00.000Z',
      archivedAt: null,
    });
    mockClient.deleteLabel.mockResolvedValue(undefined);

    await useStore.getState().addLabel({
      name: 'Personal',
      color: '#16A34A',
      bgColor: '#F0FDF4',
    });
    await useStore.getState().setLabels('rem-1', ['label-1', 'label-2']);
    await useStore.getState().deleteLabel('label-2');

    expect(mockClient.createLabel).toHaveBeenCalledOnce();
    expect(mockClient.updateReminder).toHaveBeenCalledWith('rem-1', { labels: ['label-1', 'label-2'] });
    expect(mockClient.deleteLabel).toHaveBeenCalledWith('label-2');
    expect(useStore.getState().labels.map((item) => item.id)).toEqual(['label-1']);
    expect(useStore.getState().reminders[0]?.labels).toEqual(['label-1']);
  });
});
