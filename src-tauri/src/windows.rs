//! Popup and desktop-widget window management.
//!
//! The popup is created on first use, after the native tray icons are running.
//! Both the popup and the optional desktop widget remain available between uses.
//!
//! Popup placement is computed manually from the physical tray-icon rect and the
//! monitor work area: the window is centred over the tray icon, sits on the
//! taskbar edge (8px away) and is always clamped inside the work area. Doing the
//! math ourselves keeps placement correct with multi-monitor, mixed-DPI and
//! taskbars docked on any edge (Windows only reports the work area, so the free
//! edge reveals where the taskbar is). The positioner plugin's tray tracking is
//! intentionally not required here because its state is only populated after a
//! tray event and it has no work-area clamping.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, PhysicalRect, Rect, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

use crate::settings::{self, Settings, WidgetSize};
use crate::AppState;

/// Label of the popup window, as declared in `tauri.conf.json`.
pub const POPUP_LABEL: &str = "popup";
/// Label of the runtime-created desktop widget window.
pub const WIDGET_LABEL: &str = "widget";

/// Gap between the popup and the taskbar edge, in physical pixels.
const POPUP_GAP: i32 = 8;
/// Margin of the default widget position from the screen corner, in logical px.
const WIDGET_MARGIN_LOGICAL: f64 = 24.0;
/// Coalescing delay for the rapid `Moved` events emitted while dragging.
const WIDGET_MOVE_DEBOUNCE: Duration = Duration::from_millis(400);
/// Fallback popup size (logical px) if the window cannot report its own size.
const POPUP_FALLBACK_LOGICAL_SIZE: (f64, f64) = (344.0, 424.0);

/// Latest requested widget position while a drag is still going on.
static PENDING_WIDGET_MOVE: Mutex<Option<(i32, i32)>> = Mutex::new(None);
/// Incremented on every `Moved` event so only the last one is persisted.
static WIDGET_MOVE_GENERATION: AtomicU64 = AtomicU64::new(0);
/// Guards against creating two `widget` windows at the same time.
static WIDGET_CREATE_LOCK: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------------------
// Settings access
// ---------------------------------------------------------------------------

