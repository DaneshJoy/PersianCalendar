//! Background theme watching and refresh of the calendar's current month.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::{theme, today, tray};

const TICK: Duration = Duration::from_secs(30);

/// Theme changes and retry backoff do not need to run at the clock's tick rate.
pub fn start(app: AppHandle) {
    let mut previous_theme = (theme::system_theme_is_dark(), theme::tray_icon_is_light());

    std::thread::spawn(move || loop {
        std::thread::sleep(TICK);
        let current_theme = (theme::system_theme_is_dark(), theme::tray_icon_is_light());
        if current_theme != previous_theme {
            previous_theme = current_theme;
            tray::refresh(&app);
            let _ = app.emit("theme-changed", theme::info(&today::current_settings(&app)));
        }

        // Retry after backoff even when no calendar view is open.
        refresh_daily(&app);
    });
}

/// Publish a rollover as soon as the same timer that drives the clock sees it.
pub fn refresh_today(app: &AppHandle) {
    let _ = app.emit("today-changed", today::build(app).to_json());
    tray::refresh(app);
    refresh_daily(app);
}

/// Queue the daily refresh for the month in the calendar's active zone.
pub fn refresh_daily(app: &AppHandle) {
    let events = app.state::<crate::AppState>().events.clone();
    let j = today::today_jalaali(app);
    events.schedule_daily_refresh(app, j.year, j.month);
}
