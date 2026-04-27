import { render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { NewReminderBar } from '../NewReminderBar';
import { LanguageProvider } from '../../i18n';
import { useStore } from '../../store/use-store';
import { resetBackendUrlCache } from '../../api/backend-url';

vi.mock('../../hooks/useKeyboardShortcuts', () => ({
  useKeyboardShortcuts: vi.fn(),
}));

/** Pins ISS-083: NewReminderBar's chat-mode probe used to arm an outer
 *  800ms abort that raced with port-discovery's per-port 800ms timeout.
 *  When the backend wasn't on port 3000 the race fired before the
 *  request returned, the catch returned `false`, and the bar
 *  silently flipped to form mode + toasted "LLM not configured" even
 *  though the LLM was perfectly configured.
 *
 *  This test simulates a slow `/settings` response (1500ms — well past
 *  the old 800ms ceiling) and asserts the bar stays in chat mode and
 *  no misconfigured toast is pushed. */
describe('NewReminderBar slow LLM probe (ISS-083)', () => {
  beforeEach(() => {
    localStorage.clear();
    resetBackendUrlCache();
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
    localStorage.setItem('actio-capture-mode', 'chat');
    useStore.setState((state) => ({
      reminders: [],
      ui: {
        ...state.ui,
        showBoardWindow: true,
        showNewReminderBar: true,
        activeTab: 'board',
        feedback: null,
      },
    }));
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetBackendUrlCache();
  });

  it('stays in chat mode when /settings responds after the legacy 800ms deadline', async () => {
    // Stub fetch so /settings takes 1500ms but eventually returns a
    // *configured* selection. Under the old code, the outer 800ms abort
    // would fire first → catch → false → flip to form mode + toast.
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input);
        if (url.endsWith('/health')) {
          return new Response('{}', { status: 200 });
        }
        if (url.endsWith('/settings')) {
          await new Promise((resolve) => setTimeout(resolve, 1500));
          return new Response(
            JSON.stringify({ llm: { selection: { kind: 'remote' } } }),
            { status: 200, headers: { 'Content-Type': 'application/json' } },
          );
        }
        return new Response('{}', { status: 404 });
      }),
    );

    render(
      <LanguageProvider>
        <NewReminderBar />
      </LanguageProvider>,
    );

    // Chat composer is the indicator the bar stayed in chat mode.
    expect(screen.getByText('Type or dictate a note')).toBeInTheDocument();

    // Wait long enough for the slow /settings response to settle, plus a
    // little buffer so the .then callback gets a chance to run.
    await new Promise((r) => setTimeout(r, 1700));
    await waitFor(() => {
      // No misconfigured toast was pushed.
      expect(useStore.getState().ui.feedback?.message).not.toBe(
        'feedback.llmNotConfiguredFormMode',
      );
    });
    // Still in chat mode (composer still rendered, form fields absent).
    expect(screen.getByText('Type or dictate a note')).toBeInTheDocument();
    expect(screen.queryByPlaceholderText(/Pick up the dry cleaning/i)).not.toBeInTheDocument();
  });
});