fn current_settings(app: &AppHandle) -> Settings {
    let state = app.state::<AppState>();
    let settings = match state.settings.read() {
        Ok(guard) => guard.clone(),
        // A poisoned lock should not take the whole app down; recover the value.
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    settings
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

/// A rectangle in physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bounds {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl Bounds {
    fn from_coords(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    fn from_position_size(position: PhysicalPosition<i32>, width: u32, height: u32) -> Self {
        Self::from_coords(
            position.x,
            position.y,
            position.x + width as i32,
            position.y + height as i32,
        )
    }

    fn from_monitor(monitor: &tauri::Monitor) -> Self {
        let position = *monitor.position();
        Self::from_position_size(position, monitor.size().width, monitor.size().height)
    }

    fn from_work_area(work: &PhysicalRect<i32, u32>) -> Self {
        Self::from_position_size(work.position, work.size.width, work.size.height)
    }

    fn from_tray_rect(rect: Rect) -> Self {
        // The tray-icon crate reports the icon rect in physical screen pixels,
        // so a scale factor of 1.0 is used to unwrap the generic dpi values.
        let position = rect.position.to_physical::<i32>(1.0);
        let size = rect.size.to_physical::<u32>(1.0);
        Self::from_position_size(position, size.width, size.height)
    }

    fn center_x(self) -> i32 {
        self.left + (self.right - self.left) / 2
    }

    fn center_y(self) -> i32 {
        self.top + (self.bottom - self.top) / 2
    }
}

/// Which screen edge the taskbar is docked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskbarEdge {
    Top,
    Bottom,
    Left,
    Right,
}

/// Detect the taskbar edge from the monitor/work-area difference.
///
/// With an auto-hiding taskbar the work area equals the monitor rect; in that
/// case the tray icon's own position is the only hint, and we fall back to the
/// bottom edge (the Windows default) when even that is inconclusive.
fn taskbar_edge(monitor: Bounds, work: Bounds, tray: Option<Bounds>) -> TaskbarEdge {
    let bottom_gap = monitor.bottom - work.bottom;
    let top_gap = work.top - monitor.top;
    let left_gap = work.left - monitor.left;
    let right_gap = monitor.right - work.right;
    let max_gap = bottom_gap.max(top_gap).max(left_gap).max(right_gap);

    if max_gap > 4 {
        if bottom_gap == max_gap {
            return TaskbarEdge::Bottom;
        }
        if top_gap == max_gap {
            return TaskbarEdge::Top;
        }
        if left_gap == max_gap {
            return TaskbarEdge::Left;
        }
        return TaskbarEdge::Right;
    }

    if let Some(tray) = tray {
        let (x, y) = (tray.center_x(), tray.center_y());
        if y <= work.top {
            return TaskbarEdge::Top;
        }
        if y >= work.bottom {
            return TaskbarEdge::Bottom;
        }
        if x <= work.left {
            return TaskbarEdge::Left;
        }
        if x >= work.right {
            return TaskbarEdge::Right;
        }
    }

    TaskbarEdge::Bottom
}

/// Compute the popup top-left corner in physical pixels.
fn popup_placement(
    monitor: Bounds,
    work: Bounds,
    tray: Option<Bounds>,
    popup_width: i32,
    popup_height: i32,
) -> PhysicalPosition<i32> {
    let edge = taskbar_edge(monitor, work, tray);
    let popup_width = popup_width.max(1);
    let popup_height = popup_height.max(1);

    // Without a tray rect, fall back to the bottom-right corner of the work
    // area, which is where the notification area normally lives.
    let fallback_center_x = work.right - popup_width / 2 - POPUP_GAP;
    let fallback_center_y = work.bottom - popup_height / 2 - POPUP_GAP;
    let (icon_center_x, icon_center_y) = match tray {
        Some(tray) => (tray.center_x(), tray.center_y()),
        None => (fallback_center_x, fallback_center_y),
    };

    let (x, y) = match edge {
        TaskbarEdge::Bottom => (
            icon_center_x - popup_width / 2,
            work.bottom - popup_height - POPUP_GAP,
        ),
        TaskbarEdge::Top => (icon_center_x - popup_width / 2, work.top + POPUP_GAP),
        TaskbarEdge::Left => (work.left + POPUP_GAP, icon_center_y - popup_height / 2),
        TaskbarEdge::Right => (
            work.right - popup_width - POPUP_GAP,
            icon_center_y - popup_height / 2,
        ),
    };

    clamp_to_bounds(PhysicalPosition::new(x, y), popup_width, popup_height, work)
}

/// Keep a `width`x`height` rectangle fully inside `bounds` (when it fits).
fn clamp_to_bounds(
    position: PhysicalPosition<i32>,
    width: i32,
    height: i32,
    bounds: Bounds,
) -> PhysicalPosition<i32> {
    let max_x = (bounds.right - width).max(bounds.left);
    let max_y = (bounds.bottom - height).max(bounds.top);
    PhysicalPosition::new(
        position.x.clamp(bounds.left, max_x),
        position.y.clamp(bounds.top, max_y),
    )
}

/// Resolve the target position of the popup for the given tray icon rect.
fn compute_popup_position(
    app: &AppHandle,
    window: &WebviewWindow,
    tray_rect: Option<Rect>,
) -> Option<PhysicalPosition<i32>> {
    let tray = tray_rect.map(Bounds::from_tray_rect);

    // The monitor that hosts the tray icon is the one the popup must open on.
    let monitor = match tray {
        Some(tray) => app
            .monitor_from_point(tray.center_x() as f64, tray.center_y() as f64)
            .ok()
            .flatten(),
        None => None,
    }
    .or_else(|| window.current_monitor().ok().flatten())
    .or_else(|| app.primary_monitor().ok().flatten())?;

    // Convert the popup's logical size to the target monitor's DPI so mixed-DPI
    // multi-monitor setups still get a correctly centred window.
    let target_scale = monitor.scale_factor();
    let (logical_width, logical_height) = window
        .outer_size()
        .ok()
        .zip(window.scale_factor().ok())
        .map(|(size, window_scale)| {
            if window_scale > 0.0 {
                (
                    size.width as f64 / window_scale,
                    size.height as f64 / window_scale,
                )
            } else {
                POPUP_FALLBACK_LOGICAL_SIZE
            }
        })
        .unwrap_or(POPUP_FALLBACK_LOGICAL_SIZE);
    let popup_width = ((logical_width * target_scale).round() as i32).max(1);
    let popup_height = ((logical_height * target_scale).round() as i32).max(1);

    let monitor_bounds = Bounds::from_monitor(&monitor);
    let work_bounds = Bounds::from_work_area(monitor.work_area());

    Some(popup_placement(
        monitor_bounds,
        work_bounds,
        tray,
        popup_width,
        popup_height,
    ))
}

// ---------------------------------------------------------------------------
// Popup window
// ---------------------------------------------------------------------------

struct PopupRequest {
    tray_rect: Option<Rect>,
    action: Option<(&'static str, serde_json::Value)>,
}

struct PopupLaunch {
    creating: bool,
    ready: bool,
    request: Option<PopupRequest>,
}

static POPUP_LAUNCH: Mutex<PopupLaunch> = Mutex::new(PopupLaunch {
    creating: false,
    ready: false,
    request: None,
});

#[cfg(windows)]
static POPUP_SESSION: AtomicU64 = AtomicU64::new(0);

pub fn create_configured_window(app: &AppHandle, label: &str) -> tauri::Result<WebviewWindow> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == label)
        .ok_or(tauri::Error::WindowNotFound)?;
    WebviewWindowBuilder::from_config(app, config)?.build()
}

