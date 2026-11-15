/**
 * Occasions (`get_month_events`) access with a small in-memory month cache.
 *
 * Data comes from Rust only (network/cache/backoff are handled backend-side).
 * A `none` result is deliberately *not* cached in memory so the next popup
 * open / online event can retry (Rust still enforces backoff).
 */
import type { Bridge } from "./bridge";
import { addDays, clampLunarOffset, isFriday, type JDate } from "./calendar";
import type { LunarParts, MonthDayEvents, MonthEvents, Settings } from "./types";

function monthKey(year: number, month: number): string {
  return `${year}-${month}`;
}

/** Returns the (possibly cached) month data for a Jalali year/month, or `undefined`. */
export type MonthLookup = (jy: number, jm: number) => MonthEvents | undefined;

export class EventsStore {
  private readonly cache = new Map<string, MonthEvents>();
  private readonly inFlight = new Map<string, Promise<MonthEvents | null>>();

  constructor(private readonly bridge: Bridge) {}

  /** Synchronous view of the in-memory cache for the display helpers. */
  readonly lookup: MonthLookup = (jy, jm) => this.cache.get(monthKey(jy, jm));

  cached(year: number, month: number): MonthEvents | undefined {
    return this.cache.get(monthKey(year, month));
  }

  /** Returns cached data instantly, otherwise asks Rust (which may hit its disk cache). */
  async load(year: number, month: number): Promise<MonthEvents | null> {
    const cached = this.cache.get(monthKey(year, month));
    if (cached) return cached;
    return this.fetch(year, month, false);
  }

  /** Force a backend refresh attempt (respects Rust-side backoff). */
  async refresh(year: number, month: number): Promise<MonthEvents | null> {
    return this.fetch(year, month, true);
  }

  invalidate(year: number, month: number): void {
    this.cache.delete(monthKey(year, month));
  }

  /**
   * Lunar (Hijri) date to display for a Jalali day, honouring the user's
   * ±3-day shift (`lunarOffset`): a positive offset moves the displayed lunar
   * date forward, a negative one moves it back.
   *
   * The API only reports the lunar date per Jalali day, so the shift is applied
   * by looking up the lunar date of `date + offset`. Days close to a month
   * border therefore need the neighbouring month in the cache; the calendar
   * views prefetch those neighbours while a shift is active.
   */
  lunarFor(date: JDate, offset: number): LunarParts | null {
    return lunarForDate(
      (year, month) => this.cache.get(monthKey(year, month)),
      date,
      offset,
    );
  }

  /** Clears Rust's disk cache *and* this in-memory cache. */
  async clear(): Promise<void> {
    try {
      await this.bridge.clearEventsCache();
    } catch (error) {
      console.error("clear_events_cache failed", error);
    }
    this.cache.clear();
  }

  private fetch(year: number, month: number, force: boolean): Promise<MonthEvents | null> {
    const key = monthKey(year, month);
    const pending = this.inFlight.get(key);
    if (pending) return pending;

    const request = (async (): Promise<MonthEvents | null> => {
      try {
        const data = force
          ? await this.bridge.refreshMonthEvents(year, month)
          : await this.bridge.getMonthEvents(year, month);
        if (data.source !== "none") this.cache.set(key, data);
        return data;
      } catch (error) {
        console.error(`month events ${key} failed`, error);
        return null;
      } finally {
        this.inFlight.delete(key);
      }
    })();

    this.inFlight.set(key, request);
    return request;
  }
}

/* ------------------------------------------------------------------ */
/* Display helpers                                                     */
/* ------------------------------------------------------------------ */

export function dayEvents(month: MonthEvents | null, date: JDate): MonthDayEvents | null {
  if (!month || month.year !== date.jy || month.month !== date.jm) return null;
  return month.days[String(date.jd)] ?? null;
}

/** Occasions + holiday badge composed for one Jalali day. */
export interface DayOccasions {
  events: string[];
  holiday: boolean;
}

/**
 * Fixed official Iranian solar (Jalali) holidays, as `month-day`. These stay
 * holidays even when a lunar occasion on the same day shifts away; they are the
 * only fixed holidays the API does not distinguish from lunar ones on a mixed
 * day.
 */
