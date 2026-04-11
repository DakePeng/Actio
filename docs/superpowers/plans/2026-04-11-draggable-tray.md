# Draggable Standby Tray Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users reposition the standby tray by dragging a visible pill handle, with edge snapping and position persistence across restarts.

**Architecture:** A drag handle element in the frontend triggers Tauri's native `startDragging()`. After drag ends (detected via debounced `onMoved` event), a Rust command snaps the window to the nearest screen edge and saves the position to a JSON file. On startup, saved position is restored.

**Tech Stack:** Tauri 2, React 19, TypeScript, Rust, serde/serde_json

---

### File Map

| File | Action | Responsibility |
|---|---|---|
| `backend/src-tauri/src/main.rs` | Modify | Add `TrayPosition` struct, `snap_tray_position` command, update `apply_window_mode` and `configure_startup_window` to use saved position, update height constants |
| `backend/src-tauri/tauri.conf.json` | Modify | Update initial window height 72→78 |
| `frontend/src/components/StandbyTray.tsx` | Modify | Add drag handle element with `startDragging()` + debounced snap call |
| `frontend/src/styles/globals.css` | Modify | Add `.tray-drag-handle` and `.tray-drag-pill` styles |

---

### Task 1: Rust — Position Persistence Types and Helpers

**Files:**
- Modify: `backend/src-tauri/src/main.rs:1-18` (imports and constants area)

- [ ] **Step 1: Add serde derives and new imports**

At the top of `main.rs`, add `AppHandle` to the existing `tauri` import, and add the `serde` and file I/O imports:

```rust
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, WebviewWindow,
    utils::config::WindowEffectsConfig, window::Color,
};
use serde::{Deserialize, Serialize};
use std::io::Write as _;
```

Note: `std::{thread, time::Duration}` is already imported — keep it.

- [ ] **Step 2: Add the `SNAP_MARGIN` constant and `TrayPosition` struct**

After the existing constants block (after `WINDOW_MARGIN_Y`), add:

```rust
const SNAP_MARGIN: f64 = 16.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrayPosition {
    x: f64,
    y: f64,
    edge: String,
}
```

- [ ] **Step 3: Add helper functions for reading/writing position**

After the `TrayPosition` struct, add:

```rust
fn position_file_path(app_handle: &AppHandle) -> std::path::PathBuf {
    app_handle
        .path()
        .app_data_dir()
        .expect("failed to resolve app_data_dir")
        .join("tray-position.json")
}

fn read_saved_position(app_handle: &AppHandle) -> Option<TrayPosition> {
    let path = position_file_path(app_handle);
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

fn write_saved_position(app_handle: &AppHandle, pos: &TrayPosition) {
    let path = position_file_path(app_handle);
    if let Ok(json) = serde_json::to_string_pretty(pos) {
        if let Ok(mut file) = std::fs::File::create(&path) {
            let _ = file.write_all(json.as_bytes());
        }
    }
}
```

- [ ] **Step 4: Compile to verify**

Run from `backend/src-tauri`:
```bash
cd backend && cargo check -p actio-desktop
```
Expected: compiles with no errors (the new code is used in later tasks).

- [ ] **Step 5: Commit**

```bash
git add backend/src-tauri/src/main.rs
git commit -m "feat: add TrayPosition struct and persistence helpers"
```

---

### Task 2: Rust — `snap_tray_position` Command

**Files:**
- Modify: `backend/src-tauri/src/main.rs` (add new command before `main()`, register in handler)

- [ ] **Step 1: Add the `snap_tray_position` command**

Insert this function before `fn main()`:

```rust
#[tauri::command]
fn snap_tray_position(window: WebviewWindow, app_handle: AppHandle) -> Result<(), String> {
    let scale_factor = window.scale_factor().map_err(|e| e.to_string())?;
    let outer_pos = window.outer_position().map_err(|e| e.to_string())?;
    let inner_size = window.inner_size().map_err(|e| e.to_string())?;

    let win_x = outer_pos.x as f64 / scale_factor;
    let win_y = outer_pos.y as f64 / scale_factor;
    let win_w = inner_size.width as f64 / scale_factor;
    let win_h = inner_size.height as f64 / scale_factor;

    let monitor = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or("no monitor found")?;

    let work_area = monitor.work_area();
    let mon_scale = monitor.scale_factor();
    let work_x = work_area.position.x as f64 / mon_scale;
    let work_y = work_area.position.y as f64 / mon_scale;
    let work_w = work_area.size.width as f64 / mon_scale;
    let work_h = work_area.size.height as f64 / mon_scale;

    // Distance from each window edge to the corresponding screen edge
    let dist_left = (win_x - work_x).abs();
    let dist_right = ((work_x + work_w) - (win_x + win_w)).abs();
    let dist_top = (win_y - work_y).abs();
    let dist_bottom = ((work_y + work_h) - (win_y + win_h)).abs();

    let min_dist = dist_left.min(dist_right).min(dist_top).min(dist_bottom);

    let (edge, snapped_x, snapped_y) = if min_dist == dist_left {
        let y = win_y.clamp(work_y + SNAP_MARGIN, work_y + work_h - win_h - SNAP_MARGIN);
        ("left", work_x + SNAP_MARGIN, y)
    } else if min_dist == dist_right {
        let y = win_y.clamp(work_y + SNAP_MARGIN, work_y + work_h - win_h - SNAP_MARGIN);
        ("right", work_x + work_w - win_w - SNAP_MARGIN, y)
    } else if min_dist == dist_top {
        let x = win_x.clamp(work_x + SNAP_MARGIN, work_x + work_w - win_w - SNAP_MARGIN);
        ("top", x, work_y + SNAP_MARGIN)
    } else {
        let x = win_x.clamp(work_x + SNAP_MARGIN, work_x + work_w - win_w - SNAP_MARGIN);
        ("bottom", x, work_y + work_h - win_h - SNAP_MARGIN)
    };

    window
        .set_position(LogicalPosition::new(snapped_x, snapped_y))
        .map_err(|e| e.to_string())?;

    let pos = TrayPosition {
        x: snapped_x,
        y: snapped_y,
        edge: edge.to_string(),
    };
    write_saved_position(&app_handle, &pos);

    Ok(())
}
```

