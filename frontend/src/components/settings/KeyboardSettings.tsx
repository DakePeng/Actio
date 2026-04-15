import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

const API_BASE = 'http://127.0.0.1:3000';

type ShortcutMap = Record<string, string>;

const DEFAULT_SHORTCUTS: ShortcutMap = {
  // Global
  toggle_board_tray: 'Ctrl+\\',
  start_dictation: 'Ctrl+Shift+Space',
  new_todo: 'Ctrl+N',
  // Tab navigation
  tab_board: 'Ctrl+1',
  tab_people: 'Ctrl+2',
  tab_recording: 'Ctrl+3',
  tab_archive: 'Ctrl+4',
  tab_settings: 'Ctrl+5',
  // Card navigation
  card_up: 'ArrowUp',
  card_down: 'ArrowDown',
  card_expand: 'Enter',
  card_archive: 'Delete',
};

const ACTION_LABELS: Record<string, string> = {
  toggle_board_tray: 'Toggle board / tray',
  start_dictation: 'Start dictation',
  new_todo: 'New to-do',
  tab_board: 'Board tab',
  tab_people: 'People tab',
  tab_recording: 'Recording tab',
  tab_archive: 'Archive tab',
  tab_settings: 'Settings tab',
  card_up: 'Card up',
  card_down: 'Card down',
  card_expand: 'Expand card',
  card_archive: 'Archive card',
};

const GLOBAL_ACTIONS = new Set(['toggle_board_tray', 'start_dictation', 'new_todo']);

function isTauri(): boolean {
  return typeof window !== 'undefined' && !!(window as any).__TAURI__;
}

async function fetchShortcuts(): Promise<ShortcutMap> {
  const res = await fetch(`${API_BASE}/settings`);
  if (!res.ok) throw new Error('Failed to fetch settings');
  const data = await res.json();
  return { ...DEFAULT_SHORTCUTS, ...(data.keyboard?.shortcuts ?? {}) };
}

async function patchShortcut(action: string, combo: string): Promise<void> {
  const res = await fetch(`${API_BASE}/settings`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ keyboard: { shortcuts: { [action]: combo } } }),
  });
  if (!res.ok) throw new Error('Failed to save shortcut');
}

async function patchAllShortcuts(shortcuts: ShortcutMap): Promise<void> {
  const res = await fetch(`${API_BASE}/settings`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ keyboard: { shortcuts } }),
  });
  if (!res.ok) throw new Error('Failed to save shortcuts');
}

export function KeyboardSettings() {
  const [shortcuts, setShortcuts] = useState<ShortcutMap>(DEFAULT_SHORTCUTS);
  const [editing, setEditing] = useState<string | null>(null);
  const [pendingCombo, setPendingCombo] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetchShortcuts()
      .then(setShortcuts)
      .catch(() => {});
  }, []);

  const saveShortcut = async (action: string, combo: string) => {
    setError(null);
    try {
      await patchShortcut(action, combo);
      const newShortcuts = { ...shortcuts, [action]: combo };
      setShortcuts(newShortcuts);
      setEditing(null);
      if (isTauri()) {
        invoke('reregister_shortcuts', { shortcuts: newShortcuts }).catch(console.error);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to save shortcut');
    }
  };

  const resetDefaults = async () => {
    setError(null);
    try {
      await patchAllShortcuts(DEFAULT_SHORTCUTS);
      setShortcuts(DEFAULT_SHORTCUTS);
      setEditing(null);
      if (isTauri()) {
        invoke('reregister_shortcuts', { shortcuts: DEFAULT_SHORTCUTS }).catch(console.error);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to reset shortcuts');
    }
  };

  const startEditing = (action: string) => {
    setEditing(action);
    setPendingCombo(shortcuts[action] ?? '');
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    e.preventDefault();
    const parts: string[] = [];
    if (e.ctrlKey) parts.push('Ctrl');
    if (e.shiftKey) parts.push('Shift');
    if (e.altKey) parts.push('Alt');
    if (e.metaKey) parts.push('Meta');

    const key = e.key;
    const isModifierOnly = ['Control', 'Shift', 'Alt', 'Meta'].includes(key);
    if (!isModifierOnly) {
      parts.push(key.length === 1 ? key.toUpperCase() : key);
    }

    if (parts.length > 0) {
      setPendingCombo(parts.join('+'));
    }
  };

  const groups = [
    {
      label: 'Global shortcuts',
      actions: ['toggle_board_tray', 'start_dictation', 'new_todo'],
    },
    {
      label: 'Tab navigation',
      actions: ['tab_board', 'tab_people', 'tab_recording', 'tab_archive', 'tab_settings'],
    },
    {
      label: 'Card navigation',
      actions: ['card_up', 'card_down', 'card_expand', 'card_archive'],
    },
  ];

  return (
    <section className="settings-section">
      <div className="settings-section__title">Keyboard Shortcuts</div>

      {groups.map((group) => (
        <div key={group.label}>
          <div className="settings-row__sublabel" style={{ marginTop: '0.5rem', fontWeight: 600 }}>
            {group.label}
          </div>
          {group.actions.map((action) => (
            <label key={action} className="settings-row">
              <span className="settings-row__label">
                {ACTION_LABELS[action] ?? action}
                {GLOBAL_ACTIONS.has(action) && (
                  <span className="settings-row__sublabel"> (global)</span>
                )}
              </span>
              {editing === action ? (
                <span style={{ display: 'flex', gap: '0.5rem', alignItems: 'center' }}>
                  <input
                    className="settings-row__select"
                    style={{ width: 160, textAlign: 'center', cursor: 'text' }}
                    readOnly
                    autoFocus
                    value={pendingCombo || 'Press keys…'}
                    onKeyDown={handleKeyDown}
                    onBlur={() => setEditing(null)}
                    placeholder="Press keys…"
                  />
                  <button
                    className="btn btn--small"
                    onMouseDown={(e) => {
                      e.preventDefault();
                      if (pendingCombo) saveShortcut(action, pendingCombo);
                    }}
                  >
                    Save
                  </button>
                  <button
                    className="btn btn--small btn--ghost"
                    onMouseDown={(e) => {
                      e.preventDefault();
                      setEditing(null);
                    }}
                  >
                    Cancel
                  </button>
                </span>
              ) : (
                <button
                  className="settings-row__select"
                  style={{ cursor: 'pointer', textAlign: 'center' }}
                  onClick={() => startEditing(action)}
                >
                  {shortcuts[action] ?? '—'}
                </button>
              )}
            </label>
          ))}
        </div>
      ))}

      {error && (
        <div
          className="settings-row__sublabel"
          style={{ color: 'var(--color-priority-high-text)', marginTop: '0.25rem' }}
        >
          {error}
        </div>
      )}

      <div className="settings-row" style={{ marginTop: '0.75rem' }}>
        <span />
        <button className="btn btn--small btn--ghost" onClick={resetDefaults}>
          Reset to defaults
        </button>
      </div>
    </section>
  );
}
