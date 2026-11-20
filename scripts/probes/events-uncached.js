// Fetch a month that is not in the on-disk cache (1404-01):
// first call must come from the network, the second from the cache, and the
// cleaned event strings must not contain the bracketed lunar/Gregorian hints.
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const settings = await invoke("get_settings");
  if (!settings.onlineEvents || (!settings.showEvents && !settings.showLunar)) {
    return JSON.stringify({ skipped: "online events are disabled" });
  }
  const first = await invoke("get_month_events", { year: 1404, month: 1 });
  const second = await invoke("get_month_events", { year: 1404, month: 1 });
  const sampleDay = first.days["1"] || null;
  const bracketed = Object.values(first.days).some((day) =>
    (day.events || []).some((event) => event.text.includes("[") || event.text.includes("]")),
  );
  return JSON.stringify({
    firstSource: first.source,
    firstError: first.error,
    dayCount: Object.keys(first.days).length,
    secondSource: second.source,
    stale: second.stale,
    bracketedLeftovers: bracketed,
    sampleDay,
  });
})();
