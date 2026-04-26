import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { NeedsReviewView } from '../NeedsReviewView';
import { FeedbackToast } from '../FeedbackToast';
import { LanguageProvider } from '../../i18n';
import { useStore } from '../../store/use-store';
import { useVoiceStore } from '../../store/use-voice-store';
import type { Reminder } from '../../types';

function makePending(id: string, title: string): Reminder {
  return {
    id,
    title,
    description: '',
    transcript: 'meeting at three',
    priority: 'medium',
    labels: [],
    archivedAt: null,
    completedAt: null,
    archived: false,
    completed: false,
    starred: false,
    aiGenerated: true,
    confidence: 'medium',
    status: 'pending',
    speakerId: null,
    sourceWindowId: null,
    sourceClipId: null,
    dueTime: null,
    createdAt: '2026-04-26T00:00:00Z',
    isNew: false,
  } as unknown as Reminder;
}

describe('NeedsReviewView dismiss-with-undo (ISSUES.md #54)', () => {
  beforeEach(() => {
    Object.defineProperty(navigator, 'language', {
      value: 'en-US',
      configurable: true,
    });
    useStore.setState({
      reminders: [
        makePending('r1', 'Send the slide deck'),
        makePending('r2', 'Email Alice'),
      ],
      labels: [],
    });
    useVoiceStore.setState({ speakers: [] });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    useStore.setState({ reminders: [], ui: { ...useStore.getState().ui, feedback: null } });
  });

  it('shows an Undo toast on Dismiss; clicking Undo restores the reminder to pending', async () => {
    // Spy archiveReminder + restoreReminder so we don't hit the network.
    const archiveSpy = vi.fn(async (id: string) => {
      useStore.setState((s) => ({
        reminders: s.reminders.map((r) =>
          r.id === id ? { ...r, status: 'archived', archivedAt: new Date().toISOString() } : r,
        ),
      }));
    });
    const restoreSpy = vi.fn(async (id: string) => {
      useStore.setState((s) => ({
        reminders: s.reminders.map((r) =>
          r.id === id ? { ...r, status: 'pending', archivedAt: null } : r,
        ),
      }));
    });
    useStore.setState({
      archiveReminder: archiveSpy as unknown as (id: string) => Promise<void>,
      restoreReminder: restoreSpy as unknown as (id: string) => Promise<void>,
    });

    render(
      <LanguageProvider>
        <NeedsReviewView />
        <FeedbackToast />
      </LanguageProvider>,
    );

    // Both pending reminders render initially.
    expect(screen.getByText('Send the slide deck')).toBeInTheDocument();
    expect(screen.getByText('Email Alice')).toBeInTheDocument();

    // Dismiss the first card (the Dismiss button is the first one in the card actions row).
    const firstCard = screen.getByText('Send the slide deck').closest('article')!;
    const dismissBtn = within(firstCard).getByRole('button', { name: /dismiss/i });
    fireEvent.click(dismissBtn);

    // archiveReminder fired and the card should be gone from the rendered list.
    await waitFor(() => expect(archiveSpy).toHaveBeenCalledWith('r1'));
    await waitFor(() =>
      expect(screen.queryByText('Send the slide deck')).not.toBeInTheDocument(),
    );

    // The toast should be visible with an Undo button.
    const undoBtn = await screen.findByRole('button', { name: /undo/i });
    expect(undoBtn).toBeInTheDocument();

    // Click Undo → restoreReminder fires and the card returns to the queue.
    fireEvent.click(undoBtn);
    await waitFor(() => expect(restoreSpy).toHaveBeenCalledWith('r1'));
    await waitFor(() =>
      expect(screen.getByText('Send the slide deck')).toBeInTheDocument(),
    );
  });

  it('plain confirm toast (no action button) renders without Undo', async () => {
    const restoreSpy = vi.fn(async (id: string) => {
      useStore.setState((s) => ({
        reminders: s.reminders.map((r) =>
          r.id === id ? { ...r, status: 'open' } : r,
        ),
      }));
    });
    useStore.setState({
      restoreReminder: restoreSpy as unknown as (id: string) => Promise<void>,
    });

    render(
      <LanguageProvider>
        <NeedsReviewView />
        <FeedbackToast />
      </LanguageProvider>,
    );

    const card = screen.getByText('Send the slide deck').closest('article')!;
    const confirmBtn = within(card).getByRole('button', { name: /^confirm/i });
    fireEvent.click(confirmBtn);

    await waitFor(() => expect(restoreSpy).toHaveBeenCalledWith('r1'));
    // Confirm path uses the plain (non-actionable) toast — no Undo button.
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: /undo/i })).toBeNull(),
    );
  });
});

// Local helper since `within` from @testing-library/react requires an import.
import { within } from '@testing-library/react';
