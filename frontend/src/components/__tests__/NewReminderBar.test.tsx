import { render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { NewReminderBar } from '../NewReminderBar';
import { BoardWindow } from '../BoardWindow';
import { LanguageProvider } from '../../i18n';
import { useStore } from '../../store/use-store';
import { resetBackendUrlCache } from '../../api/backend-url';

vi.mock('../../hooks/useKeyboardShortcuts', () => ({
  useKeyboardShortcuts: vi.fn(),
}));

vi.mock('../Board', () => ({ Board: () => <div /> }));
vi.mock('../NeedsReviewView', () => ({ NeedsReviewView: () => <div /> }));
vi.mock('../ArchiveView', () => ({ ArchiveView: () => <div /> }));
vi.mock('../settings/SettingsView', () => ({ SettingsView: () => <div /> }));
vi.mock('../RecordingTab', () => ({ RecordingTab: () => <div /> }));
vi.mock('../LiveTab', () => ({ LiveTab: () => <div /> }));
vi.mock('../PeopleTab', () => ({ PeopleTab: () => <div /> }));

function openNewReminder(overrides: Partial<ReturnType<typeof useStore.getState>['ui']> = {}) {
  useStore.setState((state) => ({
    reminders: [],
    ui: {
      ...state.ui,
      showBoardWindow: true,
      showNewReminderBar: true,
      activeTab: 'board',
      feedback: null,
      ...overrides,
    },
  }));
}

describe('NewReminderBar', () => {
  beforeEach(() => {
    localStorage.clear();
    resetBackendUrlCache();
    vi.restoreAllMocks();
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input);
        if (url.endsWith('/settings')) {
          return new Response(JSON.stringify({ llm: { selection: { kind: 'remote' } } }), {
            status: 200,
            headers: { 'Content-Type': 'application/json' },
          });
        }
        return new Response('{}', { status: 200 });
      }),
    );
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
    useStore.setState((state) => ({
      reminders: [],
      ui: {
        ...state.ui,
        showBoardWindow: false,
        showNewReminderBar: false,
        activeTab: 'board',
        feedback: null,
      },
    }));
  });

  it('shows the create button on non-board pages', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, showBoardWindow: true, activeTab: 'settings' },
    }));

    render(
      <LanguageProvider>
        <BoardWindow />
      </LanguageProvider>,
    );

    expect(screen.getByRole('button', { name: 'Capture note' })).toBeEnabled();
  });

  it('does not advertise image attachments in chat capture mode', () => {
    openNewReminder();

    render(
      <LanguageProvider>
        <NewReminderBar />
      </LanguageProvider>,
    );

    expect(screen.getByText('Type or dictate a note')).toBeInTheDocument();
    expect(screen.queryByText(/attach an image/i)).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /attach images/i })).not.toBeInTheDocument();
  });

  it('places the mode switch below the save action', () => {
    openNewReminder();

    render(
      <LanguageProvider>
        <NewReminderBar />
      </LanguageProvider>,
    );

    const send = screen.getByRole('button', { name: 'Send' });
    const toggle = screen.getByRole('button', { name: 'Switch to form' });

    expect(send.compareDocumentPosition(toggle) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('defaults to form mode and shows a notice when LLM is disabled', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input);
        if (url.endsWith('/health')) return new Response('{}', { status: 200 });
        if (url.endsWith('/settings')) {
          return new Response(JSON.stringify({ llm: { selection: { kind: 'disabled' } } }), {
            status: 200,
            headers: { 'Content-Type': 'application/json' },
          });
        }
        return new Response('{}', { status: 404 });
      }),
    );
    localStorage.setItem('actio-capture-mode', 'chat');
    openNewReminder();

    render(
      <LanguageProvider>
        <NewReminderBar />
      </LanguageProvider>,
    );

    await waitFor(() => expect(screen.getByText('Add a note without leaving the board')).toBeInTheDocument());
    expect(useStore.getState().ui.feedback?.message).toBe('feedback.llmNotConfiguredFormMode');
  });
});
