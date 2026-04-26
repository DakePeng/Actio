import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { useState } from 'react';
import { ConfirmDialog, useConfirm } from '../ConfirmDialog';

describe('ConfirmDialog focus management (ISSUES.md #53)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('warning tone autoFocuses the confirm button on open', async () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <ConfirmDialog
        open
        message="Continue?"
        confirmLabel="Continue"
        cancelLabel="Cancel"
        tone="warning"
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );
    const confirmBtn = await screen.findByRole('button', { name: 'Continue' });
    await waitFor(() => expect(document.activeElement).toBe(confirmBtn));
  });

  it('destructive tone autoFocuses the cancel button (Enter no longer fires destructive action)', async () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <ConfirmDialog
        open
        message="Delete forever?"
        confirmLabel="Delete"
        cancelLabel="Cancel"
        tone="destructive"
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );
    const cancelBtn = await screen.findByRole('button', { name: 'Cancel' });
    await waitFor(() => expect(document.activeElement).toBe(cancelBtn));

    // Enter on a destructive dialog hits cancel, not confirm.
    fireEvent.keyDown(document, { key: 'Enter' });
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it('Tab from confirm cycles back to cancel; Shift-Tab from cancel cycles to confirm', async () => {
    render(
      <ConfirmDialog
        open
        message="Proceed?"
        confirmLabel="Proceed"
        cancelLabel="Cancel"
        tone="warning"
        onConfirm={() => {}}
        onCancel={() => {}}
      />,
    );
    const confirmBtn = await screen.findByRole('button', { name: 'Proceed' });
    const cancelBtn = await screen.findByRole('button', { name: 'Cancel' });

    // Initial focus: confirm (warning tone).
    await waitFor(() => expect(document.activeElement).toBe(confirmBtn));

    // Tab from confirm (last in DOM order) wraps to cancel (first).
    fireEvent.keyDown(document, { key: 'Tab' });
    expect(document.activeElement).toBe(cancelBtn);

    // Shift-Tab from cancel wraps back to confirm.
    fireEvent.keyDown(document, { key: 'Tab', shiftKey: true });
    expect(document.activeElement).toBe(confirmBtn);
  });

  it('restores focus to the previously-focused element on close', async () => {
    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button
            type="button"
            onClick={() => setOpen(true)}
            data-testid="trigger"
          >
            Open
          </button>
          <ConfirmDialog
            open={open}
            message="Confirm?"
            confirmLabel="OK"
            cancelLabel="Cancel"
            tone="warning"
            onConfirm={() => setOpen(false)}
            onCancel={() => setOpen(false)}
          />
        </>
      );
    }
    render(<Harness />);
    const trigger = screen.getByTestId('trigger');
    trigger.focus();
    expect(document.activeElement).toBe(trigger);

    fireEvent.click(trigger);
    const okBtn = await screen.findByRole('button', { name: 'OK' });
    await waitFor(() => expect(document.activeElement).toBe(okBtn));

    fireEvent.click(okBtn);
    // After close, focus should be back on the trigger.
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it('Escape calls onCancel regardless of tone', () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <ConfirmDialog
        open
        message="Maybe?"
        confirmLabel="Yes"
        cancelLabel="No"
        tone="destructive"
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it('useConfirm resolves the promise with the confirm/cancel result', async () => {
    function Harness() {
      const { confirm, dialogProps } = useConfirm();
      const [result, setResult] = useState<string>('idle');
      return (
        <>
          <button
            type="button"
            onClick={async () => {
              const ok = await confirm('Continue?', {
                confirmLabel: 'Yes',
                cancelLabel: 'No',
              });
              setResult(ok ? 'confirmed' : 'cancelled');
            }}
          >
            Ask
          </button>
          <div data-testid="result">{result}</div>
          <ConfirmDialog {...dialogProps} />
        </>
      );
    }
    render(<Harness />);
    fireEvent.click(screen.getByRole('button', { name: 'Ask' }));
    fireEvent.click(await screen.findByRole('button', { name: 'No' }));
    await waitFor(() =>
      expect(screen.getByTestId('result')).toHaveTextContent('cancelled'),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Ask' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Yes' }));
    await waitFor(() =>
      expect(screen.getByTestId('result')).toHaveTextContent('confirmed'),
    );
  });
});
