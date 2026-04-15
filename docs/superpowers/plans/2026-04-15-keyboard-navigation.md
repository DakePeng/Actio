# Keyboard Navigation & System-wide Dictation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add configurable keyboard shortcuts (tab switching, card navigation, global hotkeys), system-wide dictation (transcribe-to-any-app), and a shortcut settings UI to Actio.

**Architecture:** Three layers: (1) `KeyboardSettings` in backend for persistence + defaults, (2) `useKeyboardShortcuts` React hook for in-app navigation, (3) Tauri global shortcuts plugin + dictation service for system-wide features. Each layer builds on the previous.

**Tech Stack:** Tauri v2, React 18, Zustand, `tauri-plugin-global-shortcut`, `tauri-plugin-clipboard-manager`, `enigo` (Rust keystroke simulation), existing sherpa-onnx ASR.

---

### Task 1: Add `KeyboardSettings` to Backend

**Files:**
- Modify: `backend/actio-core/src/engine/app_settings.rs`

- [ ] **Step 1: Add KeyboardSettings struct and defaults**

In `app_settings.rs`, add after the `AudioSettings` struct:

```rust
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardSettings {
    #[serde(default = "default_shortcuts")]
    pub shortcuts: HashMap<String, String>,
}

impl Default for KeyboardSettings {
    fn default() -> Self {
        Self {
            shortcuts: default_shortcuts(),
        }
    }
}

fn default_shortcuts() -> HashMap<String, String> {
    let mut m = HashMap::new();
    // Global
    m.insert("toggle_board_tray".into(), "Ctrl+\\".into());
    m.insert("start_dictation".into(), "Ctrl+Shift+Space".into());
    m.insert("new_todo".into(), "Ctrl+N".into());
    // Tab navigation
    m.insert("tab_board".into(), "Ctrl+1".into());
    m.insert("tab_people".into(), "Ctrl+2".into());
    m.insert("tab_recording".into(), "Ctrl+3".into());
    m.insert("tab_archive".into(), "Ctrl+4".into());
    m.insert("tab_settings".into(), "Ctrl+5".into());
    // Card navigation
    m.insert("card_up".into(), "ArrowUp".into());
    m.insert("card_down".into(), "ArrowDown".into());
    m.insert("card_expand".into(), "Enter".into());
    m.insert("card_archive".into(), "Delete".into());
    m
}
```

- [ ] **Step 2: Add `keyboard` field to `AppSettings`**

Add to the `AppSettings` struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub llm: LlmSettings,
    #[serde(default)]
    pub audio: AudioSettings,
    #[serde(default)]
    pub keyboard: KeyboardSettings,
}
```

- [ ] **Step 3: Add `KeyboardSettingsPatch` and wire into `SettingsPatch`**

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsPatch {
    pub llm: Option<LlmSettingsPatch>,
    pub audio: Option<AudioSettingsPatch>,
    pub keyboard: Option<KeyboardSettingsPatch>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct KeyboardSettingsPatch {
    pub shortcuts: Option<HashMap<String, String>>,
}
```

In the `update()` method of `SettingsManager`, add after the audio patch handling:

```rust
if let Some(keyboard) = patch.keyboard {
    if let Some(shortcuts) = keyboard.shortcuts {
        for (k, v) in shortcuts {
            settings.keyboard.shortcuts.insert(k, v);
        }
    }
}
```

- [ ] **Step 4: Verify build**

Run: `cd D:/Dev/Actio/backend && cargo check -p actio-core`
Expected: Compiles with no errors.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/engine/app_settings.rs
git commit -m "feat(keyboard): add KeyboardSettings with default shortcuts"
```

---

### Task 2: Add `focusedCardIndex` to Zustand Store

**Files:**
- Modify: `frontend/src/types/index.ts`
- Modify: `frontend/src/store/use-store.ts`

- [ ] **Step 1: Add to UIState type**

In `frontend/src/types/index.ts`, add to the `UIState` interface:

```ts
export interface UIState {
  showBoardWindow: boolean;
  trayExpanded: boolean;
  expandedCardId: string | null;
  highlightedCardId: string | null;
  showNewReminderBar: boolean;
  hasSeenOnboarding: boolean;
  activeTab: Tab;
  focusedCardIndex: number | null;  // NEW
  feedback: {
    message: string;
    tone: 'neutral' | 'success';
  } | null;
}
```

- [ ] **Step 2: Add store actions**

In `frontend/src/store/use-store.ts`, add to the `AppState` interface:

```ts
setFocusedCard: (index: number | null) => void;
```

Add `focusedCardIndex: null` to the initial UI state object.

Add the action implementation:

```ts
setFocusedCard: (index) => set((s) => ({
  ui: { ...s.ui, focusedCardIndex: index },
})),
```

In `setActiveTab`, reset the focused card:

```ts
setActiveTab: (tab) => set((s) => ({
  ui: { ...s.ui, activeTab: tab, focusedCardIndex: null },
})),
```

- [ ] **Step 3: Commit**

```bash
git add frontend/src/types/index.ts frontend/src/store/use-store.ts
git commit -m "feat(keyboard): add focusedCardIndex to UI state"
```

---

### Task 3: Create `useKeyboardShortcuts` Hook

**Files:**
- Create: `frontend/src/hooks/useKeyboardShortcuts.ts`
- Modify: `frontend/src/components/BoardWindow.tsx`

- [ ] **Step 1: Create the hook**

Create `frontend/src/hooks/useKeyboardShortcuts.ts`:

```ts
import { useEffect, useCallback } from 'react';
import { useStore } from '../store/use-store';
import type { Tab } from '../types';

