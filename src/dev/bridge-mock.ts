/**
 * DEVELOPER-ONLY mock of the Rust backend.
 *
 * It is loaded exclusively by `src/bridge.ts` when `import.meta.env.DEV`
 * is true and the page runs in a plain browser (no `__TAURI_INTERNALS__`).
 * Production builds never include it in the popup/widget behavior.
 */
import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  addDays,
  clampLunarOffset,
  formatGregorianIso,
  formatJalaliLong,
  formatJalaliSlash,
  gregorianOf,
  jdnOf,
  monthLength,
  todayJalali,
  weekdayOf,
} from "../calendar";
import type { Bridge } from "../bridge";
import { occasionsForDate, type MonthLookup } from "../events";
import {
  DEFAULT_SETTINGS,
  type AppEventName,
  type EventPayloads,
  type MonthDayEvents,
  type MonthEvents,
  type Occasion,
  type Settings,
  type ThemeInfo,
  type TodayInfo,
} from "../types";

const STORAGE_KEY = "persian-calendar-dev-settings";

/** Plausible-looking occasion strings so the visual states can be reviewed. */
const SAMPLE_EVENTS: Record<number, string[]> = {
  1: ["آغاز ماه جدید"],
  7: ["روز بزرگداشت نمونه"],
  12: ["مناسبت تعطیل نمونه", "مناسبت دوم همان روز"],
  15: ["مناسبت میانه ماه"],
  21: ["مناسبت نمونه آبان"],
  25: ["روز نمونه"],
  29: ["پایان ماه"],
};

/**
 * Sample days whose occasion is lunar (Hijri). Lunar occasions move with the
 * `lunarOffset` setting, so the dev preview can show them shifting; every other
 * sample occasion is a fixed solar one that stays on its day.
 */
const LUNAR_SAMPLE_DAYS = new Set([7, 12, 21]);

/**
 * Days the mock API reports as holidays. Day 12 has a fixed *and* a lunar
 * occasion, so its badge must stay; day 21 has only a lunar one, so its badge
 * must move with `lunarOffset`.
 */
const SAMPLE_HOLIDAY_DAYS = new Set([12, 21]);

function readSettings(): Settings {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed: unknown = JSON.parse(raw);
      if (typeof parsed === "object" && parsed !== null) {
        return { ...DEFAULT_SETTINGS, ...(parsed as Partial<Settings>) };
      }
    }
  } catch {
    /* ignore corrupted dev storage */
  }
  return { ...DEFAULT_SETTINGS };
}

function writeSettings(settings: Settings): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch {
    /* dev only */
  }
}

