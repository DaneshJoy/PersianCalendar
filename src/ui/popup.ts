/**
 * Popup flyout controller.
 *
 * Renders the month grid locally (jalaali-js), decorates it with the month's
 * occasions from Rust, and implements every interaction from the spec:
 * month navigation (buttons / wheel / keyboard), day selection, double-click
 * copy, day context menu, hover tooltip and the embedded settings panel.
 */
import * as cal from "../calendar";
import type { JDate } from "../calendar";
import type { Bridge } from "../bridge";
import {
  EventsStore,
  isHolidayDate,
  monthEventsMissing,
  visibleEventsForDate,
} from "../events";
import type { SettingsStore } from "../settings";
import type { MonthEvents, Settings, TodayInfo } from "../types";
import {
  clamp,
  clear,
  el,
  focusElement,
  iconButton,
  isActivatableTarget,
} from "./dom";
import { SettingsPanel } from "./settings-panel";

export interface PopupDeps {
  bridge: Bridge;
  settings: SettingsStore;
  events: EventsStore;
  today: TodayInfo;
}

/** Delay that makes a focus event count as "the popup was re-opened". */
const REOPEN_BLUR_MS = 700;
const WHEEL_COOLDOWN_MS = 180;
const TOAST_MS = 1100;

/** Neighbouring Jalali month (`delta` = ±1), clamped to the supported years. */
function neighbourMonth(jy: number, jm: number, delta: number): { jy: number; jm: number } | null {
  const total = jy * 12 + (jm - 1) + delta;
  const ny = Math.floor(total / 12);
  const nm = (((total % 12) + 12) % 12) + 1;
  if (ny < cal.MIN_YEAR || ny > cal.MAX_YEAR) return null;
  return { jy: ny, jm: nm };
}

export class PopupView {
  private readonly root: HTMLElement;
  private readonly deps: PopupDeps;

  private card!: HTMLElement;
  private titleButton!: HTMLButtonElement;
  private prevButton!: HTMLButtonElement;
  private nextButton!: HTMLButtonElement;
  private gearButton!: HTMLButtonElement;
  private weekdayRow!: HTMLElement;
  private grid!: HTMLElement;
  private footer!: HTMLElement;
  private footerText!: HTMLElement;
  private tooltip!: HTMLElement;
  private menu!: HTMLElement;
  private toast!: HTMLElement;
  private overlay!: HTMLElement;
  private panel!: SettingsPanel;

  private today: TodayInfo;
  private view: { jy: number; jm: number };
  private cursor: JDate | null = null;
  private monthData: MonthEvents | null = null;
  private monthToken = 0;
  private menuOpen = false;
  private wheelLockedUntil = 0;
  private blurAt = 0;
  private toastTimer: number | null = null;

  constructor(root: HTMLElement, deps: PopupDeps) {
    this.root = root;
    this.deps = deps;
    this.today = deps.today;
    this.view = { jy: deps.today.jalali.year, jm: deps.today.jalali.month };
  }

  mount(): void {
    this.build();
    this.bindEvents();
    this.bindBridgeEvents();
    this.render();
    this.blurAt = document.hasFocus() ? 0 : performance.now();
    void this.loadMonth({ initial: true });
  }

  /* ---------------------------------------------------------------- */
  /* DOM construction                                                  */
  /* ---------------------------------------------------------------- */

  private build(): void {
    this.card = el("div", "popup");
    this.card.setAttribute("role", "application");
    this.card.setAttribute("aria-label", "تقویم شمسی");

    const header = el("header", "popup__header");
    this.prevButton = iconButton("chevronRight", "ماه قبل");
    this.nextButton = iconButton("chevronLeft", "ماه بعد");
    this.gearButton = iconButton("gear", "تنظیمات");
    this.titleButton = el("button", "popup__title");
    this.titleButton.type = "button";
    this.titleButton.title = "بازگشت به ماه جاری";
    header.append(this.prevButton, this.titleButton, this.nextButton, this.gearButton);

    this.weekdayRow = el("div", "popup__weekdays");
    this.grid = el("div", "popup__grid");
    this.grid.setAttribute("role", "grid");

    this.footer = el("div", "popup__footer");
    this.footerText = el("span", "popup__footer-text");
    this.footer.append(this.footerText);

    this.overlay = el("div", "settings-overlay");
    this.panel = new SettingsPanel(this.overlay, {
      settings: this.deps.settings,
      onClearCache: () => this.handleClearCache(),
    });

    this.tooltip = el("div", "tooltip");
    this.tooltip.setAttribute("role", "tooltip");

    this.menu = el("div", "context-menu");
    this.menu.setAttribute("role", "menu");

    this.toast = el("div", "toast");
    this.toast.setAttribute("role", "status");
    this.toast.setAttribute("aria-live", "polite");

    this.card.append(
      header,
      this.weekdayRow,
      this.grid,
      this.footer,
      this.overlay,
      this.tooltip,
      this.menu,
      this.toast,
    );
    this.root.append(this.card);
    this.panel.mount();
  }