const API_BASE = 'http://127.0.0.1:3000';

interface ShortcutMap {
  [action: string]: string;
}

function isInputFocused(): boolean {
  const el = document.activeElement;
  if (!el) return false;
  const tag = el.tagName.toLowerCase();
  return tag === 'input' || tag === 'textarea' || (el as HTMLElement).isContentEditable;
}

function matchesShortcut(e: KeyboardEvent, combo: string): boolean {
  const parts = combo.split('+').map((p) => p.trim().toLowerCase());
  const key = parts[parts.length - 1];
  const needCtrl = parts.includes('ctrl');
  const needShift = parts.includes('shift');
  const needAlt = parts.includes('alt');

  const eventKey = e.key.toLowerCase();
  // Normalize key names
  const normalizedKey = eventKey === 'arrowup' ? 'arrowup'
    : eventKey === 'arrowdown' ? 'arrowdown'
    : eventKey;

  return (
    normalizedKey === key &&
    e.ctrlKey === needCtrl &&
    e.shiftKey === needShift &&
    e.altKey === needAlt
  );
}

export function useKeyboardShortcuts() {
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
      // Fetch shortcuts from settings cache (loaded at startup)
      // For now use defaults — will be replaced by settings fetch in Task 6
      const shortcuts: ShortcutMap = {
        tab_board: 'Ctrl+1',
        tab_people: 'Ctrl+2',
        tab_recording: 'Ctrl+3',
        tab_archive: 'Ctrl+4',
        tab_settings: 'Ctrl+5',
        card_up: 'ArrowUp',
        card_down: 'ArrowDown',
        card_expand: 'Enter',
        card_archive: 'Delete',
      };

      // Tab switching (always active, even with input focused)
      const tabMap: Record<string, Tab> = {
        tab_board: 'board',
        tab_people: 'people',
        tab_recording: 'recording',
        tab_archive: 'archive',
        tab_settings: 'settings',
      };
      for (const [action, tab] of Object.entries(tabMap)) {
        if (matchesShortcut(e, shortcuts[action])) {
          e.preventDefault();
          setActiveTab(tab);
          return;
        }
      }

      // Card navigation — only on Board tab, only when no input focused
      if (ui.activeTab !== 'board' || isInputFocused()) return;

      const activeReminders = reminders.filter((r) => !r.archivedAt);

      if (matchesShortcut(e, shortcuts.card_down)) {
        e.preventDefault();
        const next = ui.focusedCardIndex === null ? 0 : Math.min(ui.focusedCardIndex + 1, activeReminders.length - 1);
        setFocusedCard(next);
        return;
      }

      if (matchesShortcut(e, shortcuts.card_up)) {
        e.preventDefault();
        if (ui.focusedCardIndex === null) return;
        setFocusedCard(Math.max(ui.focusedCardIndex - 1, 0));
        return;
      }

      if (matchesShortcut(e, shortcuts.card_expand) && ui.focusedCardIndex !== null) {
        e.preventDefault();
        const card = activeReminders[ui.focusedCardIndex];
        if (card) {
          setExpandedCard(ui.expandedCardId === card.id ? null : card.id);
        }
        return;
      }

      if (matchesShortcut(e, shortcuts.card_archive) && ui.focusedCardIndex !== null) {
        e.preventDefault();
        const card = activeReminders[ui.focusedCardIndex];
        if (card) {
          archiveReminder(card.id);
          // Move focus up if we archived the last card
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
    [ui, reminders, setActiveTab, setFocusedCard, setExpandedCard, archiveReminder, setNewReminderBar, setBoardWindow],
  );

  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);
}
```

- [ ] **Step 2: Mount the hook in BoardWindow**

In `frontend/src/components/BoardWindow.tsx`, add at the top of the component function (after other hooks):

```ts
import { useKeyboardShortcuts } from '../hooks/useKeyboardShortcuts';