/// Prevent Windows from painting a native caption on our frameless WebViews.
/// Called on the UI thread, before the hidden window is presented.
#[cfg(windows)]
pub fn prepare_frameless(window: &WebviewWindow, transparent: bool) {
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Dwm::{DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_NCRENDERING_POLICY},
        UI::{
            Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                WM_ERASEBKGND, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCDESTROY, WM_NCPAINT, WS_CAPTION,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_THICKFRAME,
            },
        },
    };

    unsafe extern "system" fn frameless_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        id: usize,
        transparent: usize,
    ) -> LRESULT {
        match msg {
            WM_NCCALCSIZE | WM_NCPAINT => 0,
            WM_NCACTIVATE => unsafe { DefSubclassProc(hwnd, msg, wparam, -1) },
            WM_ERASEBKGND if transparent != 0 => 1,
            WM_NCDESTROY => unsafe {
                RemoveWindowSubclass(hwnd, Some(frameless_proc), id);
                DefSubclassProc(hwnd, msg, wparam, lparam)
            },
            _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
        }
    }

    let Ok(window) = window.hwnd() else {
        return;
    };
    let hwnd = window.0 as HWND;
    const ID: usize = 1;
    let mut installed = 0;
    unsafe {
        if GetWindowSubclass(hwnd, Some(frameless_proc), ID, &mut installed) != 0 {
            return;
        }
        if SetWindowSubclass(hwnd, Some(frameless_proc), ID, usize::from(transparent)) == 0 {
            return;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) & !((WS_CAPTION | WS_THICKFRAME) as isize);
        SetWindowLongPtrW(hwnd, GWL_STYLE, style);
        if transparent {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE)
                | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW) as isize;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
        }
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY as u32,
            (&DWMNCRP_DISABLED as *const i32).cast(),
            4,
        );
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
    }
}

pub fn show_native_error(title: &str, error: &impl std::fmt::Display) {
    eprintln!("[PersianCalendar] {title}: {error}");
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let message: Vec<u16> = format!("{title}\n\n{error}")
            .encode_utf16()
            .chain([0])
            .collect();
        let caption: Vec<u16> = "PersianCalendar".encode_utf16().chain([0]).collect();
        // Both strings are local and nul-terminated for the synchronous dialog.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                caption.as_ptr(),
                MB_OK | MB_ICONERROR,
            )
        };
    }
}

pub fn show_popup(app: &AppHandle, tray_rect: Option<Rect>) {
    request_popup(
        app,
        PopupRequest {
            tray_rect,
            action: None,
        },
    );
}