- [ ] **Step 2: Register the command**

Update the `invoke_handler` line in `main()`:

```rust
.invoke_handler(tauri::generate_handler![sync_window_mode, snap_tray_position])
```

- [ ] **Step 3: Compile to verify**

```bash
cd backend && cargo check -p actio-desktop
```
Expected: compiles with no errors.

- [ ] **Step 4: Commit**

```bash
git add backend/src-tauri/src/main.rs
git commit -m "feat: add snap_tray_position Tauri command with edge snapping"
```

---

### Task 3: Rust — Restore Saved Position on Startup and Expand

**Files:**
- Modify: `backend/src-tauri/src/main.rs` (update constants, `apply_window_mode`, `configure_startup_window`)

- [ ] **Step 1: Update the tray height constant**

Change:
```rust
const STANDBY_TRAY_HEIGHT: f64 = 72.0;
```
To:
```rust
const STANDBY_TRAY_HEIGHT: f64 = 78.0;
```

- [ ] **Step 2: Update `apply_window_mode` signature and position logic**

Change the function signature to accept an optional saved position:

```rust
fn apply_window_mode(
    window: &WebviewWindow,
    show_board: bool,
    tray_expanded: bool,
    reminder_count: usize,
    saved_position: Option<&TrayPosition>,
) -> tauri::Result<()> {
```

Then replace the block that computes `standby_x` and `standby_y` (the lines that calculate `default_x`, `default_y`, `standby_x`, `standby_y`). Change from:

```rust
        let default_x = work_x + work_width - next_width - WINDOW_MARGIN_X;
        let default_y = work_y + work_height - next_height - WINDOW_MARGIN_Y;
        let standby_x = default_x;
        let standby_y = default_y;
```

To:

```rust
        let (standby_x, standby_y) = if let Some(pos) = saved_position {
            // Validate saved position is within current monitor bounds
            let valid_x = pos.x >= work_x && pos.x + next_width <= work_x + work_width;
            let valid_y = pos.y >= work_y && pos.y + next_height <= work_y + work_height;
            if valid_x && valid_y {
                // Adjust y for expansion direction based on edge
                let y = match pos.edge.as_str() {
                    "bottom" => pos.y - (next_height - STANDBY_TRAY_HEIGHT),
                    _ => pos.y, // top, left, right: expand downward from saved y
                };
                let clamped_y = y.clamp(work_y, work_y + work_height - next_height);
                (pos.x, clamped_y)
            } else {
                // Saved position is off-screen, use default
                let default_x = work_x + work_width - next_width - WINDOW_MARGIN_X;
                let default_y = work_y + work_height - next_height - WINDOW_MARGIN_Y;
                (default_x, default_y)
            }
        } else {
            let default_x = work_x + work_width - next_width - WINDOW_MARGIN_X;
            let default_y = work_y + work_height - next_height - WINDOW_MARGIN_Y;
            (default_x, default_y)
        };
```

- [ ] **Step 3: Update `configure_startup_window` to load saved position**

Change `configure_startup_window` to:

```rust
fn configure_startup_window(app: &tauri::App) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    let saved_position = read_saved_position(&app.handle());
    set_window_background_transparent(&window)?;
    apply_window_mode(&window, false, false, 0, saved_position.as_ref())
}
```

- [ ] **Step 4: Update the `sync_window_mode` command to pass saved position**

Change `sync_window_mode` to:

```rust
#[tauri::command]
fn sync_window_mode(
    window: WebviewWindow,
    app_handle: AppHandle,
    show_board: bool,
    tray_expanded: bool,
    reminder_count: usize,
) -> Result<(), String> {
    let saved_position = read_saved_position(&app_handle);
    apply_window_mode(&window, show_board, tray_expanded, reminder_count, saved_position.as_ref())
        .map_err(|error| error.to_string())
}
```

