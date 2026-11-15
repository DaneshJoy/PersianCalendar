import { describe, expect, it } from "vitest";

import { isFriday, type JDate } from "../src/calendar";
import {
  isHolidayDate,
  isSolarHoliday,
  lunarForDate,
  occasionsForDate,
  visibleEventsForDate,
} from "../src/events";
import {
  DEFAULT_SETTINGS,
  type MonthDayEvents,
  type MonthEvents,
  type Occasion,
  type Settings,
} from "../src/types";

/** A fixed (solar/Gregorian/undated) occasion: it never follows `lunarOffset`. */
function fixed(text: string): Occasion {
  return { text, lunar: null };
}

/** A lunar occasion, tagged with the lunar month/day parsed from the API hint. */
function lunarOccasion(text: string, month: number, day: number): Occasion {
  return { text, lunar: { month, day } };
}

/**
 * Build a fake month where every day has a consecutive lunar date, so shifting
 * can be checked without the network: day 1 is lunar 1, day 2 is lunar 2, …
 * `overrides` adds the occasions/holidays each test needs.
 */
function fakeMonth(
  year: number,
  month: number,
  startLunarDay = 1,
  overrides: Record<number, Partial<MonthDayEvents>> = {},
): MonthEvents {
  const days: Record<string, MonthDayEvents> = {};
  for (let day = 1; day <= 30; day += 1) {
    days[String(day)] = {
      holiday: false,
      events: [],
      moon: { year: 1448, month: 5, day: startLunarDay + day - 1 },
      gregorian: null,
      ...(overrides[day] ?? {}),
    };
  }
  return { year, month, source: "cache", stale: false, days, error: null };
}

// 1405/08/01 is a Friday, so 08/01, 08/08, 08/15, 08/22 and 08/29 are Fridays.
const months: Record<string, MonthEvents> = {
  "1405-7": fakeMonth(1405, 7, 1, {
    30: { events: [lunarOccasion("قمری پایان مهر", 5, 30)] },
  }),
  "1405-8": fakeMonth(1405, 8, 1, {
    1: { events: [lunarOccasion("قمری آغاز ماه", 5, 1)] },
    10: { events: [fixed("ثابت دهم"), lunarOccasion("قمری دهم", 5, 10)] },
    12: {
      holiday: true,
      events: [
        lunarOccasion("قمری دوازدهم", 5, 12),
        fixed("ثابت دوازدهم"),
        lunarOccasion("قمری دوم دوازدهم", 5, 12),
      ],
    },
    20: { holiday: true, events: [lunarOccasion("تعطیل قمری بیستم", 5, 20)] },
    24: { holiday: true, events: [fixed("تعطیل ثابت بیست‌وچهارم")] },
    27: { events: [fixed("ثابت اول بیست‌وهفتم"), fixed("ثابت دوم بیست‌وهفتم")] },
  }),
  "1405-9": fakeMonth(1405, 9, 1),
};

function lookup(year: number, month: number): MonthEvents | undefined {
  return months[`${year}-${month}`];
}

/** A Jalali day of Aban (month 8) 1405 unless stated otherwise. */
function jdate(jd: number, jm = 8, jy = 1405): JDate {
  return { jy, jm, jd };
}

const baseSettings: Settings = {
  ...DEFAULT_SETTINGS,
  showEvents: true,
  eventsMode: "important",
  lunarOffset: 0,
};

