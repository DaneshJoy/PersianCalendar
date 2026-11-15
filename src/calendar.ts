/**
 * Persian (Jalaali) calendar math and formatting.
 *
 * Everything here is pure/local: the calendar grid, weekday math and the
 * Gregorian conversion use `jalaali-js` only. The network is never touched.
 */
import { d2j, j2d, jalaaliMonthLength, toGregorian, toJalaali } from "jalaali-js";

import type { CopyFormat, Digits, TodayInfo } from "./types";

export interface JDate {
  jy: number;
  jm: number;
  jd: number;
}

export interface GDate {
  gy: number;
  gm: number;
  gd: number;
}

/** Supported year window for navigation (API range is 1 … 3000). */
export const MIN_YEAR = 1;
export const MAX_YEAR = 3000;

/**
 * Allowed whole-day shift for the lunar (Hijri) date. Users pick it to
 * compensate for the ±1-3 day difference between the API's lunar date and the
 * official Iranian lunar calendar.
 */
export const LUNAR_OFFSET_MIN = -3;
export const LUNAR_OFFSET_MAX = 3;

/** Clamps a lunar offset into `LUNAR_OFFSET_MIN … LUNAR_OFFSET_MAX`. */
export function clampLunarOffset(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(LUNAR_OFFSET_MAX, Math.max(LUNAR_OFFSET_MIN, Math.trunc(value)));
}

export const MONTH_NAMES = [
  "فروردین",
  "اردیبهشت",
  "خرداد",
  "تیر",
  "مرداد",
  "شهریور",
  "مهر",
  "آبان",
  "آذر",
  "دی",
  "بهمن",
  "اسفند",
] as const;

/** 0 = Saturday … 6 = Friday (the Jalaali week starts on Saturday). */
export const WEEKDAY_NAMES = [
  "شنبه",
  "یکشنبه",
  "دوشنبه",
  "سه‌شنبه",
  "چهارشنبه",
  "پنج‌شنبه",
  "جمعه",
] as const;

export const WEEKDAY_SHORT = ["ش", "ی", "د", "س", "چ", "پ", "ج"] as const;

export const GREGORIAN_MONTH_NAMES = [
  "ژانویه",
  "فوریه",
  "مارس",
  "آوریل",
  "مه",
  "ژوئن",
  "ژوئیه",
  "اوت",
  "سپتامبر",
  "اکتبر",
  "نوامبر",
  "دسامبر",
] as const;

export const LUNAR_MONTH_NAMES = [
  "محرم",
  "صفر",
  "ربیع‌الاول",
  "ربیع‌الثانی",
  "جمادی‌الاول",
  "جمادی‌الثانی",
  "رجب",
  "شعبان",
  "رمضان",
  "شوال",
  "ذی‌القعده",
  "ذی‌الحجه",
] as const;

const FA_DIGITS = ["۰", "۱", "۲", "۳", "۴", "۵", "۶", "۷", "۸", "۹"] as const;

export function pad2(value: number): string {
  return value < 10 ? `0${value}` : String(value);
}

export function toFaDigits(value: string | number): string {
  return String(value).replace(/[0-9]/g, (digit) => FA_DIGITS[Number(digit)] ?? digit);
}

/** Formats a number using the digit style chosen in the settings. */
export function fmtNumber(value: number, digits: Digits): string {
  return digits === "fa" ? toFaDigits(value) : String(value);
}

function localizeNumber(value: number, digits: Digits): string {
  return digits === "fa" ? toFaDigits(value) : String(value);
}

/* ------------------------------------------------------------------ */
/* Date math                                                           */
/* ------------------------------------------------------------------ */

export function jdnOf(date: JDate): number {
  return j2d(date.jy, date.jm, date.jd);
}

export function fromJdn(jdn: number): JDate {
  const result = d2j(jdn);
  return { jy: result.jy, jm: result.jm, jd: result.jd };
}

/**
 * Day of week for a Julian Day Number: 0 = Saturday … 6 = Friday.
 * Kept timezone-free on purpose (no Date objects involved).
 */
export function weekdayOfJdn(jdn: number): number {
  return (((jdn + 2) % 7) + 7) % 7;
}

export function weekdayOf(date: JDate): number {
  return weekdayOfJdn(jdnOf(date));
}

export function addDays(date: JDate, days: number): JDate {
  return fromJdn(jdnOf(date) + days);
}

export function monthLength(jy: number, jm: number): number {
  return jalaaliMonthLength(jy, jm);
}

export function todayJalali(now: Date = new Date()): JDate {
  const result = toJalaali(now);
  return { jy: result.jy, jm: result.jm, jd: result.jd };
}

export function gregorianOf(date: JDate): GDate {
  const result = toGregorian(date.jy, date.jm, date.jd);
  return { gy: result.gy, gm: result.gm, gd: result.gd };
}

export function sameDate(a: JDate, b: JDate): boolean {
  return a.jy === b.jy && a.jm === b.jm && a.jd === b.jd;
}

export function isFriday(date: JDate): boolean {
  return weekdayOf(date) === 6;
}

export function clampYear(jy: number): number {
  return Math.min(MAX_YEAR, Math.max(MIN_YEAR, jy));
}

/* ------------------------------------------------------------------ */
/* Month grid                                                          */
/* ------------------------------------------------------------------ */

export interface GridDay {
  date: JDate;
  /** False for the leading/trailing padding cells of the month grid. */
  inMonth: boolean;
  /** 0 = Saturday … 6 = Friday */
  weekday: number;
}