  private bindEvents(): void {
    this.prevButton.addEventListener("click", () => this.goToMonth(-1));
    this.nextButton.addEventListener("click", () => this.goToMonth(1));
    this.titleButton.addEventListener("click", () => this.resetToToday(true));
    this.gearButton.addEventListener("click", () => this.openSettings());

    this.grid.addEventListener("click", (event) => this.handleGridClick(event));
    this.grid.addEventListener("dblclick", (event) => this.handleGridDoubleClick(event));
    this.grid.addEventListener("contextmenu", (event) => this.handleGridContextMenu(event));
    this.grid.addEventListener("wheel", (event) => this.handleGridWheel(event), { passive: false });
    this.grid.addEventListener("mouseleave", () => this.hideTooltip());

    this.card.addEventListener("contextmenu", (event) => event.preventDefault());

    document.addEventListener("keydown", this.handleKeyDown);
    document.addEventListener("pointerdown", this.handleDocumentPointerDown, true);
    window.addEventListener("blur", this.handleWindowBlur);
    window.addEventListener("focus", this.handleWindowFocus);
    window.addEventListener("online", this.handleWindowOnline);

    this.deps.settings.subscribe(() => {
      this.render();
      if (!this.panel.isOpen) this.hideTooltip();
      void this.ensureLunarNeighbours();
    });
  }

  private bindBridgeEvents(): void {
    void (async (): Promise<void> => {
      try {
        await Promise.all([
          this.deps.bridge.listen("settings-changed", (settings) => {
            this.deps.settings.replace(settings);
          }),
          this.deps.bridge.listen("theme-changed", (themeInfo) => {
            this.deps.settings.setThemeInfo(themeInfo);
          }),
          this.deps.bridge.listen("today-changed", (info) => this.handleTodayChanged(info)),
          this.deps.bridge.listen("events-updated", (payload) => this.handleEventsUpdated(payload)),
          this.deps.bridge.listen("open-settings", () => this.openSettings()),
          this.deps.bridge.listen("show-month", (payload) => this.goToMonthAbs(payload.year, payload.month)),
          this.deps.bridge.listen("copied", () => this.showToast("کپی شد")),
        ]);
      } catch (error) {
        console.error("failed to subscribe to backend events", error);
      }
    })();
  }

  /* ---------------------------------------------------------------- */
  /* Rendering                                                         */
  /* ---------------------------------------------------------------- */

  private render(): void {
    const settings = this.deps.settings.settings;
    this.card.classList.toggle("has-week-numbers", settings.showWeekNumber);
    this.titleButton.textContent = cal.monthTitle(this.view.jy, this.view.jm, settings.digits);
    this.renderWeekdays(settings);
    this.renderGrid(settings);
    this.renderFooter(settings);
  }

  private renderWeekdays(settings: Readonly<Settings>): void {
    clear(this.weekdayRow);
    for (const weekday of cal.weekdayOrder(settings.weekStart)) {
      const cell = el("span", "weekday", cal.WEEKDAY_SHORT[weekday]);
      if (weekday === 6) cell.classList.add("weekday--holiday");
      this.weekdayRow.append(cell);
    }
    if (settings.showWeekNumber) this.weekdayRow.append(el("span", "weekday weekday--spacer"));
  }

  private renderGrid(settings: Readonly<Settings>): void {
    clear(this.grid);
    const weeks = cal.monthWeeks(this.view.jy, this.view.jm, settings.weekStart);
    const rows = Math.max(weeks.length, 5);
    this.grid.style.setProperty("--rows", String(rows));
    const today = this.todayDate();

    for (const week of weeks) {
      for (const day of week.days) {
        if (!day.inMonth) {
          this.grid.append(el("span", "day day--empty"));
          continue;
        }
        this.grid.append(this.buildDayCell(day.date, settings, today));
      }
      if (settings.showWeekNumber) {
        this.grid.append(el("span", "weeknum", cal.fmtNumber(week.weekNumber, settings.digits)));
      }
    }
  }

