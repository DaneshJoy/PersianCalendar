/**
 * In-popup settings panel.
 *
 * Grouped Appearance / Display / Behavior controls. Every change updates the
 * store optimistically; the store debounces `save_settings` (auto-save) and we
 * show a tiny "ذخیره شد" indicator in the panel header.
 */
import { fmtNumber } from "../calendar";
import { normalizeHex, type SettingsStore } from "../settings";
import type {
  CopyFormat,
  Digits,
  EventsMode,
  FontChoice,
  Settings,
  ThemeMode,
  TrayIconMode,
  WeekStart,
  WidgetSize,
} from "../types";
import { el, iconButton } from "./dom";

export interface SettingsPanelDeps {
  settings: SettingsStore;
  /** Clears the events cache and reloads the displayed month. */
  onClearCache: () => Promise<void>;
}

interface PresetAccent {
  value: string;
  label: string;
}

const ACCENT_PRESETS: PresetAccent[] = [
  { value: "#2f6fed", label: "آبی" },
  { value: "#12a594", label: "سبزآبی" },
  { value: "#0f9d58", label: "سبز" },
  { value: "#7c5cff", label: "بنفش" },
  { value: "#e8890c", label: "نارنجی" },
  { value: "#d9485f", label: "قرمز" },
];

/** −3 … +3 day shift for the lunar date, labelled with Persian digits. */
const LUNAR_OFFSET_OPTIONS: ReadonlyArray<{ value: number; label: string }> = [
  { value: -3, label: "-۳" },
  { value: -2, label: "-۲" },
  { value: -1, label: "-۱" },
  { value: 0, label: "۰" },
  { value: 1, label: "+۱" },
  { value: 2, label: "+۲" },
  { value: 3, label: "+۳" },
];

export class SettingsPanel {
  private readonly syncers: Array<() => void> = [];
  private readonly unsubscribe: Array<() => void> = [];
  private statusEl!: HTMLElement;
  private closeButton!: HTMLButtonElement;
  private openFlag = false;
  private statusTimer: number | null = null;

  constructor(
    private readonly overlay: HTMLElement,
    private readonly deps: SettingsPanelDeps,
  ) {}

  get isOpen(): boolean {
    return this.openFlag;
  }

  mount(): void {
    this.overlay.setAttribute("role", "dialog");
    this.overlay.setAttribute("aria-modal", "true");
    this.overlay.setAttribute("aria-label", "تنظیمات");
    this.overlay.setAttribute("aria-hidden", "true");

    const panel = el("div", "settings");
    panel.append(this.buildHeader());

    const body = el("div", "settings__body");
    this.buildAppearance(body);
    this.buildDisplay(body);
    this.buildBehavior(body);
    this.buildActions(body);
    panel.append(body);

    this.overlay.append(panel);

    this.unsubscribe.push(this.deps.settings.subscribe(() => this.sync()));
    this.unsubscribe.push(
      this.deps.settings.onStatus((status) => {
        if (status.state === "saving") this.showStatus("در حال ذخیره…", false);
        else if (status.state === "saved") this.showStatus("ذخیره شد", false, 1100);
        else if (status.state === "error") this.showStatus("خطا در ذخیره", true);
      }),
    );

    this.sync();
  }

  open(): void {
    this.openFlag = true;
    this.sync();
    this.overlay.classList.add("is-open");
    this.overlay.setAttribute("aria-hidden", "false");
    this.showStatus("", false);
    this.closeButton.focus({ preventScroll: true });
  }

  close(): void {
    this.openFlag = false;
    this.overlay.classList.remove("is-open");
    this.overlay.setAttribute("aria-hidden", "true");
    if (this.statusTimer !== null) {
      window.clearTimeout(this.statusTimer);
      this.statusTimer = null;
    }
  }

  sync(): void {
    for (const syncer of this.syncers) syncer();
  }

  dispose(): void {
    for (const unsubscribe of this.unsubscribe) unsubscribe();
    this.unsubscribe.length = 0;
  }

  /* ---------------------------------------------------------------- */
  /* Building blocks                                                   */
  /* ---------------------------------------------------------------- */

  private get settings(): Readonly<Settings> {
    return this.deps.settings.settings;
  }

  private update(patch: Partial<Settings>): void {
    this.deps.settings.set(patch);
  }

  private buildHeader(): HTMLElement {
    const header = el("header", "settings__header");
    header.append(el("h2", "settings__title", "تنظیمات"));
    this.statusEl = el("span", "settings__status");
    this.statusEl.setAttribute("aria-live", "polite");
    this.closeButton = iconButton("close", "بستن تنظیمات");
    this.closeButton.addEventListener("click", () => this.close());
    header.append(this.statusEl, this.closeButton);
    return header;
  }

