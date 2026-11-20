//! Jalali (Shamsi/Persian) <-> Gregorian date conversion.
//!
//! A faithful Rust port of the algorithms in `jalaali-js` (v1.2/2.0, MIT) by
//! Behnam Mohammadi / Reza Samadzadeh, with the same `breaks` table and the
//! same `jalCal` / `g2d` / `d2g` / `j2d` / `d2j` routines.
//!
//! `div` is truncating integer division and `mod` is the matching remainder
//! (sign of the dividend) — exactly what JavaScript's `~~` and `%` do for the
//! integer operands used here.
//!
//! The calendar core of the app must work fully offline, so nothing in this
//! module touches the network. All functions are total: out-of-range years are
//! clamped instead of panicking (the app only ever uses years around 1400).

use chrono::{DateTime, Datelike, Local, Utc};
use serde::Serialize;

/// A Jalali (Shamsi) calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct JDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

/// A Gregorian (proleptic) calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

/// Jalali month names, Farvardin = index 0.
pub const MONTH_NAMES_FA: [&str; 12] = [
    "فروردین",
    "اردیبهشت",
    "خرداد",
    "تیر",
    "مرداد",
    "شهریور",
    "مهر",
    "آبان",
    "آذر",
    "دی",
    "بهمن",
    "اسفند",
];

/// Jalali weekday names, Saturday = index 0 (matches `day_of_week`).
pub const WEEKDAY_NAMES_FA: [&str; 7] = [
    "شنبه",
    "یکشنبه",
    "دوشنبه",
    "سه‌شنبه",
    "چهارشنبه",
    "پنجشنبه",
    "جمعه",
];

/// The limiting years of the 33-year leap cycle, identical to jalaali-js.
const BREAKS: [i32; 20] = [
    -61, 9, 38, 199, 426, 686, 756, 818, 1111, 1181, 1210, 1635, 2060, 2097, 2192, 2262, 2324,
    2394, 2456, 3178,
];

/// Truncating integer division (like JavaScript `~~(a / b)`).
#[inline]
fn div(a: i64, b: i64) -> i64 {
    a / b
}

/// Remainder matching `div` (like JavaScript `a % b` for integers).
#[inline]
fn mod_(a: i64, b: i64) -> i64 {
    a - div(a, b) * b
}

struct JalCal {
    leap: i32,
    gy: i32,
    march: i32,
}

/// Port of jalaali-js `jalCal(jy)`.
///
/// `without_leap` skips the leap computation (used by `j2d`, which does not
/// need it). Years outside the supported range are clamped so this never
/// panics; valid input is `-61..=3177`.
fn jal_cal(jy: i32, without_leap: bool) -> JalCal {
    let jy = jy.clamp(BREAKS[0], BREAKS[BREAKS.len() - 1] - 1);
    let bl = BREAKS.len();
    let gy = jy + 621;
    let mut leap_j: i32 = -14;
    let mut jp = BREAKS[0];
    let mut jump = 0i32;

    // Find the limiting years for the Jalali year jy. `jump` is always
    // assigned on the first iteration (the range is never empty), and the
    // initial value only exists to keep the type checker happy.
    for i in 1..bl {
        let jm = BREAKS[i];
        jump = jm - jp;
        if jy < jm {
            break;
        }
        leap_j = leap_j + div(jump as i64, 33) as i32 * 8 + div(mod_(jump as i64, 33), 4) as i32;
        jp = jm;
    }
    let mut n = jy - jp;

    // Number of leap years from AD 621 to the beginning of jy.
    leap_j = leap_j + div(n as i64, 33) as i32 * 8 + div(mod_(n as i64, 33) + 3, 4) as i32;
    if mod_(jump as i64, 33) == 4 && jump - n == 4 {
        leap_j += 1;
    }

    // The same in the Gregorian calendar, up to the year gy.
    let leap_g = div(gy as i64, 4) - div((div(gy as i64, 100) + 1) * 3, 4) - 150;

    // Gregorian date of Farvardin the 1st.
    let march = 20 + leap_j - leap_g as i32;

    // How many years have passed since the last leap year.
    let mut leap = 0i32;
    if !without_leap {
        if jump - n < 6 {
            n = n - jump + div((jump + 4) as i64, 33) as i32 * 33;
        }
        leap = mod_(mod_((n + 1) as i64, 33) - 1, 4) as i32;
        if leap == -1 {
            leap = 4;
        }
    }

    JalCal { leap, gy, march }
}

/// Gregorian date -> Julian day number (port of `g2d`).
fn g2d(gy: i32, gm: i32, gd: i32) -> i64 {
    let (gy, gm, gd) = (gy as i64, gm as i64, gd as i64);
    let mut d =
        div((gy + div(gm - 8, 6) + 100100) * 1461, 4) + div(153 * mod_(gm + 9, 12) + 2, 5) + gd
            - 34840408;
    d = d - div(div(gy + 100100 + div(gm - 8, 6), 100) * 3, 4) + 752;
    d
}

