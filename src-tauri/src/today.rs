//! Today's date in all needed shapes, plus formatting helpers shared by the
//! tray, the menu, the tooltip and the IPC layer.

use chrono::Utc;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::events_api::{DayInfo, EventsState, LunarDate};
use crate::jalaali::{self, GDate, JDate};
use crate::settings::{CopyFormat, EventsMode, Settings, LUNAR_OFFSET_MAX, LUNAR_OFFSET_MIN};
use crate::{clock, AppState};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JalaliToday {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// 0 = شنبه … 6 = جمعه
    pub weekday: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GregorianParts {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayLabels {
    pub slash: String,
    pub long: String,
    pub iso: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayInfo {
    pub jalali: JalaliToday,
    pub gregorian: GregorianParts,
    pub lunar: Option<LunarDate>,
    pub holiday: bool,
    pub events: Vec<String>,
    pub labels: TodayLabels,
}

impl TodayInfo {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// Snapshot of the currently active settings.
pub fn current_settings(app: &AppHandle) -> Settings {
    let state = app.state::<AppState>();
    let settings = state
        .settings
        .read()
        .expect("settings lock poisoned")
        .clone();
    settings
}

pub fn dates(settings: &Settings) -> (JDate, GDate) {
    let now = Utc::now();
    let clock_date = clock::reading_at(settings, now).map(|reading| reading.date());
    jalaali::calendar_dates_at(now, clock_date)
}

pub fn today_jalaali(app: &AppHandle) -> JDate {
    dates(&current_settings(app)).0
}

/// Right-to-left mark (U+200F) prefixed to a line.
///
/// Windows draws native menus and tray tooltips as left-to-right paragraphs, so
/// a Persian line that mixes words with digits («۲۲ آبان ۱۴۰۵») would put the
/// day number on the wrong side. A leading RLM makes the whole line an RTL
/// paragraph, which is how these lines must read.
pub fn rtl(text: &str) -> String {
    format!("\u{200F}{text}")
}

/// Apply the `showEvents` / `eventsMode` display rules to a composed day.
///
/// `events` are the already composed (and already offset-aware) occasion
/// texts; `holiday` is that day's final holiday badge.
pub fn visible_events(settings: &Settings, events: &[String], holiday: bool) -> Vec<String> {
    if !settings.show_events {
        return Vec::new();
    }
    match settings.events_mode {
        EventsMode::All => events.to_vec(),
        EventsMode::Important => {
            if holiday {
                events.to_vec()
            } else {
                events.first().cloned().into_iter().collect()
            }
        }
    }
}

pub fn is_friday(j: &JDate) -> bool {
    jalaali::day_of_week(j.year, j.month, j.day) == 6
}

/// Optional fixed solar holidays: the API does not tell which occasion of a
/// mixed day caused the holiday, so the official Iranian solar dates are listed
/// explicitly. They stay holidays even when a lunar occasion on the same day
/// shifts away.
const SOLAR_HOLIDAYS: [(u32, u32); 10] = [
    (1, 1),
    (1, 2),
    (1, 3),
    (1, 4),   // نوروز
    (1, 12),  // روز جمهوری اسلامی
    (1, 13),  // روز طبیعت
    (3, 14),  // رحلت امام خمینی
    (3, 15),  // قیام ۱۵ خرداد
    (11, 22), // پیروزی انقلاب اسلامی
    (12, 29), // ملی شدن صنعت نفت
];

/// True for a fixed official solar (Jalali) holiday date.
pub fn is_solar_holiday(date: &JDate) -> bool {
    SOLAR_HOLIDAYS.contains(&(date.month, date.day))
}

/// True when the day has at least one lunar occasion (moves with the offset).
fn has_lunar(day: Option<&DayInfo>) -> bool {
    day.map(|day| day.events.iter().any(|occasion| occasion.lunar.is_some()))
        .unwrap_or(false)
}

/// Compose one day's occasion texts and holiday badge from raw API data.
///
/// `own` is the API entry for the day being shown, `shifted` the entry for
/// `date + offset` (the day the adjusted lunar date falls on) and `offset` is
/// the already clamped `lunarOffset` (-3 … +3). `friday` is the weekly holiday
/// of the day being shown.
///
/// Rules:
/// * `offset == 0` → every occasion of `own`, in API order;
/// * otherwise → fixed (`lunar == None`) occasions of `own` followed by lunar
///   (`lunar == Some`) occasions of `shifted`;
/// * the holiday badge moves with the lunar occasions: a day whose holiday comes
///   from a lunar occasion loses it, even when it also has a fixed (non-holiday)
///   occasion. Official solar holidays keep their badge via [`is_solar_holiday`].
pub fn compose_day(
    own: Option<&DayInfo>,
    shifted: Option<&DayInfo>,
    offset: i32,
    friday: bool,
) -> (bool, Vec<String>) {
    // With no offset the shifted lookup is the day itself; tolerate a caller
    // that passes `None` instead of `own`.
    let shifted = if offset == 0 { own } else { shifted };

    let mut events: Vec<String> = Vec::new();
    if offset == 0 {
        if let Some(day) = own {
            events.extend(day.events.iter().map(|occasion| occasion.text.clone()));
        }
    } else {
        if let Some(day) = own {
            events.extend(
                day.events
                    .iter()
                    .filter(|occasion| occasion.lunar.is_none())
                    .map(|occasion| occasion.text.clone()),
            );
        }
        if let Some(day) = shifted {
            events.extend(
                day.events
                    .iter()
                    .filter(|occasion| occasion.lunar.is_some())
                    .map(|occasion| occasion.text.clone()),
            );
        }
    }

    let own_holiday = own.map(|day| day.holiday).unwrap_or(false);
    let fixed_holiday = own_holiday && !has_lunar(own);
    let lunar_holiday = shifted.map(|day| day.holiday).unwrap_or(false) && has_lunar(shifted);
    (friday || fixed_holiday || lunar_holiday, events)
}

/// Occasions and holiday badge for `date`, honouring `settings.lunar_offset`.
///
/// The raw cache is never modified: the offset is applied by reading the entry
/// of the shifted day and composing the two in [`compose_day`]. Official solar
/// holidays stay holidays regardless of the shift.
pub fn displayed_occasions(
    events: &EventsState,
    settings: &Settings,
    date: &JDate,
) -> (bool, Vec<String>) {
    let offset = settings
        .lunar_offset
        .clamp(LUNAR_OFFSET_MIN, LUNAR_OFFSET_MAX);
    let own = events.cached_day(date.year, date.month, date.day);
    let shifted = if offset == 0 {
        None
    } else {
        let target = jalaali::add_days(date, offset);
        events.cached_day(target.year, target.month, target.day)
    };
    let (holiday, texts) = compose_day(own.as_ref(), shifted.as_ref(), offset, is_friday(date));
    (holiday || is_solar_holiday(date), texts)
}

/// Build the full today payload used by IPC and events.
pub fn build(app: &AppHandle) -> TodayInfo {
    let settings = current_settings(app);
    let (j, g) = dates(&settings);
    let events = app.state::<AppState>().events.clone();

    // Occasions and the holiday badge both honour `lunarOffset`.
    let (holiday, day_events) = displayed_occasions(&events, &settings, &j);
    // A non-zero lunar offset shifts the reported lunar date by whole days.
    let lunar = if settings.lunar_offset == 0 {
        events
            .cached_day(j.year, j.month, j.day)
            .and_then(|day| day.moon)
    } else {
        let target = jalaali::add_days(&j, settings.lunar_offset);
        events
            .cached_day(target.year, target.month, target.day)
            .and_then(|day| day.moon)
    };
    let visible = visible_events(&settings, &day_events, holiday);

    let fa = settings.fa_digits();
    TodayInfo {
        jalali: JalaliToday {
            year: j.year,
            month: j.month,
            day: j.day,
            weekday: jalaali::day_of_week(j.year, j.month, j.day),
        },
        gregorian: GregorianParts {
            year: g.year,
            month: g.month,
            day: g.day,
        },
        lunar,
        holiday,
        events: visible,
        labels: TodayLabels {
            slash: jalaali::format_slash(&j, fa),
            long: jalaali::format_long(&j, fa),
            iso: jalaali::format_iso(&g),
        },
    }
}

/// Copyable string for the given copy format.
pub fn copy_text_for(app: &AppHandle, format: CopyFormat) -> String {
    let settings = current_settings(app);
    let fa = settings.fa_digits();
    let (j, g) = dates(&settings);
    match format {
        CopyFormat::JalaliSlash => jalaali::format_slash(&j, fa),
        CopyFormat::JalaliLong => jalaali::format_long(&j, fa),
        CopyFormat::GregorianIso => jalaali::format_iso(&g),
    }
}

/// All three copy variants (used by the tray submenu).
pub fn copy_variants(app: &AppHandle) -> (String, String, String) {
    let settings = current_settings(app);
    let fa = settings.fa_digits();
    let (j, g) = dates(&settings);
    (
        jalaali::format_slash(&j, fa),
        jalaali::format_long(&j, fa),
        jalaali::format_iso(&g),
    )
}

/// Two-line tray tooltip: full Persian date, then occasion / holiday, then the
/// Gregorian date when the user enabled it.
pub fn tooltip_text(app: &AppHandle, settings: &Settings, j: &JDate, g: &GDate) -> String {
    let fa = settings.fa_digits();
    let weekday = jalaali::WEEKDAY_NAMES_FA
        .get(jalaali::day_of_week(j.year, j.month, j.day) as usize)
        .copied()
        .unwrap_or("");
    let mut lines = vec![rtl(&format!("{weekday} {}", jalaali::format_long(j, fa)))];

    if settings.show_events {
        let events = app.state::<AppState>().events.clone();
        let (holiday, day_events) = displayed_occasions(&events, settings, j);
        let first_event = visible_events(settings, &day_events, holiday)
            .into_iter()
            .next();
        match first_event {
            Some(event) => lines.push(event),
            None if holiday => lines.push("تعطیل رسمی".to_string()),
            None => {}
        }
    }

    if settings.show_gregorian {
        let line = format!("{} | {}", jalaali::format_iso(g), "میلادی");
        lines.push(rtl(&line));
        // Keep the line tidy: drop the redundant Persian word if too long.
        if lines.len() > 3 {
            lines.truncate(3);
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events_api::{LunarMonthDay, Occasion};

    /// Build a `DayInfo` from `(text, lunar month/day)` pairs.
    fn day(holiday: bool, events: &[(&str, Option<(u32, u32)>)]) -> DayInfo {
        DayInfo {
            holiday,
            events: events
                .iter()
                .map(|&(text, lunar)| Occasion {
                    text: text.to_string(),
                    lunar: lunar.map(|(month, lunar_day)| LunarMonthDay {
                        month,
                        day: lunar_day,
                    }),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn important_mode_limits_non_holiday_events() {
        let mut settings = Settings::default();
        settings.show_events = true;
        let events = vec!["اول".to_string(), "دوم".to_string()];
        assert_eq!(visible_events(&settings, &events, false), vec!["اول"]);
        assert_eq!(visible_events(&settings, &events, true).len(), 2);

        settings.events_mode = EventsMode::All;
        assert_eq!(visible_events(&settings, &events, false).len(), 2);

        settings.show_events = false;
        assert!(visible_events(&settings, &events, true).is_empty());
    }

    #[test]
    fn compose_day_offset_shifts_only_lunar_occasions() {
        let own = day(
            true,
            &[("روز آمار", None), ("شهادت حضرت فاطمه", Some((6, 3)))],
        );
        let shifted = day(
            false,
            &[("عید سعید فطر", Some((10, 1))), ("تعطیل رسمی", None)],
        );
        let (holiday, events) = compose_day(Some(&own), Some(&shifted), 1, false);
        // Fixed occasions of the own day first, then the shifted day's lunar
        // ones; the own day's lunar occasion has moved away.
        assert_eq!(events, vec!["روز آمار", "عید سعید فطر"]);
        // The badge moves with the lunar occasion, even though the day also had
        // a fixed occasion.
        assert!(!holiday);
    }

    #[test]
    fn compose_day_lunar_holiday_moves_to_the_shifted_day() {
        // A lunar-only holiday is no longer shown on the day it was shifted
        // away from…
        let own = day(true, &[("شهادت حضرت فاطمه", Some((6, 3)))]);
        let (holiday, events) = compose_day(Some(&own), Some(&day(false, &[])), -1, false);
        assert!(!holiday);
        assert!(events.is_empty());

        // …and is shown on the day the offset points at.
        let own = day(false, &[]);
        let shifted = day(true, &[("شهادت حضرت علی", Some((9, 21)))]);
        let (holiday, events) = compose_day(Some(&own), Some(&shifted), -1, false);
        assert!(holiday);
        assert_eq!(events, vec!["شهادت حضرت علی"]);
    }

    #[test]
    fn compose_day_keeps_fixed_holidays() {
        // Fixed-only holiday: the badge stays whatever the offset does.
        let own = day(true, &[("روز آمار", None)]);
        let (holiday, events) = compose_day(Some(&own), Some(&day(false, &[])), 3, false);
        assert!(holiday);
        assert_eq!(events, vec!["روز آمار"]);
    }

    #[test]
    fn compose_day_moves_a_lunar_holiday_on_a_mixed_day() {
        // Mixed day: the lunar occasion (and the badge caused by it) moves away;
        // the fixed occasion stays. The source day must lose the holiday.
        let own = day(
            true,
            &[("ولادت حضرت قائم", Some((8, 15))), ("روز آمار", None)],
        );
        let (holiday, events) = compose_day(Some(&own), Some(&day(false, &[])), 1, false);
        assert!(!holiday);
        assert_eq!(events, vec!["روز آمار"]);

        // The target day gains the badge through the shifted lunar occasion.
        let shifted = day(true, &[("شهادت حضرت علی", Some((9, 21)))]);
        let (holiday, events) = compose_day(Some(&day(false, &[])), Some(&shifted), -1, false);
        assert!(holiday);
        assert_eq!(events, vec!["شهادت حضرت علی"]);
    }

    #[test]
    fn solar_holidays_are_detected() {
        assert!(is_solar_holiday(&JDate {
            year: 1405,
            month: 1,
            day: 1
        }));
        assert!(is_solar_holiday(&JDate {
            year: 1405,
            month: 11,
            day: 22
        }));
        assert!(is_solar_holiday(&JDate {
            year: 1405,
            month: 12,
            day: 29
        }));
        assert!(!is_solar_holiday(&JDate {
            year: 1405,
            month: 8,
            day: 23
        }));
    }

    #[test]
    fn compose_day_without_shifted_data_moves_the_lunar_holiday() {
        let own = day(true, &[("ثابت", None), ("قمری", Some((9, 21)))]);
        let (holiday, events) = compose_day(Some(&own), None, 1, false);
        assert!(!holiday); // the lunar badge has moved to an unknown day
        assert_eq!(events, vec!["ثابت"]);
    }

    #[test]
    fn compose_day_offset_zero_equals_api_holiday_or_friday() {
        let own = day(false, &[("اول", Some((1, 9))), ("دوم", None)]);
        let (holiday, events) = compose_day(Some(&own), None, 0, false);
        assert!(!holiday);
        assert_eq!(events, vec!["اول", "دوم"]); // API order is preserved

        let (holiday, _) = compose_day(Some(&own), None, 0, true);
        assert!(holiday); // Friday

        let holiday_day = day(true, &[("عید سعید فطر", Some((10, 1)))]);
        let (holiday, events) = compose_day(Some(&holiday_day), None, 0, false);
        assert!(holiday);
        assert_eq!(events, vec!["عید سعید فطر"]);

        // Missing cache data behaves like an empty day.
        let (holiday, events) = compose_day(None, None, 0, false);
        assert!(!holiday);
        assert!(events.is_empty());
    }

    #[test]
    fn rtl_helper_prefixes_the_right_to_left_mark() {
        // Native Windows menus/tooltips need the RLM to lay a mixed
        // word/digit Persian line out right-to-left.
        assert_eq!(rtl("abc"), "\u{200F}abc");
        assert!(rtl("۲۲ آبان ۱۴۰۵").starts_with('\u{200F}'));
    }
}