export interface GridWeek {
  weekNumber: number;
  days: GridDay[];
}

/** Week number of `date` inside the Jalaali year, honouring `weekStart`. */
export function weekNumberOf(date: JDate, year: number, weekStart: number): number {
  const firstOfYear = j2d(year, 1, 1);
  const lead = ((weekdayOfJdn(firstOfYear) - weekStart + 7) % 7);
  const offset = jdnOf(date) - firstOfYear;
  return Math.max(1, Math.floor((offset + lead) / 7) + 1);
}

export function monthWeeks(jy: number, jm: number, weekStart: number): GridWeek[] {
  const firstOfMonth = j2d(jy, jm, 1);
  const lead = (weekdayOfJdn(firstOfMonth) - weekStart + 7) % 7;
  const gridStart = firstOfMonth - lead;
  const totalCells = lead + jalaaliMonthLength(jy, jm);
  const weekCount = Math.ceil(totalCells / 7);
  const weeks: GridWeek[] = [];

  for (let weekIndex = 0; weekIndex < weekCount; weekIndex += 1) {
    const days: GridDay[] = [];
    for (let columnIndex = 0; columnIndex < 7; columnIndex += 1) {
      const jdn = gridStart + weekIndex * 7 + columnIndex;
      const date = fromJdn(jdn);
      days.push({
        date,
        inMonth: date.jy === jy && date.jm === jm,
        weekday: (weekStart + columnIndex) % 7,
      });
    }
    weeks.push({
      weekNumber: weekNumberOf(days[0].date, jy, weekStart),
      days,
    });
  }

  return weeks;
}

/** Weekday indices ordered for the current `weekStart` setting. */
export function weekdayOrder(weekStart: number): number[] {
  return Array.from({ length: 7 }, (_, index) => (weekStart + index) % 7);
}

/* ------------------------------------------------------------------ */
/* Formatting                                                          */
/* ------------------------------------------------------------------ */

export function monthTitle(jy: number, jm: number, digits: Digits): string {
  return `${MONTH_NAMES[jm - 1]} ${localizeNumber(jy, digits)}`;
}

export function weekdayName(weekday: number): string {
  return WEEKDAY_NAMES[weekday];
}

export function formatJalaliSlash(date: JDate, digits: Digits): string {
  const raw = `${date.jy}/${pad2(date.jm)}/${pad2(date.jd)}`;
  return digits === "fa" ? toFaDigits(raw) : raw;
}

export function formatJalaliLong(date: JDate, digits: Digits): string {
  return `${localizeNumber(date.jd, digits)} ${MONTH_NAMES[date.jm - 1]} ${localizeNumber(date.jy, digits)}`;
}

/** ISO-like Gregorian label. Kept in Latin digits so it can be pasted into forms. */
export function formatGregorianIso(date: JDate): string {
  const { gy, gm, gd } = gregorianOf(date);
  return `${gy}-${pad2(gm)}-${pad2(gd)}`;
}

/**
 * Gregorian long label, e.g. «13 نوامبر 2026».
 *
 * The Gregorian date is *always* rendered with Latin digits (never Persian
 * ones): the digits setting only applies to the Jalali and lunar dates.
 */
export function formatGregorianLong(date: JDate): string {
  const { gy, gm, gd } = gregorianOf(date);
  return `${gd} ${GREGORIAN_MONTH_NAMES[gm - 1]} ${gy}`;
}

export function formatLunarLong(lunar: { year: number; month: number; day: number }, digits: Digits): string {
  const monthName = LUNAR_MONTH_NAMES[lunar.month - 1] ?? `${localizeNumber(lunar.month, digits)}`;
  return `${localizeNumber(lunar.day, digits)} ${monthName} ${localizeNumber(lunar.year, digits)}`;
}

/** Full Persian label, e.g. «پنج‌شنبه ۲۱ آبان ۱۴۰۵». */
export function formatWeekdayLongDate(date: JDate, digits: Digits): string {
  return `${weekdayName(weekdayOf(date))} ${formatJalaliLong(date, digits)}`;
}

export function formatCopyLabel(date: JDate, format: CopyFormat, digits: Digits): string {
  switch (format) {
    case "jalaliLong":
      return formatJalaliLong(date, digits);
    case "gregorianIso":
      return formatGregorianIso(date);
    case "jalaliSlash":
    default:
      return formatJalaliSlash(date, digits);
  }
}

/** Converts a Jalaali date to a JS `Date` at local midnight (widget/analytics only). */
export function toLocalDate(date: JDate): Date {
  const { gy, gm, gd } = gregorianOf(date);
  return new Date(gy, gm - 1, gd, 0, 0, 0, 0);
}

/**
 * Fully local `TodayInfo` used when `get_today_info` is unavailable.
 * No occasions/lunar data, but the calendar itself stays correct.
 */
export function fallbackTodayInfo(digits: Digits, now: Date = new Date()): TodayInfo {
  const date = todayJalali(now);
  const gregorian = gregorianOf(date);
  const weekday = weekdayOf(date);
  return {
    jalali: { year: date.jy, month: date.jm, day: date.jd, weekday },
    gregorian: { year: gregorian.gy, month: gregorian.gm, day: gregorian.gd },
    lunar: null,
    holiday: weekday === 6,
    events: [],
    labels: {
      slash: formatJalaliSlash(date, digits),
      long: formatJalaliLong(date, digits),
      iso: formatGregorianIso(date),
    },
  };
}