/// Julian day number -> Gregorian date (port of `d2g`).
fn d2g(jdn: i64) -> GDate {
    let mut j = 4 * jdn + 139361631;
    j = j + div(div(4 * jdn + 183187720, 146097) * 3, 4) * 4 - 3908;
    let i = div(mod_(j, 1461), 4) * 5 + 308;
    let gd = div(mod_(i, 153), 5) + 1;
    let gm = mod_(div(i, 153), 12) + 1;
    let gy = div(j, 1461) - 100100 + div(8 - gm, 6);
    GDate {
        year: gy as i32,
        month: gm as u32,
        day: gd as u32,
    }
}

/// Jalali date -> Julian day number (port of `j2d`).
fn j2d(jy: i32, jm: u32, jd: u32) -> i64 {
    let r = jal_cal(jy, true);
    let (jm, jd) = (jm as i64, jd as i64);
    g2d(r.gy, 3, r.march) + (jm - 1) * 31 - div(jm, 7) * (jm - 7) + jd - 1
}

/// Julian day number -> Jalali date (port of `d2j`).
fn d2j(jdn: i64) -> JDate {
    let gy = d2g(jdn).year;
    let mut jy = gy - 621;
    let r = jal_cal(jy, false);
    let jdn1f = g2d(gy, 3, r.march);

    // Days that passed since 1 Farvardin.
    let mut k = jdn - jdn1f;
    if k >= 0 {
        if k <= 185 {
            // The first 6 months.
            return JDate {
                year: jy,
                month: (1 + div(k, 31)) as u32,
                day: (mod_(k, 31) + 1) as u32,
            };
        }
        k -= 186;
    } else {
        // Previous Jalali year.
        jy -= 1;
        k += 179;
        if r.leap == 1 {
            k += 1;
        }
    }
    JDate {
        year: jy,
        month: (7 + div(k, 30)) as u32,
        day: (mod_(k, 30) + 1) as u32,
    }
}

/// Convert a Gregorian date to Jalali.
pub fn to_jalaali(gy: i32, gm: u32, gd: u32) -> JDate {
    d2j(g2d(gy, gm as i32, gd as i32))
}

/// Convert a Jalali date to Gregorian.
pub fn to_gregorian(jy: i32, jm: u32, jd: u32) -> GDate {
    d2g(j2d(jy, jm, jd))
}

/// Shift a Jalali date by a whole number of days (negative = backwards).
///
/// Used by the lunar-date offset setting: the API reports the lunar date for
/// one Jalali day, so a ±n day shift is just a lookup `n` days away.
pub fn add_days(date: &JDate, days: i32) -> JDate {
    d2j(j2d(date.year, date.month, date.day) + days as i64)
}

/// Whether the given Jalali year is a leap year (Esfand has 30 days).
pub fn is_leap_jalaali(jy: i32) -> bool {
    jal_cal(jy, false).leap == 0
}

/// Number of days in a Jalali month.
pub fn jalaali_month_length(jy: i32, jm: u32) -> u32 {
    if jm <= 6 {
        return 31;
    }
    if jm <= 11 {
        return 30;
    }
    if is_leap_jalaali(jy) {
        return 30;
    }
    29
}

/// Day of week for a Jalali date: 0 = Saturday .. 6 = Friday.
pub fn day_of_week(jy: i32, jm: u32, jd: u32) -> u32 {
    let g = to_gregorian(jy, jm, jd);
    match chrono::NaiveDate::from_ymd_opt(g.year, g.month, g.day) {
        // Saturday = 0: Monday-based numbering (Mon=0 .. Sun=6) shifted by +2.
        Some(date) => (date.weekday().num_days_from_monday() + 2) % 7,
        None => 0,
    }
}

/// Convert ASCII digits `0-9` to Persian digits `۰-۹`.
pub fn fa_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '0'..='9' => char::from_u32('۰' as u32 + (c as u32 - '0' as u32)).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// Convert Persian (`۰-۹`) and Arabic-Indic (`٠-٩`) digits to ASCII `0-9`.
#[cfg(test)]
pub fn en_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '۰'..='۹' => char::from_u32('0' as u32 + (c as u32 - '۰' as u32)).unwrap_or(c),
            '٠'..='٩' => char::from_u32('0' as u32 + (c as u32 - '٠' as u32)).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// Format as `۱۴۰۵/۰۸/۲۲` (or `1405/08/22` when `fa` is false).
pub fn format_slash(j: &JDate, fa: bool) -> String {
    let s = format!("{:04}/{:02}/{:02}", j.year, j.month, j.day);
    if fa {
        fa_digits(&s)
    } else {
        s
    }
}