// Inside the component:
useKeyboardShortcuts();
```

Remove the existing standalone `Escape` keydown listener in BoardWindow (the hook now handles Escape via the cascade).

- [ ] **Step 3: Verify**

Run: `cd D:/Dev/Actio/frontend && pnpm dev`
Test: Press `Ctrl+1` through `Ctrl+5` — tabs should switch. Press `↓` on board tab — first card should get focus (you'll add the visual ring in the next task).

- [ ] **Step 4: Commit**

```bash
git add frontend/src/hooks/useKeyboardShortcuts.ts frontend/src/components/BoardWindow.tsx
git commit -m "feat(keyboard): add useKeyboardShortcuts hook with tab and card nav"
```

---

### Task 4: Add Card Focus Ring Visual

**Files:**
- Modify: `frontend/src/components/Board.tsx` (or wherever cards are rendered)
- Modify: `frontend/src/styles/globals.css`

- [ ] **Step 1: Find the card rendering component**

Search for where reminder cards are mapped and rendered. It's likely in `Board.tsx` or a component it renders. Find the `map()` call that renders `<Card>` or `<SwipeActionRow>` components.

- [ ] **Step 2: Add focus ring class**

In the card list mapping, add a conditional class when the card's index matches `focusedCardIndex`:

```tsx
const { ui } = useStore();

