# Keyboard Navigation & System-wide Dictation

## Overview

Add comprehensive keyboard navigation to Actio with configurable shortcuts, system-wide dictation (transcribe-to-any-app), and card navigation. Shortcuts are split into two tiers: global (OS-level, work when app is collapsed) and local (in-app, work when board is focused).

## Features

### 1. Global Shortcuts (Tauri)

Registered via `tauri-plugin-global-shortcut`. Work system-wide regardless of which app is focused.

| Action | Default Key | Behavior |
|--------|------------|----------|
| Toggle board/tray | `Ctrl+\` | Board visible → collapse to tray. Tray visible → expand to board. Hidden → bring to front as board. |
| Start dictation | `Ctrl+Shift+Space` | Toggle dictation mode. First press starts recording, second press (or 3s silence) stops and pastes transcript into focused app. |
| New todo | `Ctrl+N` | Opens NewReminderBar. If app is collapsed/hidden, brings to front first. Auto-focuses title input. |

#### Toggle Board/Tray

- Calls the existing `sync_window_mode` Tauri command
- If board is open: collapse to tray position
- If tray is showing: expand to board
- If window is hidden/minimized: show and expand to board

#### System-wide Dictation

Dictation is a separate service from the always-on ASR pipeline. It captures audio on-demand and injects the transcribed text into whatever text field is focused in any application.

**Flow:**

1. User presses `Ctrl+Shift+Space`
2. Tauri backend:
   - Saves current clipboard contents
   - Opens a new audio capture (cpal, same default device)
   - Routes audio through Silero VAD → offline ASR (whisper_base or sense_voice, whichever is downloaded)
   - Creates a small floating indicator window (always-on-top pill, ~200x40px near system tray)
3. Indicator shows "Listening..." with a pulsing dot
4. User speaks, then presses `Ctrl+Shift+Space` again (or silence timeout triggers after 3s of post-speech silence)
5. Tauri backend:
   - Stops audio capture, collects final transcript
   - Copies transcript to clipboard
   - Simulates `Ctrl+V` keystroke via `enigo` crate
   - Restores original clipboard contents (50ms delay to ensure paste completes)
   - Destroys floating indicator window
   - Emits `dictation-complete` event with transcript text

**Floating indicator window:**
- Second Tauri WebView window, not the main window
- Size: 200x40px, always-on-top, transparent background, no decorations
- Position: near system tray (bottom-right, above tray bar)
- Click-through (non-interactive except a small X to cancel)
- Content: pulsing dot + "Listening..." text
- Created on dictation start, destroyed on dictation stop

**Silence detection:**
- Reuse existing Silero VAD
- After speech is first detected, start a silence timer
- If 3 seconds of continuous silence: auto-stop dictation and paste result
- If no speech detected after 10 seconds: cancel dictation (user may have accidentally triggered)

**Dependencies:**
- `tauri-plugin-clipboard-manager` — read/write clipboard
- `enigo` — simulate Ctrl+V keystroke into the target application
- Existing sherpa-onnx ASR models (no new model downloads)

### 2. Local Shortcuts (React)

Handled by a central `useKeyboardShortcuts()` hook mounted in `BoardWindow.tsx`. Only active when the Actio window is focused.

#### Tab Navigation

| Action | Default Key |
|--------|------------|
| Board tab | `Ctrl+1` |
| People tab | `Ctrl+2` |
| Recording tab | `Ctrl+3` |
| Archive tab | `Ctrl+4` |
| Settings tab | `Ctrl+5` |

- Calls `setActiveTab()` in Zustand store
- Works from any view, immediately switches

#### Card Navigation

| Action | Default Key |
|--------|------------|
| Move up | `↑` |
| Move down | `↓` |
| Expand card | `Enter` |
| Archive card | `Delete` |

- Only active when Board tab is showing and no input/textarea is focused
- Maintains `focusedCardIndex` in component state
- Focused card gets a visible ring: `2px solid var(--color-accent)` with `border-radius` matching the card
- `Enter` expands the focused card (sets `expandedCardId` in store)
- `Delete` archives the focused card (same action as swipe-left confirm)
- Focus resets when switching tabs or when card list changes
- First `↓` press focuses the first card if nothing is focused

**Key guard:** Shortcuts using plain keys (arrows, Delete, Enter) check `document.activeElement` — suppressed when an `<input>`, `<textarea>`, or `[contenteditable]` element is focused.

#### Escape Cascade

Formalized priority order (highest first):

1. NewReminderBar is open → close it
2. A card is expanded → collapse it
3. Card navigation is active → clear focus ring
4. Board is showing → collapse to tray

### 3. Configurable Shortcuts (Settings)

#### Backend Storage

New `keyboard` section in `AppSettings`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardSettings {
    pub shortcuts: HashMap<String, String>,
}
```