describe("lunarForDate (lunar shift −3 … +3)", () => {
  it("returns the raw lunar date for a zero offset", () => {
    const lunar = lunarForDate(lookup, { jy: 1405, jm: 8, jd: 10 }, 0);
    expect(lunar).toEqual({ year: 1448, month: 5, day: 10 });
  });

  it("moves the displayed lunar date forward for positive offsets", () => {
    const date = { jy: 1405, jm: 8, jd: 10 };
    expect(lunarForDate(lookup, date, 1)?.day).toBe(11);
    expect(lunarForDate(lookup, date, 3)?.day).toBe(13);
  });

  it("moves the displayed lunar date back for negative offsets", () => {
    const date = { jy: 1405, jm: 8, jd: 10 };
    expect(lunarForDate(lookup, date, -1)?.day).toBe(9);
    expect(lunarForDate(lookup, date, -3)?.day).toBe(7);
  });

  it("crosses month borders using the neighbouring cached month", () => {
    // 1405/07/30 shifted by +2 reaches 1405/08/02.
    const date = { jy: 1405, jm: 7, jd: 30 };
    expect(lunarForDate(lookup, date, 2)).toEqual({ year: 1448, month: 5, day: 2 });
    // 1405/08/01 shifted by -2 reaches 1405/07/29.
    const first = { jy: 1405, jm: 8, jd: 1 };
    expect(lunarForDate(lookup, first, -2)).toEqual({ year: 1448, month: 5, day: 29 });
  });

  it("clamps out-of-range offsets instead of throwing or drifting", () => {
    const date = { jy: 1405, jm: 8, jd: 10 };
    expect(lunarForDate(lookup, date, 99)?.day).toBe(13);
    expect(lunarForDate(lookup, date, -99)?.day).toBe(7);
  });

  it("returns null when the target month is not cached", () => {
    // 1405/09/29 shifted by +3 lands in 1405/10, which is not in the cache.
    expect(lunarForDate(lookup, { jy: 1405, jm: 9, jd: 29 }, 3)).toBeNull();
    expect(lunarForDate(() => undefined, { jy: 1405, jm: 8, jd: 10 }, 0)).toBeNull();
  });

  it("returns null for a missing day entry", () => {
    const empty: MonthEvents = {
      year: 1405,
      month: 8,
      source: "cache",
      stale: false,
      days: {},
      error: null,
    };
    expect(lunarForDate(() => empty, { jy: 1405, jm: 8, jd: 10 }, 0)).toBeNull();
  });
});

describe("occasionsForDate (fixed vs shifted lunar occasions)", () => {
  it("keeps a fixed occasion on its day and moves a lunar one for offset +1", () => {
    // 1405/08/10 has one fixed and one lunar occasion. With +1 the lunar one
    // is displayed on 1405/08/09 (one day earlier) while the fixed one stays.
    expect(occasionsForDate(lookup, jdate(10), 1).events).toEqual(["ثابت دهم"]);
    expect(occasionsForDate(lookup, jdate(9), 1).events).toEqual(["قمری دهم"]);
    // The shifted lunar occasion can come from the neighbouring month's day 1.
    expect(occasionsForDate(lookup, jdate(30, 7), 1).events).toEqual(["قمری آغاز ماه"]);
    expect(occasionsForDate(lookup, jdate(1), 1).events).toEqual([]);
  });

  it("moves lunar occasions the other way for offset -1, across month borders", () => {
    expect(occasionsForDate(lookup, jdate(10), -1).events).toEqual(["ثابت دهم"]);
    expect(occasionsForDate(lookup, jdate(11), -1).events).toEqual(["قمری دهم"]);
    // 1405/08/01 displays the lunar occasion of the previous month's last day.
    expect(occasionsForDate(lookup, jdate(1), -1).events).toEqual(["قمری پایان مهر"]);
    expect(occasionsForDate(lookup, jdate(30, 7), -1).events).toEqual([]);
  });

  it("clamps out-of-range offsets like clampLunarOffset", () => {
    expect(occasionsForDate(lookup, jdate(10), 99)).toEqual(
      occasionsForDate(lookup, jdate(10), 3),
    );
    expect(occasionsForDate(lookup, jdate(10), -99)).toEqual(
      occasionsForDate(lookup, jdate(10), -3),
    );
  });
});

