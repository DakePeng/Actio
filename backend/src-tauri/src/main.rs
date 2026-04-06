#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Deserialize;
use std::{thread, time::Duration};
use tauri::{
    LogicalPosition, LogicalSize, Manager, WebviewWindow, utils::config::WindowEffectsConfig,
    window::Color,
};

const STANDBY_TRAY_WIDTH: f64 = 320.0;
const STANDBY_TRAY_EXPANDED_WIDTH: f64 = 440.0;
const STANDBY_TRAY_HEIGHT: f64 = 72.0;
const STANDBY_TRAY_ROW_HEIGHT: f64 = 45.0;
const STANDBY_TRAY_CTA_HEIGHT: f64 = 56.0;
const WINDOW_MARGIN_X: f64 = 16.0;
const WINDOW_MARGIN_Y: f64 = 42.0;

#[derive(Deserialize)]
struct StandbyPosition {
    x: f64,
    y: f64,
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
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
    standby_position: Option<StandbyPosition>,
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
        let standby_x = standby_position
            .as_ref()
            .map(|position| {
                clamp(
                    position.x,
                    work_x + WINDOW_MARGIN_X,
                    work_x + work_width - next_width - WINDOW_MARGIN_X,
                )
            })
            .unwrap_or(default_x);
        let standby_y = standby_position
            .as_ref()
            .map(|position| {
                clamp(
                    position.y,
                    work_y + WINDOW_MARGIN_Y,
                    work_y + work_height - next_height - WINDOW_MARGIN_Y,
                )
            })
            .unwrap_or(default_y);

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
