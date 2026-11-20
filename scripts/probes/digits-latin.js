// Switch the digit style to Latin via the real settings panel and report the
// rendered digits + persisted setting.
(async () => {
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const $ = (selector) => document.querySelector(selector);
  const $$ = (selector) => [...document.querySelectorAll(selector)];
  const invoke = window.__TAURI_INTERNALS__.invoke;

  $('button[aria-label="تنظیمات"]').click();
  await sleep(300);
  const digitRow = $$(".settings__row").find((row) => row.textContent.includes("نوع ارقام"));
  const latin = [...digitRow.querySelectorAll(".segmented__option")].find(
    (option) => option.textContent.trim() === "لاتین",
  );
  latin.click();
  await sleep(800);
  $('.settings-overlay button[aria-label="بستن تنظیمات"]').click();
  await sleep(300);

  const settings = await invoke("get_settings");
  return JSON.stringify({
    digits: settings.digits,
    dayNumbers: $$(".day__num")
      .slice(0, 6)
      .map((element) => element.textContent.trim()),
    todayNumber: $(".day.is-today .day__num")?.textContent?.trim(),
    title: $(".popup__title").textContent.trim(),
  });
})();
