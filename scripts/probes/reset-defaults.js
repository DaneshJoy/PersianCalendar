// Reset every setting to defaults through the real IPC command.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const reset = await invoke("reset_settings");
  const theme = await invoke("get_theme");
  const today = await invoke("get_today_info");
  return JSON.stringify({
    digits: reset.digits,
    theme: reset.theme,
    showEvents: reset.showEvents,
    widgetEnabled: reset.widgetEnabled,
    widgetSize: reset.widgetSize,
    accent: reset.accent,
    autostart: reset.autostart,
    themeInfo: theme,
    todayLabel: today.labels.long,
    todayHoliday: today.holiday,
    todayEvents: today.events,
  });
})();
