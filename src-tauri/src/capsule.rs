use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, WebviewWindow};

use crate::{
    clock, jalaali,
    settings::{ClockLayout, Settings},
    theme, today, tray, windows, AppState,
};

pub const LABEL: &str = "capsule";
pub const CALENDAR_VISIBLE: u8 = 1;
pub const CLOCK_VISIBLE: u8 = 2;
static SHOWN: AtomicU8 = AtomicU8::new(0);
static FALLBACK_ALLOWED: AtomicBool = AtomicBool::new(true);
#[cfg(windows)]
static PRESENTING: AtomicBool = AtomicBool::new(false);

pub fn shown() -> u8 {
    SHOWN.load(Ordering::Acquire)
}

pub fn fallback_allowed() -> bool {
    FALLBACK_ALLOWED.load(Ordering::Acquire)
}

fn set_fallback(app: &AppHandle, allowed: bool) {
    if FALLBACK_ALLOWED.swap(allowed, Ordering::AcqRel) != allowed {
        tray::schedule_date_refresh(app);
    }
}

fn publish_shown(app: &AppHandle, shown: u8) {
    SHOWN.store(shown, Ordering::Release);
    if shown != 0 {
        set_fallback(app, false);
    }
}

fn inline(settings: &Settings) -> bool {
    settings.clock_enabled && settings.clock_layout == ClockLayout::Inline
}

pub fn requested(settings: &Settings) -> bool {
    settings.capsule_enabled || inline(settings)
}

#[derive(Deserialize)]
pub struct SurfaceBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[tauri::command]
pub fn set_taskbar_surface(
    window: WebviewWindow,
    bounds: Option<SurfaceBounds>,
    calendar: bool,
    clock: bool,
) -> Result<(), String> {
    if window.label() != LABEL {
        return Err("invalid taskbar surface".to_string());
    }
    #[cfg(windows)]
    return platform::set_surface(&window, bounds, calendar, clock).inspect_err(|_| {
        use tauri::Manager;
        let _ = window.set_ignore_cursor_events(true);
        publish_shown(window.app_handle(), 0);
    });
    #[cfg(not(windows))]
    {
        let _ = (bounds, calendar, clock);
        Ok(())
    }
}

#[tauri::command]
pub fn open_taskbar_calendar(app: AppHandle, window: WebviewWindow) {
    if accepts_input(&window) {
        windows::toggle_popup(&app, None);
    }
}

#[tauri::command]
pub fn open_taskbar_menu(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    if !accepts_input(&window) {
        return Ok(());
    }
    let menu = tray::build_menu(&app).map_err(|err| err.to_string())?;
    window.popup_menu(&menu).map_err(|err| err.to_string())
}

