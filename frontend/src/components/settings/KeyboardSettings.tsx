import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useT, type TKey } from '../../i18n';

const API_BASE = 'http://127.0.0.1:3000';

type ShortcutMap = Record<string, string>;

const DEFAULT_SHORTCUTS: ShortcutMap = {
  // Global
  toggle_board_tray: 'Ctrl+\\',
  start_dictation: 'Ctrl+Shift+Space',
  new_todo: 'Ctrl+N',
  toggle_listening: 'Ctrl+Shift+M',
  // Tab navigation
  tab_board: 'Ctrl+1',
  tab_people: 'Ctrl+2',
  tab_live: 'Ctrl+3',
  tab_archive: 'Ctrl+4',
  tab_settings: 'Ctrl+5',
  // Card navigation
  card_up: 'ArrowUp',
  card_down: 'ArrowDown',
  card_expand: 'Enter',
  card_archive: 'Delete',
};

const ACTION_LABEL_KEYS: Record<string, TKey> = {
  toggle_board_tray: 'settings.shortcuts.action.toggle_board_tray',
  start_dictation: 'settings.shortcuts.action.start_dictation',
  new_todo: 'settings.shortcuts.action.new_todo',
  toggle_listening: 'settings.shortcuts.action.toggle_listening',
  tab_board: 'settings.shortcuts.action.tab_board',
  tab_people: 'settings.shortcuts.action.tab_people',
  tab_live: 'settings.shortcuts.action.tab_live',
  tab_archive: 'settings.shortcuts.action.tab_archive',
  tab_settings: 'settings.shortcuts.action.tab_settings',
  card_up: 'settings.shortcuts.action.card_up',
  card_down: 'settings.shortcuts.action.card_down',
  card_expand: 'settings.shortcuts.action.card_expand',
  card_archive: 'settings.shortcuts.action.card_archive',
};

const GLOBAL_ACTIONS = new Set(['toggle_board_tray', 'start_dictation', 'new_todo', 'toggle_listening']);

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
  const t = useT();

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
      setError(e instanceof Error ? e.message : t('settings.shortcuts.saveFailed'));
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
      setError(e instanceof Error ? e.message : t('settings.shortcuts.resetFailed'));
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

  const groups: { labelKey: TKey; actions: string[] }[] = [
    {
      labelKey: 'settings.shortcuts.group.global',
      actions: ['toggle_board_tray', 'start_dictation', 'new_todo', 'toggle_listening'],
    },
    {
      labelKey: 'settings.shortcuts.group.tab',
      actions: ['tab_board', 'tab_people', 'tab_live', 'tab_archive', 'tab_settings'],
    },
    {
      labelKey: 'settings.shortcuts.group.card',
      actions: ['card_up', 'card_down', 'card_expand', 'card_archive'],
    },
  ];

  return (
    <section className="settings-section">
      <div className="settings-section__title">{t('settings.shortcuts.title')}</div>

      {groups.map((group) => (
        <div key={group.labelKey}>
          <div className="settings-row__sublabel" style={{ marginTop: '0.5rem', fontWeight: 600 }}>
            {t(group.labelKey)}
          </div>
          {group.actions.map((action) => {
            const labelKey = ACTION_LABEL_KEYS[action];
            return (
              <label key={action} className="settings-row">
                <span className="settings-row__label">
                  {labelKey ? t(labelKey) : action}
                  {GLOBAL_ACTIONS.has(action) && (
                    <span className="settings-row__sublabel">
                      {t('settings.shortcuts.globalSuffix')}
                    </span>
                  )}
                </span>
                {editing === action ? (
                  <span style={{ display: 'flex', gap: '0.5rem', alignItems: 'center' }}>
                    <input
                      className="settings-row__select"
                      style={{ width: 160, textAlign: 'center', cursor: 'text' }}
                      readOnly
                      autoFocus
                      value={pendingCombo || t('settings.shortcuts.pressKeys')}
                      onKeyDown={handleKeyDown}
                      onBlur={() => setEditing(null)}
                      placeholder={t('settings.shortcuts.pressKeys')}
                    />
                    <button
                      className="btn btn--small"
                      onMouseDown={(e) => {
                        e.preventDefault();
                        if (pendingCombo) saveShortcut(action, pendingCombo);
                      }}
                    >
                      {t('settings.shortcuts.save')}
                    </button>
                    <button
                      className="btn btn--small btn--ghost"
                      onMouseDown={(e) => {
                        e.preventDefault();
                        setEditing(null);
                      }}
                    >
                      {t('settings.shortcuts.cancel')}
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
            );
          })}
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
          {t('settings.shortcuts.reset')}
        </button>
      </div>
    </section>
  );
}