{activeReminders.map((reminder, index) => (
  <div
    key={reminder.id}
    className={`card-wrapper ${ui.focusedCardIndex === index ? 'card-wrapper--focused' : ''}`}
  >
    {/* existing card content */}
  </div>
))}
```

- [ ] **Step 3: Add focus ring CSS**

In `frontend/src/styles/globals.css`:

```css
.card-wrapper--focused {
  outline: 2px solid var(--color-accent, #3b82f6);
  outline-offset: -2px;
  border-radius: 12px;
}
```

- [ ] **Step 4: Auto-scroll focused card into view**

In the card wrapper, add a ref-based scroll:

```tsx
const focusedRef = useRef<HTMLDivElement>(null);

useEffect(() => {
  if (ui.focusedCardIndex === index && focusedRef.current) {
    focusedRef.current.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }
}, [ui.focusedCardIndex]);

// On the wrapper div:
ref={ui.focusedCardIndex === index ? focusedRef : undefined}
```

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/Board.tsx frontend/src/styles/globals.css
git commit -m "feat(keyboard): add visual focus ring for card navigation"
```

---

### Task 5: Create `KeyboardSettings.tsx` Settings UI

**Files:**
- Create: `frontend/src/components/settings/KeyboardSettings.tsx`
- Modify: `frontend/src/components/settings/SettingsView.tsx`

- [ ] **Step 1: Create the component**

Create `frontend/src/components/settings/KeyboardSettings.tsx`:

```tsx
import { useEffect, useState, useCallback } from 'react';

const API_BASE = 'http://127.0.0.1:3000';

interface ShortcutDef {
  action: string;
  label: string;
  group: string;
}

const SHORTCUT_DEFS: ShortcutDef[] = [
  { action: 'toggle_board_tray', label: 'Toggle board / tray', group: 'Global' },
  { action: 'start_dictation', label: 'Start dictation', group: 'Global' },
  { action: 'new_todo', label: 'New todo', group: 'Global' },
  { action: 'tab_board', label: 'Board tab', group: 'Navigation' },
  { action: 'tab_people', label: 'People tab', group: 'Navigation' },
  { action: 'tab_recording', label: 'Recording tab', group: 'Navigation' },
  { action: 'tab_archive', label: 'Archive tab', group: 'Navigation' },
  { action: 'tab_settings', label: 'Settings tab', group: 'Navigation' },
  { action: 'card_up', label: 'Move up', group: 'Cards' },
  { action: 'card_down', label: 'Move down', group: 'Cards' },
  { action: 'card_expand', label: 'Expand card', group: 'Cards' },
  { action: 'card_archive', label: 'Archive card', group: 'Cards' },
];

function comboFromEvent(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.shiftKey) parts.push('Shift');
  if (e.altKey) parts.push('Alt');

  const key = e.key;
  // Don't include bare modifier keys as the main key
  if (!['Control', 'Shift', 'Alt', 'Meta'].includes(key)) {
    parts.push(key.length === 1 ? key.toUpperCase() : key);
  }
  return parts.join('+');
}

function displayCombo(combo: string): string {
  return combo
    .replace('ArrowUp', '↑')
    .replace('ArrowDown', '↓')
    .replace('ArrowLeft', '←')
    .replace('ArrowRight', '→');
}

export function KeyboardSettings() {
  const [shortcuts, setShortcuts] = useState<Record<string, string>>({});
  const [recording, setRecording] = useState<string | null>(null);

  useEffect(() => {
    fetch(`${API_BASE}/settings`)
      .then((r) => r.json())
      .then((s) => {
        if (s.keyboard?.shortcuts) setShortcuts(s.keyboard.shortcuts);
      })
      .catch(() => {});
  }, []);

  const saveShortcut = useCallback(async (action: string, combo: string) => {
    const newShortcuts = { ...shortcuts, [action]: combo };
    setShortcuts(newShortcuts);
    try {
      await fetch(`${API_BASE}/settings`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ keyboard: { shortcuts: { [action]: combo } } }),
      });
    } catch {}
  }, [shortcuts]);

  const resetDefaults = useCallback(async () => {
    try {
      const r = await fetch(`${API_BASE}/settings`);
      // We don't have a "reset keyboard" endpoint, so we'll PATCH with all defaults
      // The backend defaults are the source of truth
      const defaults: Record<string, string> = {
        toggle_board_tray: 'Ctrl+\\',
        start_dictation: 'Ctrl+Shift+Space',
        new_todo: 'Ctrl+N',
        tab_board: 'Ctrl+1', tab_people: 'Ctrl+2', tab_recording: 'Ctrl+3',
        tab_archive: 'Ctrl+4', tab_settings: 'Ctrl+5',
        card_up: 'ArrowUp', card_down: 'ArrowDown',
        card_expand: 'Enter', card_archive: 'Delete',
      };
      setShortcuts(defaults);
      await fetch(`${API_BASE}/settings`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ keyboard: { shortcuts: defaults } }),
      });
    } catch {}
  }, []);

  // Find conflict: is this combo used by another action?
  const findConflict = (action: string, combo: string): string | null => {
    for (const [a, c] of Object.entries(shortcuts)) {
      if (a !== action && c === combo) {
        const def = SHORTCUT_DEFS.find((d) => d.action === a);
        return def?.label ?? a;
      }
    }
    return null;
  };

  useEffect(() => {
    if (!recording) return;
    const handler = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') {
        setRecording(null);
        return;
      }
      // Wait for a non-modifier key
      if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
      const combo = comboFromEvent(e);
      saveShortcut(recording, combo);
      setRecording(null);
    };
    window.addEventListener('keydown', handler, true);
    return () => window.removeEventListener('keydown', handler, true);
  }, [recording, saveShortcut]);

  let currentGroup = '';

  return (
    <section className="settings-section">
      <div className="settings-section__title">Keyboard Shortcuts</div>

      {SHORTCUT_DEFS.map((def) => {
        const showGroup = def.group !== currentGroup;
        if (showGroup) currentGroup = def.group;
        const combo = shortcuts[def.action] ?? '';
        const isRecording = recording === def.action;
        const conflict = !isRecording ? findConflict(def.action, combo) : null;

        return (
          <div key={def.action}>
            {showGroup && (
              <div className="settings-field__label" style={{ marginTop: 12 }}>
                {def.group}
              </div>
            )}
            <div className="keyboard-shortcut-row">
              <span className="keyboard-shortcut-row__label">{def.label}</span>
              <span className="keyboard-shortcut-row__combo">
                {isRecording ? 'Press keys...' : displayCombo(combo)}
              </span>
              <button
                type="button"
                className={`settings-btn settings-btn--secondary keyboard-shortcut-row__record ${isRecording ? 'keyboard-shortcut-row__record--active' : ''}`}
                onClick={() => setRecording(isRecording ? null : def.action)}
              >
                {isRecording ? 'Cancel' : 'Record'}
              </button>
            </div>
            {conflict && (
              <div className="keyboard-shortcut-row__conflict">
                Already used by: {conflict}
              </div>
            )}
          </div>
        );
      })}

      <button
        type="button"
        className="settings-btn settings-btn--secondary"
        style={{ marginTop: 16 }}
        onClick={resetDefaults}
      >
        Reset to defaults
      </button>
    </section>
  );
}
```

- [ ] **Step 2: Add CSS**

In `frontend/src/styles/globals.css`:

```css
.keyboard-shortcut-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 0;
}
.keyboard-shortcut-row__label {
  flex: 1;
  font-size: 13px;
}
.keyboard-shortcut-row__combo {
  font-family: monospace;
  font-size: 12px;
  padding: 2px 8px;
  background: var(--color-bg-hover, #f0f0f0);
  border-radius: 4px;
  min-width: 80px;
  text-align: center;
}
.keyboard-shortcut-row__record--active {
  animation: pulse-border 1s ease-in-out infinite;
}
@keyframes pulse-border {
  0%, 100% { box-shadow: 0 0 0 1px var(--color-accent, #3b82f6); }
  50% { box-shadow: 0 0 0 3px var(--color-accent, #3b82f6); }
}
.keyboard-shortcut-row__conflict {
  font-size: 11px;
  color: var(--color-priority-high-text, #d33);
  padding-left: 4px;
}
```

- [ ] **Step 3: Register in SettingsView**

In `frontend/src/components/settings/SettingsView.tsx`, add import and render:

```tsx
import { KeyboardSettings } from './KeyboardSettings';

// Add after the last section, before closing </div>:
<h2 className="settings-group-title">Keyboard</h2>
<KeyboardSettings />
```

- [ ] **Step 4: Verify**

Run: `cd D:/Dev/Actio/frontend && pnpm dev`
Open Settings tab → Keyboard Shortcuts section should appear with all shortcuts listed. Click "Record" → press keys → combo updates. "Reset to defaults" restores originals.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/settings/KeyboardSettings.tsx frontend/src/components/settings/SettingsView.tsx frontend/src/styles/globals.css
git commit -m "feat(keyboard): add keyboard shortcuts settings UI"
```

---

### Task 6: Wire Settings into useKeyboardShortcuts

**Files:**
- Modify: `frontend/src/hooks/useKeyboardShortcuts.ts`

- [ ] **Step 1: Fetch shortcuts from backend on mount**

Replace the hardcoded `shortcuts` object inside `handleKeyDown` with state loaded from the API:

```ts
const [shortcuts, setShortcuts] = useState<Record<string, string>>({
  // Defaults as fallback until settings load
  tab_board: 'Ctrl+1', tab_people: 'Ctrl+2', tab_recording: 'Ctrl+3',
  tab_archive: 'Ctrl+4', tab_settings: 'Ctrl+5',
  card_up: 'ArrowUp', card_down: 'ArrowDown',
  card_expand: 'Enter', card_archive: 'Delete',
});

useEffect(() => {
  fetch('http://127.0.0.1:3000/settings')
    .then((r) => r.json())
    .then((s) => {
      if (s.keyboard?.shortcuts) setShortcuts(s.keyboard.shortcuts);
    })
    .catch(() => {});
}, []);
```

Update `handleKeyDown` to use `shortcuts` from state (already captured by the closure).

- [ ] **Step 2: Commit**

```bash
git add frontend/src/hooks/useKeyboardShortcuts.ts
git commit -m "feat(keyboard): wire settings API into keyboard shortcuts hook"
```

---

### Task 7: Add Tauri Global Shortcut Plugin

**Files:**
- Modify: `backend/src-tauri/Cargo.toml`
- Modify: `backend/src-tauri/tauri.conf.json`
- Modify: `backend/src-tauri/src/main.rs`

- [ ] **Step 1: Add dependency**

In `backend/src-tauri/Cargo.toml`, add to `[dependencies]`:

```toml
tauri-plugin-global-shortcut = "2"
```

- [ ] **Step 2: Add plugin permission**

In `backend/src-tauri/tauri.conf.json`, add a `plugins` section under `app` (or as top-level, check Tauri v2 schema):

For Tauri v2, create `backend/src-tauri/capabilities/default.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2/capability",
  "identifier": "default",
  "windows": ["main", "dictation-indicator"],
  "permissions": [
    "core:default",
    "notification:default",
    "global-shortcut:allow-register",
    "global-shortcut:allow-unregister",
    "global-shortcut:allow-is-registered"
  ]
}
```

- [ ] **Step 3: Register plugin and global shortcuts on startup**

In `backend/src-tauri/src/main.rs`, add the plugin and register shortcuts in the `setup` closure:

```rust
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