- [ ] **Step 5: Compile to verify**

```bash
cd backend && cargo check -p actio-desktop
```
Expected: compiles with no errors.

- [ ] **Step 6: Commit**

```bash
git add backend/src-tauri/src/main.rs
git commit -m "feat: restore saved tray position on startup and expand in correct direction"
```

---

### Task 4: Config — Update Initial Window Height

**Files:**
- Modify: `backend/src-tauri/tauri.conf.json`

- [ ] **Step 1: Update window height**

In `tauri.conf.json`, change:
```json
"height": 72,
```
To:
```json
"height": 78,
```

- [ ] **Step 2: Commit**

```bash
git add backend/src-tauri/tauri.conf.json
git commit -m "chore: update initial tray height to 78px for drag handle"
```

---

### Task 5: Frontend — Add Drag Handle to StandbyTray

**Files:**
- Modify: `frontend/src/components/StandbyTray.tsx`

- [ ] **Step 1: Add the drag handle element**

At the top of the file, add the Tauri imports:

```tsx
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
```

- [ ] **Step 2: Add the drag handler function**

Inside the `StandbyTray` component, before the `return`, add:

```tsx
  async function handleDragStart() {
    if (!isTauri) return;

    const appWindow = getCurrentWindow();
    let debounceTimer: number | null = null;

    const unlisten = await appWindow.onMoved(() => {
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = window.setTimeout(async () => {
        await invoke('snap_tray_position');
        unlisten();
      }, 150);
    });

    await appWindow.startDragging();
  }
```

- [ ] **Step 3: Add the drag handle element to the JSX**

Inside the `motion.div` with `className="tray tray--launcher..."`, add the drag handle as the **first child** (before the `{newCount > 0 && ...}` badge line):

```tsx
        {/* Drag handle */}
        <div
          className="tray-drag-handle"
          onMouseDown={() => void handleDragStart()}
          role="separator"
          aria-label="Drag to reposition"
        >
          <div className="tray-drag-pill" />
        </div>
```

The resulting JSX structure should be:
```
<motion.div className="tray tray--launcher...">
  <div className="tray-drag-handle">...</div>     ← NEW
  {newCount > 0 && <span className="tray-badge">...}
  <div className="tray-toggle">...
  <motion.div className="tray-list">...
</motion.div>
```

- [ ] **Step 4: Verify the frontend compiles**

```bash
cd frontend && pnpm dev
```
Expected: no TypeScript errors, dev server starts. (Visual verification in next task.)

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/StandbyTray.tsx
git commit -m "feat: add drag handle to StandbyTray with native Tauri dragging"
```

---

### Task 6: Frontend — CSS for Drag Handle

**Files:**
- Modify: `frontend/src/styles/globals.css` (insert after the `.tray` block near line 949)

- [ ] **Step 1: Add drag handle styles**

Find the `.tray` CSS block (around line 949) and insert the following styles right after the `.tray--launcher:hover` block (around line 976):

```css
.tray-drag-handle {
  display: flex;
  justify-content: center;
  padding: 5px 0 2px;
  cursor: grab;
  -webkit-app-region: no-drag;
}

.tray-drag-handle:active {
  cursor: grabbing;
}

.tray-drag-pill {
  width: 36px;
  height: 4px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.12);
  transition: background 0.2s ease;
}

.tray-drag-handle:hover .tray-drag-pill {
  background: rgba(255, 255, 255, 0.25);
}
```

- [ ] **Step 2: Verify visually**

Run the Tauri dev server:
```bash
cd backend && cargo tauri dev
```
Expected: the tray shows a subtle pill bar at the top. Hovering brightens it. Dragging moves the window. Releasing snaps to the nearest edge.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/styles/globals.css
git commit -m "style: add CSS for tray drag handle pill"
```

---

### Task 7: Manual Verification

- [ ] **Step 1: Drag to each edge**

With `cargo tauri dev` running:
1. Drag the tray to the **right** edge → verify it snaps with 16px margin
2. Drag to the **left** edge → verify snap
3. Drag to the **top** edge → verify snap
4. Drag to the **bottom** edge → verify snap
5. For each edge, verify the tray slides freely along that edge's axis

- [ ] **Step 2: Verify persistence**

1. Drag the tray to the top-left area
2. Close the app (`Ctrl+C` in terminal)
3. Restart with `cargo tauri dev`
4. Verify the tray appears at the saved position (top-left area)

- [ ] **Step 3: Verify expansion direction**

1. Drag the tray to the **top** edge
2. Click to expand the tray
3. Verify it expands **downward**
4. Drag to the **bottom** edge
5. Expand again — verify it expands **upward** (current default behavior)

- [ ] **Step 4: Verify drag while expanded**

1. Expand the tray (show reminder list)
2. Drag via the pill handle
3. Verify it moves and snaps correctly while expanded

- [ ] **Step 5: Edge case — near corner**

1. Drag tray to a screen corner
2. Verify it snaps to the **closer** edge (not the corner)