const SOLAR_HOLIDAY_DATES = new Set([
  "1-1",
  "1-2",
  "1-3",
  "1-4", // نوروز
  "1-12", // روز جمهوری اسلامی
  "1-13", // روز طبیعت
  "3-14", // رحلت امام خمینی
  "3-15", // قیام ۱۵ خرداد
  "11-22", // پیروزی انقلاب اسلامی
  "12-29", // ملی شدن صنعت نفت
]);

/** True for a fixed official solar (Jalali) holiday date. */
export function isSolarHoliday(date: JDate): boolean {
  return SOLAR_HOLIDAY_DATES.has(`${date.jm}-${date.jd}`);
}

function dayEventsIn(lookup: MonthLookup, date: JDate): MonthDayEvents | null {
  return lookup(date.jy, date.jm)?.days[String(date.jd)] ?? null;
}

/**
 * Occasions and holiday badge of a day, honouring the lunar shift.
 *
 * Fixed occasions (solar/Gregorian/undated, `lunar === null`) stay on their own
 * Jalali day. Lunar occasions are tagged with their lunar month/day, so they are
 * collected from `date + offset`: a positive `lunarOffset` makes them appear one
 * day earlier, a negative one one day later. The holiday badge moves with the
 * lunar occasion — the source day loses it, even when the day also has a fixed
 * (non-holiday) occasion. Only official solar holidays and Fridays stay put.
 */
export function occasionsForDate(lookup: MonthLookup, date: JDate, offset: number): DayOccasions {
  const shift = clampLunarOffset(offset);
  const target = shift === 0 ? date : addDays(date, shift);
  const own = dayEventsIn(lookup, date);
  const shifted = shift === 0 ? own : dayEventsIn(lookup, target);

  // Shift 0 keeps the API order untouched; otherwise the fixed occasions of the
  // day come first and the shifted lunar ones follow.
  const events =
    shift === 0
      ? (own?.events ?? []).map((occasion) => occasion.text)
      : [
          ...(own?.events ?? [])
            .filter((occasion) => occasion.lunar === null)
            .map((occasion) => occasion.text),
          ...(shifted?.events ?? [])
            .filter((occasion) => occasion.lunar !== null)
            .map((occasion) => occasion.text),
        ];

  const hasLunar = (own?.events ?? []).some((occasion) => occasion.lunar !== null);
  const fixedHoliday = isSolarHoliday(date) || (own?.holiday === true && !hasLunar);
  const lunarHoliday =
    shifted?.holiday === true && (shifted?.events ?? []).some((occasion) => occasion.lunar !== null);

  return { events, holiday: isFriday(date) || fixedHoliday || lunarHoliday };
}

/** Holiday badge for a day, including the shifted lunar holidays. */
export function isHolidayDate(lookup: MonthLookup, date: JDate, offset: number): boolean {
  return occasionsForDate(lookup, date, offset).holiday;
}

/**
 * Occasions to display for a day, honouring the lunar shift, `showEvents` and
 * `eventsMode`:
 * - "important": holidays show every occasion, other days only the first one
 * - "all": every occasion of every day
 */
export function visibleEventsForDate(
  lookup: MonthLookup,
  date: JDate,
  settings: Pick<Settings, "showEvents" | "eventsMode" | "lunarOffset">,
): string[] {
  if (!settings.showEvents) return [];
  const { events, holiday } = occasionsForDate(lookup, date, settings.lunarOffset);
  if (events.length === 0) return [];
  if (settings.eventsMode === "all") return events;
  return holiday ? events : events.slice(0, 1);
}

/**
 * Lunar date to show for `date` when the user shifts it by `offset` days.
 *
 * Positive offsets move the displayed lunar date forward. `lookup` must return
 * the (possibly cached) month data for a Jalali year/month, or `undefined`.
 */
export function lunarForDate(
  lookup: MonthLookup,
  date: JDate,
  offset: number,
): LunarParts | null {
  const shift = clampLunarOffset(offset);
  const target = shift === 0 ? date : addDays(date, shift);
  const month = lookup(target.jy, target.jm);
  return month?.days[String(target.jd)]?.moon ?? null;
}

export function monthEventsMissing(month: MonthEvents | null): boolean {
  return month !== null && month.source === "none";
}