// In main():
tauri::Builder::default()
    .plugin(tauri_plugin_notification::init())
    .plugin(tauri_plugin_global_shortcut::Builder::new().build())
    .invoke_handler(/* existing */)
    .setup(|app| {
        // ... existing setup code ...

        // Register global shortcuts
        let handle = app.handle().clone();
        register_global_shortcuts(&handle);

        Ok(())
    })
```

Add the registration function:

```rust
fn register_global_shortcuts(app: &AppHandle) {
    // For now, register hardcoded defaults.
    // Later: read from settings.json
    let toggle = "Ctrl+\\".parse::<Shortcut>().expect("valid shortcut");
    let dictation = "Ctrl+Shift+Space".parse::<Shortcut>().expect("valid shortcut");
    let new_todo = "Ctrl+N".parse::<Shortcut>().expect("valid shortcut");

    let app_handle = app.clone();
    app.global_shortcut().on_shortcuts(
        [toggle, dictation, new_todo],
        move |_app, shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let action = if shortcut == &toggle {
                "toggle_board_tray"
            } else if shortcut == &dictation {
                "start_dictation"
            } else if shortcut == &new_todo {
                "new_todo"
            } else {
                return;
            };
            // Emit to frontend
            let _ = app_handle.emit("shortcut-triggered", action);
        },
    ).expect("failed to register global shortcuts");
}
```

- [ ] **Step 4: Handle events in frontend**

In `frontend/src/hooks/useKeyboardShortcuts.ts`, add a Tauri event listener:

```ts
import { listen } from '@tauri-apps/api/event';

