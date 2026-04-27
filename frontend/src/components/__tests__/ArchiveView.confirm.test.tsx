import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

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
  DEV_TENANT_ID: '00000000-0000-0000-0000-000000000000',
}));

import { useStore } from '../../store/use-store';
import { useVoiceStore } from '../../store/use-voice-store';
import { ArchiveView } from '../ArchiveView';
import { LanguageProvider } from '../../i18n';

function renderArchive() {
  return render(
    <LanguageProvider>
      <ArchiveView />
    </LanguageProvider>,
  );
}

/** Pins ISS-077: Archive's destructive deletes (single + bulk × tasks +
 *  clips) now route through `useConfirm()`. Cancel = no API calls; confirm
 *  = exactly N. */
describe('ArchiveView destructive-action confirmation (ISS-077)', () => {
  beforeEach(() => {
    localStorage.clear();
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
    Object.values(mockClient).forEach((fn) => fn.mockReset());
    mockClient.deleteReminder.mockResolvedValue(undefined);

    useStore.setState({
      reminders: [
        {
          id: 'rem-1',
          title: 'First archived',
          description: '',
          priority: 'medium',
          labels: [],
          createdAt: '2026-04-20T10:00:00.000Z',
          archivedAt: '2026-04-21T10:00:00.000Z',
          status: 'archived',
        },
        {
          id: 'rem-2',
          title: 'Second archived',
          description: '',
          priority: 'low',
          labels: [],
          createdAt: '2026-04-20T11:00:00.000Z',
          archivedAt: '2026-04-21T11:00:00.000Z',
          status: 'archived',
        },
      ],
    });
    // ArchiveView calls loadBackendClips() on mount; stub it to a no-op so
    // the test isn't fighting fetch.
    useVoiceStore.setState({
      segments: [],
      loadBackendClips: async () => {},
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('per-row delete asks for confirmation; Cancel does not call deleteReminder', async () => {
    renderArchive();

    const deleteBtns = await screen.findAllByRole('button', { name: 'Delete' });
    fireEvent.click(deleteBtns[0]);

    // Dialog opens with the singular copy
    await screen.findByText(/Permanently delete this item/i);
    expect(mockClient.deleteReminder).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(screen.queryByText(/Permanently delete this item/i)).not.toBeInTheDocument();
    });
    expect(mockClient.deleteReminder).not.toHaveBeenCalled();
  });

  it('bulk delete confirm fires exactly N deleteReminder calls', async () => {
    renderArchive();

    // Select both archived rows by clicking on them. The row itself is a
    // toggle target (onClick={() => toggleTask(reminder.id)}).
    const firstRow = (await screen.findByText('First archived')).closest('.archive-row');
    const secondRow = screen.getByText('Second archived').closest('.archive-row');
    expect(firstRow).toBeTruthy();
    expect(secondRow).toBeTruthy();
    fireEvent.click(firstRow!);
    fireEvent.click(secondRow!);

    // Bulk bar appears with a Delete button. There are also per-row Delete
    // buttons; the bulk-bar one is inside `.archive-bulk-bar`.
    const bulkBar = await waitFor(() => {
      const bar = document.querySelector('.archive-bulk-bar');
      if (!bar) throw new Error('bulk bar not visible');
      return bar as HTMLElement;
    });
    const bulkDelete = Array.from(bulkBar.querySelectorAll('button')).find(
      (b) => b.textContent === 'Delete',
    );
    expect(bulkDelete).toBeTruthy();

    fireEvent.click(bulkDelete!);

    // Bulk-count message uses interpolated {count}
    await screen.findByText(/Permanently delete 2 items/i);
    expect(mockClient.deleteReminder).not.toHaveBeenCalled();

    // Click the destructive Confirm — exactly two delete calls. The dialog
    // confirm button is scoped inside `.confirm-dialog__actions` so we
    // don't collide with the row/bulk Delete buttons still in the DOM.
    const dialogConfirm = document
      .querySelector('.confirm-dialog__btn--destructive') as HTMLButtonElement | null;
    expect(dialogConfirm).toBeTruthy();
    fireEvent.click(dialogConfirm!);

    await waitFor(() => {
      expect(mockClient.deleteReminder).toHaveBeenCalledTimes(2);
    });
    expect(mockClient.deleteReminder.mock.calls.map(([id]) => id).sort()).toEqual([
      'rem-1',
      'rem-2',
    ]);
  });
});
