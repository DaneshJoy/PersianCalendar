//! Shared notification menu and refresh dispatch for the taskbar surfaces.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{
    image::Image,
    menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Rect,
};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::settings::{self, CopyFormat, Settings};
use crate::{capsule, clock, icon, theme, today, windows, AppState};

/// Latest tray-icon rect (physical pixels), used to place the popup when it is
/// opened from the menu (where no tray event is available).
static LAST_TRAY_RECT: Mutex<Option<Rect>> = Mutex::new(None);

static DATE_ICON: NotificationIcon = NotificationIcon::new("main");
pub static CLOCK_ICON: NotificationIcon = NotificationIcon::new("world-clock");

pub struct NotificationIcon {
    id: &'static str,
    visible: AtomicBool,
}

impl NotificationIcon {
    const fn new(id: &'static str) -> Self {
        Self {
            id,
            visible: AtomicBool::new(false),
        }
    }

    /// Update the icon on the UI thread, reusing its Shell registration.
    pub fn update(
        &self,
        app: &AppHandle,
        content: Option<(Image<'static>, String)>,
    ) -> tauri::Result<()> {
        let existing = app.tray_by_id(self.id);
        let Some((image, tooltip)) = content else {
            if let Some(icon) = existing.filter(|_| self.visible.load(Ordering::Relaxed)) {
                icon.set_visible(false)?;
                self.visible.store(false, Ordering::Relaxed);
            }
            return Ok(());
        };
        let tooltip = truncate_chars(&tooltip, 120);
        if let Some(icon) = existing {
            icon.set_icon(Some(image))?;
            icon.set_tooltip(Some(tooltip))?;
            if !self.visible.load(Ordering::Relaxed) {
                icon.set_visible(true)?;
            }
        } else {
            TrayIconBuilder::with_id(self.id)
                .icon(image)
                .tooltip(tooltip)
                .menu(&build_menu(app)?)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray_icon, event| {
                    follow_tray_event(&event);
                    if let TrayIconEvent::Click {
                        rect,
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        windows::toggle_popup(tray_icon.app_handle(), Some(rect));
                    }
                })
                .build(app)?;
        }
        self.visible.store(true, Ordering::Relaxed);
        Ok(())
    }
}

pub fn any_notification_icon_visible() -> bool {
    DATE_ICON.visible.load(Ordering::Relaxed) || CLOCK_ICON.visible.load(Ordering::Relaxed)
}

fn show_date_icon(settings: &Settings, fallback_allowed: bool) -> bool {
    !settings.capsule_enabled || fallback_allowed
}

/// Render today's local Solar Hijri day and provide the full date in the tooltip.
pub fn refresh_date(app: &AppHandle) -> tauri::Result<()> {
    let settings = current_settings(app);
    let content = show_date_icon(&settings, capsule::fallback_allowed()).then(|| {
        let day = today::today_jalaali().day;
        let color = icon::text_color(
            settings.calendar_day_color.as_deref(),
            theme::tray_icon_is_light(),
        );
        (
            icon::render_day(day, settings.fa_digits(), color, tray_icon_px(app)),
            today::tooltip_text(app),
        )
    });
    DATE_ICON.update(app, content)
}

pub fn schedule_date_refresh(app: &AppHandle) {
    let handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || refresh_date_inner(&handle)) {
        eprintln!("[tray] cannot schedule date refresh: {err}");
    }
}

fn refresh_date_inner(app: &AppHandle) {
    if let Err(err) = refresh_date(app) {
        eprintln!("[tray] cannot update date icon: {err}");
    }
}

/// Refresh the visible surfaces and rebuild the notification menu after a settings change.
pub fn refresh(app: &AppHandle) {
    let handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        refresh_date_inner(&handle);
        refresh_clock_inner(&handle);
        for icon in [&DATE_ICON, &CLOCK_ICON] {
            if let Some(icon) = handle.tray_by_id(icon.id) {
                if let Err(err) = build_menu(&handle).and_then(|menu| icon.set_menu(Some(menu))) {
                    eprintln!("[tray] cannot update the menu: {err}");
                }
            }
        }
    }) {
        eprintln!("[tray] cannot schedule refresh: {err}");
    }
}

pub fn refresh_clock(app: &AppHandle) {
    let handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || refresh_clock_inner(&handle)) {
        eprintln!("[clock] cannot schedule refresh: {err}");
    }
}

fn refresh_clock_inner(app: &AppHandle) {
    if let Err(err) = clock::refresh(app) {
        eprintln!("[clock] cannot update tray icon: {err}");
    }
}

fn follow_tray_event(event: &TrayIconEvent) {
    if let Some(rect) = event_rect(event) {
        remember_tray_rect(rect);
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

/// Physical icon size: 32px at 100% scaling, up to 64px on high-DPI screens.
pub fn tray_icon_px(app: &AppHandle) -> u32 {
    let scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| monitor.scale_factor())
        .unwrap_or(1.0);
    ((32.0 * scale).round() as u32).clamp(32, 64)
}

pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.encode_utf16().count() <= max_chars {
        return text.to_string();
    }
    let mut remaining = max_chars - 1;
    let mut truncated: String = text
        .chars()
        .take_while(|ch| {
            if ch.len_utf16() > remaining {
                return false;
            }
            remaining -= ch.len_utf16();
            true
        })
        .collect();
    truncated.push('…');
    truncated
}

pub fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
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
    match settings::update(app, |current| {
        let mut next = current.clone();
        next.widget_enabled = !next.widget_enabled;
        Ok(next)
    }) {
        Ok(settings) => {
            windows::apply_widget(app);
            let _ = app.emit("settings-changed", &settings);
            refresh(app);
        }
        Err(err) => {
            eprintln!("[tray] cannot persist widget setting: {err}");
            refresh(app);
        }
    }
}

fn toggle_autostart(app: &AppHandle) {
    match settings::update(app, |current| {
        let mut next = current.clone();
        next.autostart = !next.autostart;
        Ok(next)
    }) {
        Ok(settings) => {
            let _ = app.emit("settings-changed", &settings);
            refresh(app);
        }
        Err(err) => {
            eprintln!("[tray] cannot toggle autostart: {err}");
            refresh(app);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_icon_is_default_and_yields_to_the_capsule_when_available() {
        let mut settings = Settings::default();
        assert!(show_date_icon(&settings, false));
        settings.clock_enabled = true;
        settings.clock_layout = settings::ClockLayout::Stacked;
        assert!(show_date_icon(&settings, false));
        settings.capsule_enabled = true;
        assert!(!show_date_icon(&settings, false));
        assert!(show_date_icon(&settings, true));
    }

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
