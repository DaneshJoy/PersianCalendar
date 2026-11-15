import { isLeapJalaaliYear } from "jalaali-js";
import { describe, expect, it } from "vitest";

import {
  addDays,
  clampLunarOffset,
  fallbackTodayInfo,
  formatCopyLabel,
  formatGregorianIso,
  formatGregorianLong,
  formatJalaliLong,
  formatJalaliSlash,
  gregorianOf,
  LUNAR_OFFSET_MAX,
  LUNAR_OFFSET_MIN,
  monthLength,
  monthTitle,
  monthWeeks,
  toFaDigits,
  weekNumberOf,
  weekdayOf,
  weekdayOrder,
  WEEKDAY_SHORT,
} from "../src/calendar";

/**
 * Deterministic reference points (verified against Windows 11's Persian
 * calendar): 1405/08/22 == 2026-11-13 and that day is a Friday.
 */
const DATE_1405_08_22 = { jy: 1405, jm: 8, jd: 22 };

describe("calendar grid", () => {
  it("lays out Aban 1405 correctly when the week starts on Saturday", () => {
    const weeks = monthWeeks(1405, 8, 0);
    const inMonth = weeks.flatMap((week) => week.days).filter((day) => day.inMonth);

    // Aban 1405 starts on Friday (weekday 6) and has 30 days.
    expect(weeks).toHaveLength(6);
    expect(weeks[0].days).toHaveLength(7);
    expect(weeks[0].days.slice(0, 6).every((day) => !day.inMonth)).toBe(true);
    expect(weeks[0].days[6].date).toEqual({ jy: 1405, jm: 8, jd: 1 });
    expect(weeks[0].days[6].weekday).toBe(6);
    expect(inMonth).toHaveLength(30);
    expect(inMonth[0].date.jd).toBe(1);
    expect(inMonth[29].date.jd).toBe(30);
  });

  it("honours weekStart = 1 (Sunday) and 2 (Monday)", () => {
    // 1 Farvardin is always the first day of the week starting on Saturday.
    expect(weekdayOrder(0)).toEqual([0, 1, 2, 3, 4, 5, 6]);
    expect(weekdayOrder(1)).toEqual([1, 2, 3, 4, 5, 6, 0]);
    expect(weekdayOrder(2)).toEqual([2, 3, 4, 5, 6, 0, 1]);
    expect(WEEKDAY_SHORT[0]).toBe("ش");
    expect(WEEKDAY_SHORT[6]).toBe("ج");

    for (const weekStart of [1, 2]) {
      const weeks = monthWeeks(1405, 8, weekStart);
      const lead = (6 - weekStart + 7) % 7;
      expect(weeks[0].days[0].weekday).toBe(weekStart);
      expect(weeks[0].days[lead].date).toEqual({ jy: 1405, jm: 8, jd: 1 });
      expect(weeks[0].days[lead].weekday).toBe(6);
      expect(weeks.flatMap((week) => week.days).filter((day) => day.inMonth)).toHaveLength(30);
    }
  });

  it("computes Persian week numbers from Farvardin 1", () => {
    expect(weekNumberOf({ jy: 1405, jm: 1, jd: 1 }, 1405, 0)).toBe(1);
    const weeks = monthWeeks(1405, 8, 0);
    expect(weeks.map((week) => week.weekNumber)).toEqual([31, 32, 33, 34, 35, 36]);
  });

  it("lays out Farvardin 1405 (starts on Saturday, 31 days, 5 weeks)", () => {
    const weeks = monthWeeks(1405, 1, 0);
    expect(weeks).toHaveLength(5);
    expect(weeks[0].days[0].date).toEqual({ jy: 1405, jm: 1, jd: 1 });
    expect(weeks[0].days[0].inMonth).toBe(true);
    expect(weeks[0].days[0].weekday).toBe(0);
    expect(weeks.flatMap((week) => week.days).filter((day) => day.inMonth)).toHaveLength(31);
  });
});