fn request_popup(app: &AppHandle, request: PopupRequest) {
    let exists = app.get_webview_window(POPUP_LABEL).is_some();
    {
        let mut launch = POPUP_LAUNCH.lock().unwrap();
        launch.request = Some(request);
        if launch.creating {
            return;
        }
        if !exists {
            launch.creating = true;
            launch.ready = false;
        }
    }
    if exists {
        show_requested_popup(app);
        return;
    }

    let handle = app.clone();
    std::thread::spawn(move || {
        let result = create_configured_window(&handle, POPUP_LABEL);
        {
            let mut launch = POPUP_LAUNCH.lock().unwrap();
            launch.creating = false;
            if result.is_err() {
                launch.ready = false;
                launch.request = None;
            }
        }
        match result {
            Ok(_) => show_requested_popup(&handle),
            Err(err) => show_native_error("باز کردن پنجرهٔ تقویم ناموفق بود", &err),
        }
    });
}

/// The popup calls this once its tray menu event handlers are registered.
#[tauri::command]
pub fn popup_ready(app: AppHandle) {
    POPUP_LAUNCH.lock().unwrap().ready = true;
    show_requested_popup(&app);
}

fn show_requested_popup(app: &AppHandle) {
    let request = {
        let mut launch = POPUP_LAUNCH.lock().unwrap();
        if launch.creating || !launch.ready {
            return;
        }
        launch.request.take()
    };
    if let Some(request) = request {
        let handle = app.clone();
        if let Err(error) = app.run_on_main_thread(move || present_popup(&handle, request)) {
            eprintln!("[popup] cannot schedule show: {error}");
        }
    }
}

fn present_popup(app: &AppHandle, request: PopupRequest) {
    let Some(window) = app.get_webview_window(POPUP_LABEL) else {
        return;
    };
    let tray_rect = request.tray_rect;

    #[cfg(windows)]
    prepare_frameless(&window, false);

    match compute_popup_position(app, &window, tray_rect) {
        Some(position) => {
            if let Err(err) = window.set_position(position) {
                eprintln!("[popup] cannot move window: {err}");
            }
        }
        None => eprintln!("[popup] no monitor information, keeping the current position"),
    }

    #[cfg(windows)]
    let previous_foreground = popup_dismissal::foreground();
    if let Err(err) = window.show() {
        eprintln!("[popup] cannot show window: {err}");
        return;
    }
    // Focus after showing so Esc works and the blur-close handler does not fire
    // immediately. A failure here is not fatal: the window is still visible.
    if let Err(err) = window.set_focus() {
        eprintln!("[popup] cannot focus window: {err}");
    }
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        popup_dismissal::watch(app.clone(), hwnd.0 as usize, previous_foreground);
    }
    if let Some((name, payload)) = request.action {
        if let Err(err) = app.emit_to(POPUP_LABEL, name, payload) {
            eprintln!("[popup] cannot emit {name}: {err}");
        }
    }
}

/// Hide the popup without destroying it (cheap to reopen).
pub fn hide_popup(app: &AppHandle) {
    #[cfg(windows)]
    POPUP_SESSION.fetch_add(1, Ordering::AcqRel);
    let Some(window) = app.get_webview_window(POPUP_LABEL) else {
        return;
    };
    if let Err(err) = window.hide() {
        eprintln!("[popup] cannot hide window: {err}");
    }
}

