# Always-On Listening — Tray Toggle, Live Tab, Hotkey

**Status:** Draft (2026-04-25)

## Overview

Surface the existing `audio.always_listening` backend setting as a one-click control in the tray, mirror it inside what is now the Recording tab, and bind it to a global hotkey. The Recording tab is renamed to **Live** and loses its manual record/stop UI — with always-listening as the default mental model, that surface becomes a real-time monitor of what the pipeline is hearing, plus the same toggle.

The feature is mostly UI plumbing. No new backend fields, no new database state, no new pipeline modes. Behavior on toggle uses the existing supervisor hibernate path: when the toggle flips off, capture stops at the OS level (mic LED off), the ASR worker is dropped, memory frees. Resume cost is ~2 s cold-start when toggled back on or when dictation hot-keys the pipeline awake.

## Goals

- Add a mic toggle button to the tray (collapsed strip, beside the existing chevron-launch).
- Rename the Recording tab to **Live**, drop manual record/stop, mirror the toggle in the tab header.
- Wordmark in the tray reflects the toggle: listening animation when on, standby when off (transient dictation states still take priority).
- New global shortcut `toggle_listening`, default `Ctrl+Shift+M`, rebindable in Keyboard settings.
- Brief feedback toast on hotkey-driven toggle so users get confirmation when the tray is hidden behind a fullscreen window.

## Non-goals (v1)

- Keeping the ASR worker warm across off → on transitions (the "Approach C" optimization). Revisit if user behavior shows frequent toggling.
- Per-app or per-window mute. Toggle is global.
- Auto-mute heuristics (camera off, screen lock, calendar busy).

## Architecture

```
┌──────────────────────────────┐
│  Tray mic button  ──┐        │
│  Live-tab toggle  ──┤        │   onClick / onHotkey
│  Settings checkbox ─┘        │ ──────────► useStore.setListening(v)
└──────────────────────────────┘                │
                                                ▼
                              patchSettings({ audio: { always_listening: v }})
                                                │
                                                ▼
                                   PATCH /settings ──► app_settings.rs
                                                │              │
                                                │              ▼
                                                │     pipeline_supervisor wakes /
                                                │     hibernates via the existing
                                                │     `audio.always_listening` branch
                                                ▼
                                       useStore.ui.listeningEnabled
                                                │
                                                ▼
                            StandbyTray, LiveTab, ActioWordmark all derive
```

### Single source of truth

`audio.always_listening` is the canonical state. The store's `ui.listeningEnabled` is a shadow refreshed on the same load path that already populates the AudioSettings checkbox; the new `setListening(v)` action calls the existing `patchSettings` helper, which optimistically updates the store before the network round-trip resolves.

### Pipeline behavior on toggle

Approach A (the existing hibernate path) — when the toggle flips false:

1. Settings PATCH lands. Supervisor reads `audio.always_listening = false`.
2. After the existing idle grace period (or immediately if no live subscribers), the supervisor stops the inference pipeline.
3. cpal capture stops → OS mic indicator turns off.
4. ASR worker drops, memory freed.

When the toggle flips true: supervisor starts the pipeline; first VAD chunk takes ~2 s while the ASR model loads.

Dictation (`Ctrl+Shift+Space`) is unchanged. Pressing it while the toggle is off connects to `/ws`, which already wakes the pipeline. After dictation completes the pipeline auto-hibernates again because the toggle is still off.

## Tray mic button

### Layout

Collapsed-tray right side becomes:

```
[ drag pill ]   [ wordmark / live transcript ]   [ 🎤 ] [ ↗ ]
```

Mic button is immediately to the left of the chevron-launch, same 28 px square / ghost background / hover-lighten styling. Mic-on-the-left puts the privacy-sensitive control adjacent to the wordmark it controls; "open board" stays as the rightmost action.

### States

Two only:

- **On** (`listeningEnabled = true`): filled mic glyph (rect + dome).
- **Off**: same glyph with a diagonal slash, `currentColor` stroke.

Both inline SVGs in `StandbyTray.tsx`, matching the chevron's pattern. Promote to a shared `<MicIcon>` once the Live tab also uses it.

### Interaction

