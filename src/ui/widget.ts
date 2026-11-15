/**
 * Desktop widget controller.
 *
 * Small mode: today's day number + month name.
 * Full mode: compact month grid with month navigation.
 * Dragging is enabled only while the widget position is unlocked, via the
 * `data-tauri-drag-region` attribute on the header/card.
 */
import * as cal from "../calendar";
import type { JDate } from "../calendar";
import type { Bridge } from "../bridge";
import { EventsStore, isHolidayDate, visibleEventsForDate } from "../events";
import type { SettingsStore } from "../settings";
import type { Settings, TodayInfo } from "../types";
import { clamp, clear, el, icon, iconButton } from "./dom";

export interface WidgetDeps {
  bridge: Bridge;
  settings: SettingsStore;
  events: EventsStore;
  today: TodayInfo;
}

const WHEEL_COOLDOWN_MS = 220;

/** Neighbouring Jalali month (`delta` = ±1), clamped to the supported years. */
function neighbourMonth(jy: number, jm: number, delta: number): { jy: number; jm: number } | null {
  const total = jy * 12 + (jm - 1) + delta;
  const ny = Math.floor(total / 12);
  const nm = (((total % 12) + 12) % 12) + 1;
  if (ny < cal.MIN_YEAR || ny > cal.MAX_YEAR) return null;
  return { jy: ny, jm: nm };
}

export class WidgetView {
  private readonly root: HTMLElement;
  private readonly deps: WidgetDeps;

  private widget!: HTMLElement;
  private card!: HTMLElement;
  private header: HTMLElement | null = null;
  private weekdayRow: HTMLElement | null = null;
  private grid: HTMLElement | null = null;
  private menu!: HTMLElement;

  private today: TodayInfo;
  private view: { jy: number; jm: number };
  private cursor: JDate | null = null;
  private monthToken = 0;
  private menuOpen = false;
  private wheelLockedUntil = 0;

  constructor(root: HTMLElement, deps: WidgetDeps) {
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
    void this.loadMonth();
  }

  /* ---------------------------------------------------------------- */
  /* DOM construction                                                  */
  /* ---------------------------------------------------------------- */

  private build(): void {
    this.widget = el("div", "widget");
    this.widget.dataset.mode = this.deps.settings.settings.widgetSize;
    this.card = el("div", "widget__card");
    this.widget.append(this.card);

    this.menu = el("div", "context-menu widget-menu");
    this.menu.setAttribute("role", "menu");
    this.widget.append(this.menu);

    this.root.append(this.widget);
  }