#[cfg(windows)]
mod popup_dismissal {
    use super::{hide_popup, AppHandle, Bounds, Manager, Ordering, POPUP_SESSION};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::{
        Foundation::{HWND, POINT},
        UI::{
            HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON},
            WindowsAndMessaging::{
                GetAncestor, GetCursorPos, GetForegroundWindow, WindowFromPoint, GA_ROOTOWNER,
            },
        },
    };

    pub fn foreground() -> usize {
        unsafe { GetForegroundWindow() as usize }
    }

    fn root_owner(window: HWND) -> usize {
        unsafe { GetAncestor(window, GA_ROOTOWNER) as usize }
    }

    fn mouse_buttons() -> [bool; 3] {
        [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
            .map(|button| (unsafe { GetAsyncKeyState(i32::from(button)) }) < 0)
    }

    enum Click {
        Inside,
        Toggle,
        Outside,
    }

    fn click_target(app: &AppHandle, popup: usize) -> Click {
        let mut point = POINT::default();
        if unsafe { GetCursorPos(&mut point) } == 0 {
            return Click::Inside;
        }
        let target = root_owner(unsafe { WindowFromPoint(point) });
        if target == popup {
            return Click::Inside;
        }
        let capsule = app
            .get_webview_window(crate::capsule::LABEL)
            .and_then(|window| window.hwnd().ok())
            .map(|hwnd| hwnd.0 as usize);
        let over_tray_icon = crate::tray::any_notification_icon_visible()
            && crate::tray::last_tray_rect()
                .map(Bounds::from_tray_rect)
                .is_some_and(|rect| {
                    point.x >= rect.left
                        && point.x < rect.right
                        && point.y >= rect.top
                        && point.y < rect.bottom
                });
        if capsule == Some(target) || over_tray_icon {
            Click::Toggle
        } else {
            Click::Outside
        }
    }

    pub fn watch(app: AppHandle, popup: usize, previous_foreground: usize) {
        let session = POPUP_SESSION.fetch_add(1, Ordering::AcqRel) + 1;
        std::thread::spawn(move || {
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
            let mut keys = mouse_buttons();
            let mut own_focus = false;
            let mut allow_focus_dismissal = Instant::now() + Duration::from_millis(170);
            while POPUP_SESSION.load(Ordering::Acquire) == session {
                let now = Instant::now();
                let next_keys = mouse_buttons();
                let clicked = next_keys
                    .iter()
                    .zip(keys)
                    .any(|(down, was_down)| *down && !was_down);
                keys = next_keys;
                if clicked {
                    match click_target(&app, popup) {
                        Click::Outside => break,
                        // Let the surface's click handler toggle the window on release.
                        Click::Toggle => allow_focus_dismissal = now + Duration::from_millis(200),
                        Click::Inside => {}
                    }
                }
                let foreground = foreground();
                if foreground == popup || root_owner(foreground as HWND) == popup {
                    own_focus = true;
                } else if foreground != 0
                    && (own_focus || foreground != previous_foreground)
                    && now >= allow_focus_dismissal
                    && !keys.into_iter().any(|down| down)
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if POPUP_SESSION.load(Ordering::Acquire) == session {
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    if POPUP_SESSION.load(Ordering::Acquire) == session {
                        hide_popup(&handle);
                    }
                });
            }
        });
    }
}

/// Toggle the popup: hide it when visible, otherwise show it at the tray icon.
pub fn toggle_popup(app: &AppHandle, tray_rect: Option<Rect>) {
    let visible = app
        .get_webview_window(POPUP_LABEL)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    if visible {
        hide_popup(app);
    } else {
        show_popup(app, tray_rect);
    }
}

/// Show the popup and emit an event to the frontend (tray menu actions).
pub fn show_popup_and_emit(app: &AppHandle, event_name: &'static str, payload: serde_json::Value) {
    request_popup(
        app,
        PopupRequest {
            tray_rect: crate::tray::last_tray_rect(),
            action: Some((event_name, payload)),
        },
    );
}

// ---------------------------------------------------------------------------
// Desktop widget window
// ---------------------------------------------------------------------------

fn widget_logical_size(size: WidgetSize) -> (f64, f64) {
    match size {
        WidgetSize::Small => (200.0, 92.0),
        WidgetSize::Full => (300.0, 330.0),
    }
}