- Single click toggles. No confirmation. Wordmark animation flips immediately for feedback.
- `aria-pressed={listeningEnabled}`, `aria-label` keys `tray.aria.toggleListening.on` / `tray.aria.toggleListening.off`.
- `title` attribute for hover tooltip — reuse whatever the chevron uses (currently none).
- Native button focus + Space/Enter for keyboard. Global shortcut handled separately (see below).

## Wordmark behavior

`useActioState` priority chain:

```
1. preview         (Shift+Alt+Tab dev cycle)              [unchanged]
2. flash           (success after dictation paste, etc.)  [unchanged]
3. transient hook                                          [unchanged]
4. dictation phases — transcribing / processing            [unchanged]
5. listeningEnabled ? 'listening' : 'standby'              ← NEW
6. legacy isRecording → 'listening'                        ← demoted
```

When nothing transient applies, the wordmark *is* the toggle: sonar rings continuously when on, static standby when off. Dictation and previews still override because those are intentional foreground actions.

The off → on supervisor teardown latency (capture is still running for ~ a second after the toggle flips off) is intentionally hidden — the wordmark reflects user intent, not real-time pipeline state. The mic LED, which the user can verify directly, reflects the actual hardware state.

## Live tab

### Rename

- `TabBar.tsx`: `{ id: 'recording', labelKey: 'tab.recording' }` → `{ id: 'live', labelKey: 'tab.live' }`.
- Tab id becomes `'live'` everywhere — `BoardWindow` switch, `useStore.activeTab` type, `KeyboardSettings` shortcut map (`tab_recording` → `tab_live`).
- Locale keys `tab.recording` → `tab.live` ("Live" / "实时") in both `en.ts` and `zh-CN.ts`.
- Component file `RecordingTab.tsx` → `LiveTab.tsx`. CSS class names `recording-tab__*` → `live-tab__*`.

### Layout

```
┌────────────────────────────────────────────┐
│  Listening / Off          [🎤 toggle]      │
│                                            │
│  Listening since 2:14 PM • 47 minutes      │
│                                            │
│  ┌─ Live transcript ───────────────────┐   │
│  │  (existing <LiveTranscript />)      │   │
│  └─────────────────────────────────────┘   │
└────────────────────────────────────────────┘
```

- Header toggle is the same `<ListeningToggle>` extracted from the tray.
- "Listening since" reads `ui.listeningStartedAt` — set when toggle flips false → true, cleared on the inverse. Not persisted; restart resets the clock, which matches user expectations of "this run".
- `<LiveTranscript />` unchanged.

### Removed

- Manual record/stop button.
- Warmup state (`'idle' | 'warming' | 'ready' | 'error'`) and the "Loading model" UI.
- The audio meter when not listening.

The empty state when toggle is off: "Listening is paused — turn it on in the tray or here."

### Migration

Users with `tab_recording` saved in their shortcut JSON would lose the binding. In `app_settings.rs::load`, if `shortcuts.tab_recording` is present and `tab_live` is absent, copy the binding to `tab_live` and drop the old key. ~6 lines.

## Hotkey

- New global shortcut `toggle_listening`, default `Ctrl+Shift+M`. No collision with the existing bindings.
- Added to `DEFAULT_GLOBAL_SHORTCUTS` in `useGlobalShortcuts.ts`, registered via the existing `reregister_shortcuts` invoke on mount.
- Handled in the `shortcut-triggered` listener: `else if (action === 'toggle_listening')` reads `ui.listeningEnabled` and calls `setListening(!current)`. Same path as the tray click.
- Surfaced in `KeyboardSettings.tsx` — append `'toggle_listening'` to the listening-section actions array, add `settings.shortcuts.action.toggle_listening` to both locales.
- Brief feedback toast on toggle (`feedback.listeningOn` / `feedback.listeningOff`) so the user gets confirmation when the tray is occluded.

## Edge cases

