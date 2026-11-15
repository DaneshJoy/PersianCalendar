//! Application settings: schema, defaults, validation and persistence.
//!
//! Settings live in a single JSON file managed by `tauri-plugin-store`
//! (`settings.json`, key `settings`). The frontend reads/writes them through the
//! IPC commands only; Rust is the single owner of the file.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TrayIconMode {
    /// Only the Persian day number, e.g. «۲۲».
    #[default]
    Day,
    /// A fixed calendar glyph.
    Static,
}

impl<'de> Deserialize<'de> for TrayIconMode {
    /// Unknown values (e.g. the removed `dayMonth`) fall back to `Day` so a
    /// stale `settings.json` cannot reset the user's whole configuration.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(match raw.as_str() {
            "static" => Self::Static,
            _ => Self::Day,
        })
    }
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
    pub tray_icon_mode: TrayIconMode,
    pub week_start: u8,
    pub show_gregorian: bool,
    pub show_lunar: bool,
    /// Whole-day shift of the displayed lunar (Hijri) date: -3 … +3.
    pub lunar_offset: i32,
    pub show_events: bool,
    pub events_mode: EventsMode,
    pub show_week_number: bool,
    pub copy_format: CopyFormat,
    pub autostart: bool,
    pub close_on_blur: bool,
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
            tray_icon_mode: TrayIconMode::Day,
            week_start: 0,
            show_gregorian: false,
            show_lunar: false,
            lunar_offset: 0,
            show_events: true,
            events_mode: EventsMode::Important,
            show_week_number: false,
            copy_format: CopyFormat::JalaliSlash,
            autostart: false,
            close_on_blur: true,
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
    match app.store(STORE_FILE) {
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
    store.set(STORE_KEY, value);
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn removed_day_month_tray_mode_falls_back_to_day() {
        // `dayMonth` was removed; an old settings file must still load.
        let json = r#"{"trayIconMode":"dayMonth","theme":"dark"}"#;
        let settings: Settings = serde_json::from_str(json).expect("legacy tray mode parses");
        assert_eq!(settings.tray_icon_mode, TrayIconMode::Day);
        assert_eq!(settings.theme, ThemeMode::Dark);
    }

    #[test]
    fn tray_mode_round_trips_as_camel_case() {
        let mut settings = Settings::default();
        settings.tray_icon_mode = TrayIconMode::Static;
        let value = serde_json::to_value(&settings).expect("settings serialize");
        assert_eq!(value["trayIconMode"], "static");
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
