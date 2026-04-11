#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{thread, time::Duration};
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, WebviewWindow,
    utils::config::WindowEffectsConfig, window::Color,
};
use serde::{Deserialize, Serialize};
use std::io::Write as _;

const STANDBY_TRAY_WIDTH: f64 = 320.0;
const STANDBY_TRAY_EXPANDED_WIDTH: f64 = 440.0;
const STANDBY_TRAY_HEIGHT: f64 = 72.0;
const STANDBY_TRAY_ROW_HEIGHT: f64 = 45.0;
const STANDBY_TRAY_CTA_HEIGHT: f64 = 56.0;
const WINDOW_MARGIN_X: f64 = 16.0;
const WINDOW_MARGIN_Y: f64 = 42.0;
const SNAP_MARGIN: f64 = 16.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrayPosition {
    x: f64,
    y: f64,
    edge: String,
}

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

fn lerp(start: f64, end: f64, progress: f64) -> f64 {
    start + (end - start) * progress
}

fn animate_standby_window(
    window: &WebviewWindow,
    start_width: f64,
    start_height: f64,
    start_x: f64,
    start_y: f64,
    end_width: f64,
    end_height: f64,
    end_x: f64,
    end_y: f64,
) -> tauri::Result<()> {
    const STEPS: u32 = 12;
    const FRAME_MS: u64 = 18;

    for step in 1..=STEPS {
        let progress = step as f64 / STEPS as f64;
        let eased = 1.0 - (1.0 - progress).powi(3);
        let width = lerp(start_width, end_width, eased);
        let height = lerp(start_height, end_height, eased);
        let x = lerp(start_x, end_x, eased);
        let y = lerp(start_y, end_y, eased);

        window.set_size(LogicalSize::new(width, height))?;
        window.set_position(LogicalPosition::new(x, y))?;

        if step < STEPS {
            thread::sleep(Duration::from_millis(FRAME_MS));
        }
    }

    Ok(())
}

fn set_window_background_transparent(window: &WebviewWindow) -> tauri::Result<()> {
    window
        .as_ref()
        .window()
        .set_background_color(None::<Color>)?;
    window
        .as_ref()
        .set_background_color(Some(Color(0, 0, 0, 0)))
}

fn clear_window_compositor_effects(window: &WebviewWindow) -> tauri::Result<()> {
    window.set_shadow(false)?;
    window.set_effects(None::<WindowEffectsConfig>)
}

fn apply_window_mode(
    window: &WebviewWindow,
    show_board: bool,
    tray_expanded: bool,
    reminder_count: usize,
) -> tauri::Result<()> {
    let monitor = window
        .current_monitor()?
        .or(window.primary_monitor()?)
        .or_else(|| {
            window
                .available_monitors()
                .ok()
                .and_then(|mut monitors| monitors.drain(..).next())
        });
    let tray_width = if tray_expanded {
        STANDBY_TRAY_EXPANDED_WIDTH
    } else {
        STANDBY_TRAY_WIDTH
    };

    let tray_height = if tray_expanded {
        STANDBY_TRAY_HEIGHT
            + (reminder_count.min(6) as f64 * STANDBY_TRAY_ROW_HEIGHT)
            + STANDBY_TRAY_CTA_HEIGHT
    } else {
        STANDBY_TRAY_HEIGHT
    };
    let next_width = tray_width;
    let next_height = tray_height;

    window.set_always_on_top(!show_board)?;
    window.set_skip_taskbar(!show_board)?;
    window.set_decorations(false)?;
    window.set_resizable(false)?;
    clear_window_compositor_effects(window)?;
    set_window_background_transparent(window)?;

    if show_board {
        window.set_fullscreen(true)?;
        window.set_min_size(None::<LogicalSize<f64>>)?;
        window.set_max_size(None::<LogicalSize<f64>>)?;
        window.set_focus()?;
        return Ok(());
    }

    window.set_fullscreen(false)?;

    if let Some(monitor) = monitor {
        let work_area = monitor.work_area();
        let scale_factor = monitor.scale_factor();
        let work_x = work_area.position.x as f64 / scale_factor;
        let work_y = work_area.position.y as f64 / scale_factor;
        let work_width = work_area.size.width as f64 / scale_factor;
        let work_height = work_area.size.height as f64 / scale_factor;

        let current_size = window.inner_size().ok();
        let current_position = window.outer_position().ok();

        window.set_min_size(Some(LogicalSize::new(next_width, next_height)))?;
        window.set_max_size(Some(LogicalSize::new(next_width, next_height)))?;

        let default_x = work_x + work_width - next_width - WINDOW_MARGIN_X;
        let default_y = work_y + work_height - next_height - WINDOW_MARGIN_Y;
        let standby_x = default_x;
        let standby_y = default_y;

        if let (Some(current_size), Some(current_position)) = (current_size, current_position) {
            let start_width = current_size.width as f64 / scale_factor;
            let start_height = current_size.height as f64 / scale_factor;
            let start_x = current_position.x as f64 / scale_factor;
            let start_y = current_position.y as f64 / scale_factor;

            animate_standby_window(
                window,
                start_width,
                start_height,
                start_x,
                start_y,
                next_width,
                next_height,
                standby_x,
                standby_y,
            )?;
        } else {
            window.set_size(LogicalSize::new(next_width, next_height))?;
            window.set_position(LogicalPosition::new(standby_x, standby_y))?;
        }
    } else {
        window.set_size(LogicalSize::new(next_width, next_height))?;
    }

    window.set_focus()?;

    Ok(())
}

fn configure_startup_window(app: &tauri::App) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    set_window_background_transparent(&window)?;
    apply_window_mode(&window, false, false, 0)
}

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

#[tauri::command]
fn sync_window_mode(
    window: WebviewWindow,
    show_board: bool,
    tray_expanded: bool,
    reminder_count: usize,
) -> Result<(), String> {
    apply_window_mode(&window, show_board, tray_expanded, reminder_count)
        .map_err(|error| error.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![sync_window_mode, snap_tray_position])
        .setup(|app| {
            configure_startup_window(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Actio desktop");
}
