//! Windows presentation state for the custom taskbar surface.

use std::{cell::Cell, ffi::c_void, mem::size_of};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND, RECT},
    Graphics::{
        Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS},
        Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
    },
    System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    },
    UI::{
        Shell::{
            SHQueryUserNotificationState, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
        },
        WindowsAndMessaging::{
            GetAncestor, GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
            GetWindowThreadProcessId, IsIconic, IsWindowVisible, GA_ROOTOWNER, GWL_STYLE,
            WS_CAPTION, WS_POPUP,
        },
    },
};

use crate::capsule_layout::Rect;

pub fn rect(raw: RECT) -> Rect {
    Rect {
        left: raw.left,
        top: raw.top,
        right: raw.right,
        bottom: raw.bottom,
    }
}

pub fn monitor_for(window: HWND) -> Option<Rect> {
    let monitor = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if monitor.is_null() || unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return None;
    }
    Some(rect(info.rcMonitor))
}

fn attribute<T>(window: HWND, attribute: i32, value: &mut T) -> bool {
    unsafe {
        DwmGetWindowAttribute(
            window,
            attribute as u32,
            (value as *mut T).cast::<c_void>(),
            size_of::<T>() as u32,
        ) >= 0
    }
}

fn visible_bounds(window: HWND) -> Option<Rect> {
    let mut cloaked = 0u32;
    if window.is_null()
        || unsafe { IsWindowVisible(window) } == 0
        || unsafe { IsIconic(window) } != 0
        || (attribute(window, DWMWA_CLOAKED, &mut cloaked) && cloaked != 0)
    {
        return None;
    }
    let mut bounds = RECT::default();
    if !attribute(window, DWMWA_EXTENDED_FRAME_BOUNDS, &mut bounds)
        && unsafe { GetWindowRect(window, &mut bounds) } == 0
    {
        return None;
    }
    Some(rect(bounds))
}

#[derive(Clone, Copy)]
enum CaptureHost {
    Other,
    Clipping,
    Editor,
}

thread_local! {
    static LAST_HOST: Cell<(u32, CaptureHost)> = const { Cell::new((0, CaptureHost::Other)) };
}

fn identify_host(pid: u32) -> CaptureHost {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return CaptureHost::Other;
    }
    let mut path = [0u16; 1024];
    let mut len = path.len() as u32;
    let read = unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut len) } != 0;
    unsafe { CloseHandle(process) };
    if !read {
        return CaptureHost::Other;
    }

    let path = String::from_utf16_lossy(&path[..len as usize]);
    let exe = path.rsplit(['\\', '/']).next().unwrap_or_default();
    if exe.eq_ignore_ascii_case("ScreenClippingHost.exe") {
        CaptureHost::Clipping
    } else if exe.eq_ignore_ascii_case("SnippingTool.exe") {
        CaptureHost::Editor
    } else {
        CaptureHost::Other
    }
}

fn is_capture_window(window: HWND) -> bool {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    let host = LAST_HOST.with(|cache| {
        let (last_pid, previous) = cache.get();
        if last_pid == pid {
            return previous;
        }
        let host = identify_host(pid);
        cache.set((pid, host));
        host
    });

    match host {
        CaptureHost::Clipping => true,
        CaptureHost::Editor => {
            // Snipping Tool's floating capture controls are popups. The editor
            // window is a regular application window and leaves the taskbar usable.
            let style = unsafe { GetWindowLongPtrW(window, GWL_STYLE) } as u32;
            style & WS_POPUP != 0 && style & WS_CAPTION == 0
        }
        CaptureHost::Other => false,
    }
}

fn blocks_surface(window: HWND, screen: Rect) -> bool {
    let Some(bounds) = visible_bounds(window) else {
        return false;
    };
    let mut name = [0u16; 40];
    let len = unsafe { GetClassNameW(window, name.as_mut_ptr(), name.len() as i32) };
    let name = String::from_utf16_lossy(&name[..len.max(0) as usize]);
    if matches!(
        name.as_str(),
        "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "WorkerW" | "Progman"
    ) {
        return false;
    }
    is_capture_window(window) || bounds.fills_display(screen)
}

pub fn active(screen: Rect) -> bool {
    let mut state = 0;
    if unsafe { SHQueryUserNotificationState(&mut state) } >= 0
        && matches!(state, QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE)
    {
        return true;
    }
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        return false;
    }
    if blocks_surface(foreground, screen) {
        return true;
    }
    let owner = unsafe { GetAncestor(foreground, GA_ROOTOWNER) };
    owner != foreground && blocks_surface(owner, screen)
}
