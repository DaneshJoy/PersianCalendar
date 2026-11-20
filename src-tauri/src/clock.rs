//! Independent, offline clock for the Windows notification area.

use chrono::{Datelike, Timelike, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::{
    icon, jalaali,
    settings::{ClockLayout, Settings},
    theme, today, tray,
};

pub const LOCAL_ZONE: &str = "local";
pub const DEFAULT_ZONE: &str = "Iran Standard Time";
#[derive(Clone, PartialEq, Eq)]
pub struct Reading {
    pub hour: u32,
    pub minute: u32,
    second: Option<u32>,
    date: jalaali::JDate,
}

#[derive(Serialize)]
pub struct ClockInfo {
    time: String,
    date: String,
}

impl Reading {
    fn from_time(time: impl Datelike + Timelike, seconds: bool) -> Self {
        Self {
            hour: time.hour(),
            minute: time.minute(),
            second: seconds.then(|| time.second()),
            date: jalaali::to_jalaali(time.year(), time.month(), time.day()),
        }
    }

    fn info(&self, fa: bool) -> ClockInfo {
        let time = match self.second {
            Some(second) => format!("{:02}:{:02}:{second:02}", self.hour, self.minute),
            None => format!("{:02}:{:02}", self.hour, self.minute),
        };
        ClockInfo {
            time: if fa { jalaali::fa_digits(&time) } else { time },
            date: jalaali::format_slash(&self.date, fa),
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::collections::BTreeMap;
    use std::sync::OnceLock;

    use windows_sys::Win32::{
        Foundation::{ERROR_SUCCESS, SYSTEMTIME},
        System::Time::{
            EnumDynamicTimeZoneInformation, SystemTimeToTzSpecificLocalTimeEx,
            DYNAMIC_TIME_ZONE_INFORMATION,
        },
    };

    use super::*;

    pub fn zones() -> &'static BTreeMap<String, DYNAMIC_TIME_ZONE_INFORMATION> {
        static ZONES: OnceLock<BTreeMap<String, DYNAMIC_TIME_ZONE_INFORMATION>> = OnceLock::new();
        ZONES.get_or_init(|| {
            let mut zones = BTreeMap::new();
            for index in 0.. {
                let mut zone = DYNAMIC_TIME_ZONE_INFORMATION::default();
                // Windows writes into a correctly sized, initialized structure.
                if unsafe { EnumDynamicTimeZoneInformation(index, &mut zone) } != ERROR_SUCCESS {
                    break;
                }
                let key_end = zone
                    .TimeZoneKeyName
                    .iter()
                    .position(|&ch| ch == 0)
                    .unwrap_or(zone.TimeZoneKeyName.len());
                let key = String::from_utf16_lossy(&zone.TimeZoneKeyName[..key_end]);
                if !key.is_empty() {
                    zones.insert(key, zone);
                }
            }
            zones
        })
    }

    pub fn convert(
        now: chrono::DateTime<Utc>,
        zone: &DYNAMIC_TIME_ZONE_INFORMATION,
        seconds: bool,
    ) -> Option<Reading> {
        let utc = SYSTEMTIME {
            wYear: u16::try_from(now.year()).ok()?,
            wMonth: now.month() as u16,
            wDay: now.day() as u16,
            wHour: now.hour() as u16,
            wMinute: now.minute() as u16,
            wSecond: now.second() as u16,
            ..Default::default()
        };
        let mut local = SYSTEMTIME::default();
        // The conversion uses the selected Windows zone's rules for this UTC instant.
        let ok = unsafe { SystemTimeToTzSpecificLocalTimeEx(zone, &utc, &mut local) } != 0;
        ok.then(|| Reading {
            hour: local.wHour.into(),
            minute: local.wMinute.into(),
            second: seconds.then(|| local.wSecond.into()),
            date: jalaali::to_jalaali(local.wYear.into(), local.wMonth.into(), local.wDay.into()),
        })
    }
}

pub fn valid_zone(zone: &str) -> bool {
    if zone == LOCAL_ZONE {
        return true;
    }
    #[cfg(windows)]
    return platform::zones().contains_key(zone);
    #[cfg(not(windows))]
    return false;
}

pub fn reading(settings: &Settings) -> Option<Reading> {
    if !settings.clock_enabled {
        return None;
    }
    let now = Utc::now();
    if settings.clock_zone == LOCAL_ZONE {
        let local = now.with_timezone(&chrono::Local);
        return Some(Reading::from_time(local, settings.clock_seconds));
    }
    #[cfg(windows)]
    return platform::convert(
        now,
        platform::zones().get(&settings.clock_zone)?,
        settings.clock_seconds,
    );
    #[cfg(not(windows))]
    return None;
}

#[tauri::command]
pub fn get_clock_zones() -> Vec<String> {
    #[cfg(windows)]
    return platform::zones().keys().cloned().collect();
    #[cfg(not(windows))]
    return Vec::new();
}

#[tauri::command]
pub fn get_clock(app: AppHandle) -> Option<ClockInfo> {
    let settings = today::current_settings(&app);
    reading(&settings).map(|reading| reading.info(settings.fa_digits()))
}

/// A dedicated wait keeps calendar, theme, and events handling off the minute boundary.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let timer = crate::minute_timer::ClockTimer::new();
        let mut settings = today::current_settings(&app);
        let mut previous = reading(&settings);
        loop {
            timer.wait(settings.clock_enabled && settings.clock_seconds);
            settings = today::current_settings(&app);
            let current = reading(&settings);
            if current != previous {
                tray::refresh_clock(&app);
                previous = current;
            }
        }
    });
}