function systemTheme(): "light" | "dark" {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** Rough tabular-Islamic conversion; good enough for a dev preview. */
function approximateLunar(date: { jy: number; jm: number; jd: number }): {
  year: number;
  month: number;
  day: number;
} {
  const totalDays = jdnOf(date) - 1948440;
  const year = Math.floor(totalDays / 354.367) + 1;
  const dayOfYear = totalDays - Math.floor((year - 1) * 354.367);
  const month = Math.min(12, Math.max(1, Math.floor(dayOfYear / 29.53) + 1));
  const day = Math.min(30, Math.max(1, Math.floor(dayOfYear - (month - 1) * 29.53) + 1));
  return { year, month, day };
}

/**
 * Builds the sample occasions of a day. Lunar-tagged ones carry the approximate
 * lunar month/day of that Jalali day, mirroring what the Rust backend parses
 * from the API's bracketed date hint.
 */
function sampleOccasions(date: { jy: number; jm: number; jd: number }): Occasion[] {
  const texts = SAMPLE_EVENTS[date.jd] ?? [];
  if (texts.length === 0) return [];
  const lunar = LUNAR_SAMPLE_DAYS.has(date.jd) ? approximateLunar(date) : null;
  return texts.map((text, index) => {
    // Day 12 previews ordering: the fixed occasion stays, the lunar one shifts.
    const isLunar = lunar !== null && (texts.length === 1 || index > 0);
    return {
      text,
      lunar: isLunar ? { month: lunar.month, day: lunar.day } : null,
    };
  });
}

function buildMonthEvents(year: number, month: number): MonthEvents {
  const days: Record<string, MonthDayEvents> = {};
  const length = monthLength(year, month);

  for (let day = 1; day <= length; day += 1) {
    const date = { jy: year, jm: month, jd: day };
    const gregorian = gregorianOf(date);
    const friday = weekdayOf(date) === 6;
    const apiHoliday = SAMPLE_HOLIDAY_DAYS.has(day);
    days[String(day)] = {
      holiday: friday || apiHoliday,
      events: sampleOccasions(date),
      moon: approximateLunar(date),
      gregorian: { year: gregorian.gy, month: gregorian.gm, day: gregorian.gd },
    };
  }

  return { year, month, source: "network", stale: false, days, error: null };
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

export function createDevBridge(): Bridge {
  let settings = readSettings();
  const eventsCache = new Map<string, MonthEvents>();

  const monthEvents = (year: number, month: number): MonthEvents => {
    const key = `${year}-${month}`;
    const cached = eventsCache.get(key);
    if (cached) return cached;
    const built = buildMonthEvents(year, month);
    eventsCache.set(key, built);
    return built;
  };

  const monthLookup: MonthLookup = (jy, jm) => monthEvents(jy, jm);

  const todayInfo = (): TodayInfo => {
    const date = todayJalali();
    // Mirror the backend contract: `lunar` is shifted by `lunarOffset` days.
    const offset = clampLunarOffset(settings.lunarOffset ?? 0);
    const lunarDate = offset === 0 ? date : addDays(date, offset);
    const lunar =
      monthEvents(lunarDate.jy, lunarDate.jm).days[String(lunarDate.jd)]?.moon ?? null;
    const gregorian = gregorianOf(date);
    // Same composition as the calendar views: fixed occasions stay, lunar ones
    // shift, and the holiday badge follows.
    const dayOccasions = occasionsForDate(monthLookup, date, offset);
    let events: string[] = [];
    if (settings.showEvents) {
      events =
        settings.eventsMode === "all" || dayOccasions.holiday
          ? dayOccasions.events
          : dayOccasions.events.slice(0, 1);
    }
    return {
      jalali: { year: date.jy, month: date.jm, day: date.jd, weekday: weekdayOf(date) },
      gregorian: { year: gregorian.gy, month: gregorian.gm, day: gregorian.gd },
      lunar,
      holiday: dayOccasions.holiday,
      events,
      labels: {
        slash: formatJalaliSlash(date, settings.digits),
        long: formatJalaliLong(date, settings.digits),
        iso: formatGregorianIso(date),
      },
    };
  };

  return {
    isTauri: false,
    windowLabel: document.body.classList.contains("widget-body") ? "widget" : "popup",
    getSettings: async () => ({ ...settings }),
    saveSettings: async (next: Settings) => {
      settings = { ...next };
      writeSettings(settings);
      return { ...settings };
    },
    resetSettings: async () => {
      settings = { ...DEFAULT_SETTINGS };
      writeSettings(settings);
      return { ...settings };
    },
    getTheme: async (): Promise<ThemeInfo> => {
      const system = systemTheme();
      return { mode: settings.theme === "auto" ? system : settings.theme, system };
    },
    getTodayInfo: async () => {
      await delay(40);
      return todayInfo();
    },
    getMonthEvents: async (year, month) => {
      await delay(140);
      return monthEvents(year, month);
    },
    refreshMonthEvents: async (year, month) => {
      await delay(180);
      return monthEvents(year, month);
    },
    clearEventsCache: async () => {
      eventsCache.clear();
    },
    copyText: async (text) => {
      try {
        await window.navigator.clipboard?.writeText(text);
      } catch {
        console.debug("[dev] copy_text", text);
      }
    },
    hidePopup: async () => {
      console.debug("[dev] hide_popup");
    },
    showPopup: async () => {
      console.debug("[dev] show_popup");
    },
    quitApp: async () => {
      console.debug("[dev] quit_app");
    },
    requestOpenSettings: async () => {
      console.debug("[dev] open-settings");
    },
    listen: async <K extends AppEventName>(
      _event: K,
      _handler: (payload: EventPayloads[K]) => void,
    ): Promise<UnlistenFn> => {
      return async () => {
        /* no-op in dev */
      };
    },
  };
}
