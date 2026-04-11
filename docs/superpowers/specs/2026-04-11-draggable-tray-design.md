# Draggable Standby Tray

## Summary

Allow the user to reposition the standby tray by dragging a visible handle. The tray snaps to the nearest screen edge on release, slides freely along that edge, and remembers its position across app restarts.

## Scope

- **In scope:** Standby tray only (collapsed and expanded states). Drag handle, native dragging, edge snapping, position persistence.
- **Out of scope:** Board window dragging, corner snapping, multi-monitor roaming.

## Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Drag target | Visible top-center pill handle | Discoverable without cluttering the tray surface |
| Drag mechanism | Tauri `startDragging()` | Native OS-level window drag — smooth, no jank |
| Snapping | Nearest screen edge, 16px margin | Keeps tray anchored to an edge, prevents floating mid-screen |
| Free axis | Slide freely along the snapped edge | User chooses where along the edge the tray sits |
| Persistence | JSON file in `app_data_dir` | Survives restarts; separate from frontend localStorage |
| Draggable when | Both collapsed and expanded | User can reposition anytime |

## Frontend Changes

### Drag Handle Element

Add a drag handle `<div>` at the top of `StandbyTray`, above the existing `tray-toggle` row:

```
<div class="tray-drag-handle" onMouseDown={() => getCurrentWindow().startDragging()}>
  <div class="tray-drag-pill" />
</div>
```

- Pill dimensions: 36×4px, border-radius 2px
- Color: `rgba(255,255,255, 0.12)`, hover: `rgba(255,255,255, 0.25)`
- Cursor: `grab` (default), `grabbing` (active)
- Padding: 5px top, 2px bottom — adds ~6px to tray height

### Post-Drag Snap Call

After `startDragging()` completes, invoke a Tauri command to snap and persist. Since `startDragging()` captures the mouse at the OS level, `mouseup` on `document` won't fire reliably. Instead, listen for Tauri's window `moved` event with a debounce — the last `moved` event after dragging stops indicates the final position:

```ts
import { getCurrentWindow } from '@tauri-apps/api/window';

// In the drag handle's onMouseDown:
async function handleDragStart() {
  const window = getCurrentWindow();
  let debounceTimer: number | null = null;

  const unlisten = await window.onMoved(() => {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = window.setTimeout(async () => {
      await invoke('snap_tray_position');
      unlisten();
    }, 150);
  });

  await window.startDragging();
}
```

### StandbyTray Constants Update

Collapsed tray height increases from 72px to 78px to accommodate the drag handle. Update both:
- Frontend: `StandbyTray.tsx` (if height is referenced)
- Rust: `STANDBY_TRAY_HEIGHT` constant in `main.rs` (72.0 → 78.0)

### CSS

```css
.tray-drag-handle {
  display: flex;
  justify-content: center;
  padding: 5px 0 2px;
  cursor: grab;
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

## Rust Changes

### New Constants

```rust
const SNAP_MARGIN: f64 = 16.0; // px from screen edge after snapping
```

### Position File

Stored at `app_data_dir/tray-position.json`:

```json
{
  "x": 1200.0,
  "y": 800.0,
  "edge": "bottom"
}
```

- `edge` is one of `"top"`, `"bottom"`, `"left"`, `"right"`
- Used to determine expansion direction

### New Tauri Command: `snap_tray_position`

```rust
#[tauri::command]
fn snap_tray_position(window: WebviewWindow, app_handle: AppHandle) -> Result<(), String> {
    // 1. Get current window position and size
    // 2. Get current monitor work area
    // 3. Determine nearest edge (compare distances from each window edge to corresponding screen edge)
    // 4. Snap: move window to that edge with SNAP_MARGIN, keep free-axis coordinate clamped within bounds
    // 5. Save { x, y, edge } to tray-position.json in app_data_dir
    // 6. Apply position with window.set_position()
}
```

**Edge detection logic:**
- Calculate distance from each window edge to the corresponding screen edge
- Nearest edge wins
- Clamp the free-axis coordinate so the tray stays fully on-screen

### Modified: `apply_window_mode`

When computing standby position:
1. Attempt to read `tray-position.json`
2. If valid position exists and is within current monitor bounds, use it
3. Otherwise fall back to default bottom-right (current behavior)

The saved `edge` field determines expansion direction:
- `"bottom"` → expand upward (current behavior)
- `"top"` → expand downward
- `"left"` / `"right"` → expand downward (default)

When expanding/collapsing, the position animation adjusts the anchor point based on edge:
- Bottom edge: y decreases as height increases (grow upward)
- Top edge: y stays fixed, height increases downward
- Left/Right edge: y stays fixed, height increases downward

### Modified: `configure_startup_window`

On startup, read saved position and pass to `apply_window_mode`. If the file doesn't exist or the position is off-screen (monitor layout changed), use the default.

### Modified: `tauri.conf.json`

Update initial window height from 72 to 78 to match the new drag handle height.

## Registration

Add `snap_tray_position` to `tauri::generate_handler![]` alongside `sync_window_mode`.

## Testing

- Manual: drag tray to each edge, verify snap + persistence + correct expansion direction
- Edge case: drag near corner — should snap to the closer edge
- Edge case: change monitor layout between sessions — should fall back to default
- Edge case: drag while expanded — should work, re-snap, and stay expanded