  private buildAppearance(body: HTMLElement): void {
    const group = this.section(body, "ظاهر");

    group.append(
      this.row(
        "تم",
        this.segmented<ThemeMode>(
          [
            { value: "auto", label: "خودکار" },
            { value: "light", label: "روشن" },
            { value: "dark", label: "تیره" },
          ],
          () => this.settings.theme,
          (theme) => this.update({ theme }),
        ),
      ),
    );

    group.append(this.row("رنگ تأکید", this.accentControl()));
    group.append(this.row("رنگ سفارشی", this.customAccentControl()));

    group.append(
      this.row(
        "نوع ارقام",
        this.segmented<Digits>(
          [
            { value: "fa", label: "فارسی" },
            { value: "en", label: "لاتین" },
          ],
          () => this.settings.digits,
          (digits) => this.update({ digits }),
        ),
      ),
    );

    group.append(
      this.row(
        "فونت",
        this.segmented<FontChoice>(
          [
            { value: "vazirmatn", label: "وزیرمتن" },
            { value: "system", label: "سیستم" },
          ],
          () => this.settings.font,
          (font) => this.update({ font }),
        ),
      ),
    );
  }

  private buildDisplay(body: HTMLElement): void {
    const group = this.section(body, "نمایش");

    group.append(
      this.row(
        "آیکون Tray",
        this.segmented<TrayIconMode>(
          [
            { value: "day", label: "عدد" },
            { value: "static", label: "ثابت" },
          ],
          () => this.settings.trayIconMode,
          (trayIconMode) => this.update({ trayIconMode }),
        ),
      ),
    );

    group.append(
      this.row(
        "شروع هفته",
        this.segmented<WeekStart>(
          [
            { value: 0, label: "شنبه" },
            { value: 1, label: "یکشنبه" },
            { value: 2, label: "دوشنبه" },
          ],
          () => this.settings.weekStart,
          (weekStart) => this.update({ weekStart }),
        ),
      ),
    );

    group.append(
      this.row(
        "تاریخ میلادی",
        this.toggle(
          () => this.settings.showGregorian,
          (showGregorian) => this.update({ showGregorian }),
        ),
      ),
    );

    group.append(
      this.row(
        "تاریخ قمری",
        this.toggle(
          () => this.settings.showLunar,
          (showLunar) => this.update({ showLunar }),
        ),
      ),
    );

    const lunarOffsetControl = this.segmented<number>(
      LUNAR_OFFSET_OPTIONS,
      () => this.settings.lunarOffset,
      (lunarOffset) => this.update({ lunarOffset }),
      { className: "segmented--compact", dir: "ltr" },
    );
    lunarOffsetControl.title =
      "جابه‌جایی لایه‌ی قمری (تاریخ قمری، مناسبت‌های قمری و تعطیلات قمری) به اندازه‌ی چند روز (‎−۳ تا ‎+۳)";
    group.append(this.row("شیفت تاریخ قمری", lunarOffsetControl));

    group.append(
      this.row(
        "تعطیلات و مناسبت‌ها",
        this.toggle(
          () => this.settings.showEvents,
          (showEvents) => this.update({ showEvents }),
        ),
      ),
    );

    group.append(
      this.row(
        "حالت مناسبت‌ها",
        this.segmented<EventsMode>(
          [
            { value: "important", label: "مهم‌ها" },
            { value: "all", label: "همه" },
          ],
          () => this.settings.eventsMode,
          (eventsMode) => this.update({ eventsMode }),
        ),
      ),
    );

    group.append(
      this.row(
        "شماره‌ی هفته",
        this.toggle(
          () => this.settings.showWeekNumber,
          (showWeekNumber) => this.update({ showWeekNumber }),
        ),
      ),
    );

    group.append(
      this.row(
        "فرمت کپی تاریخ",
        this.select<CopyFormat>(
          [
            { value: "jalaliSlash", label: "۱۴۰۵/۰۸/۲۱" },
            { value: "jalaliLong", label: "۲۱ آبان ۱۴۰۵" },
            { value: "gregorianIso", label: "2026-11-12" },
          ],
          () => this.settings.copyFormat,
          (copyFormat) => this.update({ copyFormat }),
        ),
      ),
    );
  }

