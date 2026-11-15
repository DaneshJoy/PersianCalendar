/**
 * Shared types that mirror docs/CONTRACT.md exactly.
 * These are the shapes exchanged with the Rust backend; keep them in sync.
 */

export type ThemeMode = "auto" | "light" | "dark";
export type ThemeValue = "light" | "dark";
export type Digits = "fa" | "en";
export type FontChoice = "vazirmatn" | "system";
export type TrayIconMode = "day" | "static";
export type WeekStart = 0 | 1 | 2;
export type EventsMode = "important" | "all";
export type CopyFormat = "jalaliSlash" | "jalaliLong" | "gregorianIso";
export type WidgetSize = "small" | "full";

export interface Settings {
  theme: ThemeMode;
  accent: string;
  digits: Digits;
  font: FontChoice;
  trayIconMode: TrayIconMode;
  weekStart: WeekStart;
  showGregorian: boolean;
  showLunar: boolean;
  /** Whole-day shift applied to the lunar (Hijri) date: -3 … +3. */
  lunarOffset: number;
  showEvents: boolean;
  eventsMode: EventsMode;
  showWeekNumber: boolean;
  copyFormat: CopyFormat;
  autostart: boolean;
  closeOnBlur: boolean;
  widgetEnabled: boolean;
  widgetSize: WidgetSize;
  /** 30 … 100 */
  widgetOpacity: number;
  widgetLocked: boolean;
  widgetX: number | null;
  widgetY: number | null;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "auto",
  accent: "#2f6fed",
  digits: "fa",
  font: "vazirmatn",
  trayIconMode: "day",
  weekStart: 0,
  showGregorian: false,
  showLunar: false,
  lunarOffset: 0,
  showEvents: true,
  eventsMode: "important",
  showWeekNumber: false,
  copyFormat: "jalaliSlash",
  autostart: false,
  closeOnBlur: true,
  widgetEnabled: false,
  widgetSize: "full",
  widgetOpacity: 92,
  widgetLocked: false,
  widgetX: null,
  widgetY: null,
};

export interface JalaliToday {
  year: number;
  month: number;
  day: number;
  /** 0 = Saturday … 6 = Friday */
  weekday: number;
}

export interface GregorianParts {
  year: number;
  month: number;
  day: number;
}

export interface LunarParts {
  year: number;
  month: number;
  day: number;
}

export interface TodayInfo {
  jalali: JalaliToday;
  gregorian: GregorianParts;
  lunar: LunarParts | null;
  holiday: boolean;
  events: string[];
  /** Pre-formatted labels, digits already applied by the backend. */
  labels: { slash: string; long: string; iso: string };
}

export type EventsSource = "cache" | "network" | "none";

/** Lunar month (1 … 12) and day (1 … 30) parsed from an occasion's date hint. */
export interface LunarMonthDay {
  month: number;
  day: number;
}

/**
 * One occasion of a day. `lunar === null` means the occasion has a fixed date
 * (solar/Gregorian/undated) and never moves; a non-null value is its lunar
 * date, used to shift the occasion with the user's `lunarOffset`.
 */
export interface Occasion {
  /** Cleaned text (the "[ … ]" part already stripped by Rust). */
  text: string;
  lunar: LunarMonthDay | null;
}

export interface MonthDayEvents {
  holiday: boolean;
  /** API order; fixed and lunar occasions are interleaved. */
  events: Occasion[];
  moon: LunarParts | null;
  gregorian: GregorianParts | null;
}

export interface MonthEvents {
  year: number;
  month: number;
  source: EventsSource;
  /** An expired cache entry was served while revalidating. */
  stale: boolean;
  /** Keyed by day-of-month as a decimal string. */
  days: Record<string, MonthDayEvents>;
  error: string | null;
}

export interface ThemeInfo {
  /** Resolved theme according to the setting. */
  mode: ThemeValue;
  /** Windows/taskbar theme. */
  system: ThemeValue;
}

/* ----- Rust → WebView events ----- */

export interface EventPayloads {
  "settings-changed": Settings;
  "theme-changed": ThemeInfo;
  "today-changed": TodayInfo;
  "events-updated": { year: number; month: number };
  "open-settings": Record<string, never>;
  "show-month": { year: number; month: number };
  copied: { what: string };
}

export type AppEventName = keyof EventPayloads;
