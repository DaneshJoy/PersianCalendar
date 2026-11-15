// Toggle the app theme to the opposite of the current resolved theme, verify
// the rendered palette, then restore the previous setting.
(async () => {
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const settings = await invoke("get_settings");
  const themeInfo = await invoke("get_theme");
  const target = themeInfo.mode === "dark" ? "light" : "dark";

  await invoke("save_settings", { settings: { ...settings, theme: target } });
  await sleep(500);
  const applied = document.documentElement.dataset.theme;
  const surface = getComputedStyle(document.querySelector(".popup")).backgroundColor;
  const text = getComputedStyle(document.body).color;

  // restore the user's original theme choice
  await invoke("save_settings", { settings });
  await sleep(300);

  return JSON.stringify({
    previous: themeInfo,
    requested: target,
    applied,
    surface,
    text,
    restored: document.documentElement.dataset.theme,
  });
})();