  private buildDayCell(date: JDate, settings: Readonly<Settings>, today: JDate): HTMLButtonElement {
    const cell = el("button", "day");
    cell.type = "button";
    cell.dataset.jdn = String(cal.jdnOf(date));

    const isToday = cal.sameDate(date, today);
    const holiday = isHolidayDate(this.deps.events.lookup, date, settings.lunarOffset);
    const selected = this.cursor !== null && cal.sameDate(date, this.cursor);
    if (isToday) cell.classList.add("is-today");
    if (holiday) cell.classList.add("is-holiday");
    if (selected) cell.classList.add("is-selected");

    cell.setAttribute("aria-label", cal.formatWeekdayLongDate(date, settings.digits));
    cell.setAttribute("aria-pressed", String(selected));
    if (isToday) cell.setAttribute("aria-current", "date");

    // Requirement #4: Gregorian above, Shamsi in the middle, lunar below.
    const gregorian = settings.showGregorian ? cal.gregorianOf(date) : null;
    const lunar = settings.showLunar ? this.deps.events.lunarFor(date, settings.lunarOffset) : null;
    if (gregorian) cell.classList.add("has-greg");
    if (lunar) cell.classList.add("has-lunar");

    if (gregorian) cell.append(el("span", "day__greg", String(gregorian.gd)));
    cell.append(el("span", "day__num", cal.fmtNumber(date.jd, settings.digits)));
    if (lunar) cell.append(el("span", "day__lunar", cal.fmtNumber(lunar.day, settings.digits)));

    cell.addEventListener("mouseenter", () => this.showTooltip(cell, date));
    cell.addEventListener("mouseleave", () => this.hideTooltip());
    cell.addEventListener("focus", () => this.showTooltip(cell, date));
    cell.addEventListener("blur", () => this.hideTooltip());
    return cell;
  }

  private renderFooter(settings: Readonly<Settings>): void {
    let message: string | null = null;
    let offline = false;

    if (settings.showEvents) {
      if (monthEventsMissing(this.monthData)) {
        message = "برای نمایش مناسبت‌ها به اینترنت متصل شوید";
        offline = true;
      } else if (this.cursor) {
        const events = visibleEventsForDate(this.deps.events.lookup, this.cursor, settings);
        if (events.length > 0) message = events.join(" · ");
      }
    }

    this.footer.classList.toggle("popup__footer--offline", offline);
    this.footerText.textContent = message ?? "";
    this.footer.setAttribute("aria-hidden", message === null ? "true" : "false");
  }

  /* ---------------------------------------------------------------- */
  /* Tooltip                                                           */
  /* ---------------------------------------------------------------- */

  private showTooltip(cell: HTMLElement, date: JDate): void {
    if (this.panel.isOpen || this.menuOpen) return;
    const settings = this.deps.settings.settings;
    clear(this.tooltip);

    this.tooltip.append(el("div", "tooltip__title", cal.formatWeekdayLongDate(date, settings.digits)));

    if (settings.showGregorian) {
      this.tooltip.append(
        el("div", "tooltip__line", `میلادی: ${cal.formatGregorianLong(date)}`),
      );
    }

    const lunar = settings.showLunar ? this.deps.events.lunarFor(date, settings.lunarOffset) : null;
    if (lunar) {
      this.tooltip.append(
        el("div", "tooltip__line", `قمری: ${cal.formatLunarLong(lunar, settings.digits)}`),
      );
    }

    if (settings.showEvents) {
      const events = visibleEventsForDate(this.deps.events.lookup, date, settings);
      for (const event of events) {
        this.tooltip.append(el("div", "tooltip__line tooltip__line--event", event));
      }
    }

    this.tooltip.classList.add("is-visible");
    this.positionTooltip(cell);
  }

  private positionTooltip(cell: HTMLElement): void {
    const cardRect = this.card.getBoundingClientRect();
    const cellRect = cell.getBoundingClientRect();
    const tipRect = this.tooltip.getBoundingClientRect();

    let left = cellRect.left - cardRect.left + cellRect.width / 2 - tipRect.width / 2;
    left = clamp(left, 6, Math.max(6, cardRect.width - tipRect.width - 6));

    let top = cellRect.top - cardRect.top - tipRect.height - 6;
    if (top < 4) top = cellRect.bottom - cardRect.top + 6;
    top = clamp(top, 4, Math.max(4, cardRect.height - tipRect.height - 4));

    this.tooltip.style.left = `${Math.round(left)}px`;
    this.tooltip.style.top = `${Math.round(top)}px`;
  }