/// Format as `۲۲ آبان ۱۴۰۵` (or `22 آبان 1405` when `fa` is false).
pub fn format_long(j: &JDate, fa: bool) -> String {
    let month = MONTH_NAMES_FA
        .get(j.month.wrapping_sub(1) as usize)
        .copied()
        .unwrap_or("");
    let s = format!("{} {} {}", j.day, month, j.year);
    if fa {
        fa_digits(&s)
    } else {
        s
    }
}

/// Format a Gregorian date as `2026-11-13`.
pub fn format_iso(g: &GDate) -> String {
    format!("{:04}-{:02}-{:02}", g.year, g.month, g.day)
}

/// Resolve the date shared by calendar surfaces from the enabled clock reading.
pub fn calendar_dates_at(instant: DateTime<Utc>, clock_date: Option<JDate>) -> (JDate, GDate) {
    match clock_date {
        Some(date) => (date, to_gregorian(date.year, date.month, date.day)),
        None => local_dates_at(instant),
    }
}

/// The system-local date of a specific instant as `(Jalali, Gregorian)`.
pub fn local_dates_at(instant: DateTime<Utc>) -> (JDate, GDate) {
    let now = instant.with_timezone(&Local);
    let g = GDate {
        year: now.year(),
        month: now.month(),
        day: now.day(),
    };
    let j = to_jalaali(g.year, g.month, g.day);
    (j, g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_known_gregorian_to_jalaali() {
        // Today in the project (2026-11-13) is 1405/08/22.
        assert_eq!(
            to_jalaali(2026, 11, 13),
            JDate {
                year: 1405,
                month: 8,
                day: 22
            }
        );
        // 1405/08/01 == 2026-10-23, 1405/01/01 == 2026-03-21.
        assert_eq!(
            to_jalaali(2026, 10, 23),
            JDate {
                year: 1405,
                month: 8,
                day: 1
            }
        );
        assert_eq!(
            to_jalaali(2026, 3, 21),
            JDate {
                year: 1405,
                month: 1,
                day: 1
            }
        );
        // A well-known date: 2024-01-01 == 1402/10/11.
        assert_eq!(
            to_jalaali(2024, 1, 1),
            JDate {
                year: 1402,
                month: 10,
                day: 11
            }
        );
    }

    #[test]
    fn converts_known_jalaali_to_gregorian() {
        assert_eq!(
            to_gregorian(1405, 8, 22),
            GDate {
                year: 2026,
                month: 11,
                day: 13
            }
        );
        assert_eq!(
            to_gregorian(1405, 8, 1),
            GDate {
                year: 2026,
                month: 10,
                day: 23
            }
        );
        assert_eq!(
            to_gregorian(1405, 1, 1),
            GDate {
                year: 2026,
                month: 3,
                day: 21
            }
        );
        // Leap year 1403 has Esfand 30.
        assert_eq!(
            to_gregorian(1403, 12, 30),
            GDate {
                year: 2025,
                month: 3,
                day: 20
            }
        );
    }

    #[test]
    fn round_trips_across_a_decade() {
        for jy in 1395..=1410 {
            for jm in 1..=12u32 {
                let len = jalaali_month_length(jy, jm);
                for jd in 1..=len {
                    let g = to_gregorian(jy, jm, jd);
                    let back = to_jalaali(g.year, g.month, g.day);
                    assert_eq!(
                        back,
                        JDate {
                            year: jy,
                            month: jm,
                            day: jd
                        },
                        "round trip failed for {jy}/{jm}/{jd} -> {g:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn shifts_dates_by_whole_days() {
        assert_eq!(
            add_days(
                &JDate {
                    year: 1405,
                    month: 8,
                    day: 1
                },
                -1
            ),
            JDate {
                year: 1405,
                month: 7,
                day: 30
            }
        );
        assert_eq!(
            add_days(
                &JDate {
                    year: 1405,
                    month: 8,
                    day: 30
                },
                1
            ),
            JDate {
                year: 1405,
                month: 9,
                day: 1
            }
        );
        assert_eq!(
            add_days(
                &JDate {
                    year: 1405,
                    month: 12,
                    day: 29
                },
                1
            ),
            JDate {
                year: 1406,
                month: 1,
                day: 1
            }
        );
        // Every supported lunar offset must round-trip.
        let date = JDate {
            year: 1405,
            month: 8,
            day: 22,
        };
        for shift in -3..=3 {
            assert_eq!(
                add_days(&add_days(&date, shift), -shift),
                date,
                "shift {shift} did not round-trip"
            );
        }
    }

    #[test]
    fn detects_leap_years() {
        assert!(is_leap_jalaali(1399));
        assert!(is_leap_jalaali(1403));
        assert!(!is_leap_jalaali(1404));
        assert!(!is_leap_jalaali(1405));
    }

    #[test]
    fn month_lengths_follow_the_rule() {
        for jm in 1..=6u32 {
            assert_eq!(jalaali_month_length(1405, jm), 31, "month {jm}");
        }
        for jm in 7..=11u32 {
            assert_eq!(jalaali_month_length(1405, jm), 30, "month {jm}");
        }
        assert_eq!(jalaali_month_length(1403, 12), 30, "leap Esfand");
        assert_eq!(jalaali_month_length(1404, 12), 29, "common Esfand");
        assert_eq!(jalaali_month_length(1405, 12), 29, "common Esfand");
    }

    #[test]
    fn day_of_week_matches_chrono() {
        // 2026-11-13 is a Friday -> 6 in Saturday-based numbering.
        assert_eq!(day_of_week(1405, 8, 22), 6);
        // 1405/01/01 == 2026-03-21, a Saturday -> 0.
        assert_eq!(day_of_week(1405, 1, 1), 0);
        // 1403/12/30 == 2025-03-20, a Thursday -> 5.
        assert_eq!(day_of_week(1403, 12, 30), 5);
        for jy in 1400..=1406 {
            for jd in [1u32, 15, 29] {
                let g = to_gregorian(jy, 1, jd);
                let expected = chrono::NaiveDate::from_ymd_opt(g.year, g.month, g.day)
                    .map(|d| (d.weekday().num_days_from_monday() + 2) % 7)
                    .unwrap_or(0);
                assert_eq!(day_of_week(jy, 1, jd), expected);
            }
        }
    }

    #[test]
    fn formats_slash_and_long() {
        let j = JDate {
            year: 1405,
            month: 8,
            day: 22,
        };
        assert_eq!(format_slash(&j, true), "۱۴۰۵/۰۸/۲۲");
        assert_eq!(format_slash(&j, false), "1405/08/22");
        assert_eq!(format_long(&j, true), "۲۲ آبان ۱۴۰۵");
        assert_eq!(format_long(&j, false), "22 آبان 1405");
        // Single-digit day is not zero-padded in the long form.
        let j = JDate {
            year: 1405,
            month: 1,
            day: 1,
        };
        assert_eq!(format_long(&j, true), "۱ فروردین ۱۴۰۵");
    }

    #[test]
    fn formats_iso_and_digits() {
        assert_eq!(
            format_iso(&GDate {
                year: 2026,
                month: 11,
                day: 13
            }),
            "2026-11-13"
        );
        assert_eq!(
            format_iso(&GDate {
                year: 2026,
                month: 1,
                day: 3
            }),
            "2026-01-03"
        );
        assert_eq!(fa_digits("1405/08/22"), "۱۴۰۵/۰۸/۲۲");
        assert_eq!(en_digits("۱۴۰۵/۰۸/۲۲"), "1405/08/22");
        assert_eq!(en_digits("٣ Ramadan"), "3 Ramadan");
        // Non-digit characters are left untouched.
        assert_eq!(fa_digits("آبان 1405"), "آبان ۱۴۰۵");
    }

    #[test]
    fn today_local_round_trips() {
        let (j, g) = local_dates_at(Utc::now());
        assert_eq!(to_jalaali(g.year, g.month, g.day), j);
        assert!((1..=12).contains(&j.month));
        assert!((1..=31).contains(&j.day));
        assert!((1..=12).contains(&g.month));
    }

    #[test]
    fn calendar_rolls_over_at_tehran_midnight_while_the_system_is_still_on_the_29th() {
        use chrono::{FixedOffset, TimeZone};

        let tehran = FixedOffset::east_opt(3 * 3600 + 30 * 60).unwrap();
        let system = FixedOffset::west_opt(8 * 3600).unwrap();
        let midnight = Utc.with_ymd_and_hms(2026, 11, 20, 20, 30, 0).unwrap();
        let before = midnight - chrono::Duration::seconds(1);
        let clock_date = |instant: DateTime<Utc>| {
            let time = instant.with_timezone(&tehran);
            to_jalaali(time.year(), time.month(), time.day())
        };
        let system_date = midnight.with_timezone(&system);
        let old = calendar_dates_at(before, Some(clock_date(before)));
        let new = calendar_dates_at(midnight, Some(clock_date(midnight)));
        assert_eq!(system_date.day(), 20);
        assert_eq!((old.0.year, old.0.month, old.0.day), (1405, 8, 29));
        assert_eq!((new.0.year, new.0.month, new.0.day), (1405, 8, 30));
        assert_eq!(format_iso(&new.1), "2026-11-21");
        assert_eq!(calendar_dates_at(midnight, None), local_dates_at(midnight));
    }
}