  private buildBehavior(body: HTMLElement): void {
    const group = this.section(body, "رفتار");

    group.append(
      this.row(
        "اجرا همراه ویندوز",
        this.toggle(
          () => this.settings.autostart,
          (autostart) => this.update({ autostart }),
        ),
      ),
    );

    group.append(
      this.row(
        "بستن با کلیک بیرون",
        this.toggle(
          () => this.settings.closeOnBlur,
          (closeOnBlur) => this.update({ closeOnBlur }),
        ),
      ),
    );

    group.append(
      this.row(
        "ویجت دسکتاپ",
        this.toggle(
          () => this.settings.widgetEnabled,
          (widgetEnabled) => this.update({ widgetEnabled }),
        ),
      ),
    );

    group.append(
      this.row(
        "اندازه‌ی ویجت",
        this.segmented<WidgetSize>(
          [
            { value: "small", label: "کوچک" },
            { value: "full", label: "کامل" },
          ],
          () => this.settings.widgetSize,
          (widgetSize) => this.update({ widgetSize }),
        ),
      ),
    );

    group.append(this.row("شفافیت ویجت", this.opacityControl()));

    group.append(
      this.row(
        "قفل موقعیت ویجت",
        this.toggle(
          () => this.settings.widgetLocked,
          (widgetLocked) => this.update({ widgetLocked }),
        ),
      ),
    );

    const cacheButton = el("button", "action-button", "پاک کردن کش مناسبت‌ها");
    cacheButton.type = "button";
    cacheButton.addEventListener("click", () => {
      void this.handleClearCache(cacheButton);
    });
    const actions = el("div", "settings__actions");
    actions.append(cacheButton);
    group.append(actions);
  }

  private buildActions(body: HTMLElement): void {
    const note = el(
      "p",
      "settings__note",
      "تغییرات به‌صورت خودکار ذخیره می‌شوند و بلافاصله اعمال می‌گردند.",
    );
    body.append(note);

    const resetButton = el("button", "action-button", "بازنشانی به پیش‌فرض");
    resetButton.type = "button";
    resetButton.addEventListener("click", () => {
      void this.handleReset(resetButton);
    });
    const actions = el("div", "settings__actions");
    actions.append(resetButton);
    body.append(actions);
  }

  private section(body: HTMLElement, title: string): HTMLElement {
    const group = el("section", "settings__group");
    group.append(el("h3", "settings__group-title", title));
    body.append(group);
    return group;
  }

  private row(label: string, control: HTMLElement): HTMLElement {
    const row = el("div", "settings__row");
    row.append(el("span", "settings__label", label));
    const wrapper = el("div", "settings__control");
    wrapper.append(control);
    row.append(wrapper);
    return row;
  }

  /* ---------------------------------------------------------------- */
  /* Controls                                                          */
  /* ---------------------------------------------------------------- */

  private segmented<T extends string | number>(
    options: ReadonlyArray<{ value: T; label: string }>,
    get: () => T,
    set: (value: T) => void,
    variant: { className?: string; dir?: "rtl" | "ltr" } = {},
  ): HTMLElement {
    const wrap = el("div", "segmented");
    if (variant.className) wrap.classList.add(variant.className);
    wrap.setAttribute("role", "group");
    const buttons: Array<{ value: T; button: HTMLButtonElement }> = [];

    for (const option of options) {
      const button = el("button", "segmented__option", option.label);
      button.type = "button";
      // Number options (e.g. «-۳») must not be reordered by the RTL context.
      if (variant.dir) button.dir = variant.dir;
      button.setAttribute("aria-pressed", "false");
      button.addEventListener("click", () => set(option.value));
      wrap.append(button);
      buttons.push({ value: option.value, button });
    }

    this.syncers.push(() => {
      const active = get();
      for (const entry of buttons) {
        const isActive = entry.value === active;
        entry.button.classList.toggle("is-active", isActive);
        entry.button.setAttribute("aria-pressed", String(isActive));
      }
    });

    return wrap;
  }

  private toggle(get: () => boolean, set: (value: boolean) => void): HTMLButtonElement {
    const button = el("button", "switch");
    button.type = "button";
    button.setAttribute("role", "switch");
    button.setAttribute("aria-checked", "false");
    button.addEventListener("click", () => set(!get()));

    this.syncers.push(() => {
      const on = get();
      button.classList.toggle("is-on", on);
      button.setAttribute("aria-checked", String(on));
    });

    return button;
  }

  private select<T extends string>(
    options: ReadonlyArray<{ value: T; label: string }>,
    get: () => T,
    set: (value: T) => void,
  ): HTMLSelectElement {
    const select = el("select", "select");
    // The copy-format labels are dates; keep them RTL like the rest of the UI.
    select.dir = "rtl";
    for (const option of options) {
      const node = el("option", undefined, option.label);
      node.value = option.value;
      node.dir = "rtl";
      select.append(node);
    }
    select.addEventListener("change", () => set(select.value as T));

    this.syncers.push(() => {
      select.value = get();
    });

    return select;
  }

