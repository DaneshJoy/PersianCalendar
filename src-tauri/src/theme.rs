//! Windows theme detection and resolution.
//!
//! The tray icon colour and the UI theme follow the Windows "app theme"
//! (`AppsUseLightTheme` under HKCU). The frontend receives resolved values via
//! the `theme-changed` event and `get_theme` command.

use crate::settings::{Settings, ThemeMode};

/// Payload of the `theme-changed` event and result of the `get_theme` command.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ThemeInfo {
    /// Resolved theme according to the user setting.
    pub mode: &'static str,
    /// Raw Windows/taskbar theme.
    pub system: &'static str,
}

/// Build the theme payload for the given settings.
pub fn info(settings: &Settings) -> ThemeInfo {
    ThemeInfo {
        mode: resolve(settings.theme),
        system: if system_theme_is_dark() {
            "dark"
        } else {
            "light"
        },
    }
}

/// Returns true when Windows is using the dark app theme.
#[cfg(windows)]
pub fn system_theme_is_dark() -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    match hkcu.open_subkey_with_flags(path, KEY_READ) {
        Ok(key) => match key.get_value::<u32, _>("AppsUseLightTheme") {
            Ok(light) => light == 0,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

#[cfg(not(windows))]
pub fn system_theme_is_dark() -> bool {
    false
}

/// Resolve the effective theme for the UI/tray: `"light"` or `"dark"`.
pub fn resolve(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Light => "light",
        ThemeMode::Dark => "dark",
        ThemeMode::Auto => {
            if system_theme_is_dark() {
                "dark"
            } else {
                "light"
            }
        }
    }
}

/// Text colour to render on the tray icon so it stays readable on the taskbar.
pub fn tray_icon_is_light() -> bool {
    system_theme_is_dark()
}
