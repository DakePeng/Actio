import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { SwipeActionCoordinatorProvider } from '../SwipeActionCoordinator';
import { SwipeActionRow } from '../SwipeActionRow';

function renderRow(onDelete = vi.fn(), onEdit = vi.fn()) {
  return render(
    <SwipeActionCoordinatorProvider>
      <SwipeActionRow
        rowId="row-1"
        leftAction={{
          label: 'Delete',
          confirmLabel: 'Tap again to confirm',
          onExecute: onDelete,
          destructive: true,
        }}
        rightAction={{
          label: 'Edit',
          confirmLabel: 'Tap again to edit',
          onExecute: onEdit,
        }}
      >
        <div>row content</div>
      </SwipeActionRow>
    </SwipeActionCoordinatorProvider>,
  );
}

describe('SwipeActionRow', () => {
  it('reveals a left action and requires two clicks to execute it', async () => {
    const onDelete = vi.fn();
    renderRow(onDelete);

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Reveal delete action' }));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    });
    expect(screen.getByRole('button', { name: 'Tap again to confirm' })).toBeInTheDocument();
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Tap again to confirm' }));
    });
    expect(onDelete).toHaveBeenCalledTimes(1);
  });

  it('closes the first row when another row opens', () => {
    render(
      <SwipeActionCoordinatorProvider>
        <SwipeActionRow
          rowId="row-1"
          leftAction={{
            label: 'Delete',
            confirmLabel: 'Tap again to confirm',
            onExecute: vi.fn(),
            destructive: true,
          }}
          rightAction={{
            label: 'Edit',
            confirmLabel: 'Tap again to edit',
            onExecute: vi.fn(),
          }}
        >
          <div>first</div>
        </SwipeActionRow>
        <SwipeActionRow
          rowId="row-2"
          leftAction={{
            label: 'Delete',
            confirmLabel: 'Tap again to confirm',
            onExecute: vi.fn(),
            destructive: true,
          }}
          rightAction={{
            label: 'Edit',
            confirmLabel: 'Tap again to edit',
            onExecute: vi.fn(),
          }}
        >
          <div>second</div>
        </SwipeActionRow>
      </SwipeActionCoordinatorProvider>,
    );

    fireEvent.click(screen.getAllByRole('button', { name: 'Reveal edit action' })[0]);
    fireEvent.click(screen.getAllByRole('button', { name: 'Reveal edit action' })[1]);
    expect(screen.getAllByRole('button', { name: 'Edit' })).toHaveLength(1);
  });

  it('supports keyboard reveal and confirmation', async () => {
    const onDelete = vi.fn();
    renderRow(onDelete);

    await act(async () => {
      fireEvent.keyDown(screen.getByText('row content'), { key: 'Delete' });
    });
    await act(async () => {
      fireEvent.keyDown(screen.getByText('row content'), { key: 'Enter' });
    });
    await act(async () => {
      fireEvent.keyDown(screen.getByText('row content'), { key: 'Enter' });
    });
    expect(onDelete).toHaveBeenCalledTimes(1);
  });
});