describe("occasionsForDate holiday badge", () => {
  it("moves a lunar-only holiday with its occasion", () => {
    // 1405/08/20 is a holiday because of a lunar occasion only.
    expect(occasionsForDate(lookup, jdate(20), 0).holiday).toBe(true);
    expect(isHolidayDate(lookup, jdate(20), 1)).toBe(false);
    expect(isHolidayDate(lookup, jdate(19), 1)).toBe(true);
    expect(isHolidayDate(lookup, jdate(21), -1)).toBe(true);
    expect(isHolidayDate(lookup, jdate(20), -1)).toBe(false);
  });

  it("keeps a fixed-only holiday on its day", () => {
    // 1405/08/24 is a holiday because of a fixed occasion, so it must not move.
    expect(isHolidayDate(lookup, jdate(24), 1)).toBe(true);
    expect(isHolidayDate(lookup, jdate(24), -1)).toBe(true);
    expect(isHolidayDate(lookup, jdate(23), 1)).toBe(false);
    expect(isHolidayDate(lookup, jdate(25), -1)).toBe(false);
  });

  it("moves the badge on a mixed lunar+fixed day", () => {
    // 1405/08/12 is a holiday with both a lunar and a fixed occasion; with +1
    // the badge leaves that day and lands on 1405/08/11.
    expect(occasionsForDate(lookup, jdate(12), 0).holiday).toBe(true);
    expect(occasionsForDate(lookup, jdate(12), 1).holiday).toBe(false);
    expect(occasionsForDate(lookup, jdate(11), 1).holiday).toBe(true);
  });

  it("keeps official solar holidays fixed for every offset", () => {
    for (const offset of [-3, 0, 3]) {
      // نوروز ۱ فروردین and ۲۲ بهمن stay holidays even without any cached data.
      expect(occasionsForDate(() => undefined, jdate(1, 1), offset).holiday).toBe(true);
      expect(occasionsForDate(() => undefined, jdate(22, 11), offset).holiday).toBe(true);
    }
    expect(isSolarHoliday(jdate(1, 1))).toBe(true);
    expect(isSolarHoliday(jdate(13, 1))).toBe(true);
    expect(isSolarHoliday(jdate(23))).toBe(false);
  });

  it("treats Friday as a holiday for every offset, with or without data", () => {
    for (const offset of [-3, -1, 0, 1, 3]) {
      // 1405/08/08 is a Friday.
      expect(isHolidayDate(lookup, jdate(8), offset)).toBe(true);
      expect(isHolidayDate(() => undefined, jdate(8), offset)).toBe(true);
    }
  });

  it("matches the API holiday flag plus Friday at offset 0", () => {
    const month = months["1405-8"];
    for (let dayNumber = 1; dayNumber <= 30; dayNumber += 1) {
      const date = jdate(dayNumber);
      const apiHoliday = month.days[String(dayNumber)].holiday;
      expect(occasionsForDate(lookup, date, 0).holiday).toBe(apiHoliday || isFriday(date));
    }
  });

  it("preserves the API event order at offset 0", () => {
    expect(occasionsForDate(lookup, jdate(12), 0).events).toEqual([
      "قمری دوازدهم",
      "ثابت دوازدهم",
      "قمری دوم دوازدهم",
    ]);
    expect(occasionsForDate(lookup, jdate(27), 0).events).toEqual([
      "ثابت اول بیست‌وهفتم",
      "ثابت دوم بیست‌وهفتم",
    ]);
  });
});

describe("visibleEventsForDate", () => {
  it("returns nothing when showEvents is disabled", () => {
    expect(
      visibleEventsForDate(lookup, jdate(12), { ...baseSettings, showEvents: false }),
    ).toEqual([]);
  });

  it("shows one occasion on normal days and all on holidays (important)", () => {
    expect(visibleEventsForDate(lookup, jdate(12), baseSettings)).toEqual([
      "قمری دوازدهم",
      "ثابت دوازدهم",
      "قمری دوم دوازدهم",
    ]);
    expect(visibleEventsForDate(lookup, jdate(27), baseSettings)).toEqual([
      "ثابت اول بیست‌وهفتم",
    ]);
  });

  it("shows every occasion with eventsMode all", () => {
    expect(
      visibleEventsForDate(lookup, jdate(27), { ...baseSettings, eventsMode: "all" }),
    ).toEqual(["ثابت اول بیست‌وهفتم", "ثابت دوم بیست‌وهفتم"]);
  });

  it("uses the shifted occasions and holiday for the offset", () => {
    // 1405/08/11 inherits day 12's lunar occasions (and its holiday) with +1.
    expect(
      visibleEventsForDate(lookup, jdate(11), { ...baseSettings, lunarOffset: 1 }),
    ).toEqual(["قمری دوازدهم", "قمری دوم دوازدهم"]);
    expect(
      visibleEventsForDate(lookup, jdate(10), { ...baseSettings, lunarOffset: 1 }),
    ).toEqual(["ثابت دهم"]);
  });
});