  private hideTooltip(): void {
    this.tooltip.classList.remove("is-visible");
  }

  /* ---------------------------------------------------------------- */
  /* Grid interactions                                                 */
  /* ---------------------------------------------------------------- */

  private handleGridClick(event: MouseEvent): void {
    const cell = this.cellFromEvent(event);
    if (!cell) return;
    const date = this.dateFromCell(cell);
    if (!date) return;
    this.hideTooltip();
    this.selectDate(date);
  }

  private handleGridDoubleClick(event: MouseEvent): void {
    const cell = this.cellFromEvent(event);
    if (!cell) return;
    const date = this.dateFromCell(cell);
    if (!date) return;
    event.preventDefault();
    this.selectDate(date);
    void this.copyByFormat(date);
  }

  private handleGridContextMenu(event: MouseEvent): void {
    event.preventDefault();
    const cell = this.cellFromEvent(event);
    if (!cell) return;
    const date = this.dateFromCell(cell);
    if (!date) return;
    this.hideTooltip();
    this.selectDate(date);
    this.openDayMenu(date, event.clientX, event.clientY);
  }

  private handleGridWheel(event: WheelEvent): void {
    if (this.panel.isOpen) return;
    event.preventDefault();
    const now = performance.now();
    if (now < this.wheelLockedUntil) return;
    this.wheelLockedUntil = now + WHEEL_COOLDOWN_MS;
    const delta = event.deltaY !== 0 ? Math.sign(event.deltaY) : Math.sign(event.deltaX);
    if (delta === 0) return;
    this.goToMonth(delta);
  }

  private cellFromEvent(event: Event): HTMLElement | null {
    const target = event.target;
    if (!(target instanceof Element)) return null;
    return target.closest<HTMLElement>(".day[data-jdn]");
  }

  private dateFromCell(cell: HTMLElement): JDate | null {
    const raw = cell.dataset.jdn;
    if (!raw) return null;
    const jdn = Number(raw);
    return Number.isFinite(jdn) ? cal.fromJdn(jdn) : null;
  }

  private selectDate(date: JDate): void {
    this.cursor = date;
    this.updateSelection();
    this.renderFooter(this.deps.settings.settings);
  }

  private updateSelection(): void {
    const selectedJdn = this.cursor ? cal.jdnOf(this.cursor) : null;
    for (const cell of this.grid.querySelectorAll<HTMLElement>(".day[data-jdn]")) {
      const isSelected = selectedJdn !== null && cell.dataset.jdn === String(selectedJdn);
      cell.classList.toggle("is-selected", isSelected);
      cell.setAttribute("aria-pressed", String(isSelected));
    }
  }

  /* ---------------------------------------------------------------- */
  /* Day context menu                                                  */
  /* ---------------------------------------------------------------- */

  private openDayMenu(date: JDate, clientX: number, clientY: number): void {
    clear(this.menu);
    const items: Array<{ label: string; action: () => void }> = [
      { label: "کپی تاریخ شمسی", action: () => void this.copyJalali(date) },
      { label: "کپی تاریخ میلادی", action: () => void this.copyGregorian(date) },
      { label: "رفتن به امروز", action: () => this.resetToToday(true) },
    ];

    for (const item of items) {
      const button = el("button", "menu-item", item.label);
      button.type = "button";
      button.setAttribute("role", "menuitem");
      button.addEventListener("click", () => {
        this.closeMenu();
        item.action();
      });
      this.menu.append(button);
    }

    this.menu.classList.add("is-open");
    this.menuOpen = true;

    const cardRect = this.card.getBoundingClientRect();
    const width = this.menu.offsetWidth;
    const height = this.menu.offsetHeight;
    const x = clamp(clientX - cardRect.left - width / 2, 6, Math.max(6, cardRect.width - width - 6));
    let y = clientY - cardRect.top - height - 4;
    if (y < 6) y = clientY - cardRect.top + 10;
    y = clamp(y, 6, Math.max(6, cardRect.height - height - 6));

    this.menu.style.left = `${Math.round(x)}px`;
    this.menu.style.top = `${Math.round(y)}px`;
  }

  private closeMenu(): void {
    if (!this.menuOpen) return;
    this.menuOpen = false;
    this.menu.classList.remove("is-open");
  }

  /* ---------------------------------------------------------------- */
  /* Month / cursor navigation                                         */
  /* ---------------------------------------------------------------- */