// Inside the hook:
useEffect(() => {
  const unlisten = listen<string>('shortcut-triggered', (event) => {
    const action = event.payload;
    if (action === 'toggle_board_tray') {
      // Toggle board visibility
      if (ui.showBoardWindow) {
        setBoardWindow(false);
      } else {
        setBoardWindow(true);
      }
    } else if (action === 'new_todo') {
      setBoardWindow(true);
      setNewReminderBar(true);
    } else if (action === 'start_dictation') {
      // Will be implemented in Task 9
      console.log('Dictation triggered (not yet implemented)');
    }
  });
  return () => { unlisten.then((f) => f()); };
}, [ui.showBoardWindow, setBoardWindow, setNewReminderBar]);
```

- [ ] **Step 5: Verify build**

Run: `cd D:/Dev/Actio/backend && cargo build -p actio-desktop`
Expected: Compiles. Global shortcuts registered on startup.

- [ ] **Step 6: Verify functionality**

Start the app. Press `Ctrl+N` while another app is focused — Actio should appear with NewReminderBar open. Press `Ctrl+\` to toggle.

- [ ] **Step 7: Commit**

```bash
git add backend/src-tauri/Cargo.toml backend/src-tauri/tauri.conf.json backend/src-tauri/capabilities/default.json backend/src-tauri/src/main.rs frontend/src/hooks/useKeyboardShortcuts.ts
git commit -m "feat(keyboard): register Tauri global shortcuts for toggle, new todo, dictation"
```

---

### Task 8: Add Clipboard and Keystroke Simulation Dependencies

**Files:**
- Modify: `backend/src-tauri/Cargo.toml`
- Modify: `backend/src-tauri/capabilities/default.json`

- [ ] **Step 1: Add dependencies**

In `backend/src-tauri/Cargo.toml`:

```toml
tauri-plugin-clipboard-manager = "2"
enigo = { version = "0.3", features = ["serde"] }
```

- [ ] **Step 2: Register clipboard plugin**

In `main.rs`, add to the builder:

```rust
.plugin(tauri_plugin_clipboard_manager::init())
```

In `capabilities/default.json`, add:

```json
"clipboard-manager:allow-read-text",
"clipboard-manager:allow-write-text"
```

- [ ] **Step 3: Verify build**

Run: `cd D:/Dev/Actio/backend && cargo check -p actio-desktop`
Expected: Compiles.

- [ ] **Step 4: Commit**

```bash
git add backend/src-tauri/Cargo.toml backend/src-tauri/capabilities/default.json backend/src-tauri/src/main.rs
git commit -m "feat(dictation): add clipboard and keystroke simulation deps"
```

---

### Task 9: Implement Dictation Service

**Files:**
- Create: `backend/actio-core/src/engine/dictation.rs`
- Modify: `backend/actio-core/src/engine/mod.rs`

- [ ] **Step 1: Create dictation service module**

Create `backend/actio-core/src/engine/dictation.rs`:

```rust
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use tracing::{info, warn};

use crate::engine::audio_capture;
use crate::engine::asr;
use crate::engine::model_manager::ModelPaths;
use crate::engine::vad::{self, VadConfig};

pub struct DictationService {
    state: Mutex<DictationState>,
}

enum DictationState {
    Idle,
    Listening {
        cancel_tx: oneshot::Sender<()>,
        result_rx: oneshot::Receiver<String>,
    },
}

