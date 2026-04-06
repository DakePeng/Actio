#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Deserialize;
use tauri::{LogicalPosition, LogicalSize, Manager, WebviewWindow};

const STANDBY_WIDTH: f64 = 320.0;
const STANDBY_EXPANDED_WIDTH: f64 = 440.0;
const STANDBY_HEIGHT: f64 = 72.0;
const STANDBY_ROW_HEIGHT: f64 = 45.0;
const STANDBY_CTA_HEIGHT: f64 = 56.0;
const WINDOW_MARGIN: f64 = 16.0;
const BOARD_MIN_WIDTH: f64 = 1024.0;
const BOARD_MIN_HEIGHT: f64 = 720.0;

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
    let mut next_width = if tray_expanded {
        STANDBY_EXPANDED_WIDTH
    } else {
        STANDBY_WIDTH
    };

    let mut next_height = if tray_expanded {
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
        LogicalSize::new(BOARD_MIN_WIDTH, BOARD_MIN_HEIGHT)
    } else {
        LogicalSize::new(next_width, next_height)
    }))?;
    window.set_max_size(if show_board {
        None::<LogicalSize<f64>>
    } else {
        Some(LogicalSize::new(next_width, next_height))
    })?;

    if let Some(monitor) = monitor {
        let monitor_position = monitor.position();
        let monitor_size = monitor.size();
        let work_area = monitor.work_area();
        let scale_factor = monitor.scale_factor();
        let screen_x = monitor_position.x as f64 / scale_factor;
        let screen_y = monitor_position.y as f64 / scale_factor;
        let screen_width = monitor_size.width as f64 / scale_factor;
        let screen_height = monitor_size.height as f64 / scale_factor;
        let work_x = work_area.position.x as f64 / scale_factor;
        let work_y = work_area.position.y as f64 / scale_factor;
        let work_width = work_area.size.width as f64 / scale_factor;
        let work_height = work_area.size.height as f64 / scale_factor;

        if show_board {
            next_width = screen_width.max(BOARD_MIN_WIDTH);
            next_height = screen_height.max(BOARD_MIN_HEIGHT);
        }

        window.set_size(LogicalSize::new(next_width, next_height))?;

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
            screen_x
        } else {
            standby_x
        };
        let y = if show_board {
            screen_y
        } else {
            standby_y
        };

        window.set_position(LogicalPosition::new(x, y))?;
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
