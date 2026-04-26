import { useCallback, useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';

export type ConfirmTone = 'warning' | 'destructive';

export interface ConfirmDialogProps {
  open: boolean;
  message: string;
  confirmLabel: string;
  cancelLabel: string;
  tone?: ConfirmTone;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * In-app confirmation modal. Replaces `window.confirm()` so destructive
 * actions render the same dialog on every platform — `window.confirm()`
 * is unstyled, looks OS-native against the rest of the Tailwind UI, and
 * on some Linux WebKitGTK builds silently returns false without showing
 * a prompt at all (see ISSUES.md #43).
 */
export function ConfirmDialog({
  open,
  message,
  confirmLabel,
  cancelLabel,
  tone = 'warning',
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const cancelBtnRef = useRef<HTMLButtonElement | null>(null);
  const confirmBtnRef = useRef<HTMLButtonElement | null>(null);
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);

  // Focus management: capture the previously-focused element when the dialog
  // opens, move focus to the safer button (cancel for destructive tones, so
  // an accidental Enter doesn't fire the destructive action), and restore
  // focus to the original element on close. Without this, keyboard users
  // get dropped on <body> after dismissing the dialog.
  const isDestructive = tone === 'destructive';
  useEffect(() => {
    if (!open) return;
    previouslyFocusedRef.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    const target = isDestructive ? cancelBtnRef.current : confirmBtnRef.current;
    target?.focus();
    return () => {
      previouslyFocusedRef.current?.focus();
    };
  }, [open, isDestructive]);

  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onCancel();
      } else if (e.key === 'Enter') {
        e.preventDefault();
        // Enter triggers whichever action is the safer default for this
        // tone — confirm for warnings, cancel for destructive prompts.
        if (isDestructive) {
          onCancel();
        } else {
          onConfirm();
        }
      } else if (e.key === 'Tab') {
        // Trap focus between the two buttons. Without this, Tab leaves the
        // modal even though aria-modal="true" claims the rest of the page
        // is inert.
        const cancelBtn = cancelBtnRef.current;
        const confirmBtn = confirmBtnRef.current;
        if (!cancelBtn || !confirmBtn) return;
        const active = document.activeElement;
        if (e.shiftKey) {
          if (active === cancelBtn) {
            e.preventDefault();
            confirmBtn.focus();
          }
        } else {
          if (active === confirmBtn) {
            e.preventDefault();
            cancelBtn.focus();
          }
        }
      }
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [open, onConfirm, onCancel, isDestructive]);

  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          className="confirm-dialog__backdrop"
          role="dialog"
          aria-modal="true"
          aria-describedby="confirm-dialog__message"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12 }}
          onClick={onCancel}
        >
          <motion.div
            className="confirm-dialog"
            initial={{ opacity: 0, scale: 0.96 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.96 }}
            transition={{ duration: 0.12 }}
            onClick={(e) => e.stopPropagation()}
          >
            <p id="confirm-dialog__message" className="confirm-dialog__message">
              {message}
            </p>
            <div className="confirm-dialog__actions">
              <button
                ref={cancelBtnRef}
                type="button"
                className="confirm-dialog__btn confirm-dialog__btn--cancel"
                onClick={onCancel}
              >
                {cancelLabel}
              </button>
              <button
                ref={confirmBtnRef}
                type="button"
                className={`confirm-dialog__btn confirm-dialog__btn--${tone}`}
                onClick={onConfirm}
              >
                {confirmLabel}
              </button>
            </div>
          </motion.div>
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}

interface ConfirmOptions {
  confirmLabel: string;
  cancelLabel: string;
  tone?: ConfirmTone;
}

/**
 * Promise-based wrapper around `<ConfirmDialog>`. Hosts call `confirm()`
 * to await a yes/no decision, and spread `dialogProps` onto a single
 * `<ConfirmDialog>` instance somewhere in their tree.
 */
export function useConfirm() {
  const [open, setOpen] = useState(false);
  const [message, setMessage] = useState('');
  const [opts, setOpts] = useState<ConfirmOptions>({
    confirmLabel: '',
    cancelLabel: '',
    tone: 'warning',
  });
  const resolverRef = useRef<((v: boolean) => void) | null>(null);

  const confirm = useCallback(
    (msg: string, options: ConfirmOptions) => {
      setMessage(msg);
      setOpts({
        confirmLabel: options.confirmLabel,
        cancelLabel: options.cancelLabel,
        tone: options.tone ?? 'warning',
      });
      setOpen(true);
      return new Promise<boolean>((resolve) => {
        resolverRef.current = resolve;
      });
    },
    [],
  );

  const close = useCallback((result: boolean) => {
    setOpen(false);
    const resolver = resolverRef.current;
    resolverRef.current = null;
    resolver?.(result);
  }, []);

  const onConfirm = useCallback(() => close(true), [close]);
  const onCancel = useCallback(() => close(false), [close]);

  return {
    confirm,
    dialogProps: {
      open,
      message,
      confirmLabel: opts.confirmLabel,
      cancelLabel: opts.cancelLabel,
      tone: opts.tone,
      onConfirm,
      onCancel,
    } satisfies ConfirmDialogProps,
  };
}