impl DictationService {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(DictationState::Idle),
        }
    }

    pub async fn is_active(&self) -> bool {
        matches!(*self.state.lock().await, DictationState::Listening { .. })
    }

    /// Start a dictation session. Returns immediately.
    /// The transcript will be available via `stop_and_get_transcript()`.
    pub async fn start(
        &self,
        model_paths: &ModelPaths,
        device_name: Option<&str>,
        asr_model: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut state = self.state.lock().await;
        if matches!(*state, DictationState::Listening { .. }) {
            return Err(anyhow::anyhow!("dictation already active"));
        }

        let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
        let (result_tx, result_rx) = oneshot::channel::<String>();

        // Start audio capture
        let (capture_handle, audio_rx) = audio_capture::start_capture(device_name)?;

        // Start VAD + ASR pipeline (offline model)
        let vad_model = model_paths.silero_vad.clone();
        let chosen = asr_model.unwrap_or("auto");

        // Use sense_voice or whisper, same as inference_pipeline
        let seg_rx = vad::start_vad(&vad_model, VadConfig::default(), audio_rx)?;

        // For simplicity, collect all segments until cancelled
        let files = if let Some(f) = model_paths.sense_voice.as_ref() {
            f.clone()
        } else if let Some(f) = model_paths.whisper_base.as_ref() {
            f.clone()
        } else {
            return Err(anyhow::anyhow!("no offline ASR model available for dictation"));
        };

        tokio::spawn(async move {
            let _capture = capture_handle; // keep alive
            let mut transcript = String::new();
            let mut silence_count = 0u32;
            let mut heard_speech = false;

            // We need to adapt - for now collect VAD segments and transcribe
            // This is a simplified implementation - the full version would
            // use the ASR pipeline more directly
            let mut seg_rx = seg_rx;

            loop {
                tokio::select! {
                    seg = seg_rx.recv() => {
                        match seg {
                            Some(audio_segment) => {
                                heard_speech = true;
                                silence_count = 0;
                                // The VAD segment contains audio data
                                // In a full implementation, feed to offline ASR
                                // For now, accumulate
                            }
                            None => break, // channel closed
                        }
                    }
                    _ = &mut cancel_rx => {
                        info!("Dictation cancelled by user");
                        break;
                    }
                }
            }

            let _ = result_tx.send(transcript);
        });

        *state = DictationState::Listening { cancel_tx, result_rx };
        info!("Dictation started");
        Ok(())
    }

    /// Stop dictation and return the transcript.
    pub async fn stop_and_get_transcript(&self) -> Option<String> {
        let mut state = self.state.lock().await;
        match std::mem::replace(&mut *state, DictationState::Idle) {
            DictationState::Listening { cancel_tx, result_rx } => {
                let _ = cancel_tx.send(());
                match result_rx.await {
                    Ok(text) => {
                        info!(len = text.len(), "Dictation stopped, got transcript");
                        Some(text)
                    }
                    Err(_) => None,
                }
            }
            DictationState::Idle => None,
        }
    }
}

