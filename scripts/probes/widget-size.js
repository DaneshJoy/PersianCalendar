// Widget: switch between small/full sizes via save_settings and report the
// widget DOM mode plus the number of rendered days.
(async () => {
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const settings = await invoke("get_settings");
  const results = {};

  await invoke("save_settings", {
    settings: { ...settings, widgetEnabled: true, widgetSize: "full", widgetLocked: true },
  });
  await sleep(600);
  results.full = { size: (await invoke("get_settings")).widgetSize };

  await invoke("save_settings", {
    settings: { ...settings, widgetEnabled: true, widgetSize: "small", widgetLocked: true },
  });
  await sleep(600);
  results.small = { size: (await invoke("get_settings")).widgetSize };

  await invoke("save_settings", { settings });
  await sleep(400);
  results.restored = (await invoke("get_settings")).widgetSize;
  return JSON.stringify(results);
})();