  private goToMonth(offset: number): void {
    const total = this.view.jy * 12 + (this.view.jm - 1) + offset;
    const jy = Math.floor(total / 12);
    const jm = (((total % 12) + 12) % 12) + 1;
    if (jy < cal.MIN_YEAR || jy > cal.MAX_YEAR) return;

    this.view = { jy, jm };
    if (this.cursor) {
      this.cursor = { jy, jm, jd: Math.min(this.cursor.jd, cal.monthLength(jy, jm)) };
    }
    this.hideTooltip();
    this.closeMenu();
    this.render();
    void this.loadMonth();
  }

  private goToMonthAbs(year: number, month: number): void {
    if (year < cal.MIN_YEAR || year > cal.MAX_YEAR || month < 1 || month > 12) return;
    this.view = { jy: year, jm: month };
    const today = this.todayDate();
    this.cursor = today.jy === year && today.jm === month ? today : null;
    this.hideTooltip();
    this.closeMenu();
    this.render();
    void this.loadMonth();
  }

  private moveCursor(days: number): void {
    const base = this.cursor ?? this.defaultCursor();
    const target = cal.addDays(base, days);
    if (target.jy < cal.MIN_YEAR || target.jy > cal.MAX_YEAR) return;

    const monthChanged = target.jy !== this.view.jy || target.jm !== this.view.jm;
    this.cursor = target;
    if (monthChanged) this.view = { jy: target.jy, jm: target.jm };
    this.hideTooltip();
    this.closeMenu();
    this.render();
    if (monthChanged) void this.loadMonth();
  }

  /** Keyboard start point when nothing is selected yet. */
  private defaultCursor(): JDate {
    const today = this.todayDate();
    if (today.jy === this.view.jy && today.jm === this.view.jm) return today;
    return { jy: this.view.jy, jm: this.view.jm, jd: 1 };
  }

  private resetToToday(navigate: boolean): void {
    const today = this.todayDate();
    this.cursor = today;
    this.hideTooltip();
    this.closeMenu();
    if (!navigate) {
      this.updateSelection();
      this.renderFooter(this.deps.settings.settings);
      return;
    }
    this.view = { jy: today.jy, jm: today.jm };
    this.render();
    void this.loadMonth();
  }

  private todayDate(): JDate {
    return {
      jy: this.today.jalali.year,
      jm: this.today.jalali.month,
      jd: this.today.jalali.day,
    };
  }

  /* ---------------------------------------------------------------- */
  /* Events data                                                       */
  /* ---------------------------------------------------------------- */

  private async loadMonth(options: { initial?: boolean } = {}): Promise<void> {
    const token = ++this.monthToken;
    const { jy, jm } = this.view;
    this.monthData = this.deps.events.cached(jy, jm) ?? null;
    if (!options.initial) this.render();

    const data = await this.deps.events.load(jy, jm);
    if (token !== this.monthToken) return;
    if (data) this.monthData = data;
    this.render();
    await this.ensureLunarNeighbours();
  }

  /**
   * The lunar shift looks up the lunar date of `date + offset`, so days near a
   * month border read from the neighbouring month — both for the displayed
   * lunar date and for the shifted lunar occasions/holiday badge. While a shift
   * is active, prefetch those two months once and re-render when they arrive.
   */
  private async ensureLunarNeighbours(): Promise<void> {
    const { lunarOffset } = this.deps.settings.settings;
    if (cal.clampLunarOffset(lunarOffset) === 0) return;
    const token = this.monthToken;
    const pending = [
      neighbourMonth(this.view.jy, this.view.jm, -1),
      neighbourMonth(this.view.jy, this.view.jm, 1),
    ]
      .filter((month): month is { jy: number; jm: number } => month !== null)
      .filter((month) => this.deps.events.cached(month.jy, month.jm) === undefined);
    if (pending.length === 0) return;

    await Promise.all(pending.map((month) => this.deps.events.load(month.jy, month.jm)));
    if (token !== this.monthToken) return;
    this.render();
  }

  private async handleClearCache(): Promise<void> {
    await this.deps.events.clear();
    this.deps.events.invalidate(this.view.jy, this.view.jm);
    await this.loadMonth();
  }

  private handleEventsUpdated(payload: { year: number; month: number }): void {
    if (payload.year !== this.view.jy || payload.month !== this.view.jm) return;
    this.deps.events.invalidate(payload.year, payload.month);
    void this.loadMonth();
  }