/// Create the runtime widget window (hidden; the caller decides when to show it).
///
/// Must run on the main thread (the Tauri setup hook is the usual caller); from
/// IPC commands prefer [`apply_widget`], which creates the window on a worker
/// thread to avoid the Windows webview-creation deadlock.
pub fn create_widget(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(WIDGET_LABEL).is_some() {
        return Ok(());
    }

    let settings = current_settings(app);
    let (width, height) = widget_logical_size(settings.widget_size);

    WebviewWindowBuilder::new(app, WIDGET_LABEL, WebviewUrl::App("widget.html".into()))
        .title("ویجت تقویم شمسی")
        .inner_size(width, height)
        .transparent(true)
        .decorations(false)
        .skip_taskbar(true)
        .always_on_bottom(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .visible(false)
        .build()?;

    Ok(())
}

/// Create/configure/show/hide the widget so it matches the current settings.
///
/// Safe to call from IPC commands and tray handlers: when the window still has
/// to be created this spawns a worker thread, because building a webview on the
/// main thread from inside a synchronous command can deadlock on Windows.
pub fn apply_widget(app: &AppHandle) {
    let settings = current_settings(app);

    if let Some(widget) = app.get_webview_window(WIDGET_LABEL) {
        if settings.widget_enabled {
            sync_widget(app, &widget, &settings);
        } else if let Err(err) = widget.hide() {
            eprintln!("[widget] cannot hide: {err}");
        }
        return;
    }

    if settings.widget_enabled {
        spawn_widget_creation(app.clone());
    }
}

fn spawn_widget_creation(app: AppHandle) {
    std::thread::spawn(move || {
        // Only one creation may run at a time; later calls are no-ops because
        // the window will already exist when they get here.
        let Ok(_guard) = WIDGET_CREATE_LOCK.try_lock() else {
            return;
        };
        if app.get_webview_window(WIDGET_LABEL).is_some() {
            return;
        }
        if let Err(err) = create_widget(&app) {
            eprintln!("[widget] cannot create window: {err}");
            return;
        }

        let settings = current_settings(&app);
        let Some(widget) = app.get_webview_window(WIDGET_LABEL) else {
            return;
        };
        if settings.widget_enabled {
            sync_widget(&app, &widget, &settings);
        } else if let Err(err) = widget.hide() {
            eprintln!("[widget] cannot hide: {err}");
        }
    });
}

fn sync_widget(app: &AppHandle, widget: &WebviewWindow, settings: &Settings) {
    configure_widget(widget, settings);
    position_widget(app, widget, settings);
    if let Err(err) = widget.show() {
        eprintln!("[widget] cannot show: {err}");
    }
}

fn configure_widget(widget: &WebviewWindow, settings: &Settings) {
    let (width, height) = widget_logical_size(settings.widget_size);
    if let Err(err) = widget.set_size(LogicalSize::new(width, height)) {
        eprintln!("[widget] cannot resize: {err}");
    }
    if let Err(err) = widget.set_always_on_bottom(true) {
        eprintln!("[widget] cannot pin below other windows: {err}");
    }
}

fn position_widget(app: &AppHandle, widget: &WebviewWindow, settings: &Settings) {
    match widget_position(app, settings) {
        Some(position) => {
            if let Err(err) = widget.set_position(position) {
                eprintln!("[widget] cannot move window: {err}");
            }
        }
        None => eprintln!("[widget] no monitor information, keeping the current position"),
    }
}

/// Position of the widget: the saved coordinates when present, otherwise the
/// bottom-left corner of the primary work area (RTL-friendly default).
fn widget_position(app: &AppHandle, settings: &Settings) -> Option<PhysicalPosition<i32>> {
    let (logical_width, logical_height) = widget_logical_size(settings.widget_size);

    if let (Some(x), Some(y)) = (settings.widget_x, settings.widget_y) {
        let monitor = app
            .monitor_from_point(x as f64, y as f64)
            .ok()
            .flatten()
            .or_else(|| app.primary_monitor().ok().flatten())?;
        let work = Bounds::from_work_area(monitor.work_area());
        let scale = monitor.scale_factor();
        let width = (logical_width * scale).round() as i32;
        let height = (logical_height * scale).round() as i32;
        return Some(clamp_to_bounds(
            PhysicalPosition::new(x, y),
            width,
            height,
            work,
        ));
    }

    let monitor = app.primary_monitor().ok().flatten()?;
    let work = Bounds::from_work_area(monitor.work_area());
    let scale = monitor.scale_factor();
    let margin = (WIDGET_MARGIN_LOGICAL * scale).round() as i32;
    let width = (logical_width * scale).round() as i32;
    let height = (logical_height * scale).round() as i32;
    let default = PhysicalPosition::new(work.left + margin, work.bottom - height - margin);
    Some(clamp_to_bounds(default, width, height, work))
}

/// Persist the widget position into the in-memory settings, the store, and let
/// the frontend know through `settings-changed`.
pub fn save_widget_position(app: &AppHandle, x: i32, y: i32) {
    let current = current_settings(app);
    if current.widget_x == Some(x) && current.widget_y == Some(y) {
        return;
    }
    match settings::update(app, |current| {
        let mut next = current.clone();
        next.widget_x = Some(x);
        next.widget_y = Some(y);
        Ok(next)
    }) {
        Ok(next) => {
            let _ = app.emit("settings-changed", &next);
        }
        Err(err) => eprintln!("[widget] cannot persist position: {err}"),
    }
}

/// `Moved` handler for the widget: debounce 400ms, then store the last position.
pub fn on_widget_moved(app: &AppHandle, x: i32, y: i32) {
    let generation = WIDGET_MOVE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    if let Ok(mut pending) = PENDING_WIDGET_MOVE.lock() {
        *pending = Some((x, y));
    }

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(WIDGET_MOVE_DEBOUNCE).await;
        // A newer move arrived while we were waiting: let its own task persist.
        if WIDGET_MOVE_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        let pending = PENDING_WIDGET_MOVE
            .lock()
            .ok()
            .and_then(|mut pending| pending.take());
        if let Some((x, y)) = pending {
            save_widget_position(&app_handle, x, y);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(left: i32, top: i32, right: i32, bottom: i32) -> Bounds {
        Bounds::from_coords(left, top, right, bottom)
    }

    #[test]
    fn places_popup_above_a_bottom_taskbar_centred_on_the_icon() {
        let monitor = bounds(0, 0, 1920, 1080);
        let work = bounds(0, 0, 1920, 1040);
        let tray = bounds(1700, 1040, 1724, 1064);
        let position = popup_placement(monitor, work, Some(tray), 344, 424);
        assert_eq!(position.x, 1712 - 172);
        assert_eq!(position.y, 1040 - 424 - POPUP_GAP);
    }

    #[test]
    fn places_popup_below_a_top_taskbar() {
        let monitor = bounds(0, 0, 1920, 1080);
        let work = bounds(0, 40, 1920, 1080);
        let tray = bounds(1700, 0, 1724, 24);
        let position = popup_placement(monitor, work, Some(tray), 344, 424);
        assert_eq!(position.y, 40 + POPUP_GAP);
        assert_eq!(position.x, 1712 - 172);
    }

    #[test]
    fn places_popup_beside_left_and_right_taskbars() {
        let monitor = bounds(0, 0, 1920, 1080);

        let work = bounds(48, 0, 1920, 1080);
        let tray = bounds(0, 500, 48, 524);
        let left = popup_placement(monitor, work, Some(tray), 344, 424);
        assert_eq!(left.x, 48 + POPUP_GAP);
        assert_eq!(left.y, 512 - 212);

        let work = bounds(0, 0, 1872, 1080);
        let tray = bounds(1872, 500, 1920, 524);
        let right = popup_placement(monitor, work, Some(tray), 344, 424);
        assert_eq!(right.x, 1872 - 344 - POPUP_GAP);
        assert_eq!(right.y, 512 - 212);
    }

    #[test]
    fn clamps_popup_inside_the_work_area() {
        let monitor = bounds(0, 0, 1920, 1080);
        let work = bounds(0, 0, 1920, 1040);
        // Icon at the far right edge: centring would overflow the work area.
        let tray = bounds(1900, 1040, 1924, 1064);
        let position = popup_placement(monitor, work, Some(tray), 344, 424);
        assert_eq!(position.x, 1920 - 344);
        assert_eq!(position.y, 1040 - 424 - POPUP_GAP);
    }

    #[test]
    fn auto_hidden_taskbar_falls_back_to_the_bottom_edge() {
        let monitor = bounds(0, 0, 1920, 1080);
        let work = monitor;
        let tray = bounds(1700, 1056, 1724, 1080);
        let position = popup_placement(monitor, work, Some(tray), 344, 424);
        assert_eq!(position.y, 1080 - 424 - POPUP_GAP);
    }

    #[test]
    fn popup_larger_than_the_work_area_is_pinned_to_its_top_left() {
        let monitor = bounds(0, 0, 800, 600);
        let work = bounds(0, 0, 800, 560);
        let tray = bounds(790, 560, 814, 584);
        let position = popup_placement(monitor, work, Some(tray), 900, 700);
        assert_eq!(position.x, 0);
        assert_eq!(position.y, 0);
    }

    #[test]
    fn missing_tray_rect_uses_the_bottom_right_corner() {
        let monitor = bounds(0, 0, 1920, 1080);
        let work = bounds(0, 0, 1920, 1040);
        let position = popup_placement(monitor, work, None, 344, 424);
        assert_eq!(position.x, 1920 - 344 - POPUP_GAP);
        assert_eq!(position.y, 1040 - 424 - POPUP_GAP);
    }
}
