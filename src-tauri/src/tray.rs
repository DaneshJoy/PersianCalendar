//! System tray icon, tooltip and context menu.
//!
//! The tray icon is a live rendering of today's Jalali day number (see
//! [`crate::icon`]); the tooltip and menu carry the full date, today's
//! occasion/holiday, the copy shortcuts and the quick toggles. Everything is
//! rebuilt from [`crate::settings::Settings`] by [`refresh`], so the icon, the
//! tooltip and the menu checkbox/label states never drift out of sync.
//!
//! Left click toggles the calendar popup; the context menu is shown on right
//! click only (`.show_menu_on_left_click(false)`).

use std::sync::Mutex;

use tauri::{
    image::Image,
    menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Rect,
};
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::settings::{self, CopyFormat, Settings};
use crate::{icon, theme, today, windows, AppState};

/// Id of our single tray icon.
pub const TRAY_ID: &str = "main";

/// Longest tooltip Windows displays reliably.
const TOOLTIP_MAX_CHARS: usize = 120;

/// Latest tray-icon rect (physical pixels), used to place the popup when it is
/// opened from the menu (where no tray event is available).
static LAST_TRAY_RECT: Mutex<Option<Rect>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// Creation / refresh
// ---------------------------------------------------------------------------

/// Create the tray icon together with its tooltip and menu.
///
/// Call from the Tauri setup hook (main thread). The menu items are *not*
/// routed here: `main.rs` registers a single global menu-event listener that
/// calls [`handle_menu_event`], so app-level and tray menus share one path.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let tooltip = build_tooltip(app);
    let icon_image = render_tray_icon(app);

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon_image)
        .tooltip(tooltip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            if let Some(rect) = event_rect(&event) {
                remember_tray_rect(rect);
            }
            // `MouseButtonState::Up` is emitted for WM_LBUTTONUP, i.e. on release.
            if let TrayIconEvent::Click {
                rect,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                windows::toggle_popup(app, Some(rect));
            }
        })
        .build(app)?;

    Ok(())
}

/// Rebuild the tray icon, tooltip and menu from the current settings.
///
/// Safe to call from any thread: the update is marshalled onto the main thread.
pub fn refresh(app: &AppHandle) {
    let handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || refresh_inner(&handle)) {
        eprintln!("[tray] cannot schedule refresh: {err}");
    }
}

fn refresh_inner(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        eprintln!("[tray] refresh requested before the tray icon was created");
        return;
    };

    if let Err(err) = tray.set_icon(Some(render_tray_icon(app))) {
        eprintln!("[tray] cannot update icon: {err}");
    }
    if let Err(err) = tray.set_tooltip(Some(build_tooltip(app))) {
        eprintln!("[tray] cannot update tooltip: {err}");
    }
    match build_menu(app) {
        Ok(menu) => {
            if let Err(err) = tray.set_menu(Some(menu)) {
                eprintln!("[tray] cannot update menu: {err}");
            }
        }
        Err(err) => eprintln!("[tray] cannot rebuild menu: {err}"),
    }
}

/// Last known tray-icon rect, if the icon has ever emitted an event.
pub fn last_tray_rect() -> Option<Rect> {
    LAST_TRAY_RECT.lock().ok().and_then(|slot| *slot)
}

fn remember_tray_rect(rect: Rect) {
    if let Ok(mut slot) = LAST_TRAY_RECT.lock() {
        *slot = Some(rect);
    }
}