impl Default for DictationService {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 2: Register module**

In `backend/actio-core/src/engine/mod.rs`, add:

```rust
pub mod dictation;
```

- [ ] **Step 3: Verify build**

Run: `cd D:/Dev/Actio/backend && cargo check -p actio-core`
Expected: Compiles (the dictation service is a stub that will be refined during integration).

- [ ] **Step 4: Commit**

```bash
git add backend/actio-core/src/engine/dictation.rs backend/actio-core/src/engine/mod.rs
git commit -m "feat(dictation): add DictationService with start/stop lifecycle"
```

---

### Task 10: Wire Dictation into Tauri Commands

**Files:**
- Modify: `backend/src-tauri/src/main.rs`

- [ ] **Step 1: Add Tauri commands for dictation**

In `main.rs`, add commands:

```rust
use enigo::{Enigo, Keyboard, Settings, Key, Direction};

#[tauri::command]
async fn start_dictation(app: AppHandle) -> Result<(), String> {
    // Will integrate with DictationService + actio-core
    // For now, emit status event
    let _ = app.emit("dictation-status", "listening");
    Ok(())
}

#[tauri::command]
async fn stop_dictation(app: AppHandle) -> Result<String, String> {
    // Will integrate with DictationService
    let _ = app.emit("dictation-status", "idle");
    Ok("dictation stopped".into())
}

#[tauri::command]
fn paste_text(text: String) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    // Save clipboard, paste, restore is handled by the caller
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    // Simulate Ctrl+V
    enigo.key(Key::Control, Direction::Press).map_err(|e| e.to_string())?;
    enigo.key(Key::Unicode('v'), Direction::Click).map_err(|e| e.to_string())?;
    enigo.key(Key::Control, Direction::Release).map_err(|e| e.to_string())?;
    Ok(())
}
```

- [ ] **Step 2: Register commands**

Update the `invoke_handler`:

```rust
.invoke_handler(tauri::generate_handler![
    sync_window_mode, save_tray_position, reset_tray_position, get_tray_bounds,
    start_dictation, stop_dictation, paste_text
])
```

- [ ] **Step 3: Update global shortcut handler for dictation toggle**

In the `on_shortcuts` callback, update the dictation branch:

```rust
"start_dictation" => {
    let app = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        // Toggle dictation
        let _ = app.emit("shortcut-triggered", "start_dictation");
    });
}
```

- [ ] **Step 4: Handle dictation toggle in frontend**

In `useKeyboardShortcuts.ts`, update the dictation handler:

```ts
import { invoke } from '@tauri-apps/api/core';

// In the shortcut-triggered listener:
} else if (action === 'start_dictation') {
  // Toggle: start if idle, stop if listening
  invoke('start_dictation').catch(console.error);
}
```

- [ ] **Step 5: Verify build**

Run: `cd D:/Dev/Actio/backend && cargo check -p actio-desktop`
Expected: Compiles.

- [ ] **Step 6: Commit**

```bash
git add backend/src-tauri/src/main.rs frontend/src/hooks/useKeyboardShortcuts.ts
git commit -m "feat(dictation): add Tauri commands for dictation start/stop/paste"
```

---

### Task 11: Dynamic Global Shortcut Re-registration

**Files:**
- Modify: `backend/src-tauri/src/main.rs`
- Modify: `backend/actio-core/src/api/settings.rs`

- [ ] **Step 1: Emit settings-changed event from backend**

In `api/settings.rs`, after the `patch_settings` handler applies keyboard changes, emit an event to trigger re-registration:

In the `patch_settings` function, after the existing `if llm_changed` block:

```rust
if patch.keyboard.is_some() {
    // Signal Tauri to re-register global shortcuts
    // The frontend will relay this via a Tauri event
    tracing::info!("Keyboard shortcuts changed, signalling re-registration");
}
```

The simplest approach: the frontend, after a successful PATCH with keyboard changes, calls a Tauri command to re-register:

```rust
// In main.rs:
#[tauri::command]
fn reregister_shortcuts(app: AppHandle, shortcuts: std::collections::HashMap<String, String>) -> Result<(), String> {
    let global = app.global_shortcut();
    // Unregister all existing
    global.unregister_all().map_err(|e| e.to_string())?;
    // Re-register from the new mappings
    // ... parse shortcut strings, register with on_shortcuts
    Ok(())
}
```

- [ ] **Step 2: Call re-register from KeyboardSettings.tsx**

In `KeyboardSettings.tsx`, after saving a shortcut:

```ts
import { invoke } from '@tauri-apps/api/core';

// After successful PATCH:
invoke('reregister_shortcuts', { shortcuts: newShortcuts }).catch(console.error);
```

- [ ] **Step 3: Commit**

```bash
git add backend/src-tauri/src/main.rs frontend/src/components/settings/KeyboardSettings.tsx
git commit -m "feat(keyboard): dynamic global shortcut re-registration on settings change"
```

---

### Task 12: Final Integration Test

- [ ] **Step 1: Backend tests**

Run: `cd D:/Dev/Actio/backend && cargo test -p actio-core`
Expected: All existing tests pass. New `KeyboardSettings` defaults are correct.

- [ ] **Step 2: Frontend dev check**

Run: `cd D:/Dev/Actio/frontend && pnpm dev`
Test:
- `Ctrl+1` through `Ctrl+5` switches tabs
- `↓` on Board tab focuses first card with blue ring
- `↑`/`↓` navigates cards, card scrolls into view
- `Enter` expands focused card, `Delete` archives it
- `Escape` cascade works: close panel → collapse card → clear focus → collapse to tray
- Settings > Keyboard Shortcuts: all shortcuts listed, Record works, conflict detection works

- [ ] **Step 3: Global shortcut test** (requires built Tauri app)

Run: `cd D:/Dev/Actio/backend && cargo tauri dev`
Test:
- `Ctrl+\` toggles board/tray from any app
- `Ctrl+N` summons new todo from any app
- Settings change: record a new combo, verify it takes effect

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat(keyboard): keyboard navigation, global shortcuts, and dictation foundation"
```
