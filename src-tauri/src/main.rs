#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! PersianCalendar — a minimal Persian (Shamsi) calendar that lives in the Windows
//! system tray. See `docs/CONTRACT.md` for the module/IPC contract and
//! `README.md` for the user documentation.

mod commands;
mod events_api;
mod icon;
mod jalaali;
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
    tauri::Builder::default()
        // `single-instance` must be registered first: a second launch just
        // reveals the existing popup instead of starting another tray icon.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::show_popup(app, None);
        }))
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let handle = app.handle().clone();

            let mut settings = settings::load(&handle);
            // Trust Windows about the actual autostart registration.
            let os_autostart = tray::get_autostart(&handle);
            if settings.autostart != os_autostart {
                settings.autostart = os_autostart;
                let _ = settings::persist(&handle, &settings);
            }

            let cache_dir = app.path().app_cache_dir()?;
            app.manage(AppState {
                settings: RwLock::new(settings),
                events: Arc::new(EventsState::new(cache_dir)),
            });

            // Widget window exists for the whole session (hidden when disabled)
            // so enabling it is instant and the webview is never re-created.
            if let Err(err) = windows::create_widget(&handle) {
                eprintln!("[main] cannot create widget window: {err}");
            }
            windows::apply_widget(&handle);

            tray::create(&handle)?;

            scheduler::start(handle.clone());
            scheduler::refresh_daily(&handle);

            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(focused) => {
                // Flyout behaviour: clicking outside hides the popup.
                if window.label() == "popup" && !*focused {
                    let settings = today::current_settings(window.app_handle());
                    if settings.close_on_blur {
                        let _ = window.hide();
                    }
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
                let _ = window.hide();
            }
            _ => {}
        })
        .on_menu_event(|app, event| {
            tray::handle_menu_event(app, event.id().as_ref());
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::reset_settings,
            commands::get_theme,
            commands::get_today_info,
            commands::get_month_events,
            commands::refresh_month_events,
            commands::clear_events_cache,
            commands::copy_text,
            commands::hide_popup,
            commands::show_popup,
            commands::quit_app,
            commands::set_autostart,
            commands::get_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
