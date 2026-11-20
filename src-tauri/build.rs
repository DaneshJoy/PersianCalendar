fn main() {
    let manifest = tauri_build::AppManifest::new().commands(&[
        "get_settings",
        "get_theme",
        "get_today_info",
        "get_month_events",
        "refresh_month_events",
        "save_settings",
        "save_widget_settings",
        "reset_settings",
        "clear_events_cache",
        "copy_text",
        "hide_popup",
        "popup_ready",
        "get_clock",
        "get_capsule_data",
        "set_taskbar_surface",
        "open_taskbar_calendar",
        "open_taskbar_menu",
        "get_clock_zones",
        "open_popup_settings",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("Tauri build failed");
}