Keys are action identifiers (`toggle_board_tray`, `start_dictation`, `tab_board`, etc.). Values are key combo strings (`Ctrl+\\`, `Ctrl+Shift+Space`, `Ctrl+1`, etc.).

Default values defined in `KeyboardSettings::default()`. Persisted via existing `SettingsManager` to `settings.json`. Exposed via existing `/settings` GET and PATCH endpoints.

#### Frontend Settings UI

New `KeyboardSettings.tsx` component in `frontend/src/components/settings/`. Rendered as a section in `SettingsView.tsx` below Language Models.

Layout: minimal flat list with light group labels.

```
Keyboard Shortcuts
──────────────────

Global
  Toggle board/tray        [Ctrl+\]        [Record]
  Start dictation          [Ctrl+Shift+Space] [Record]
  New todo                 [Ctrl+N]        [Record]

Navigation
  Board tab                [Ctrl+1]        [Record]
  People tab               [Ctrl+2]        [Record]
  Recording tab            [Ctrl+3]        [Record]
  Archive tab              [Ctrl+4]        [Record]
  Settings tab             [Ctrl+5]        [Record]

Cards
  Move up                  [↑]             [Record]
  Move down                [↓]             [Record]
  Expand card              [Enter]         [Record]
  Archive card             [Delete]        [Record]

                    [Reset to defaults]
```

**Recording flow:**
1. Click "Record" → button text changes to "Press keys..." with pulsing border
2. Capture next keydown: display modifier+key combo (e.g., `Ctrl+Shift+D`)
3. Save immediately via PATCH `/settings`
4. Conflict detection: if combo is already assigned to another action, show inline warning "Already used by: [action name]"
5. Escape during recording → cancel without saving
6. For global shortcuts: Tauri unregisters old combo, registers new one

**Reset to defaults:** Restores all shortcuts to the default values.

#### Shortcut Lifecycle

1. App startup: backend reads `KeyboardSettings` from settings.json, registers global shortcuts via Tauri plugin
2. Frontend startup: reads shortcuts from `/settings`, passes to `useKeyboardShortcuts` hook
3. User changes a shortcut in Settings UI: PATCH to backend → backend re-registers global shortcuts → frontend re-reads and updates hook
4. Global shortcut fires: Tauri emits event → frontend event listener → dispatches action
5. Local shortcut fires: React keydown handler → checks mapping → dispatches action

## Architecture

```
┌─────────────────────────────────────────────────┐
│ OS Keyboard Event                                │
└──────────────┬──────────────────────────────────┘
               │
       ┌───────▼────────┐
       │ Tauri Global    │  Ctrl+\, Ctrl+Shift+Space, Ctrl+N
       │ Shortcut Plugin │  (registered on startup from settings)
       └───────┬─────────┘
               │ emit("shortcut-triggered", action)
       ┌───────▼────────┐
       │ Frontend Event  │
       │ Listener        │
       └───────┬─────────┘
               │
    ┌──────────┼─────────────────┐
    │          │                 │
    ▼          ▼                 ▼
 toggle     dictation        new todo
 board/     start/stop       open panel
 tray

┌─────────────────────────────────────────────────┐
│ React Window (focused)                           │
│                                                  │
│  useKeyboardShortcuts()                          │
│  ├─ Ctrl+1..5 → setActiveTab()                  │
│  ├─ ↑↓ → focusedCardIndex (if no input focused) │
│  ├─ Enter → expandCard (if card focused)         │
│  ├─ Delete → archiveCard (if card focused)       │
│  └─ Escape → cascade close                      │
│                                                  │
└─────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────┐
│ Dictation Service (backend)                      │
│                                                  │
│  start_dictation()                               │
│  ├─ cpal audio capture                           │
│  ├─ Silero VAD → silence detection               │
│  ├─ Offline ASR (whisper/sense_voice)            │
│  └─ Final transcript                             │
│                                                  │
│  stop_dictation()                                │
│  ├─ Save clipboard                               │
│  ├─ Copy transcript → Ctrl+V (enigo)            │
│  └─ Restore clipboard                            │
└─────────────────────────────────────────────────┘
```

