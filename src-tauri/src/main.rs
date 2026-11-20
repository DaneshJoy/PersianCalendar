#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! PersianCalendar — a minimal Persian (Shamsi) calendar that lives in the Windows
//! system tray. See `docs/CONTRACT.md` for the module/IPC contract and
//! `README.md` for the user documentation.

mod autostart;
mod capsule;
#[cfg(any(windows, test))]
mod capsule_layout;
mod clock;
mod commands;
mod events_api;
mod icon;
mod jalaali;
mod minute_timer;
#[cfg(windows)]
mod presentation;
mod scheduler;
mod settings;
mod theme;
mod today;
mod tray;
mod windows;

use std::sync::{Arc, RwLock};

use tauri::{Manager, WindowEvent};

use crate::events_api::EventsState;
use crate::settings::Settings;

/// Shared application state managed by Tauri (`app.state::<AppState>()`).
pub struct AppState {
    pub settings: RwLock<Settings>,
    pub events: Arc<EventsState>,
}

fn main() {
    let app = tauri::Builder::default()
        // `single-instance` must be registered first: a second launch just
        // reveals the existing popup instead of starting another tray icon.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::show_popup(app, None);
        }))
        .plugin(
            tauri::plugin::Builder::<tauri::Wry, ()>::new("local-navigation")
                .on_navigation(|_webview, url| {
                    let origin = (url.scheme(), url.host_str(), url.port());
                    let local = matches!(
                        origin,
                        ("http", Some("tauri.localhost"), None)
                            | ("tauri", Some("localhost"), None)
                    );
                    let dev = cfg!(dev) && origin == ("http", Some("127.0.0.1"), Some(1420));
                    (local || dev)
                        && matches!(
                            url.path(),
                            "/" | "/index.html" | "/widget.html" | "/capsule.html"
                        )
                })
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let handle = app.handle().clone();

            let mut settings = settings::load(&handle);
            // Trust Windows about the actual autostart registration.
            if let Err(err) = autostart::repair_legacy() {
                eprintln!("[settings] cannot update legacy startup registration: {err}");
            }
            let os_autostart = autostart::is_enabled();
            if settings.autostart != os_autostart {
                settings.autostart = os_autostart;
                let _ = settings::persist(&handle, &settings);
            }

            let cache_dir = match app.path().app_cache_dir() {
                Ok(path) => path,
                Err(error) => {
                    windows::show_native_error("راه‌اندازی تقویم ناموفق بود", &error);
                    app.handle().exit(1);
                    return Ok(());
                }
            };
            app.manage(AppState {
                settings: RwLock::new(settings),
                events: Arc::new(EventsState::new(cache_dir)),
            });

            // Capture the rollover baseline before any startup display reads the date.
            let startup_day = today::today_jalaali(&handle);
            // Register the native tray icons before any optional WebView is opened.
            let date_icon = tray::refresh_date(&handle);
            let clock_icon = clock::refresh(&handle);
            if let Err(err) = date_icon.and(clock_icon) {
                windows::show_native_error("ساخت آیکون برنامه ناموفق بود", &err);
                windows::show_popup(&handle, None);
            }

            windows::apply_widget(&handle);
            capsule::start(handle.clone());
            clock::start(handle.clone(), startup_day);
            scheduler::start(handle.clone());
            scheduler::refresh_daily(&handle);

            Ok(())
        })
        .on_window_event(|window, event| match event {
            #[cfg(not(windows))]
            WindowEvent::Focused(focused) => {
                if window.label() == "popup" && !*focused {
                    windows::hide_popup(window.app_handle());
                }
            }
            WindowEvent::Moved(position) => {
                if window.label() == "widget" {
                    windows::on_widget_moved(window.app_handle(), position.x, position.y);
                }
            }
            WindowEvent::CloseRequested { api, .. } => {
                // The app lives in the tray: closing windows just hides them.
                api.prevent_close();
                if window.label() == windows::POPUP_LABEL {
                    windows::hide_popup(window.app_handle());
                } else {
                    let _ = window.hide();
                }
            }
            _ => {}
        })
        .on_menu_event(|app, event| {
            tray::handle_menu_event(app, event.id().as_ref());
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::save_widget_settings,
            commands::reset_settings,
            commands::get_theme,
            commands::get_today_info,
            commands::get_month_events,
            commands::refresh_month_events,
            commands::clear_events_cache,
            commands::copy_text,
            commands::hide_popup,
            windows::popup_ready,
            commands::open_popup_settings,
            clock::get_clock,
            clock::get_clock_zones,
            capsule::get_capsule_data,
            capsule::set_taskbar_surface,
            capsule::open_taskbar_calendar,
            capsule::open_taskbar_menu,
        ])
        .build(tauri::generate_context!());

    match app {
        Ok(app) => app.run(|_, event| {
            // The native icons keep the app alive while no WebView is open.
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        }),
        Err(err) => windows::show_native_error("راه‌اندازی تقویم ناموفق بود", &err),
    }
}
