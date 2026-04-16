#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{thread, time::Duration, sync::Arc};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewWindow,
    utils::config::WindowEffectsConfig, window::Color,
};
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use actio_core::engine::dictation::DictationService;

const STANDBY_TRAY_WIDTH: f64 = 320.0;
const STANDBY_TRAY_EXPANDED_WIDTH: f64 = 440.0;
const STANDBY_TRAY_HEIGHT: f64 = 78.0;
const STANDBY_TRAY_ROW_HEIGHT: f64 = 45.0;
const STANDBY_TRAY_CTA_HEIGHT: f64 = 56.0;
const WINDOW_MARGIN_X: f64 = 16.0;
const WINDOW_MARGIN_Y: f64 = 42.0;
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
    const STEPS: u32 = 16;
    const FRAME_MS: u64 = 12;

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
    saved_position: Option<&TrayPosition>,
    skip_animation: bool,
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

    // Ensure native fullscreen is OFF — we emulate fullscreen manually to avoid Windows decoration flash
    if window.is_fullscreen().unwrap_or(false) {
        window.set_fullscreen(false)?;
        window.set_decorations(false)?;
    }

    if show_board {
        // Manually size window to full work area instead of native fullscreen
        if let Some(monitor) = &monitor {
            let work_area = monitor.work_area();
            let scale_factor = monitor.scale_factor();
            let work_x = work_area.position.x as f64 / scale_factor;
            let work_y = work_area.position.y as f64 / scale_factor;
            let work_width = work_area.size.width as f64 / scale_factor;
            let work_height = work_area.size.height as f64 / scale_factor;

            window.set_min_size(None::<LogicalSize<f64>>)?;
            window.set_max_size(None::<LogicalSize<f64>>)?;
            window.set_size(LogicalSize::new(work_width, work_height))?;
            window.set_position(LogicalPosition::new(work_x, work_y))?;
        }
        window.set_focus()?;
        return Ok(());
    }

    // Detect if we're coming from board mode (window was covering full work area)
    let pre_exit_size = window.inner_size().ok();
    let pre_exit_position = window.outer_position().ok();
    let was_board_mode = if let (Some(size), Some(monitor)) = (pre_exit_size, &monitor) {
        let scale = monitor.scale_factor();
        let work_w = monitor.work_area().size.width as f64 / scale;
        // Consider it board mode if the window is significantly larger than the expanded tray
        (size.width as f64 / scale) > STANDBY_TRAY_EXPANDED_WIDTH + 50.0
            && (size.width as f64 / scale) >= work_w - 2.0
    } else {
        false
    };

    if let Some(monitor) = monitor {
        let work_area = monitor.work_area();
        let scale_factor = monitor.scale_factor();
        let work_x = work_area.position.x as f64 / scale_factor;
        let work_y = work_area.position.y as f64 / scale_factor;
        let work_width = work_area.size.width as f64 / scale_factor;
        let work_height = work_area.size.height as f64 / scale_factor;

        // Unlock min/max so animation can interpolate freely (Windows clamps set_size otherwise)
        window.set_min_size(None::<LogicalSize<f64>>)?;
        window.set_max_size(None::<LogicalSize<f64>>)?;

        // When coming from board mode, start the animation from the visible board bounds
        // (matching CSS: width = min(100vw - 32px, 75vw), centered)
        let (current_size, current_position) = if was_board_mode {
            let board_w = (work_width - 32.0).min(work_width * 0.75);
            let board_h = (work_height - 32.0).min(work_height * 0.75);
            let board_x = work_x + (work_width - board_w) / 2.0;
            let board_y = work_y + (work_height - board_h) / 2.0;

            // Pre-shrink the window to the board bounds so the animation starts from there
            window.set_size(LogicalSize::new(board_w, board_h))?;
            window.set_position(LogicalPosition::new(board_x, board_y))?;

            (
                Some(tauri::PhysicalSize {
                    width: (board_w * scale_factor) as u32,
                    height: (board_h * scale_factor) as u32,
                }),
                Some(tauri::PhysicalPosition {
                    x: (board_x * scale_factor) as i32,
                    y: (board_y * scale_factor) as i32,
                }),
            )
        } else {
            (pre_exit_size, pre_exit_position)
        };

        let (standby_x, standby_y) = if let Some(pos) = saved_position {
            // Validate against COLLAPSED dimensions (that's what was saved)
            let valid_x = pos.x >= work_x && pos.x + STANDBY_TRAY_WIDTH <= work_x + work_width;
            let valid_y = pos.y >= work_y && pos.y + STANDBY_TRAY_HEIGHT <= work_y + work_height;
            if valid_x && valid_y {
                // Adjust x for expanded width based on edge
                let x = match pos.edge.as_str() {
                    "right" => pos.x - (next_width - STANDBY_TRAY_WIDTH),
                    _ => pos.x,
                };
                // Adjust y for expanded height based on edge
                let y = match pos.edge.as_str() {
                    "bottom" => pos.y - (next_height - STANDBY_TRAY_HEIGHT),
                    _ => pos.y,
                };
                let clamped_x = x.clamp(work_x, work_x + work_width - next_width);
                let clamped_y = y.clamp(work_y, work_y + work_height - next_height);
                (clamped_x, clamped_y)
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

        if skip_animation {
            window.set_size(LogicalSize::new(next_width, next_height))?;
            window.set_position(LogicalPosition::new(standby_x, standby_y))?;
        } else if let (Some(current_size), Some(current_position)) = (current_size, current_position) {
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

        // Lock min/max to tray dimensions AFTER animation completes
        window.set_min_size(Some(LogicalSize::new(next_width, next_height)))?;
        window.set_max_size(Some(LogicalSize::new(next_width, next_height)))?;
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

    let saved_position = read_saved_position(&app.handle());
    set_window_background_transparent(&window)?;
    apply_window_mode(&window, false, false, 0, saved_position.as_ref(), true)
}

#[tauri::command]
fn reset_tray_position(
    window: WebviewWindow,
    app_handle: AppHandle,
    tray_expanded: bool,
    reminder_count: usize,
) -> Result<(), String> {
    // Delete the saved position file
    let path = position_file_path(&app_handle);
    let _ = std::fs::remove_file(&path);

    // Re-apply window mode with no saved position — reverts to default bottom-right
    apply_window_mode(&window, false, tray_expanded, reminder_count, None, false)
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct TrayBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Returns where the collapsed tray will be placed in screen logical coordinates.
/// Used by the frontend to compute exit-animation transforms.
#[tauri::command]
fn get_tray_bounds(window: WebviewWindow, app_handle: AppHandle) -> Result<TrayBounds, String> {
    let monitor = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or("no monitor found")?;

    let work_area = monitor.work_area();
    let scale = monitor.scale_factor();
    let work_x = work_area.position.x as f64 / scale;
    let work_y = work_area.position.y as f64 / scale;
    let work_width = work_area.size.width as f64 / scale;
    let work_height = work_area.size.height as f64 / scale;

    let saved = read_saved_position(&app_handle);
    let (x, y) = if let Some(pos) = &saved {
        let valid_x = pos.x >= work_x && pos.x + STANDBY_TRAY_WIDTH <= work_x + work_width;
        let valid_y = pos.y >= work_y && pos.y + STANDBY_TRAY_HEIGHT <= work_y + work_height;
        if valid_x && valid_y {
            (pos.x, pos.y)
        } else {
            (
                work_x + work_width - STANDBY_TRAY_WIDTH - WINDOW_MARGIN_X,
                work_y + work_height - STANDBY_TRAY_HEIGHT - WINDOW_MARGIN_Y,
            )
        }
    } else {
        (
            work_x + work_width - STANDBY_TRAY_WIDTH - WINDOW_MARGIN_X,
            work_y + work_height - STANDBY_TRAY_HEIGHT - WINDOW_MARGIN_Y,
        )
    };

    Ok(TrayBounds {
        x,
        y,
        width: STANDBY_TRAY_WIDTH,
        height: STANDBY_TRAY_HEIGHT,
    })
}

#[tauri::command]
fn save_tray_position(window: WebviewWindow, app_handle: AppHandle) -> Result<(), String> {
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

    // Determine nearest edge (for expansion direction only)
    let dist_left = (win_x - work_x).abs();
    let dist_right = ((work_x + work_w) - (win_x + win_w)).abs();
    let dist_top = (win_y - work_y).abs();
    let dist_bottom = ((work_y + work_h) - (win_y + win_h)).abs();
    let min_dist = dist_left.min(dist_right).min(dist_top).min(dist_bottom);

    let edge = if min_dist == dist_left {
        "left"
    } else if min_dist == dist_right {
        "right"
    } else if min_dist == dist_top {
        "top"
    } else {
        "bottom"
    };

    let pos = TrayPosition {
        x: win_x,
        y: win_y,
        edge: edge.to_string(),
    };
    write_saved_position(&app_handle, &pos);

    Ok(())
}

#[tauri::command]
fn sync_window_mode(
    window: WebviewWindow,
    app_handle: AppHandle,
    show_board: bool,
    tray_expanded: bool,
    reminder_count: usize,
    skip_animation: Option<bool>,
) -> Result<(), String> {
    let saved_position = read_saved_position(&app_handle);
    apply_window_mode(
        &window,
        show_board,
        tray_expanded,
        reminder_count,
        saved_position.as_ref(),
        skip_animation.unwrap_or(false),
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_dictation(
    app: tauri::AppHandle,
    dictation: tauri::State<Arc<DictationService>>,
) -> Result<(), String> {
    dictation.start().map_err(|e| e.to_string())?;
    let _ = app.emit("dictation-status", "listening");
    Ok(())
}

#[tauri::command]
fn stop_dictation(
    app: tauri::AppHandle,
    dictation: tauri::State<Arc<DictationService>>,
) -> Result<String, String> {
    let transcript = dictation.stop().unwrap_or_default();
    let _ = app.emit("dictation-status", "idle");
    Ok(transcript)
}

#[tauri::command]
fn paste_text(app: tauri::AppHandle, text: String) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    use enigo::{Enigo, Keyboard, Key, Direction, Settings};

    // Write transcript to system clipboard
    app.clipboard().write_text(&text).map_err(|e| e.to_string())?;

    // Small delay to let the clipboard settle
    thread::sleep(Duration::from_millis(50));

    // Simulate Ctrl+V to paste into the focused input
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    enigo.key(Key::Control, Direction::Press).map_err(|e| e.to_string())?;
    enigo.key(Key::Unicode('v'), Direction::Click).map_err(|e| e.to_string())?;
    enigo.key(Key::Control, Direction::Release).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn reregister_shortcuts(
    app: tauri::AppHandle,
    shortcuts: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    // Unregister all existing global shortcuts
    app.global_shortcut().unregister_all().map_err(|e| e.to_string())?;

    // Re-register the global ones from the new map
    let global_actions = ["toggle_board_tray", "start_dictation", "new_todo"];

    for action in &global_actions {
        if let Some(combo) = shortcuts.get(*action) {
            if let Ok(shortcut) = combo.parse::<tauri_plugin_global_shortcut::Shortcut>() {
                let action_str = action.to_string();
                let app_clone = app.clone();
                let _ = app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    if event.state == ShortcutState::Pressed {
                        let _ = app_clone.emit("shortcut-triggered", &action_str);
                    }
                });
            } else {
                tracing::warn!(combo = %combo, action = %action, "reregister_shortcuts: failed to parse shortcut");
            }
        }
    }

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![sync_window_mode, save_tray_position, reset_tray_position, get_tray_bounds, start_dictation, stop_dictation, paste_text, reregister_shortcuts])
        .setup(|app| {
            app.manage(Arc::new(DictationService::new()));
            // Resolve app data directory for database and models
            let data_dir = app.path().app_data_dir()
                .expect("failed to resolve app_data_dir");
            std::fs::create_dir_all(&data_dir)
                .expect("failed to create app_data_dir");

            eprintln!("[actio] data_dir = {}", data_dir.display());
            eprintln!("[actio] model_dir = {}", data_dir.join("models").display());

            let config = actio_core::CoreConfig {
                data_dir: data_dir.clone(),
                db_path: data_dir.join("actio.db"),
                model_dir: data_dir.join("models"),
                http_port: 3000,
            };

            // Spawn Axum HTTP server as a background task
            tauri::async_runtime::spawn(async move {
                if let Err(e) = actio_core::start_server(config).await {
                    eprintln!("Actio HTTP server error: {e}");
                }
            });

            configure_startup_window(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Actio desktop");
}