  /** Six ready-made accent colours. */
  private accentControl(): HTMLElement {
    const row = el("div", "accent-row");

    const swatches: Array<{ value: string; button: HTMLButtonElement }> = [];
    for (const preset of ACCENT_PRESETS) {
      const button = el("button", "accent-swatch");
      button.type = "button";
      button.style.backgroundColor = preset.value;
      button.title = preset.label;
      button.setAttribute("aria-label", `رنگ تأکید ${preset.label}`);
      button.setAttribute("aria-pressed", "false");
      button.addEventListener("click", () => this.update({ accent: preset.value }));
      row.append(button);
      swatches.push({ value: preset.value, button });
    }

    this.syncers.push(() => {
      const accent = normalizeHex(this.settings.accent);
      for (const entry of swatches) {
        const isActive = accent === entry.value;
        entry.button.classList.toggle("is-active", isActive);
        entry.button.setAttribute("aria-pressed", String(isActive));
      }
    });

    return row;
  }

  /**
   * Free-form accent colour as a hex field. A text field (instead of the
   * native OS colour picker) keeps everything inside the flyout so the
   * close-on-blur behavior can never hide the popup mid-selection.
   */
  private customAccentControl(): HTMLElement {
    const wrap = el("div", "custom-accent");
    const input = el("input", "custom-accent-input");
    input.type = "text";
    input.maxLength = 7;
    input.spellcheck = false;
    input.dir = "ltr";
    input.setAttribute("aria-label", "کد رنگ سفارشی");
    input.placeholder = "#2f6fed";
    const preview = el("span", "custom-accent-preview");

    const commit = (): void => {
      const raw = input.value.trim();
      const normalized = normalizeHex(raw.startsWith("#") ? raw : `#${raw}`);
      if (normalized) {
        input.classList.remove("is-invalid");
        if (normalized !== this.settings.accent) this.update({ accent: normalized });
      } else {
        input.classList.add("is-invalid");
      }
    };

    input.addEventListener("input", commit);
    input.addEventListener("blur", () => {
      input.classList.remove("is-invalid");
      input.value = normalizeHex(this.settings.accent) ?? this.settings.accent;
    });

    wrap.append(input, preview);

    this.syncers.push(() => {
      const accent = normalizeHex(this.settings.accent);
      if (accent) {
        if (document.activeElement !== input) input.value = accent;
        preview.style.backgroundColor = accent;
      }
    });

    return wrap;
  }

  private opacityControl(): HTMLElement {
    const holder = el("div", "settings__range");
    const input = el("input", "range");
    input.type = "range";
    input.min = "30";
    input.max = "100";
    input.step = "1";
    input.setAttribute("aria-label", "شفافیت ویجت");
    const value = el("span", "settings__value");

    input.addEventListener("input", () => {
      this.update({ widgetOpacity: Number(input.value) });
    });

    holder.append(input, value);

    this.syncers.push(() => {
      const opacity = this.settings.widgetOpacity;
      input.value = String(opacity);
      value.textContent = `${fmtNumber(opacity, this.settings.digits)}٪`;
    });

    return holder;
  }

  /* ---------------------------------------------------------------- */
  /* Actions + status                                                  */
  /* ---------------------------------------------------------------- */

  private async handleClearCache(button: HTMLButtonElement): Promise<void> {
    button.disabled = true;
    try {
      await this.deps.onClearCache();
      this.showStatus("کش مناسبت‌ها پاک شد", false, 1600);
    } catch (error) {
      console.error("clear cache failed", error);
      this.showStatus("پاک کردن کش ناموفق بود", true);
    } finally {
      button.disabled = false;
    }
  }

  private async handleReset(button: HTMLButtonElement): Promise<void> {
    button.disabled = true;
    try {
      await this.deps.settings.reset();
      this.showStatus("تنظیمات به پیش‌فرض بازگشت", false, 1600);
    } catch (error) {
      console.error("reset settings failed", error);
      this.showStatus("بازنشانی ناموفق بود", true);
    } finally {
      button.disabled = false;
    }
  }

  private showStatus(message: string, isError: boolean, hideAfterMs = 0): void {
    if (this.statusTimer !== null) {
      window.clearTimeout(this.statusTimer);
      this.statusTimer = null;
    }
    this.statusEl.textContent = message;
    this.statusEl.classList.toggle("is-error", isError);
    this.statusEl.classList.toggle("is-visible", message.length > 0);

    if (hideAfterMs > 0) {
      this.statusTimer = window.setTimeout(() => {
        this.statusTimer = null;
        this.statusEl.classList.remove("is-visible");
      }, hideAfterMs);
    }
  }
}