describe("calendar conversions and rollover", () => {
  it("converts the known Jalaali/Gregorian pair", () => {
    expect(gregorianOf(DATE_1405_08_22)).toEqual({ gy: 2026, gm: 11, gd: 13 });
    expect(weekdayOf(DATE_1405_08_22)).toBe(6);
    expect(gregorianOf({ jy: 1405, jm: 8, jd: 21 })).toEqual({ gy: 2026, gm: 11, gd: 12 });
    expect(weekdayOf({ jy: 1405, jm: 8, jd: 21 })).toBe(5);
  });

  it("rolls over month and year boundaries", () => {
    expect(addDays({ jy: 1405, jm: 8, jd: 1 }, -1)).toEqual({ jy: 1405, jm: 7, jd: 30 });
    expect(addDays({ jy: 1405, jm: 8, jd: 30 }, 1)).toEqual({ jy: 1405, jm: 9, jd: 1 });
    expect(addDays({ jy: 1405, jm: 12, jd: 29 }, 1)).toEqual({ jy: 1406, jm: 1, jd: 1 });
  });

  it("handles the leap Esfand of 1395", () => {
    expect(isLeapJalaaliYear(1395)).toBe(true);
    expect(monthLength(1395, 12)).toBe(30);
    expect(addDays({ jy: 1395, jm: 12, jd: 30 }, 1)).toEqual({ jy: 1396, jm: 1, jd: 1 });
    const days = monthWeeks(1395, 12, 0)
      .flatMap((week) => week.days)
      .filter((day) => day.inMonth);
    expect(days).toHaveLength(30);
    expect(days[29].date).toEqual({ jy: 1395, jm: 12, jd: 30 });
  });
});

describe("calendar formatting", () => {
  it("formats Persian and Latin digits", () => {
    expect(toFaDigits(1405)).toBe("۱۴۰۵");
    expect(toFaDigits("1405/08/21")).toBe("۱۴۰۵/۰۸/۲۱");
    expect(formatJalaliSlash(DATE_1405_08_22, "fa")).toBe("۱۴۰۵/۰۸/۲۲");
    expect(formatJalaliSlash(DATE_1405_08_22, "en")).toBe("1405/08/22");
    expect(formatJalaliLong({ jy: 1405, jm: 8, jd: 21 }, "fa")).toBe("۲۱ آبان ۱۴۰۵");
    expect(formatJalaliLong({ jy: 1405, jm: 8, jd: 21 }, "en")).toBe("21 آبان 1405");
    expect(monthTitle(1405, 8, "fa")).toBe("آبان ۱۴۰۵");
    expect(monthTitle(1405, 8, "en")).toBe("آبان 1405");
  });

  it("produces every copy format, keeping the ISO label in Latin digits", () => {
    expect(formatGregorianIso(DATE_1405_08_22)).toBe("2026-11-13");
    expect(formatCopyLabel(DATE_1405_08_22, "jalaliSlash", "fa")).toBe("۱۴۰۵/۰۸/۲۲");
    expect(formatCopyLabel(DATE_1405_08_22, "jalaliLong", "fa")).toBe("۲۲ آبان ۱۴۰۵");
    expect(formatCopyLabel(DATE_1405_08_22, "gregorianIso", "fa")).toBe("2026-11-13");
    expect(formatCopyLabel(DATE_1405_08_22, "gregorianIso", "en")).toBe("2026-11-13");
  });

  it("builds a fully local TodayInfo fallback", () => {
    const info = fallbackTodayInfo("fa", new Date(2026, 10, 13, 12, 0, 0));
    expect(info.jalali).toEqual({ year: 1405, month: 8, day: 22, weekday: 6 });
    expect(info.gregorian).toEqual({ year: 2026, month: 11, day: 13 });
    expect(info.holiday).toBe(true);
    expect(info.lunar).toBeNull();
    expect(info.events).toEqual([]);
    expect(info.labels).toEqual({
      slash: "۱۴۰۵/۰۸/۲۲",
      long: "۲۲ آبان ۱۴۰۵",
      iso: "2026-11-13",
    });
  });
});

describe("Gregorian labels and the lunar offset", () => {
  it("keeps the Gregorian long label in Latin digits regardless of settings", () => {
    // The digits setting only applies to Jalali and lunar dates.
    expect(formatGregorianLong(DATE_1405_08_22)).toBe("13 نوامبر 2026");
    expect(formatGregorianLong({ jy: 1405, jm: 8, jd: 1 })).toBe("23 اکتبر 2026");
    expect(formatGregorianLong({ jy: 1405, jm: 1, jd: 1 })).toBe("21 مارس 2026");
  });

  it("clamps the lunar offset to the supported −3 … +3 day range", () => {
    expect([LUNAR_OFFSET_MIN, LUNAR_OFFSET_MAX]).toEqual([-3, 3]);
    for (const value of [-3, -1, 0, 1, 3]) {
      expect(clampLunarOffset(value)).toBe(value);
    }
    expect(clampLunarOffset(-10)).toBe(-3);
    expect(clampLunarOffset(10)).toBe(3);
    expect(clampLunarOffset(-3.7)).toBe(-3);
    expect(clampLunarOffset(2.9)).toBe(2);
    expect(clampLunarOffset(Number.NaN)).toBe(0);
    expect(clampLunarOffset(Number.POSITIVE_INFINITY)).toBe(0);
  });
});
