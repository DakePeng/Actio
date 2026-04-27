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

vi.mock('../../../api/actio-api', () => ({
  createActioApiClient: () => mockClient,
  DEV_TENANT_ID: '00000000-0000-0000-0000-000000000000',
}));

import { useStore } from '../../../store/use-store';
import { LabelManager } from '../LabelManager';
import { LanguageProvider } from '../../../i18n';

function renderManager() {
  return render(
    <LanguageProvider>
      <LabelManager />
    </LanguageProvider>,
  );
}

/** Pins ISS-082: deleting a label now shows a confirm with the cascade
 *  count derived from the reminders selector; Cancel = no DELETE call,
 *  Confirm = exactly one. */
describe('LabelManager destructive-delete confirmation (ISS-082)', () => {
  beforeEach(() => {
    localStorage.clear();
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
    Object.values(mockClient).forEach((fn) => fn.mockReset());
    mockClient.deleteLabel.mockResolvedValue(undefined);

    useStore.setState({
      labels: [
        { id: 'lbl-work', name: 'Work', color: '#6366F1', bgColor: '#EEF2FF' },
        { id: 'lbl-orphan', name: 'Orphan', color: '#DC2626', bgColor: '#FEF2F2' },
      ],
      reminders: [
        {
          id: 'rem-1',
          title: 'A',
          description: '',
          priority: 'medium',
          labels: ['lbl-work'],
          createdAt: '2026-04-20T10:00:00.000Z',
          archivedAt: null,
        },
        {
          id: 'rem-2',
          title: 'B',
          description: '',
          priority: 'low',
          labels: ['lbl-work'],
          createdAt: '2026-04-20T11:00:00.000Z',
          archivedAt: null,
        },
      ],
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('cascade-count message matches the number of reminders using the label', async () => {
    renderManager();

    // Click the "×" delete button on the Work chip.
    const deleteBtn = screen.getByRole('button', { name: /Delete Work/i });
    fireEvent.click(deleteBtn);

    // Cascade-count copy: "removed from 2 reminder(s)"
    await screen.findByText(/removed from 2 reminder/i);
    expect(mockClient.deleteLabel).not.toHaveBeenCalled();
  });

  it('Cancel keeps the label and calls no API', async () => {
    renderManager();

    fireEvent.click(screen.getByRole('button', { name: /Delete Work/i }));
    await screen.findByText(/removed from 2 reminder/i);

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(screen.queryByText(/removed from 2 reminder/i)).not.toBeInTheDocument();
    });
    expect(mockClient.deleteLabel).not.toHaveBeenCalled();
  });

  it('Confirm fires exactly one deleteLabel call with the right id', async () => {
    renderManager();

    fireEvent.click(screen.getByRole('button', { name: /Delete Work/i }));
    await screen.findByText(/removed from 2 reminder/i);

    // The destructive Confirm button — scope to the dialog's destructive
    // class so we don't collide with other "Delete" buttons in the DOM.
    const dialogConfirm = document.querySelector(
      '.confirm-dialog__btn--destructive',
    ) as HTMLButtonElement | null;
    expect(dialogConfirm).toBeTruthy();
    fireEvent.click(dialogConfirm!);

    await waitFor(() => {
      expect(mockClient.deleteLabel).toHaveBeenCalledTimes(1);
    });
    expect(mockClient.deleteLabel).toHaveBeenCalledWith('lbl-work');
  });

  it('an unused label uses the shorter "no cascade" copy', async () => {
    renderManager();

    fireEvent.click(screen.getByRole('button', { name: /Delete Orphan/i }));
    // Should NOT mention reminder count
    await screen.findByText(/Delete the "Orphan" label\?/i);
    expect(screen.queryByText(/removed from/i)).not.toBeInTheDocument();
  });
});
