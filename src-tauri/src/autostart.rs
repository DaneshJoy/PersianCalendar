//! Launch-at-login registration for the current Windows user.

#[cfg(windows)]
mod platform {
    use std::{ffi::OsString, io, path::Path};
    use winreg::{
        enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE},
        RegKey,
    };

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const NAME: &str = "PersianCalendar";

    fn quoted(path: &Path) -> OsString {
        let mut command = OsString::from("\"");
        command.push(path);
        command.push("\"");
        command
    }

    fn blocked(hkcu: &RegKey) -> bool {
        hkcu.open_subkey_with_flags(APPROVED, KEY_READ)
            .and_then(|key| key.get_raw_value(NAME))
            .is_ok_and(|value| matches!(value.bytes.first(), Some(3 | 7)))
    }

    pub fn is_enabled() -> bool {
        let Ok(path) = std::env::current_exe() else {
            return false;
        };
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let Ok(saved) = hkcu
            .open_subkey_with_flags(RUN, KEY_READ)
            .and_then(|key| key.get_value::<OsString, _>(NAME))
        else {
            return false;
        };
        saved == quoted(&path) && !blocked(&hkcu)
    }

    pub fn repair_legacy() -> io::Result<()> {
        let path = std::env::current_exe()?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let mut legacy = path.as_os_str().to_owned();
        legacy.push(" ");
        match hkcu
            .open_subkey_with_flags(RUN, KEY_READ)
            .and_then(|key| key.get_value::<OsString, _>(NAME))
        {
            Ok(saved) if saved == legacy => hkcu
                .open_subkey_with_flags(RUN, KEY_SET_VALUE)?
                .set_value(NAME, &quoted(&path)),
            Ok(_) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }

    pub fn set_enabled(enabled: bool) -> io::Result<bool> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if enabled {
            if blocked(&hkcu) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "disabled in Windows Startup Apps",
                ));
            }
            let path = std::env::current_exe()?;
            let (run, _) = hkcu.create_subkey_with_flags(RUN, KEY_SET_VALUE)?;
            run.set_value(NAME, &quoted(&path))?;
        } else {
            let result = hkcu
                .open_subkey_with_flags(RUN, KEY_SET_VALUE)
                .and_then(|run| run.delete_value(NAME));
            if let Err(err) = result {
                if err.kind() != io::ErrorKind::NotFound {
                    return Err(err);
                }
            }
        }
        Ok(enabled)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn quotes_the_whole_startup_path() {
            let path = Path::new(r"C:\Users\Test User\App\persian-calendar.exe");
            assert_eq!(
                quoted(path),
                r#""C:\Users\Test User\App\persian-calendar.exe""#
            );
        }
    }
}

#[cfg(windows)]
pub use platform::{is_enabled, repair_legacy, set_enabled};

#[cfg(not(windows))]
pub fn is_enabled() -> bool {
    false
}

#[cfg(not(windows))]
pub fn repair_legacy() -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(windows))]
pub fn set_enabled(_: bool) -> std::io::Result<bool> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Windows only",
    ))
}
