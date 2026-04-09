# Settings & Archive Views — Design Spec

**Date:** 2026-04-09  
**Status:** Approved

---

## Overview

Add two new views — **Archive** and **Settings** — to the Actio board window. The board window becomes a multi-view layout with a persistent top tab bar. "Mark done" is repurposed as an archive action; permanent deletion moves to the Archive view. Label management moves from the Board filters panel into Settings.

---

## 1. Layout & Navigation

### Tab bar

A `<TabBar>` component is inserted inside `.desktop-window`, between `.desktop-toolbar` and `.desktop-window__body`. It renders three tabs: **Board**, **Archive**, **Settings**.

### State

`UIState` gains one new field:

```ts
activeTab: 'board' | 'archive' | 'settings';  // default: 'board'
```

A `setActiveTab(tab)` action is added to the store. Switching tabs triggers the same cleanup as closing the board window: resets `expandedCardId` and sets `showNewReminderBar` to `false`.

### Routing

`BoardWindow` conditionally renders content based on `activeTab`:

- `'board'` → `<Board />`
- `'archive'` → `<ArchiveView />`
- `'settings'` → `<SettingsView />`

The `<NewReminderBar />` remains mounted at the bottom of `.desktop-window` but only activates from the Board tab (the "Capture note" toolbar button is disabled on other tabs).

---

## 2. Archive View

### Archiving behaviour

`markDone` is replaced by `archiveReminder(id)`, which sets `archivedAt` to the current ISO timestamp instead of removing the reminder. The board's `useFilteredReminders` gains an implicit filter: only reminders with `archivedAt === null` are shown.

### ArchiveView component

A new `<ArchiveView />` component renders a flat, scrollable list of archived reminders sorted by `archivedAt` descending (most recently archived first).

Each row displays:
- Title
- Priority badge (colour-coded, same palette as `Card`)
- Archived date (formatted, e.g. "Apr 8, 2026")
- Two action buttons: **Restore** and **Delete**

### New store actions

```ts
archiveReminder(id: string)   // sets archivedAt = new Date().toISOString()
restoreReminder(id: string)   // sets archivedAt = null
deleteReminder(id: string)    // removes reminder from state permanently
```

### Swipe gesture

`Card`'s swipe-to-done gesture calls `archiveReminder` instead of `markDone`. The overlay copy remains "Mark done" — this is a product-level label; the underlying action is now archive.

### Empty state

If there are no archived reminders, show a brief empty state message: "Nothing archived yet."

---

## 3. Settings View

`<SettingsView />` renders three sections stacked vertically.

### 3a. Profile

Editable fields:
- **Name** (text input)
- **Initials** (text input, max 2 chars — used in the tray avatar)

Stored in a new `profile` slice in the Zustand store, persisted to `localStorage` under the key `actio-profile`. Default: `{ name: '', initials: 'JD' }`.

The `Header` component and any other hardcoded `"JD"` references are updated to read from the store.

### 3b. Label Management

The existing label management UI (colour wheel, add form, delete chips) is extracted from `Board` into a standalone `<LabelManager />` component and rendered here.

`Board` is simplified: the label filter row retains the clickable label chips for filtering, but removes the "Edit labels" / "Done" toggle button and the inline add form entirely.

### 3c. App Preferences

Three controls, all stored in `localStorage` and loaded into a `preferences` slice of the store (`actio-preferences`):

| Setting | Type | Default | Notes |
|---|---|---|---|
| Theme | `'light' \| 'system' \| 'dark'` | `'system'` | Applies `data-theme="light\|dark"` to `<html>`. System mode reads `prefers-color-scheme`. |
| Launch at login | `boolean` | `false` | Calls Tauri `autostart` plugin when in Tauri context; no-op in browser. |
| Notifications | `boolean` | `true` | Stored preference; consumed by the reminder creation flow. |

CSS dark mode variables are added to `globals.css` under `[data-theme="dark"]`.

---

## 4. Component Map

| New / Changed | Type | Notes |
|---|---|---|
| `TabBar` | New component | Renders the three tab buttons, reads/writes `activeTab` |
| `ArchiveView` | New component | Archive list with restore/delete actions |
| `SettingsView` | New component | Container for the three settings sections |
| `LabelManager` | New component | Extracted from `Board`, used in `SettingsView` |
| `BoardWindow` | Modified | Adds `<TabBar>`, conditional view rendering |
| `Board` | Modified | Removes label edit UI; `useFilteredReminders` filters archived |
| `Card` | Modified | Swipe calls `archiveReminder` instead of `markDone` |
| `use-store.ts` | Modified | New actions, new state fields (`activeTab`, `profile`, `preferences`) |
| `types/index.ts` | Modified | `UIState` gains `activeTab`; new `Profile` and `Preferences` types |
| `globals.css` | Modified | Dark mode variables, tab bar styles, archive row styles |

---

## 5. Out of Scope

- Backend persistence for settings/preferences (localStorage only)
- Tauri autostart wiring beyond a best-effort `invoke` call
- Bulk archive/delete actions
- Archive search or filtering
