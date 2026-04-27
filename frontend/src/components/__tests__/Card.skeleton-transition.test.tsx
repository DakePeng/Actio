import { act, render, screen } from '@testing-library/react';
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
import { Board } from '../Board';
import { LanguageProvider } from '../../i18n';

function renderBoard() {
  return render(
    <LanguageProvider>
      <Board />
    </LanguageProvider>,
  );
}

/** Pins ISS-085: rendering a Card with `isExtracting: true` previously
 *  hit a Rules-of-Hooks violation because Card.tsx returned the skeleton
 *  branch after only one hook (`useT`) but ran 32 more hooks for the
 *  real-card path. Switching the same reminder.id from extracting → not
 *  on a single Card instance changed the hook count between renders.
 *
 *  After the fix, Board.tsx routes extracting reminders to <CardSkeleton>
 *  (a separate component with its own hook list) and only mounts <Card>
 *  for finished reminders. The transition is now an unmount-of-skeleton
 *  + mount-of-real, with no shared component instance to corrupt. */
describe('Card extracting → real transition (ISS-085)', () => {
  beforeEach(() => {
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
    Object.values(mockClient).forEach((fn) => fn.mockReset());
    useStore.setState((state) => ({
      labels: [],
      filter: { ...state.filter, label: null },
      ui: {
        ...state.ui,
        showBoardWindow: true,
        activeTab: 'board',
        expandedCardId: null,
        focusedCardIndex: -1,
        feedback: null,
      },
    }));
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders the skeleton when isExtracting is true and the real card after it flips', async () => {
    const errors: unknown[][] = [];
    const errorSpy = vi.spyOn(console, 'error').mockImplementation((...args) => {
      errors.push(args);
    });

    useStore.setState({
      reminders: [
        {
          id: 'r-1',
          title: 'Half-extracted reminder',
          description: '',
          priority: 'medium',
          labels: [],
          createdAt: '2026-04-27T10:00:00.000Z',
          archivedAt: null,
          isExtracting: true,
        },
      ],
    });

    const { container } = renderBoard();

    // Skeleton path: a `.card--skeleton` is in the DOM, real card title isn't.
    expect(container.querySelector('.card--skeleton')).toBeTruthy();
    expect(screen.queryByText('Half-extracted reminder')).not.toBeInTheDocument();

    // Flip isExtracting → false. Same reminder.id; with the fix this unmounts
    // the skeleton and mounts the real card — separate component instances.
    await act(async () => {
      useStore.setState((state) => ({
        reminders: state.reminders.map((r) =>
          r.id === 'r-1' ? { ...r, isExtracting: false } : r,
        ),
      }));
    });

    // Real card now rendered.
    expect(screen.getByText('Half-extracted reminder')).toBeInTheDocument();
    expect(container.querySelector('.card--skeleton')).toBeFalsy();

    // No "Rendered more hooks than during the previous render" warning,
    // and no "Rules of Hooks" warning, fired during the transition.
    const hookViolation = errors.some((args) =>
      args.some(
        (a) =>
          typeof a === 'string' &&
          (a.includes('Rendered more hooks') ||
            a.includes('Rules of Hooks') ||
            a.includes('change in the order of Hooks')),
      ),
    );
    expect(hookViolation).toBe(false);

    errorSpy.mockRestore();
  });
});