// Runs on the UI thread. Only the optional stacked layout needs a clock icon.
pub fn refresh(app: &AppHandle) -> tauri::Result<()> {
    let settings = today::current_settings(app);
    let reading = reading(&settings);
    let info = reading
        .as_ref()
        .map(|reading| reading.info(settings.fa_digits()));
    if let Err(err) = app.emit("clock-changed", &info) {
        eprintln!("[clock] cannot notify the taskbar surfaces: {err}");
    }
    let content = reading
        .zip(info)
        .filter(|_| settings.clock_layout == ClockLayout::Stacked)
        .map(|(reading, info)| {
            let light = theme::tray_icon_is_light();
            let parts = [
                (
                    reading.hour,
                    icon::text_color(settings.clock_hour_color.as_deref(), light),
                ),
                (
                    reading.minute,
                    icon::text_color(settings.clock_minute_color.as_deref(), light),
                ),
                (
                    reading.second.unwrap_or_default(),
                    icon::text_color(settings.clock_second_color.as_deref(), light),
                ),
            ];
            let rows = if reading.second.is_some() { 3 } else { 2 };
            let image = icon::render_clock(
                &parts[..rows],
                settings.fa_digits(),
                tray::tray_icon_px(app),
            );
            let zone = if settings.clock_zone == LOCAL_ZONE {
                "منطقهٔ ویندوز"
            } else {
                &settings.clock_zone
            };
            let tooltip = format!("{} • {}\n{}", info.time, zone, info.date);
            (image, tooltip)
        });
    tray::CLOCK_ICON.update(app, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_install_has_no_second_clock_reading() {
        assert!(reading(&Settings::default()).is_none());
    }

    #[test]
    fn formats_the_selected_zone_date_and_digits() {
        use chrono::TimeZone;
        let instant = Utc.with_ymd_and_hms(2026, 3, 21, 0, 7, 5).unwrap();
        let reading = Reading::from_time(instant, false);
        let fa = reading.info(true);
        assert_eq!(fa.date, "۱۴۰۵/۰۱/۰۱");
        assert_eq!(fa.time, "۰۰:۰۷");
        assert_eq!(reading.info(false).time, "00:07");
        assert_eq!(
            Reading::from_time(instant, true).info(true).time,
            "۰۰:۰۷:۰۵"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_zone_rules_handle_midnight_and_daylight_saving() {
        use chrono::TimeZone;

        let zones = platform::zones();
        let tehran = &zones[DEFAULT_ZONE];
        let instant = Utc
            .with_ymd_and_hms(2026, 3, 20, 21, 0, 0)
            .single()
            .unwrap();
        let reading = platform::convert(instant, tehran, false)
            .unwrap()
            .info(false);
        assert_eq!(
            (reading.time.as_str(), reading.date.as_str()),
            ("00:30", "1405/01/01")
        );

        let pacific = &zones["Pacific Standard Time"];
        for (month, expected) in [(1, "04:00"), (7, "05:00")] {
            let noon = Utc
                .with_ymd_and_hms(2026, month, 15, 12, 0, 0)
                .single()
                .unwrap();
            assert_eq!(
                platform::convert(noon, pacific, false)
                    .unwrap()
                    .info(false)
                    .time,
                expected
            );
        }
    }
}
