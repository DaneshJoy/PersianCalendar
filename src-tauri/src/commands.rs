//! Tauri IPC commands (the whole frontend/backend surface).
//!
//! Command names and payloads are fixed by `docs/CONTRACT.md`.

use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::events_api::MonthEvents;
use crate::settings::{self, Settings};
use crate::theme::{self, ThemeInfo};
use crate::today::{self, TodayInfo};
use crate::{events_api, scheduler, tray, windows, AppState};

const MAX_YEAR: i32 = 3000;

/// Apply every side effect that follows a settings change.
pub fn apply_side_effects(app: &AppHandle, settings: &Settings) {
    crate::minute_timer::ClockTimer::settings_changed();
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
pub fn save_settings(app: AppHandle, settings: Map<String, Value>) -> Result<Settings, String> {
    apply_settings_patch(&app, settings)
}

#[tauri::command]
pub fn save_widget_settings(
    app: AppHandle,
    mut settings: Map<String, Value>,
) -> Result<Settings, String> {
    settings.retain(|key, _| {
        matches!(
            key.as_str(),
            "widgetSize" | "widgetLocked" | "widgetEnabled"
        )
    });
    apply_settings_patch(&app, settings)
}

fn apply_settings_patch(app: &AppHandle, patch: Map<String, Value>) -> Result<Settings, String> {
    let was_online = events_api::online_allowed(app);
    let settings = settings::update(app, |current| {
        let Value::Object(mut merged) =
            serde_json::to_value(current).map_err(|err| err.to_string())?
        else {
            return Err("invalid settings record".to_string());
        };
        merged.extend(patch);
        serde_json::from_value(Value::Object(merged)).map_err(|err| err.to_string())
    })?;
    apply_side_effects(app, &settings);
    if !was_online && settings.online_events && (settings.show_events || settings.show_lunar) {
        scheduler::refresh_daily(app);
    }
    Ok(settings)
}

#[tauri::command]
pub fn reset_settings(app: AppHandle) -> Result<Settings, String> {
    let settings = settings::update(&app, |_| Ok(Settings::default()))?;
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
    events.clear_cache()?;
    let date = today::today_jalaali();
    let _ = app.emit(
        "events-updated",
        serde_json::json!({ "year": date.year, "month": date.month }),
    );
    Ok(())
}

#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    if text.len() > 256 || text.chars().any(char::is_control) {
        return Err("invalid date text".to_string());
    }
    app.clipboard()
        .write_text(text)
        .map_err(|err| format!("clipboard error: {err}"))
}

#[tauri::command]
pub fn hide_popup(app: AppHandle) {
    windows::hide_popup(&app);
}

#[tauri::command]
pub fn open_popup_settings(app: AppHandle) {
    windows::show_popup_and_emit(&app, "open-settings", serde_json::json!({}));
}
