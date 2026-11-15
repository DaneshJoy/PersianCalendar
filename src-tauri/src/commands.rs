//! Tauri IPC commands (the whole frontend/backend surface).
//!
//! Command names and payloads are fixed by `docs/CONTRACT.md`.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::events_api::MonthEvents;
use crate::settings::{self, Settings};
use crate::theme::{self, ThemeInfo};
use crate::today::{self, TodayInfo};
use crate::{tray, windows, AppState};

const MAX_YEAR: i32 = 3000;

fn emit_copied(app: &AppHandle, what: &str) {
    let _ = app.emit("copied", serde_json::json!({ "what": what }));
}

/// Apply every side effect that follows a settings change.
pub fn apply_side_effects(app: &AppHandle, settings: &Settings) {
    if let Err(err) = tray::set_autostart(app, settings.autostart) {
        eprintln!("[commands] could not apply autostart: {err}");
    }
    tray::refresh(app);
    windows::apply_widget(app);
    let _ = app.emit("settings-changed", settings);
    let _ = app.emit("theme-changed", theme::info(settings));
    let _ = app.emit("today-changed", today::build(app).to_json());
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    today::current_settings(&app)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, mut settings: Settings) -> Result<Settings, String> {
    settings.sanitize();
    {
        let state = app.state::<AppState>();
        *state
            .settings
            .write()
            .map_err(|_| "settings lock poisoned".to_string())? = settings.clone();
    }
    settings::persist(&app, &settings)?;
    apply_side_effects(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub fn reset_settings(app: AppHandle) -> Result<Settings, String> {
    let settings = Settings::default();
    {
        let state = app.state::<AppState>();
        *state
            .settings
            .write()
            .map_err(|_| "settings lock poisoned".to_string())? = settings.clone();
    }
    settings::persist(&app, &settings)?;
    apply_side_effects(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub fn get_theme(app: AppHandle) -> ThemeInfo {
    theme::info(&today::current_settings(&app))
}

#[tauri::command]
pub fn get_today_info(app: AppHandle) -> TodayInfo {
    today::build(&app)
}

#[tauri::command]
pub async fn get_month_events(app: AppHandle, year: i32, month: u32) -> MonthEvents {
    let events = app.state::<AppState>().events.clone();
    events
        .get_month(&app, year.clamp(1, MAX_YEAR), month.clamp(1, 12), false)
        .await
}

#[tauri::command]
pub async fn refresh_month_events(app: AppHandle, year: i32, month: u32) -> MonthEvents {
    let events = app.state::<AppState>().events.clone();
    events
        .get_month(&app, year.clamp(1, MAX_YEAR), month.clamp(1, 12), true)
        .await
}

#[tauri::command]
pub fn clear_events_cache(app: AppHandle) -> Result<(), String> {
    let events = app.state::<AppState>().events.clone();
    events.clear_cache()
}

#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|err| format!("clipboard error: {err}"))
}

#[tauri::command]
pub fn hide_popup(app: AppHandle) {
    windows::hide_popup(&app);
}

#[tauri::command]
pub fn show_popup(app: AppHandle) {
    windows::show_popup(&app, None);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let result = tray::set_autostart(&app, enabled)?;
    let settings = {
        let state = app.state::<AppState>();
        let mut guard = match state.settings.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.autostart = result;
        guard.clone()
    };
    let _ = settings::persist(&app, &settings);
    let _ = app.emit("settings-changed", &settings);
    tray::refresh(&app);
    Ok(result)
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> bool {
    tray::get_autostart(&app)
}

/// Used by the tray: copy the given format and notify the popup with a toast.
pub fn copy_from_tray(app: &AppHandle, what: &str) {
    let format = match what {
        "jalali-long" => settings::CopyFormat::JalaliLong,
        "gregorian-iso" => settings::CopyFormat::GregorianIso,
        _ => settings::CopyFormat::JalaliSlash,
    };
    let text = today::copy_text_for(app, format);
    if let Err(err) = app.clipboard().write_text(&text) {
        eprintln!("[tray] clipboard error: {err}");
        return;
    }
    emit_copied(app, what);
}