| Scenario | Behavior |
|---|---|
| Dictation while toggle off | `Ctrl+Shift+Space` works as today. Pipeline wakes via WS connect, captures, finalizes, pastes. Auto-hibernates after the existing grace period. Wordmark cycles transcribing → processing → success → standby. |
| Toggle off mid-utterance | Currently-buffered VAD segment finalizes (existing supervisor behavior). Subsequent capture stops. |
| Settings PATCH fails | Local `listeningEnabled` reverts to prior value; `pushFeedback('feedback.listeningToggleFailed')` toast. Mic icon snaps back. |
| First-launch default | `always_listening` defaults to `true` server-side. Until settings load completes (~50–200 ms typical), the mic icon shows a neutral disabled state. |
| Multi-window | Both windows hit the same backend setting; the existing settings-changed WS broadcast refreshes the other window's store. |
| Voiceprint enrollment + toggle off | Enrollment already takes over the pipeline regardless of the toggle (`engine_owned_session` path). No change. |

## Tests

### Backend

- `app_settings.rs` — extend the existing patch-roundtrip test to assert `audio.always_listening` toggles persist.
- `app_settings.rs` — new test for the one-shot `tab_recording → tab_live` shortcut migration.

### Frontend

- `useActioState.test.tsx` — `listeningEnabled = true` → `'listening'`, `= false` → `'standby'`, dictation states still take priority.
- `StandbyTray.test.tsx` — mic button renders with correct aria-pressed, click invokes `setListening`, wordmark reflects toggle.
- New `LiveTab.test.tsx` (replacing existing RecordingTab tests if any) — header toggle, "Listening since" timer present when on, paused message when off, transcript still renders.
- `useGlobalShortcuts` test — firing `shortcut-triggered` with `'toggle_listening'` calls `setListening` with the inverted value.
- `KeyboardSettings` test — new action appears in the rebindable list and persists when changed.
- `parity.test.ts` — automatically catches drift in the new keys (`tab.live`, `tray.aria.toggleListening.on/off`, `live.pausedHint`, `feedback.listeningOn/Off/ToggleFailed`, `settings.shortcuts.action.toggle_listening`).

## Files touched

**Modified:**

- `frontend/src/components/StandbyTray.tsx` — add mic button to the toggle row.
- `frontend/src/components/RecordingTab.tsx` → renamed to `LiveTab.tsx`, drop manual record UI, add header toggle + "listening since" timer.
- `frontend/src/components/TabBar.tsx` — rename tab id + label key.
- `frontend/src/components/BoardWindow.tsx` — switch arm for new tab id.
- `frontend/src/components/settings/KeyboardSettings.tsx` — register `toggle_listening` action, rename `tab_recording` → `tab_live`.
- `frontend/src/hooks/useActioState.ts` — splice `listeningEnabled` into the priority chain.
- `frontend/src/hooks/useGlobalShortcuts.ts` — add `toggle_listening` to `DEFAULT_GLOBAL_SHORTCUTS` + handler branch.
- `frontend/src/store/use-store.ts` — `ui.listeningEnabled`, `ui.listeningStartedAt`, `setListening` action.
- `frontend/src/i18n/locales/en.ts` + `zh-CN.ts` — new keys, rename `tab.recording` → `tab.live`.
- `frontend/src/styles/globals.css` — `recording-tab__*` → `live-tab__*` rename, mic button styles.
- `backend/actio-core/src/engine/app_settings.rs` — `tab_recording → tab_live` shortcut migration.

**New:**

- `frontend/src/components/ListeningToggle.tsx` — extracted shared mic button (used by tray + Live tab).
- `frontend/src/components/__tests__/LiveTab.test.tsx`.

**Reused (no change):**

- Pipeline supervisor hibernate path.
- `LiveTranscript.tsx`.
- Settings PATCH flow.
- `flashWordmark` / `useWordmarkPreview`.

## Verification

1. Toggle off in tray → mic LED turns off within 1–2 s; wordmark snaps to standby.
2. Toggle on → mic LED on; ~2 s warm-up; first transcript chunk arrives.
3. Press `Ctrl+Shift+Space` while toggle is off → dictation works; toast on paste; pipeline hibernates again.
4. Press `Ctrl+Shift+M` while tray is occluded → toast shows "Listening on" / "Listening off"; setting persists across restart.
5. Open Live tab → header reflects same state as tray; "Listening since" timer ticks when on; toggle in tab works identically.
6. Restart the app → `tab_recording` shortcut binding (if user had one) is migrated to `tab_live`.
7. Run `pnpm test` — all new and updated tests pass; parity test green. `cargo test -p actio-core` passes the migration test.
