import { act, renderHook } from '@testing-library/react';
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

import { useStore, useFilteredReminders } from '../use-store';

/** Pins ISS-088: useFilteredReminders must return a stable reference
 *  across re-renders when neither `state.reminders` nor `state.filter`
 *  changed. The Board re-renders many times per second on j/k keyboard
 *  nav (each one bumps `focusedCardIndex`); without memoization, the
 *  filter would rebuild O(N) per keystroke. */
describe('useFilteredReminders memoization (ISS-088)', () => {
  beforeEach(() => {
    Object.values(mockClient).forEach((fn) => fn.mockReset());
    useStore.setState({
      reminders: [
        {
          id: 'r-1',
          title: 'A',
          description: '',
          priority: 'medium',
          labels: [],
          createdAt: '2026-04-27T10:00:00.000Z',
          archivedAt: null,
        },
        {
          id: 'r-2',
          title: 'B',
          description: '',
          priority: 'high',
          labels: [],
          createdAt: '2026-04-27T11:00:00.000Z',
          archivedAt: null,
        },
      ],
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns the same array reference when only an unrelated UI field changes', () => {
    const { result, rerender } = renderHook(() => useFilteredReminders());
    const first = result.current;

    // Force a Board-equivalent re-render via an unrelated UI field.
    // Without memoization, useFilteredReminders would build a new array
    // every render and `first === second` would be false.
    act(() => {
      useStore.setState((state) => ({
        ui: { ...state.ui, focusedCardIndex: 1 },
      }));
    });
    rerender();
    const second = result.current;

    expect(second).toBe(first);

    // Bump the same unrelated field a few more times — still stable.
    act(() => {
      useStore.setState((state) => ({
        ui: { ...state.ui, focusedCardIndex: 2 },
      }));
    });
    rerender();
    expect(result.current).toBe(first);
  });

  it('returns a NEW reference when reminders mutate', () => {
    const { result, rerender } = renderHook(() => useFilteredReminders());
    const first = result.current;

    act(() => {
      useStore.setState((state) => ({
        reminders: [
          ...state.reminders,
          {
            id: 'r-3',
            title: 'C',
            description: '',
            priority: 'low',
            labels: [],
            createdAt: '2026-04-27T12:00:00.000Z',
            archivedAt: null,
          },
        ],
      }));
    });
    rerender();
    expect(result.current).not.toBe(first);
    expect(result.current.length).toBe(3);
  });

  it('returns a NEW reference when filter changes', () => {
    const { result, rerender } = renderHook(() => useFilteredReminders());
    const first = result.current;

    act(() => {
      useStore.getState().setFilter({ priority: 'high' });
    });
    rerender();
    expect(result.current).not.toBe(first);
    expect(result.current.map((r) => r.id)).toEqual(['r-2']);
  });
});