  private bindEvents(): void {
    this.widget.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      this.openMenu(event.clientX, event.clientY);
    });
    this.widget.addEventListener("wheel", (event) => this.handleWheel(event), { passive: false });

    document.addEventListener("pointerdown", this.handleDocumentPointerDown, true);
    document.addEventListener("keydown", this.handleKeyDown);
    window.addEventListener("blur", () => this.closeMenu());

    this.deps.settings.subscribe(() => {
      this.render();
      void this.ensureLunarNeighbours();
    });
  }

  private bindBridgeEvents(): void {
    void (async (): Promise<void> => {
      try {
        await Promise.all([
          this.deps.bridge.listen("settings-changed", (settings) => this.deps.settings.replace(settings)),
          this.deps.bridge.listen("theme-changed", (themeInfo) => this.deps.settings.setThemeInfo(themeInfo)),
          this.deps.bridge.listen("today-changed", (info) => this.handleTodayChanged(info)),
          this.deps.bridge.listen("events-updated", (payload) => this.handleEventsUpdated(payload)),
        ]);
      } catch (error) {
        console.error("widget: failed to subscribe to backend events", error);
      }
    })();

    window.addEventListener("online", () => {
      void this.refreshOnReconnect();
    });
  }

  /* ---------------------------------------------------------------- */
  /* Rendering                                                         */
  /* ---------------------------------------------------------------- */

  private render(): void {
    const settings = this.deps.settings.settings;
    this.widget.dataset.mode = settings.widgetSize;
    this.widget.style.setProperty("--widget-opacity", String(settings.widgetOpacity / 100));
    this.widget.classList.toggle("has-week-numbers", settings.showWeekNumber);

    if (settings.widgetSize === "small") {
      this.renderSmall(settings);
    } else {
      this.renderFull(settings);
    }
    this.updateDragRegion();
    this.updateSelection();
  }

  private renderSmall(settings: Readonly<Settings>): void {
    clear(this.card);
    this.header = null;
    this.weekdayRow = null;
    this.grid = null;

    const today = this.todayDate();
    this.card.append(
      el("div", "widget__day", cal.fmtNumber(today.jd, settings.digits)),
      el("div", "widget__month", cal.monthTitle(today.jy, today.jm, settings.digits)),
    );
  }

  private renderFull(settings: Readonly<Settings>): void {
    clear(this.card);
    this.card.append(el("header", "widget__header"));
    const header = this.card.lastElementChild as HTMLElement;
    this.header = header;

    const prevButton = iconButton("chevronRight", "ماه قبل");
    const nextButton = iconButton("chevronLeft", "ماه بعد");
    prevButton.addEventListener("click", () => this.goToMonth(-1));
    nextButton.addEventListener("click", () => this.goToMonth(1));

    const title = el("span", "widget__title", cal.monthTitle(this.view.jy, this.view.jm, settings.digits));
    header.append(prevButton, title, nextButton);

    this.weekdayRow = el("div", "widget__weekdays");
    this.grid = el("div", "widget__grid");
    this.grid.addEventListener("click", (event) => this.handleGridClick(event));
    this.card.append(this.weekdayRow, this.grid);

    this.renderWeekdays(settings);
    this.renderGrid(settings);
  }

  private renderWeekdays(settings: Readonly<Settings>): void {
    if (!this.weekdayRow) return;
    clear(this.weekdayRow);
    for (const weekday of cal.weekdayOrder(settings.weekStart)) {
      const cell = el("span", "weekday", cal.WEEKDAY_SHORT[weekday]);
      if (weekday === 6) cell.classList.add("weekday--holiday");
      this.weekdayRow.append(cell);
    }
    if (settings.showWeekNumber) this.weekdayRow.append(el("span", "weekday weekday--spacer"));
  }

  private renderGrid(settings: Readonly<Settings>): void {
    if (!this.grid) return;
    clear(this.grid);
    const weeks = cal.monthWeeks(this.view.jy, this.view.jm, settings.weekStart);
    this.grid.style.setProperty("--rows", String(Math.max(weeks.length, 5)));
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

  private buildDayCell(date: JDate, settings: Readonly<Settings>, today: JDate): HTMLElement {
    const cell = el("div", "day");
    cell.dataset.jdn = String(cal.jdnOf(date));
    if (cal.sameDate(date, today)) cell.classList.add("is-today");
    if (isHolidayDate(this.deps.events.lookup, date, settings.lunarOffset)) {
      cell.classList.add("is-holiday");
    }

    // Requirement #4: Gregorian above, Shamsi in the middle, lunar below.
    const gregorian = settings.showGregorian ? cal.gregorianOf(date) : null;
    const lunar = settings.showLunar ? this.deps.events.lunarFor(date, settings.lunarOffset) : null;
    if (gregorian) cell.classList.add("has-greg");
    if (lunar) cell.classList.add("has-lunar");

    if (gregorian) cell.append(el("span", "day__greg", String(gregorian.gd)));
    cell.append(el("span", "day__num", cal.fmtNumber(date.jd, settings.digits)));
    if (lunar) cell.append(el("span", "day__lunar", cal.fmtNumber(lunar.day, settings.digits)));

    const events = visibleEventsForDate(this.deps.events.lookup, date, settings);
    if (events.length > 0) cell.title = events.join(" · ");
    return cell;
  }

  private updateDragRegion(): void {
    const unlocked = !this.deps.settings.settings.widgetLocked;
    this.card.toggleAttribute("data-tauri-drag-region", unlocked);
    this.header?.toggleAttribute("data-tauri-drag-region", unlocked);
  }

  private updateSelection(): void {
    if (!this.grid) return;
    const selectedJdn = this.cursor ? cal.jdnOf(this.cursor) : null;
    for (const cell of this.grid.querySelectorAll<HTMLElement>(".day[data-jdn]")) {
      const isSelected = selectedJdn !== null && cell.dataset.jdn === String(selectedJdn);
      cell.classList.toggle("is-selected", isSelected);
    }
  }

  /* ---------------------------------------------------------------- */
  /* Interactions                                                      */
  /* ---------------------------------------------------------------- */

  private handleWheel(event: WheelEvent): void {
    if (this.deps.settings.settings.widgetSize !== "full") return;
    if (this.menuOpen) return;
    event.preventDefault();
    const now = performance.now();
    if (now < this.wheelLockedUntil) return;
    this.wheelLockedUntil = now + WHEEL_COOLDOWN_MS;
    const delta = event.deltaY !== 0 ? Math.sign(event.deltaY) : Math.sign(event.deltaX);
    if (delta !== 0) this.goToMonth(delta);
  }

  private handleGridClick(event: MouseEvent): void {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const cell = target.closest<HTMLElement>(".day[data-jdn]");
    if (!cell) return;
    const raw = cell.dataset.jdn;
    if (!raw) return;
    const jdn = Number(raw);
    if (!Number.isFinite(jdn)) return;
    this.cursor = cal.fromJdn(jdn);
    this.updateSelection();
  }

  private goToMonth(offset: number): void {
    const total = this.view.jy * 12 + (this.view.jm - 1) + offset;
    const jy = Math.floor(total / 12);
    const jm = (((total % 12) + 12) % 12) + 1;
    if (jy < cal.MIN_YEAR || jy > cal.MAX_YEAR) return;

    this.view = { jy, jm };
    const settings = this.deps.settings.settings;
    if (this.header) {
      const title = this.header.querySelector(".widget__title");
      if (title) title.textContent = cal.monthTitle(jy, jm, settings.digits);
    }
    this.renderGrid(settings);
    this.updateSelection();
    this.closeMenu();
    void this.loadMonth();
  }

  private resetToToday(): void {
    const today = this.todayDate();
    this.view = { jy: today.jy, jm: today.jm };
    this.cursor = today;
    this.render();
    void this.loadMonth();
  }

  /* ---------------------------------------------------------------- */
  /* Quick menu (right click)                                          */
  /* ---------------------------------------------------------------- */

  private openMenu(clientX: number, clientY: number): void {
    clear(this.menu);
    const settings = this.deps.settings.settings;
    const isFull = settings.widgetSize === "full";

    const addItem = (label: string, action: () => void, active = false): void => {
      const button = el("button", "menu-item", label);
      button.type = "button";
      button.setAttribute("role", "menuitem");
      if (active) {
        button.classList.add("is-active");
        button.prepend(icon("check"));
      }
      button.addEventListener("click", () => {
        this.closeMenu();
        action();
      });
      this.menu.append(button);
    };

    addItem(isFull ? "نمایش کوچک" : "نمایش کامل", () => {
      this.deps.settings.set({ widgetSize: isFull ? "small" : "full" });
    });
    addItem("قفل موقعیت", () => {
      this.deps.settings.set({ widgetLocked: !settings.widgetLocked });
    }, settings.widgetLocked);
    addItem("تنظیمات", () => {
      void this.openPopupSettings();
    });
    addItem("پنهان", () => {
      this.deps.settings.set({ widgetEnabled: false });
    });

    this.menu.classList.add("is-open");
    this.menuOpen = true;

    const rect = this.widget.getBoundingClientRect();
    const width = this.menu.offsetWidth;
    const height = this.menu.offsetHeight;
    const x = clamp(clientX - rect.left - width / 2, 3, Math.max(3, rect.width - width - 3));
    let y = clientY - rect.top - height - 3;
    if (y < 3) y = clientY - rect.top + 6;
    y = clamp(y, 3, Math.max(3, rect.height - height - 3));

    this.menu.style.left = `${Math.round(x)}px`;
    this.menu.style.top = `${Math.round(y)}px`;
  }

  private closeMenu(): void {
    if (!this.menuOpen) return;
    this.menuOpen = false;
    this.menu.classList.remove("is-open");
  }

  private async openPopupSettings(): Promise<void> {
    try {
      await this.deps.bridge.showPopup();
      await this.deps.bridge.requestOpenSettings();
    } catch (error) {
      console.error("widget: open settings failed", error);
    }
  }

  /* ---------------------------------------------------------------- */
  /* Data + window events                                              */
  /* ---------------------------------------------------------------- */

  private async loadMonth(): Promise<void> {
    const token = ++this.monthToken;
    const { jy, jm } = this.view;
    await this.deps.events.load(jy, jm);
    if (token !== this.monthToken) return;
    if (this.deps.settings.settings.widgetSize === "full") this.renderGrid(this.deps.settings.settings);
    this.updateSelection();
    await this.ensureLunarNeighbours();
  }

  /**
   * The lunar shift looks up the lunar date of `date + offset`, so days near a
   * month border read from the neighbouring month — both for the displayed
   * lunar date and for the shifted lunar occasions/holiday badge. While a shift
   * is active, prefetch those two months once and re-render when they arrive.
   */
  private async ensureLunarNeighbours(): Promise<void> {
    const settings = this.deps.settings.settings;
    if (settings.widgetSize !== "full") return;
    if (cal.clampLunarOffset(settings.lunarOffset) === 0) return;
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
    if (this.deps.settings.settings.widgetSize !== "full") return;
    this.renderGrid(this.deps.settings.settings);
    this.updateSelection();
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
    if (cursorWasToday) this.cursor = this.todayDate();
    this.render();
    if (this.deps.settings.settings.widgetSize === "full") void this.loadMonth();
  }

  private async refreshOnReconnect(): Promise<void> {
    if (!this.deps.settings.settings.showEvents) return;
    const data = await this.deps.events.refresh(this.view.jy, this.view.jm);
    if (!data) return;
    if (this.deps.settings.settings.widgetSize === "full") this.renderGrid(this.deps.settings.settings);
    this.updateSelection();
  }

  private todayDate(): JDate {
    return {
      jy: this.today.jalali.year,
      jm: this.today.jalali.month,
      jd: this.today.jalali.day,
    };
  }

  private readonly handleDocumentPointerDown = (event: PointerEvent): void => {
    if (!this.menuOpen) return;
    if (event.target instanceof Node && this.menu.contains(event.target)) return;
    this.closeMenu();
  };

  private readonly handleKeyDown = (event: KeyboardEvent): void => {
    if (event.key === "Escape") {
      if (this.menuOpen) {
        event.preventDefault();
        this.closeMenu();
      }
      return;
    }
    if (this.menuOpen || this.deps.settings.settings.widgetSize !== "full") return;

    switch (event.key) {
      case "ArrowLeft":
        event.preventDefault();
        this.goToMonth(1);
        break;
      case "ArrowRight":
        event.preventDefault();
        this.goToMonth(-1);
        break;
      case "t":
      case "T":
        this.resetToToday();
        break;
      default:
        break;
    }
  };
}