  private handleTodayChanged(info: TodayInfo): void {
    const previousToday = this.todayDate();
    const cursorWasToday = this.cursor !== null && cal.sameDate(this.cursor, previousToday);
    this.today = info;
    if (cursorWasToday) {
      const nextToday = this.todayDate();
      this.cursor = nextToday;
      this.view = { jy: nextToday.jy, jm: nextToday.jm };
      void this.loadMonth();
    }
    this.render();
  }

  private readonly handleWindowOnline = (): void => {
    void this.refreshEventsOnReconnect();
  };

  private async refreshEventsOnReconnect(): Promise<void> {
    if (!this.deps.settings.settings.showEvents) return;
    const { jy, jm } = this.view;
    const data = await this.deps.events.refresh(jy, jm);
    if (!data) return;
    if (this.view.jy !== jy || this.view.jm !== jm) return;
    this.monthData = data;
    this.render();
  }

  private async refreshToday(): Promise<void> {
    try {
      const info = await this.deps.bridge.getTodayInfo();
      this.handleTodayChanged(info);
    } catch (error) {
      console.error("get_today_info failed", error);
    }
  }

  /* ---------------------------------------------------------------- */
  /* Copy + toast                                                      */
  /* ---------------------------------------------------------------- */

  private async copy(text: string): Promise<void> {
    try {
      await this.deps.bridge.copyText(text);
      this.showToast("کپی شد");
    } catch (error) {
      console.error("copy_text failed", error);
    }
  }

  private copyJalali(date: JDate): Promise<void> {
    const { digits } = this.deps.settings.settings;
    return this.copy(cal.formatJalaliSlash(date, digits));
  }

  private copyGregorian(date: JDate): Promise<void> {
    return this.copy(cal.formatGregorianIso(date));
  }

  private copyByFormat(date: JDate): Promise<void> {
    const { copyFormat, digits } = this.deps.settings.settings;
    return this.copy(cal.formatCopyLabel(date, copyFormat, digits));
  }

  private showToast(message: string): void {
    this.toast.textContent = message;
    this.toast.classList.add("is-visible");
    if (this.toastTimer !== null) window.clearTimeout(this.toastTimer);
    this.toastTimer = window.setTimeout(() => {
      this.toastTimer = null;
      this.toast.classList.remove("is-visible");
    }, TOAST_MS);
  }

  /* ---------------------------------------------------------------- */
  /* Settings panel                                                    */
  /* ---------------------------------------------------------------- */

  private openSettings(): void {
    this.closeMenu();
    this.hideTooltip();
    this.panel.open();
  }

  private closeSettings(): void {
    this.panel.close();
    focusElement(this.gearButton);
  }

  /* ---------------------------------------------------------------- */
  /* Window + document events                                          */
  /* ---------------------------------------------------------------- */

  private readonly handleKeyDown = (event: KeyboardEvent): void => {
    if (this.panel.isOpen) {
      if (event.key === "Escape") {
        event.preventDefault();
        this.closeSettings();
      }
      return;
    }

    if (this.menuOpen) {
      if (event.key === "Escape") {
        event.preventDefault();
        this.closeMenu();
      }
      return;
    }

    if (isActivatableTarget(event.target)) return;

    switch (event.key) {
      // RTL: left arrow moves forward in time (next day), right arrow backwards.
      case "ArrowLeft":
        event.preventDefault();
        this.moveCursor(1);
        break;
      case "ArrowRight":
        event.preventDefault();
        this.moveCursor(-1);
        break;
      case "ArrowUp":
        event.preventDefault();
        this.moveCursor(-7);
        break;
      case "ArrowDown":
        event.preventDefault();
        this.moveCursor(7);
        break;
      case "Escape":
        event.preventDefault();
        void this.deps.bridge.hidePopup();
        break;
      default:
        break;
    }
  };

  private readonly handleDocumentPointerDown = (event: PointerEvent): void => {
    if (!this.menuOpen) return;
    if (event.target instanceof Node && this.menu.contains(event.target)) return;
    this.closeMenu();
  };

  private readonly handleWindowBlur = (): void => {
    this.blurAt = performance.now();
    this.hideTooltip();
  };

  private readonly handleWindowFocus = (): void => {
    const elapsed = this.blurAt > 0 ? performance.now() - this.blurAt : 0;
    this.blurAt = 0;
    if (elapsed < REOPEN_BLUR_MS) return;
    // Re-opened from the tray: go back to today and refresh stale data.
    this.resetToToday(true);
    void this.refreshToday();
  };
}
