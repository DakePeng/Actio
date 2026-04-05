#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Deserialize;
use tauri::{LogicalPosition, LogicalSize, Manager, WebviewWindow};

const STANDBY_WIDTH: f64 = 320.0;
const STANDBY_EXPANDED_WIDTH: f64 = 440.0;
const STANDBY_HEIGHT: f64 = 72.0;
const STANDBY_ROW_HEIGHT: f64 = 45.0;
const STANDBY_CTA_HEIGHT: f64 = 56.0;
const WINDOW_MARGIN: f64 = 16.0;
const BOARD_WIDTH: f64 = 1320.0;
const BOARD_HEIGHT: f64 = 860.0;

#[derive(Deserialize)]
struct StandbyPosition {
    x: f64,
    y: f64,
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

fn apply_window_mode(
    window: &WebviewWindow,
    show_board: bool,
    tray_expanded: bool,
    reminder_count: usize,
    standby_position: Option<StandbyPosition>,
) -> tauri::Result<()> {
    let monitor = window
        .current_monitor()?
        .or(window.primary_monitor()?)
        .or_else(|| window.available_monitors().ok().and_then(|mut monitors| monitors.drain(..).next()));

    let next_width = if show_board {
        BOARD_WIDTH
    } else if tray_expanded {
        STANDBY_EXPANDED_WIDTH
    } else {
        STANDBY_WIDTH
    };

    let next_height = if show_board {
        BOARD_HEIGHT
    } else if tray_expanded {
        STANDBY_HEIGHT + (reminder_count.min(6) as f64 * STANDBY_ROW_HEIGHT) + STANDBY_CTA_HEIGHT
    } else {
        STANDBY_HEIGHT
    };

    window.set_always_on_top(!show_board)?;
    window.set_skip_taskbar(!show_board)?;
    window.set_decorations(false)?;
    window.set_resizable(show_board)?;
    window.set_shadow(show_board)?;
    window.set_min_size(Some(if show_board {
        LogicalSize::new(1024.0, 720.0)
    } else {
        LogicalSize::new(next_width, next_height)
    }))?;
    window.set_max_size(if show_board {
        None::<LogicalSize<f64>>
    } else {
        Some(LogicalSize::new(next_width, next_height))
    })?;
    window.set_size(LogicalSize::new(next_width, next_height))?;

    if let Some(monitor) = monitor {
        let work_area = monitor.work_area();
        let scale_factor = monitor.scale_factor();
        let work_x = work_area.position.x as f64 / scale_factor;
        let work_y = work_area.position.y as f64 / scale_factor;
        let work_width = work_area.size.width as f64 / scale_factor;
        let work_height = work_area.size.height as f64 / scale_factor;

        let default_x = work_x + work_width - next_width - WINDOW_MARGIN;
        let default_y = work_y + work_height - next_height - WINDOW_MARGIN;
        let standby_x = standby_position
            .as_ref()
            .map(|position| {
                clamp(
                    position.x,
                    work_x + WINDOW_MARGIN,
                    work_x + work_width - next_width - WINDOW_MARGIN,
                )
            })
            .unwrap_or(default_x);
        let standby_y = standby_position
            .as_ref()
            .map(|position| {
                clamp(
                    position.y,
                    work_y + WINDOW_MARGIN,
                    work_y + work_height - next_height - WINDOW_MARGIN,
                )
            })
            .unwrap_or(default_y);

        let x = if show_board {
            work_x + ((work_width - next_width) / 2.0)
        } else {
            standby_x
        };
        let y = if show_board {
            work_y + ((work_height - next_height) / 2.0)
        } else {
            standby_y
        };

        window.set_position(LogicalPosition::new(x, y))?;
    }

    window.set_focus()?;

    Ok(())
}

fn configure_startup_window(app: &tauri::App) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    apply_window_mode(&window, false, false, 0, None)
}

#[tauri::command]
fn sync_window_mode(
    window: WebviewWindow,
    show_board: bool,
    tray_expanded: bool,
    reminder_count: usize,
    standby_position: Option<StandbyPosition>,
) -> Result<(), String> {
    apply_window_mode(
        &window,
        show_board,
        tray_expanded,
        reminder_count,
        standby_position,
    )
    .map_err(|error| error.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![sync_window_mode])
        .setup(|app| {
            configure_startup_window(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Actio desktop");
}
