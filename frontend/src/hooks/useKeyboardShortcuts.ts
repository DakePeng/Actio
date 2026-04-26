import { useEffect, useCallback, useState } from 'react';
import { useStore } from '../store/use-store';
import { primaryMod } from '../utils/platform';
import type { Tab } from '../types';

interface ShortcutMap {
  [action: string]: string;
}

function isInputFocused(): boolean {
  const el = document.activeElement;
  if (!el) return false;
  const tag = el.tagName.toLowerCase();
  return tag === 'input' || tag === 'textarea' || (el as HTMLElement).isContentEditable;
}

/** Normalize a `KeyboardEvent.key` value to the lowercased token used in
 *  shortcut combo strings. The spacebar's `key` is a literal " ", which
 *  would never match a "Space" combo without explicit translation. */
function normalizeKey(k: string): string {
  if (k === ' ') return 'space';
  return k.toLowerCase();
}

function matchesShortcut(e: KeyboardEvent, combo: string): boolean {
  const parts = combo.split('+').map((p) => p.trim().toLowerCase());
  const key = parts[parts.length - 1];
  // Tauri-plugin-global-shortcut and DOM events both use a single "primary"
  // modifier slot for Cmd-on-Mac / Win-on-Windows (KeyboardEvent.metaKey).
  // Treat all four aliases the user / recorder may have produced — meta, cmd,
  // command, super — as a single condition on metaKey.
  const needMeta = parts.some(
    (p) => p === 'meta' || p === 'cmd' || p === 'command' || p === 'super',
  );
  const needCtrl = parts.includes('ctrl') || parts.includes('control');
  const needShift = parts.includes('shift');
  const needAlt = parts.includes('alt') || parts.includes('option');

  return (
    normalizeKey(e.key) === key &&
    e.ctrlKey === needCtrl &&
    e.metaKey === needMeta &&
    e.shiftKey === needShift &&
    e.altKey === needAlt
  );
}

const DEFAULT_SHORTCUTS: ShortcutMap = {
  tab_board: `${primaryMod}+1`,
  tab_people: `${primaryMod}+2`,
  tab_live: `${primaryMod}+3`,
  tab_needs_review: `${primaryMod}+6`,
  tab_archive: `${primaryMod}+4`,
  tab_settings: `${primaryMod}+5`,
  card_up: 'ArrowUp',
  card_down: 'ArrowDown',
  card_expand: 'Enter',
  card_archive: 'Delete',
};

export function useKeyboardShortcuts() {
  const [shortcuts, setShortcuts] = useState<ShortcutMap>(DEFAULT_SHORTCUTS);

  const {
    ui,
    reminders,
    setActiveTab,
    setFocusedCard,
    setExpandedCard,
    archiveReminder,
    setNewReminderBar,
    setBoardWindow,
  } = useStore();

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      // Tab switching (always active, even with input focused)
      const tabMap: Record<string, Tab> = {
        tab_board: 'board',
        tab_people: 'people',
        tab_live: 'live',
        tab_needs_review: 'needs-review',
        tab_archive: 'archive',
        tab_settings: 'settings',
      };
      for (const [action, tab] of Object.entries(tabMap)) {
        if (shortcuts[action] && matchesShortcut(e, shortcuts[action])) {
          e.preventDefault();
          setActiveTab(tab);
          return;
        }
      }

      // Card navigation — only on Board tab, only when no input focused
      if (ui.activeTab !== 'board' || isInputFocused()) return;

      const activeReminders = reminders.filter((r) => !r.archivedAt);

      if (shortcuts.card_down && matchesShortcut(e, shortcuts.card_down)) {
        e.preventDefault();
        const next = ui.focusedCardIndex === null ? 0 : Math.min(ui.focusedCardIndex + 1, activeReminders.length - 1);
        setFocusedCard(next);
        return;
      }

      if (shortcuts.card_up && matchesShortcut(e, shortcuts.card_up)) {
        e.preventDefault();
        if (ui.focusedCardIndex === null) return;
        setFocusedCard(Math.max(ui.focusedCardIndex - 1, 0));
        return;
      }

      if (shortcuts.card_expand && matchesShortcut(e, shortcuts.card_expand) && ui.focusedCardIndex !== null) {
        e.preventDefault();
        const card = activeReminders[ui.focusedCardIndex];
        if (card) {
          setExpandedCard(ui.expandedCardId === card.id ? null : card.id);
        }
        return;
      }

      if (shortcuts.card_archive && matchesShortcut(e, shortcuts.card_archive) && ui.focusedCardIndex !== null) {
        e.preventDefault();
        const card = activeReminders[ui.focusedCardIndex];
        if (card) {
          archiveReminder(card.id);
          if (ui.focusedCardIndex >= activeReminders.length - 1) {
            setFocusedCard(Math.max(0, ui.focusedCardIndex - 1));
          }
        }
        return;
      }

      // Escape cascade
      if (e.key === 'Escape') {
        if (ui.showNewReminderBar) {
          setNewReminderBar(false);
        } else if (ui.expandedCardId) {
          setExpandedCard(null);
        } else if (ui.focusedCardIndex !== null) {
          setFocusedCard(null);
        } else {
          setBoardWindow(false);
        }
        return;
      }
    },
    [shortcuts, ui, reminders, setActiveTab, setFocusedCard, setExpandedCard, archiveReminder, setNewReminderBar, setBoardWindow],
  );

  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);

  return { shortcuts, setShortcuts };
}
