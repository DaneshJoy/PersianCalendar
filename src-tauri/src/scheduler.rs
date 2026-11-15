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

use crate::events_api::EventsState;
use crate::jalaali;
use crate::today;
use crate::{theme, tray};

const MAX_TICK: u64 = 30;
const MIN_TICK: u64 = 5;

struct Previous {
    date: (i32, u32, u32),
    theme_is_dark: bool,
}

/// Spin up the scheduler thread. Called once from `main`'s setup hook.
pub fn start(app: AppHandle) {
    let mut previous = Previous {
        date: current_date(),
        theme_is_dark: theme::system_theme_is_dark(),
    };

    std::thread::spawn(move || loop {
        let seconds = jalaali::seconds_until_local_midnight();
        let wait = seconds.clamp(MIN_TICK, MAX_TICK);
        std::thread::sleep(Duration::from_secs(wait));

        let date = current_date();
        let theme_is_dark = theme::system_theme_is_dark();

        let date_changed = date != previous.date;
        let theme_changed = theme_is_dark != previous.theme_is_dark;

        if date_changed || theme_changed {
            previous.date = date;
            previous.theme_is_dark = theme_is_dark;
            tray::refresh(&app);
            let _ = app.emit("today-changed", today::build(&app).to_json());
            let _ = app.emit("theme-changed", theme::info(&today::current_settings(&app)));
            refresh_daily(&app);
        }
    });
}

fn current_date() -> (i32, u32, u32) {
    let (j, _) = jalaali::today_local();
    (j.year, j.month, j.day)
}

/// Refresh the current month once per day and prefetch the next month.
pub fn refresh_daily(app: &AppHandle) {
    let events = app.state::<crate::AppState>().events.clone();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let (j, _) = jalaali::today_local();
        if events.needs_daily_refresh(j.year, j.month) {
            // A cached month is served immediately; the actual refresh happens
            // in the background inside EventsState.
            let _ = events.get_month(&handle, j.year, j.month, true).await;
        }
        let next = next_month(j.year, j.month);
        events.prefetch_if_needed(&handle, next.0, next.1).await;
    });
}

fn next_month(year: i32, month: u32) -> (i32, u32) {
    if month >= 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

#[allow(dead_code)]
fn events_state(app: &AppHandle) -> std::sync::Arc<EventsState> {
    app.state::<crate::AppState>().events.clone()
}
