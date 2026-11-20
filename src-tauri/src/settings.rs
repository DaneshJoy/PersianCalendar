//! Application settings: schema, defaults, validation and persistence.
//!
//! Settings live in a single JSON file managed by `tauri-plugin-store`
//! (`settings.json`, key `settings`). The frontend reads/writes them through the
//! IPC commands only; Rust is the single owner of the file.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";
const STORE_KEY: &str = "settings";

/// Allowed whole-day shift for the displayed lunar (Hijri) date.
pub const LUNAR_OFFSET_MIN: i32 = -3;
pub const LUNAR_OFFSET_MAX: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Auto,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DigitStyle {
    #[default]
    Fa,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FontChoice {
    #[default]
    Vazirmatn,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ClockLayout {
    #[default]
    Inline,
    Stacked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EventsMode {
    /// Holidays show all their events, other days only the first one.
    #[default]
    Important,
    /// Every event of every day.
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CopyFormat {
    #[default]
    JalaliSlash,
    JalaliLong,
    GregorianIso,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WidgetSize {
    Small,
    #[default]
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: ThemeMode,
    pub accent: String,
    pub digits: DigitStyle,
    pub font: FontChoice,
    pub clock_enabled: bool,
    pub clock_layout: ClockLayout,
    pub clock_zone: String,
    pub clock_hour_color: Option<String>,
    pub clock_minute_color: Option<String>,
    pub clock_seconds: bool,
    pub clock_second_color: Option<String>,
    pub calendar_day_color: Option<String>,
    pub capsule_enabled: bool,
    pub capsule_interval_seconds: u32,
    pub week_start: u8,
    pub show_gregorian: bool,
    pub show_lunar: bool,
    /// Whole-day shift of the displayed lunar (Hijri) date: -3 … +3.
    pub lunar_offset: i32,
    pub show_events: bool,
    pub online_events: bool,
    pub events_mode: EventsMode,
    pub show_week_number: bool,
    pub copy_format: CopyFormat,
    pub autostart: bool,
    pub widget_enabled: bool,
    pub widget_size: WidgetSize,
    pub widget_opacity: u8,
    pub widget_locked: bool,
    pub widget_x: Option<i32>,
    pub widget_y: Option<i32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Auto,
            accent: "#2f6fed".to_string(),
            digits: DigitStyle::Fa,
            font: FontChoice::Vazirmatn,
            clock_enabled: false,
            clock_layout: ClockLayout::Inline,
            clock_zone: crate::clock::DEFAULT_ZONE.to_string(),
            clock_hour_color: None,
            clock_minute_color: None,
            clock_seconds: false,
            clock_second_color: None,
            calendar_day_color: None,
            capsule_enabled: false,
            capsule_interval_seconds: 60,
            week_start: 0,
            show_gregorian: false,
            show_lunar: false,
            lunar_offset: 0,
            show_events: true,
            online_events: false,
            events_mode: EventsMode::Important,
            show_week_number: false,
            copy_format: CopyFormat::JalaliSlash,
            autostart: false,
            widget_enabled: false,
            widget_size: WidgetSize::Full,
            widget_opacity: 92,
            widget_locked: false,
            widget_x: None,
            widget_y: None,
        }
    }
}

impl Settings {
    /// True when digits should be rendered as ۰-۹.
    pub fn fa_digits(&self) -> bool {
        self.digits == DigitStyle::Fa
    }

    /// Clamp/normalise values arriving from the frontend or a stale config file.
    pub fn sanitize(&mut self) {
        for color in [
            &mut self.clock_hour_color,
            &mut self.clock_minute_color,
            &mut self.clock_second_color,
            &mut self.calendar_day_color,
        ] {
            if color
                .as_deref()
                .is_some_and(|value| !is_valid_hex_color(value))
            {
                *color = None;
            }
        }
        if self.capsule_interval_seconds != 30 {
            self.capsule_interval_seconds = self.capsule_interval_seconds.clamp(60, 1800) / 60 * 60;
        }
        if !crate::clock::valid_zone(&self.clock_zone) {
            self.clock_zone = crate::clock::LOCAL_ZONE.to_string();
        }
        if self.week_start > 2 {
            self.week_start = 0;
        }
        self.lunar_offset = self.lunar_offset.clamp(LUNAR_OFFSET_MIN, LUNAR_OFFSET_MAX);
        if self.widget_opacity < 30 {
            self.widget_opacity = 30;
        }
        if self.widget_opacity > 100 {
            self.widget_opacity = 100;
        }
        if !is_valid_hex_color(&self.accent) {
            self.accent = Settings::default().accent;
        }
    }
}

pub fn is_valid_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

/// Load settings from the store, falling back to defaults for missing keys.
pub fn load(app: &AppHandle) -> Settings {
    let mut settings = Settings::default();
    match app.store_builder(STORE_FILE).disable_auto_save().build() {
        Ok(store) => {
            if let Some(value) = store.get(STORE_KEY) {
                match serde_json::from_value::<Settings>(value) {
                    Ok(parsed) => settings = parsed,
                    Err(err) => eprintln!("[settings] failed to parse stored settings: {err}"),
                }
            }
        }
        Err(err) => eprintln!("[settings] failed to open store: {err}"),
    }
    settings.sanitize();
    settings
}

/// Persist settings to the store (the file is written by the store plugin).
pub fn persist(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    let previous = store.get(STORE_KEY);
    store.set(STORE_KEY, value);
    if let Err(err) = store.save() {
        if let Some(previous) = previous {
            store.set(STORE_KEY, previous);
        } else {
            store.delete(STORE_KEY);
        }
        return Err(err.to_string());
    }
    Ok(())
}

/// Persist a single edit before exposing it to other windows and the scheduler.
pub fn update(
    app: &AppHandle,
    edit: impl FnOnce(&Settings) -> Result<Settings, String>,
) -> Result<Settings, String> {
    let state = app.state::<crate::AppState>();
    let mut current = state
        .settings
        .write()
        .map_err(|_| "settings lock poisoned")?;
    let mut next = edit(&current)?;
    next.sanitize();

    let autostart_changed = next.autostart != current.autostart;
    if autostart_changed {
        next.autostart =
            crate::autostart::set_enabled(next.autostart).map_err(|err| err.to_string())?;
    }
    if let Err(err) = persist(app, &next) {
        if autostart_changed {
            if let Err(rollback) = crate::autostart::set_enabled(current.autostart) {
                current.autostart = crate::autostart::is_enabled();
                return Err(format!("{err}; autostart rollback failed: {rollback}"));
            }
        }
        return Err(err);
    }
    *current = next.clone();
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_settings_keep_explicit_choices_for_clock_and_capsule() {
        let json = r#"{"clockEnabled":true,"clockZone":"Iran Standard Time","capsuleEnabled":true,"digits":"en"}"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        assert!(settings.clock_enabled);
        assert_eq!(settings.clock_zone, "Iran Standard Time");
        assert_eq!(settings.clock_minute_color, None);
        assert!(!settings.clock_seconds);
        assert_eq!(settings.clock_second_color, None);
        assert_eq!(settings.calendar_day_color, None);
        assert!(settings.capsule_enabled);
        assert_eq!(settings.capsule_interval_seconds, 60);
    }

    #[test]
    fn blank_settings_open_with_native_date_and_no_extra_clock() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.fa_digits());
        assert!(!settings.clock_enabled);
        assert!(!settings.capsule_enabled);
    }

    #[test]
    fn validates_independent_colors_and_the_rotation_interval() {
        let mut settings = Settings {
            clock_hour_color: Some("#7046ab".into()),
            clock_minute_color: Some("invalid".into()),
            clock_second_color: Some("#c3aa11".into()),
            calendar_day_color: Some("#e01caa".into()),
            capsule_interval_seconds: 2000,
            ..Settings::default()
        };
        settings.sanitize();
        assert_eq!(settings.clock_hour_color.as_deref(), Some("#7046ab"));
        assert_eq!(settings.clock_minute_color, None);
        assert_eq!(settings.clock_second_color.as_deref(), Some("#c3aa11"));
        assert_eq!(settings.calendar_day_color.as_deref(), Some("#e01caa"));
        assert_eq!(settings.capsule_interval_seconds, 1800);
    }

    #[test]
    fn defaults_include_a_zero_lunar_offset() {
        assert_eq!(Settings::default().lunar_offset, 0);
    }

    #[test]
    fn sanitize_clamps_the_lunar_offset() {
        let mut settings = Settings::default();
        settings.lunar_offset = 9;
        settings.sanitize();
        assert_eq!(settings.lunar_offset, LUNAR_OFFSET_MAX);

        settings.lunar_offset = -9;
        settings.sanitize();
        assert_eq!(settings.lunar_offset, LUNAR_OFFSET_MIN);

        for value in LUNAR_OFFSET_MIN..=LUNAR_OFFSET_MAX {
            settings.lunar_offset = value;
            settings.sanitize();
            assert_eq!(settings.lunar_offset, value);
        }
    }

    #[test]
    fn upgrades_old_settings_to_inline_clock_and_preserves_colors() {
        let json = r##"{"trayIconMode":"day","theme":"dark","clockHourColor":"#d9485f"}"##;
        let settings: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.clock_layout, ClockLayout::Inline);
        assert_eq!(settings.clock_hour_color.as_deref(), Some("#d9485f"));
        assert_eq!(settings.theme, ThemeMode::Dark);
    }

    #[test]
    fn deserializes_a_legacy_settings_file_without_the_offset() {
        // Files written before the lunar shift existed must keep working.
        let json = r#"{"theme":"dark","digits":"en","weekStart":1,"widgetOpacity":50}"#;
        let settings: Settings = serde_json::from_str(json).expect("legacy settings parse");
        assert_eq!(settings.lunar_offset, 0);
        assert_eq!(settings.digits, DigitStyle::En);
        assert_eq!(settings.week_start, 1);
    }

    #[test]
    fn serializes_the_offset_in_camel_case() {
        let value = serde_json::to_value(Settings::default()).expect("settings serialize");
        assert_eq!(value["lunarOffset"], 0);
    }
}
