/**
 * Shared bootstrap for both webview windows.
 *
 * Creates the IPC bridge, loads/apply settings, prepares the events store and
 * resolves today's info. Every step has a local fallback so the UI still
 * renders if a backend command is temporarily unavailable (or when the page
 * is opened in a plain browser for visual review).
 */
import { createBridge, type Bridge } from "./bridge";
import { fallbackTodayInfo } from "./calendar";
import { EventsStore } from "./events";
import { bindAppearance, SettingsStore } from "./settings";
import type { TodayInfo } from "./types";

export interface AppServices {
  bridge: Bridge;
  settings: SettingsStore;
  events: EventsStore;
  today: TodayInfo;
}

export type WindowKind = "popup" | "widget";

async function loadTodayInfo(bridge: Bridge, settings: SettingsStore): Promise<TodayInfo> {
  try {
    return await bridge.getTodayInfo();
  } catch (error) {
    console.warn("get_today_info failed; using local fallback", error);
    return fallbackTodayInfo(settings.settings.digits);
  }
}

export async function bootstrapServices(expectedWindow: WindowKind): Promise<AppServices> {
  const bridge = await createBridge();
  const windowLabel = bridge.windowLabel;
  document.body.dataset.window = windowLabel;

  if (bridge.isTauri && windowLabel !== expectedWindow) {
    console.warn(`[${expectedWindow}] unexpected window label: "${windowLabel}"`);
  }

  const settings = await SettingsStore.load(bridge);
  bindAppearance(settings);

  const events = new EventsStore(bridge);
  const today = await loadTodayInfo(bridge, settings);

  return { bridge, settings, events, today };
}
