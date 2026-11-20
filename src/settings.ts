/**
 * Settings state: loads from Rust, applies appearance to the document and
 * auto-saves the whole object back with a short debounce (contract: ~250 ms).
 */
import type { Bridge } from "./bridge";
import { DEFAULT_SETTINGS, type Settings, type ThemeInfo, type ThemeValue } from "./types";

export type SaveState = "idle" | "saving" | "saved" | "error";

export interface SaveStatus {
  state: SaveState;
  at: number;
}

const SAVE_DEBOUNCE_MS = 250;

/* ------------------------------------------------------------------ */
/* Colour helpers                                                      */
/* ------------------------------------------------------------------ */

export function normalizeHex(value: string): string | null {
  const trimmed = value.trim().toLowerCase();
  const short = /^#([0-9a-f]{3})$/.exec(trimmed);
  if (short) {
    const [r, g, b] = short[1];
    return `#${r}${r}${g}${g}${b}${b}`;
  }
  return /^#[0-9a-f]{6}$/.test(trimmed) ? trimmed : null;
}

function hexToRgb(hex: string): { r: number; g: number; b: number } {
  return {
    r: Number.parseInt(hex.slice(1, 3), 16),
    g: Number.parseInt(hex.slice(3, 5), 16),
    b: Number.parseInt(hex.slice(5, 7), 16),
  };
}

export function colorWithAlpha(hex: string, alpha: number): string {
  const { r, g, b } = hexToRgb(hex);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

/** Readable text colour for a filled accent surface. */
export function contrastingText(hex: string): string {
  const { r, g, b } = hexToRgb(hex);
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luminance > 0.62 ? "#161a20" : "#ffffff";
}

/* ------------------------------------------------------------------ */
/* Appearance                                                          */
/* ------------------------------------------------------------------ */

export function resolveTheme(settings: Settings, themeInfo: ThemeInfo): ThemeValue {
  return settings.theme === "auto" ? themeInfo.system : settings.theme;
}

/** Applies theme/font/digits/accent as CSS custom properties. */
export function applyAppearance(settings: Settings, themeInfo: ThemeInfo): void {
  const root = document.documentElement;
  root.dataset.theme = resolveTheme(settings, themeInfo);
  root.dataset.font = settings.font;
  root.dataset.digits = settings.digits;

  const accent = normalizeHex(settings.accent) ?? DEFAULT_SETTINGS.accent;
  root.style.setProperty("--accent", accent);
  root.style.setProperty("--accent-weak", colorWithAlpha(accent, 0.16));
  root.style.setProperty("--accent-soft", colorWithAlpha(accent, 0.09));
  root.style.setProperty("--accent-line", colorWithAlpha(accent, 0.32));
  root.style.setProperty("--accent-contrast", contrastingText(accent));
}

/* ------------------------------------------------------------------ */
/* Store                                                               */
/* ------------------------------------------------------------------ */

export class SettingsStore {
  private state: Settings;
  private saved: Settings;
  private queued: Partial<Settings> = {};
  private savingPatch: Partial<Settings> = {};
  private theme: ThemeInfo;
  private readonly listeners = new Set<(settings: Settings) => void>();
  private readonly statusListeners = new Set<(status: SaveStatus) => void>();
  private saveTimer: number | null = null;
  private savePromise: Promise<void> | null = null;
  private resetting = false;
  private status: SaveStatus = { state: "idle", at: 0 };

  private constructor(
    private readonly bridge: Bridge,
    initial: Settings,
    themeInfo: ThemeInfo,
  ) {
    this.state = { ...initial };
    this.saved = { ...initial };
    this.theme = { ...themeInfo };
  }

  static async load(bridge: Bridge): Promise<SettingsStore> {
    try {
      const [settings, theme] = await Promise.all([bridge.getSettings(), bridge.getTheme()]);
      return new SettingsStore(bridge, settings, theme);
    } catch (error) {
      // Keep the UI usable if the backend command is not available yet
      // (e.g. while the Rust side is still being wired up).
      console.warn("settings load failed; using defaults", error);
      const system: ThemeValue = window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light";
      return new SettingsStore(bridge, { ...DEFAULT_SETTINGS }, { mode: system, system });
    }
  }

  get settings(): Readonly<Settings> {
    return this.state;
  }

  get themeInfo(): Readonly<ThemeInfo> {
    return this.theme;
  }

  get resolvedTheme(): ThemeValue {
    return resolveTheme(this.state, this.theme);
  }

  subscribe(listener: (settings: Settings) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  onStatus(listener: (status: SaveStatus) => void): () => void {
    this.statusListeners.add(listener);
    return () => {
      this.statusListeners.delete(listener);
    };
  }

  get saveStatus(): Readonly<SaveStatus> {
    return this.status;
  }

  /** Optimistic local update, then a debounced `save_settings`. */
  set(patch: Partial<Settings>): void {
    Object.assign(this.queued, patch);
    this.reconcile();
    this.scheduleSave();
  }

  /** Full replacement coming from Rust (`settings-changed` event). */
  replace(next: Settings): void {
    this.saved = { ...next };
    this.reconcile();
  }

  setThemeInfo(themeInfo: ThemeInfo): void {
    this.theme = { ...themeInfo };
    this.notify();
  }

  async reset(): Promise<void> {
    this.resetting = true;
    if (this.saveTimer !== null) window.clearTimeout(this.saveTimer);
    this.saveTimer = null;
    this.queued = {};
    this.reconcile();

    try {
      await this.savePromise;
      this.setStatus("saving");
      this.replace(await this.bridge.resetSettings());
      this.setStatus("saved");
    } catch (error) {
      this.setStatus("error");
      throw error;
    } finally {
      this.resetting = false;
      if (Object.keys(this.queued).length > 0) this.scheduleSave();
    }
  }

  private reconcile(): void {
    this.state = { ...this.saved, ...this.savingPatch, ...this.queued };
    this.notify();
  }

  private notify(): void {
    for (const listener of this.listeners) listener(this.state);
  }

  private setStatus(state: SaveState): void {
    this.status = { state, at: Date.now() };
    for (const listener of this.statusListeners) listener(this.status);
  }

  private scheduleSave(): void {
    if (this.saveTimer !== null) window.clearTimeout(this.saveTimer);
    if (this.savePromise || this.resetting) return;
    this.saveTimer = window.setTimeout(() => {
      this.saveTimer = null;
      this.savePromise = this.flush().finally(() => {
        this.savePromise = null;
        if (Object.keys(this.queued).length > 0) this.scheduleSave();
      });
    }, SAVE_DEBOUNCE_MS);
  }

  private async flush(): Promise<void> {
    while (!this.resetting && Object.keys(this.queued).length > 0) {
      this.savingPatch = this.queued;
      this.queued = {};
      this.setStatus("saving");
      try {
        this.saved = await this.bridge.saveSettings(this.savingPatch);
        this.setStatus("saved");
      } catch (error) {
        console.error("save_settings failed", error);
        this.setStatus("error");
      } finally {
        this.savingPatch = {};
        this.reconcile();
      }
    }
  }
}

/** Applies appearance now and on every settings/theme change. */
export function bindAppearance(store: SettingsStore): () => void {
  const apply = (): void => applyAppearance(store.settings, store.themeInfo);
  apply();
  return store.subscribe(apply);
}