## New Files

| File | Purpose |
|------|---------|
| `backend/src-tauri/src/shortcuts.rs` | Global shortcut registration, event emission, re-registration on settings change |
| `backend/src-tauri/src/dictation.rs` | Dictation Tauri commands: start, stop, cancel. Manages capture + ASR + clipboard + paste |
| `backend/actio-core/src/engine/dictation.rs` | Dictation service: audio capture → VAD → ASR → transcript. Separate from inference_pipeline. |
| `frontend/src/hooks/useKeyboardShortcuts.ts` | Central local shortcut dispatcher. Reads mapping from settings, handles key guard. |
| `frontend/src/components/settings/KeyboardSettings.tsx` | Shortcut configuration UI with record-key-combo flow |

## Modified Files

| File | Change |
|------|--------|
| `backend/src-tauri/Cargo.toml` | Add `tauri-plugin-global-shortcut`, `tauri-plugin-clipboard-manager`, `enigo` |
| `backend/src-tauri/src/main.rs` | Register global shortcuts on startup, add dictation commands to invoke handler |
| `backend/src-tauri/tauri.conf.json` | Add `global-shortcut` and `clipboard-manager` plugin permissions |
| `backend/actio-core/src/engine/app_settings.rs` | Add `KeyboardSettings` struct with defaults, add to `AppSettings` |
| `frontend/src/components/BoardWindow.tsx` | Mount `useKeyboardShortcuts` hook, add card focus ring rendering |
| `frontend/src/components/settings/SettingsView.tsx` | Add `<KeyboardSettings />` section |
| `frontend/src/components/Board.tsx` | Support `focusedCardIndex` prop, render focus ring on focused card |
| `frontend/src/store/use-store.ts` | Add `focusedCardIndex`, `setFocusedCardIndex` to store |

## New Dependencies

| Crate/Plugin | Purpose | Scope |
|-------------|---------|-------|
| `tauri-plugin-global-shortcut` | Register OS-level hotkeys | src-tauri |
| `tauri-plugin-clipboard-manager` | Read/write clipboard for dictation | src-tauri |
| `enigo` | Simulate Ctrl+V keystroke into target app | src-tauri |

## Non-Goals

- Shortcut hint overlay (`?` to show shortcuts) — can be added later
- Quick search/filter shortcut — not in this iteration
- Vim-style navigation (hjkl) — configurable shortcuts allow this if the user wants it
- Voice command recognition ("Hey Actio") — too complex, separate feature
- Multi-key chord sequences (e.g., `Ctrl+K Ctrl+S`) — single combo only

## Risks

| Risk | Mitigation |
|------|-----------|
| `enigo` Ctrl+V simulation fails on certain apps (e.g., terminal emulators, games) | Document as known limitation. Clipboard is still populated, user can manually paste. |
| Global shortcut conflicts with other apps' shortcuts | Shortcuts are configurable. Show conflict warning if a registration fails. |
| Dictation audio capture conflicts with always-on pipeline | Use separate cpal stream. Both can read from the same device simultaneously on most OSes. |
| Floating indicator window steals focus from target app | Set as non-focusable, click-through, no taskbar entry. |
| Clipboard restore race condition (paste hasn't completed before restore) | 50ms delay between Ctrl+V simulation and clipboard restore. Configurable if needed. |
