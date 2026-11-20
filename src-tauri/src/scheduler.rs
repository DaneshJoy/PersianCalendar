//! Lightweight background scheduler.
//!
//! Two responsibilities:
//!
//! 1. **Midnight rollover** – wake up at/before local midnight (sleep is capped
//!    at 30s so returning from sleep and timezone/clock changes are picked up
//!    quickly without any Windows-specific power API), then refresh the tray
//!    icon/tooltip, emit `today-changed` and refresh the events cache.
//! 2. **Environment watch** – every wake-up compares the local date and the
//!    Windows app theme; a change re-renders the icon and notifies the UI.
//!
//! The sleep interval is computed from the *remaining seconds until the next
//! local midnight*, so the loop is idle almost all the time (a cheap 30s tick).

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::jalaali;
use crate::today;
use crate::{theme, tray};

const MAX_TICK: u64 = 30;
const MIN_TICK: u64 = 5;

struct Previous {
    date: (i32, u32, u32),
    theme_is_dark: bool,
    tray_is_dark: bool,
}

/// Spin up the scheduler thread. Called once from `main`'s setup hook.
pub fn start(app: AppHandle) {
    let mut previous = Previous {
        date: current_date(),
        theme_is_dark: theme::system_theme_is_dark(),
        tray_is_dark: theme::tray_icon_is_light(),
    };

    std::thread::spawn(move || loop {
        let seconds = jalaali::seconds_until_local_midnight();
        let wait = seconds.clamp(MIN_TICK, MAX_TICK);
        std::thread::sleep(Duration::from_secs(wait));

        let date = current_date();
        let theme_is_dark = theme::system_theme_is_dark();
        let tray_is_dark = theme::tray_icon_is_light();

        let date_changed = date != previous.date;
        let theme_changed = theme_is_dark != previous.theme_is_dark;

        if date_changed || theme_changed || tray_is_dark != previous.tray_is_dark {
            previous.tray_is_dark = tray_is_dark;
            previous.date = date;
            previous.theme_is_dark = theme_is_dark;
            tray::refresh(&app);
            let _ = app.emit("today-changed", today::build(&app).to_json());
            let _ = app.emit("theme-changed", theme::info(&today::current_settings(&app)));
        }
        // Retry after the provider's backoff even when no calendar WebView is open.
        refresh_daily(&app);
    });
}

fn current_date() -> (i32, u32, u32) {
    let (j, _) = jalaali::today_local();
    (j.year, j.month, j.day)
}

/// Queue the daily refresh when due, including after a failed attempt.
pub fn refresh_daily(app: &AppHandle) {
    let events = app.state::<crate::AppState>().events.clone();
    let (j, _) = jalaali::today_local();
    events.schedule_daily_refresh(app, j.year, j.month);
}
