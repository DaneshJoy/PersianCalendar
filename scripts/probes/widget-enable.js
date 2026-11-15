// Enable the desktop widget in the requested size and show it.
// `window.__widgetSize` must be set by the caller (small|full).
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const settings = await invoke("get_settings");
  const size = window.__widgetSize === "small" ? "small" : "full";
  await invoke("save_settings", {
    settings: { ...settings, widgetEnabled: true, widgetSize: size, widgetLocked: true },
  });
  return JSON.stringify({ widgetEnabled: true, widgetSize: size });
})();