fn accepts_input(window: &WebviewWindow) -> bool {
    window.label() == LABEL && {
        #[cfg(windows)]
        {
            platform::accepts_input(window)
        }
        #[cfg(not(windows))]
        {
            true
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapsuleData {
    date: String,
    day: String,
    day_color: Option<String>,
    calendar_enabled: bool,
    inline_clock: bool,
    clock: Option<clock::ClockInfo>,
    hour_color: Option<String>,
    minute_color: Option<String>,
    second_color: Option<String>,
    events: Vec<String>,
    interval_seconds: u32,
    dark: bool,
}

/// Only today's display data is exposed to the capsule's WebView.
#[tauri::command]
pub fn get_capsule_data(app: AppHandle) -> CapsuleData {
    use tauri::Manager;

    let settings = today::current_settings(&app);
    let date = today::today_jalaali();
    let day = date.day.to_string();
    let mut events = Vec::new();
    if settings.show_events {
        let state = app.state::<AppState>();
        let (_, names) = today::displayed_occasions(&state.events, &settings, &date);
        for name in names {
            if !name.trim().is_empty() && !events.contains(&name) {
                events.push(name);
            }
        }
        if events.is_empty() {
            if let Some(name) = solar_holiday_name(date.month, date.day) {
                events.push(name.to_string());
            }
        }
    }

    let inline_clock = inline(&settings);
    CapsuleData {
        date: jalaali::format_slash(&date, false),
        day: if settings.fa_digits() {
            jalaali::fa_digits(&day)
        } else {
            day
        },
        day_color: settings.calendar_day_color,
        calendar_enabled: settings.capsule_enabled,
        inline_clock,
        clock: inline_clock.then(|| clock::get_clock(app)).flatten(),
        hour_color: settings.clock_hour_color,
        minute_color: settings.clock_minute_color,
        second_color: settings.clock_second_color,
        events,
        interval_seconds: settings.capsule_interval_seconds,
        dark: theme::tray_icon_is_light(),
    }
}

fn solar_holiday_name(month: u32, day: u32) -> Option<&'static str> {
    Some(match (month, day) {
        (1, 1..=4) => "عید نوروز",
        (1, 12) => "روز جمهوری اسلامی ایران",
        (1, 13) => "روز طبیعت",
        (3, 14) => "رحلت امام خمینی",
        (3, 15) => "قیام ۱۵ خرداد",
        (11, 22) => "پیروزی انقلاب اسلامی",
        (12, 29) => "ملی شدن صنعت نفت ایران",
        _ => return None,
    })
}

#[cfg(windows)]
mod platform {
    use std::ptr::{null, null_mut};
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use ::windows::Win32::{
        Foundation::HWND as AutomationHwnd,
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED,
        },
        UI::Accessibility::*,
    };
    use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
    use windows_sys::{
        core::w,
        Win32::{
            Foundation::{HWND, RECT},
            Graphics::Gdi::{CreateRectRgn, DeleteObject, SetWindowRgn},
            UI::{
                HiDpi::{
                    GetDpiForWindow, SetThreadDpiAwarenessContext,
                    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
                },
                WindowsAndMessaging::{
                    FindWindowExW, FindWindowW, GetWindow, GetWindowLongPtrW, GetWindowRect,
                    IsWindowVisible, SetWindowPos, GWL_EXSTYLE, GW_HWNDPREV, HWND_TOP,
                    HWND_TOPMOST, SWP_ASYNCWINDOWPOS, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE,
                    SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, WS_EX_TOPMOST,
                },
            },
        },
    };

    use super::{
        inline, publish_shown, requested, set_fallback, shown, SurfaceBounds, CALENDAR_VISIBLE,
        CLOCK_VISIBLE, LABEL, PRESENTING,
    };
    use crate::{
        capsule_layout::{place, MissingSurface, PlacementSample, Rect, StablePlacement},
        presentation, today, windows,
    };

    #[derive(Clone, Copy, PartialEq, Eq)]
    struct Placement {
        rect: Rect,
        taskbar: usize,
        right: bool,
    }

    /// All Explorer accessibility calls run off the application UI thread.
    pub fn start(app: AppHandle) {
        std::thread::spawn(move || {
            if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }.is_err() {
                return;
            }
            // Keep both accessibility and Win32 geometry in physical pixels.
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
            watch_taskbar(&app);
            unsafe { CoUninitialize() };
        });
    }

    fn watch_taskbar(app: &AppHandle) {
        let Ok(automation) = (unsafe {
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        }) else {
            return;
        };

        let Ok(cache) = (unsafe { automation.CreateCacheRequest() }) else {
            return;
        };
        for property in [
            UIA_BoundingRectanglePropertyId,
            UIA_ControlTypePropertyId,
            UIA_IsOffscreenPropertyId,
        ] {
            if unsafe { cache.AddProperty(property) }.is_err() {
                return;
            }
        }
        let Ok(filter) = (unsafe { automation.ControlViewCondition() }) else {
            return;
        };

        let mut last = None;
        let mut stable = StablePlacement::new();
        let mut missing = MissingSurface::default();
        let mut surface = 0;
        loop {
            let settings = today::current_settings(app);
            let enabled = requested(&settings);
            if !enabled && last.is_none() {
                std::thread::sleep(Duration::from_millis(750));
                continue;
            }
            let taskbar = unsafe { FindWindowW(w!("Shell_TrayWnd"), null()) };
            let screen = (!taskbar.is_null())
                .then(|| presentation::monitor_for(taskbar))
                .flatten();
            let presenting = screen.is_some_and(presentation::active);
            if presenting {
                pause(app, surface as HWND);
            } else {
                PRESENTING.store(false, Ordering::Release);
            }

            let sample = if enabled && !presenting {
                match screen {
                    Some(screen) => locate(&automation, &cache, &filter, taskbar, screen),
                    None => PlacementSample::Unavailable,
                }
            } else {
                PlacementSample::Hidden
            };
            let next = stable.observe(sample);
            if next.is_some() && surface == 0 {
                match windows::create_configured_window(app, LABEL) {
                    Ok(window) => {
                        let Ok(hwnd) = window.hwnd() else {
                            return;
                        };
                        surface = hwnd.0 as usize;
                    }
                    Err(error) => {
                        eprintln!("[capsule] cannot create WebView: {error}");
                        set_fallback(app, true);
                        return;
                    }
                }
            }
            if next != last {
                let app_handle = app.clone();
                if app
                    .run_on_main_thread(move || display(&app_handle, next))
                    .is_err()
                {
                    return;
                }
                last = next;
            }

            let hidden = matches!(sample, PlacementSample::Hidden);
            let fallback_due = missing.fallback_due(!hidden && shown() == 0, Instant::now());
            if hidden || fallback_due {
                set_fallback(app, fallback_due);
            }

            for _ in 0..6 {
                std::thread::sleep(Duration::from_millis(125));
                if presenting != screen.is_some_and(presentation::active) {
                    if !presenting {
                        pause(app, surface as HWND);
                        // Even a presentation that comes and goes between scans
                        // must allow the surface to be shown again.
                        last = None;
                        stable.observe(PlacementSample::Hidden);
                        missing.fallback_due(false, Instant::now());
                    }
                    break;
                }
                if let Some(position) = next {
                    keep_with_taskbar(surface as HWND, position.taskbar as HWND);
                }
            }
        }
    }

    fn pause(app: &AppHandle, surface: HWND) {
        PRESENTING.store(true, Ordering::Release);
        set_fallback(app, false);
        publish_shown(app, 0);
        if !surface.is_null() && unsafe { IsWindowVisible(surface) } != 0 {
            unsafe {
                SetWindowPos(
                    surface,
                    null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_HIDEWINDOW
                        | SWP_NOMOVE
                        | SWP_NOSIZE
                        | SWP_NOACTIVATE
                        | SWP_NOZORDER
                        | SWP_ASYNCWINDOWPOS,
                );
            }
        }
    }

    fn anchor_to_taskbar(surface: HWND, taskbar: HWND) -> (HWND, u32) {
        // Place the overlay immediately above Explorer, below unrelated topmost windows.
        let before = unsafe { GetWindow(taskbar, GW_HWNDPREV) };
        if before == surface {
            return (null_mut(), SWP_NOZORDER);
        }
        if !before.is_null() {
            return (before, 0);
        }
        let topmost =
            unsafe { GetWindowLongPtrW(taskbar, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST != 0;
        (if topmost { HWND_TOPMOST } else { HWND_TOP }, 0)
    }

    fn keep_with_taskbar(surface: HWND, taskbar: HWND) {
        if unsafe { IsWindowVisible(surface) } == 0 || unsafe { IsWindowVisible(taskbar) } == 0 {
            return;
        }
        let (anchor, flags) = anchor_to_taskbar(surface, taskbar);
        if flags == 0 {
            unsafe {
                SetWindowPos(
                    surface,
                    anchor,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE
                        | SWP_NOSIZE
                        | SWP_NOACTIVATE
                        | SWP_NOOWNERZORDER
                        | SWP_ASYNCWINDOWPOS,
                );
            }
        }
    }

    pub fn accepts_input(window: &WebviewWindow) -> bool {
        !PRESENTING.load(Ordering::Acquire)
            && window.hwnd().is_ok_and(|hwnd| {
                let hwnd = hwnd.0 as HWND;
                (unsafe { IsWindowVisible(hwnd) != 0 })
                    && !presentation::monitor_for(hwnd).is_some_and(presentation::active)
            })
    }

    fn window_rect(hwnd: HWND) -> Option<Rect> {
        let mut raw = RECT::default();
        if hwnd.is_null() || unsafe { GetWindowRect(hwnd, &mut raw) } == 0 {
            return None;
        }
        let rect = presentation::rect(raw);
        (rect.width() > 0 && rect.height() > 0).then_some(rect)
    }

    fn locate(
        automation: &IUIAutomation,
        cache: &IUIAutomationCacheRequest,
        filter: &IUIAutomationCondition,
        taskbar: HWND,
        screen: Rect,
    ) -> PlacementSample<Placement> {
        use PlacementSample::*;
        if unsafe { IsWindowVisible(taskbar) } == 0 {
            return Hidden;
        }
        let notify =
            unsafe { FindWindowExW(taskbar, std::ptr::null_mut(), w!("TrayNotifyWnd"), null()) };
        let (Some(bar), Some(tray)) = (window_rect(taskbar), window_rect(notify)) else {
            return Unavailable;
        };
        let dpi = unsafe { GetDpiForWindow(taskbar) };
        // Skip accessibility work when there is no visible horizontal slot.
        if place(bar, tray, screen, dpi, &[]).is_none() {
            return Hidden;
        }

        match taskbar_controls(automation, cache, filter, taskbar)
            .and_then(|occupied| place(bar, tray, screen, dpi, &occupied))
        {
            Some(rect) => Active(Placement {
                rect,
                taskbar: taskbar as usize,
                right: tray.right - bar.left >= bar.right - tray.left,
            }),
            None => Unavailable,
        }
    }

    fn taskbar_controls(
        automation: &IUIAutomation,
        cache: &IUIAutomationCacheRequest,
        filter: &IUIAutomationCondition,
        taskbar: HWND,
    ) -> Option<Vec<Rect>> {
        let root = unsafe { automation.ElementFromHandle(AutomationHwnd(taskbar)) }.ok()?;
        let controls =
            unsafe { root.FindAllBuildCache(TreeScope_Descendants, filter, cache) }.ok()?;
        let mut occupied = Vec::new();
        for index in 0..unsafe { controls.Length() }.ok()? {
            let Ok(control) = (unsafe { controls.GetElement(index) }) else {
                continue;
            };
            if unsafe { control.CachedIsOffscreen() }.map_or(true, |value| value.as_bool()) {
                continue;
            }
            let Ok(kind) = (unsafe { control.CachedControlType() }) else {
                continue;
            };
            if ![
                UIA_ButtonControlTypeId,
                UIA_EditControlTypeId,
                UIA_ListItemControlTypeId,
                UIA_TabItemControlTypeId,
                UIA_SplitButtonControlTypeId,
                UIA_HyperlinkControlTypeId,
                UIA_CheckBoxControlTypeId,
                UIA_ComboBoxControlTypeId,
            ]
            .contains(&kind)
            {
                continue;
            }
            if let Ok(rect) = unsafe { control.CachedBoundingRectangle() } {
                let rect = Rect {
                    left: rect.left,
                    top: rect.top,
                    right: rect.right,
                    bottom: rect.bottom,
                };
                if rect.width() > 0 && rect.height() > 0 {
                    occupied.push(rect);
                }
            }
        }
        Some(occupied)
    }

    fn display(app: &AppHandle, position: Option<Placement>) {
        let Some(window) = app.get_webview_window(LABEL) else {
            return;
        };
        let settings = today::current_settings(app);
        let active = requested(&settings) && !PRESENTING.load(Ordering::Acquire);
        match (position, active) {
            (
                Some(Placement {
                    rect,
                    taskbar,
                    right,
                }),
                true,
            ) => {
                windows::prepare_frameless(&window, true);
                let positioned = window.hwnd().is_ok_and(|hwnd| {
                    let hwnd = hwnd.0 as HWND;
                    let (anchor, flags) = anchor_to_taskbar(hwnd, taskbar as HWND);
                    unsafe {
                        SetWindowPos(
                            hwnd,
                            anchor,
                            rect.left,
                            rect.top,
                            rect.width(),
                            rect.height(),
                            flags | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
                        ) != 0
                    }
                });
                if positioned {
                    let _ = window.emit("taskbar-placement", right);
                } else {
                    let _ = window.hide();
                    publish_shown(app, 0);
                }
            }
            _ => {
                let _ = window.hide();
                publish_shown(app, 0);
            }
        }
    }

    pub fn set_surface(
        window: &WebviewWindow,
        bounds: Option<SurfaceBounds>,
        calendar: bool,
        clock: bool,
    ) -> Result<(), String> {
        let app = window.app_handle();
        let Some(bounds) = bounds else {
            publish_shown(app, 0);
            return window
                .set_ignore_cursor_events(true)
                .map_err(|err| err.to_string());
        };
        if ![bounds.x, bounds.y, bounds.width, bounds.height]
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0)
        {
            return Err("invalid surface boundaries".to_string());
        }

        let size = window.inner_size().map_err(|err| err.to_string())?;
        let scale = window.scale_factor().map_err(|err| err.to_string())?;
        let left = ((bounds.x * scale).floor() as i32).min(size.width as i32);
        let top = ((bounds.y * scale).floor() as i32).min(size.height as i32);
        let right = (((bounds.x + bounds.width) * scale).ceil() as i32).min(size.width as i32);
        let bottom = (((bounds.y + bounds.height) * scale).ceil() as i32).min(size.height as i32);
        if right <= left || bottom <= top {
            publish_shown(app, 0);
            return window
                .set_ignore_cursor_events(true)
                .map_err(|err| err.to_string());
        }

        let hwnd = window.hwnd().map_err(|err| err.to_string())?;
        // Clip the real input surface to the things the user can see.
        let region = unsafe { CreateRectRgn(left, top, right, bottom) };
        if region.is_null() {
            return Err("cannot create the taskbar surface region".to_string());
        }
        if unsafe { SetWindowRgn(hwnd.0 as HWND, region, 1) } == 0 {
            // On success Windows owns the region; on error it is ours.
            unsafe { DeleteObject(region) };
            return Err("cannot apply the taskbar surface region".to_string());
        }

        window
            .set_ignore_cursor_events(false)
            .map_err(|err| err.to_string())?;
        let settings = today::current_settings(app);
        let mut flags = 0;
        if !PRESENTING.load(Ordering::Acquire) && window.is_visible().unwrap_or(false) {
            if calendar && settings.capsule_enabled {
                flags |= CALENDAR_VISIBLE;
            }
            if clock && inline(&settings) {
                flags |= CLOCK_VISIBLE;
            }
        }
        publish_shown(app, flags);
        Ok(())
    }
}

#[cfg(windows)]
pub use platform::start;

#[cfg(not(windows))]
pub fn start(_: AppHandle) {}
