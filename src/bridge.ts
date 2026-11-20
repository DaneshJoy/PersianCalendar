/**
 * Typed bridge to the Tauri backend.
 *
 * Every IPC call and Rust → WebView event from docs/CONTRACT.md is wrapped
 * here so the rest of the UI never talks to `invoke`/`listen` directly.
 *
 * In `vite dev` (a plain browser, no Tauri runtime) a small in-memory mock is
 * used so the UI can be developed/previewed without the Rust shell.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type {
  AppEventName,
  ClockInfo,
  EventPayloads,
  MonthEvents,
  Settings,
  ThemeInfo,
  TodayInfo,
} from "./types";

export interface Bridge {
  getSettings(): Promise<Settings>;
  saveSettings(settings: Partial<Settings>): Promise<Settings>;
  resetSettings(): Promise<Settings>;
  getTheme(): Promise<ThemeInfo>;
  getTodayInfo(): Promise<TodayInfo>;
  getClock(): Promise<ClockInfo | null>;
  getClockZones(): Promise<string[]>;
  getMonthEvents(year: number, month: number): Promise<MonthEvents>;
  refreshMonthEvents(year: number, month: number): Promise<MonthEvents>;
  clearEventsCache(): Promise<void>;
  copyText(text: string): Promise<void>;
  hidePopup(): Promise<void>;
  popupReady(): Promise<void>;
  /** Widget → popup: show the popup and ask it to open its settings panel. */
  requestOpenSettings(): Promise<void>;
  listen<K extends AppEventName>(event: K, handler: (payload: EventPayloads[K]) => void): Promise<UnlistenFn>;
  readonly windowLabel: string;
  readonly isTauri: boolean;
}

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function currentWindowLabel(): string {
  if (isTauriRuntime()) return getCurrentWindow().label;
  return document.body.classList.contains("widget-body") ? "widget" : "popup";
}

const tauriBridge: Bridge = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings) => invoke<Settings>(
    currentWindowLabel() === "widget" ? "save_widget_settings" : "save_settings", { settings },
  ),
  resetSettings: () => invoke<Settings>("reset_settings"),
  getTheme: () => invoke<ThemeInfo>("get_theme"),
  getTodayInfo: () => invoke<TodayInfo>("get_today_info"),
  getClock: () => invoke<ClockInfo | null>("get_clock"),
  getClockZones: () => invoke<string[]>("get_clock_zones"),
  getMonthEvents: (year, month) => invoke<MonthEvents>("get_month_events", { year, month }),
  refreshMonthEvents: (year, month) => invoke<MonthEvents>("refresh_month_events", { year, month }),
  clearEventsCache: async () => {
    await invoke("clear_events_cache");
  },
  copyText: async (text) => {
    await invoke("copy_text", { text });
  },
  hidePopup: async () => {
    await invoke("hide_popup");
  },
  popupReady: () => invoke("popup_ready"),
  requestOpenSettings: () => invoke("open_popup_settings"),
  listen: <K extends AppEventName>(event: K, handler: (payload: EventPayloads[K]) => void) =>
    listen<EventPayloads[K]>(event, (received) => handler(received.payload)),
  get windowLabel(): string {
    // Read lazily: `getCurrentWindow()` must be called inside the Tauri shell
    // (it is never called in a plain browser dev/preview session).
    return currentWindowLabel();
  },
  isTauri: true,
};

/**
 * Loaded only when the page is not running inside the Tauri shell (plain
 * browser dev/preview). The real application always takes the Tauri branch.
 */
async function loadDevBridge(): Promise<Bridge> {
  const { createDevBridge } = await import("./dev/bridge-mock");
  return createDevBridge();
}

let bridgePromise: Promise<Bridge> | null = null;

/**
 * Creates (once) the bridge for the current window. Inside Tauri this is the
 * real `invoke`/`listen` IPC bridge; in a plain browser (vite dev or preview)
 * it resolves to the mock implementation so the UI can be reviewed without
 * the Rust shell. Production Tauri builds never load the mock.
 */
export function createBridge(): Promise<Bridge> {
  if (!bridgePromise) {
    if (isTauriRuntime()) {
      bridgePromise = Promise.resolve(tauriBridge);
    } else if (import.meta.env.DEV) {
      bridgePromise = loadDevBridge();
    } else {
      bridgePromise = Promise.reject(new Error("Tauri runtime unavailable"));
    }
  }
  return bridgePromise;
}