fn event_rect(event: &TrayIconEvent) -> Option<Rect> {
    match event {
        TrayIconEvent::Click { rect, .. }
        | TrayIconEvent::DoubleClick { rect, .. }
        | TrayIconEvent::Enter { rect, .. }
        | TrayIconEvent::Move { rect, .. }
        | TrayIconEvent::Leave { rect, .. } => Some(*rect),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Icon, tooltip and menu content
// ---------------------------------------------------------------------------

fn render_tray_icon(app: &AppHandle) -> Image<'static> {
    let settings = current_settings(app);
    let today_jalali = today::today_jalaali();
    icon::render(
        today_jalali.day,
        settings.fa_digits(),
        settings.tray_icon_mode,
        theme::tray_icon_is_light(),
        tray_icon_px(app),
    )
}

/// Physical icon size: 32px at 100% scaling, up to 64px on high-DPI screens.
fn tray_icon_px(app: &AppHandle) -> u32 {
    let scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or(1.0);
    ((32.0 * scale).round() as u32).clamp(32, 64)
}

fn build_tooltip(app: &AppHandle) -> String {
    truncate_chars(&today::tooltip_text(app), TOOLTIP_MAX_CHARS)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(max_chars - 1).collect();
    truncated.push('…');
    truncated
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let settings = current_settings(app);
    let (slash, long, iso) = today::copy_variants(app);

    // Native Windows menus are laid out left-to-right; the RLM makes the mixed
    // word/digit date strings read right-to-left (see `today::rtl`).
    let copy_slash =
        MenuItemBuilder::with_id("copy-jalali-slash", today::rtl(&slash)).build(app)?;
    let copy_long = MenuItemBuilder::with_id("copy-jalali-long", today::rtl(&long)).build(app)?;
    let copy_iso = MenuItemBuilder::with_id("copy-iso", today::rtl(&iso)).build(app)?;
    let copy_menu = SubmenuBuilder::new(app, "کپی تاریخ امروز")
        .item(&copy_slash)
        .item(&copy_long)
        .item(&copy_iso)
        .build()?;

    let widget_toggle = CheckMenuItemBuilder::with_id("toggle-widget", "حالت ویجت دسکتاپ")
        .checked(settings.widget_enabled)
        .build(app)?;
    let autostart_toggle = CheckMenuItemBuilder::with_id("toggle-autostart", "اجرا همراه ویندوز")
        .checked(settings.autostart)
        .build(app)?;

    MenuBuilder::new(app)
        .text("today", "امروز")
        .item(&copy_menu)
        .item(&widget_toggle)
        .text("settings", "تنظیمات")
        .item(&autostart_toggle)
        .separator()
        .text("quit", "خروج")
        .build()
}

// ---------------------------------------------------------------------------
// Menu handling
// ---------------------------------------------------------------------------

/// Dispatch a menu item id coming from the tray menu.
pub fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "today" => {
            let date = today::today_jalaali();
            windows::show_popup_and_emit(
                app,
                "show-month",
                serde_json::json!({ "year": date.year, "month": date.month }),
            );
        }
        "copy-jalali-slash" => copy_today(app, CopyFormat::JalaliSlash, "jalaliSlash"),
        "copy-jalali-long" => copy_today(app, CopyFormat::JalaliLong, "jalaliLong"),
        "copy-iso" => copy_today(app, CopyFormat::GregorianIso, "gregorianIso"),
        "toggle-widget" => toggle_widget(app),
        "settings" => {
            windows::show_popup_and_emit(app, "open-settings", serde_json::json!({}));
        }
        "toggle-autostart" => toggle_autostart(app),
        "quit" => app.exit(0),
        // The handler is global (it also sees events from other menus).
        _ => {}
    }
}

fn copy_today(app: &AppHandle, format: CopyFormat, what: &str) {
    let text = today::copy_text_for(app, format);
    if let Err(err) = app.clipboard().write_text(text) {
        eprintln!("[tray] cannot write to clipboard: {err}");
        return;
    }
    if let Err(err) = app.emit("copied", serde_json::json!({ "what": what })) {
        eprintln!("[tray] cannot emit copied: {err}");
    }
}

fn toggle_widget(app: &AppHandle) {
    let Some(settings) = mutate_settings(app, |settings| {
        settings.widget_enabled = !settings.widget_enabled;
    }) else {
        return;
    };
    if let Err(err) = settings::persist(app, &settings) {
        eprintln!("[tray] cannot persist widget setting: {err}");
    }
    windows::apply_widget(app);
    // The check item flips itself when clicked (muda toggles check items), so
    // the menu is deliberately not rebuilt from inside its own event handler.
    emit_settings_changed(app, &settings);
}

fn toggle_autostart(app: &AppHandle) {
    let current = current_settings(app).autostart;
    match set_autostart(app, !current) {
        Ok(_) => emit_settings_changed(app, &current_settings(app)),
        Err(err) => eprintln!("[tray] cannot toggle autostart: {err}"),
    }
}

/// Enable/disable launch-at-login, then mirror the real state in the settings.
pub fn set_autostart(app: &AppHandle, enabled: bool) -> Result<bool, String> {
    let actual = {
        let manager = app.autolaunch();
        let operation = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        operation.map_err(|err| format!("cannot change autostart: {err}"))?;
        manager
            .is_enabled()
            .map_err(|err| format!("cannot read autostart state: {err}"))?
    };

    let Some(settings) = mutate_settings(app, |settings| settings.autostart = actual) else {
        return Err("settings state is unavailable".to_string());
    };
    if let Err(err) = settings::persist(app, &settings) {
        return Err(format!("cannot persist autostart setting: {err}"));
    }
    Ok(actual)
}

fn emit_settings_changed(app: &AppHandle, settings: &Settings) {
    if let Err(err) = app.emit("settings-changed", settings) {
        eprintln!("[tray] cannot emit settings-changed: {err}");
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn current_settings(app: &AppHandle) -> Settings {
    let state = app.state::<AppState>();
    let settings = match state.settings.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    settings
}

/// Whether Windows will start the app with the user's session.
pub fn get_autostart(app: &AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

fn mutate_settings<F: FnOnce(&mut Settings)>(app: &AppHandle, mutate: F) -> Option<Settings> {
    let state = app.state::<AppState>();
    let mut guard = match state.settings.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    mutate(&mut guard);
    Some(guard.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_tooltip_is_kept_as_is() {
        assert_eq!(
            truncate_chars("پنجشنبه ۲۲ آبان ۱۴۰۵", 120),
            "پنجشنبه ۲۲ آبان ۱۴۰۵"
        );
    }

    #[test]
    fn long_tooltip_is_truncated_to_the_limit() {
        let text = "ا".repeat(200);
        let truncated = truncate_chars(&text, 120);
        assert_eq!(truncated.chars().count(), 120);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn zero_limit_produces_an_empty_string() {
        assert_eq!(truncate_chars("whatever", 0), "");
    }
}
