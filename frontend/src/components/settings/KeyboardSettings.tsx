import { useEffect, useState } from 'react';
import { useT, type TKey } from '../../i18n';
import { primaryMod } from '../../utils/platform';
import { getApiUrl } from '../../api/backend-url';

type ShortcutMap = Record<string, string>;

// Fallback values shown briefly while GET /settings is in flight. The backend
// is the source of truth — these only matter for the first paint.
const DEFAULT_SHORTCUTS: ShortcutMap = {
  // Global
  toggle_board_tray: `${primaryMod}+\\`,
  start_dictation: `${primaryMod}+Shift+Space`,
  new_todo: `${primaryMod}+N`,
  toggle_listening: `${primaryMod}+Shift+M`,
  // Tab navigation
  tab_board: `${primaryMod}+1`,
  tab_people: `${primaryMod}+2`,
  tab_live: `${primaryMod}+3`,
  tab_needs_review: `${primaryMod}+6`,
  tab_archive: `${primaryMod}+4`,
  tab_settings: `${primaryMod}+5`,
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
  tab_needs_review: 'settings.shortcuts.action.tab_needs_review',
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
  const res = await fetch(await getApiUrl('/settings'));
  if (!res.ok) throw new Error('Failed to fetch settings');
  const data = await res.json();
  return { ...DEFAULT_SHORTCUTS, ...(data.keyboard?.shortcuts ?? {}) };
}

async function patchShortcut(action: string, combo: string): Promise<void> {
  const res = await fetch(await getApiUrl('/settings'), {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ keyboard: { shortcuts: { [action]: combo } } }),
  });
  if (!res.ok) throw new Error('Failed to save shortcut');
}

async function patchAllShortcuts(shortcuts: ShortcutMap): Promise<void> {
  const res = await fetch(await getApiUrl('/settings'), {
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
        const { invoke } = await import('@tauri-apps/api/core');
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
        const { invoke } = await import('@tauri-apps/api/core');
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
      // Spacebar reports `key === " "`, which renders as an invisible token
      // in the saved combo. Substitute "Space" so the persisted string and
      // the in-process matcher agree.
      const named = key === ' ' ? 'Space' : key;
      parts.push(named.length === 1 ? named.toUpperCase() : named);
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
      actions: ['tab_board', 'tab_people', 'tab_live', 'tab_needs_review', 'tab_archive', 'tab_settings'],
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
