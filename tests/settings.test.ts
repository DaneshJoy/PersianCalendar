import { afterEach, describe, expect, it, vi } from "vitest";

import type { Bridge } from "../src/bridge";
import { SettingsStore } from "../src/settings";
import { DEFAULT_SETTINGS, type Settings } from "../src/types";

async function settingsStore() {
  vi.useFakeTimers();
  vi.stubGlobal("window", { setTimeout, clearTimeout });
  let saved = { ...DEFAULT_SETTINGS };
  const saveSettings = vi.fn(async (patch: Partial<Settings>) => {
    saved = { ...saved, ...patch };
    return saved;
  });
  const bridge = {
    getSettings: async () => saved,
    getTheme: async () => ({ mode: "light", system: "light" }),
    saveSettings,
    resetSettings: async () => (saved = { ...DEFAULT_SETTINGS }),
  } as unknown as Bridge;
  const store = await SettingsStore.load(bridge);
  return { store, saveSettings, reload: () => SettingsStore.load(bridge) };
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("settings autosave", () => {
  it("starts with the second clock off and restores the last saved choice", async () => {
    const { store, reload } = await settingsStore();
    expect(store.settings.clockEnabled).toBe(false);
    expect(store.settings.capsuleEnabled).toBe(false);

    store.set({ clockEnabled: true });
    await vi.advanceTimersByTimeAsync(250);
    const reopened = await reload();
    expect(reopened.settings.clockEnabled).toBe(true);

    reopened.set({ clockEnabled: false });
    await vi.advanceTimersByTimeAsync(250);
    expect((await reload()).settings.clockEnabled).toBe(false);
  });

  it("preserves a pending time zone when another window broadcasts settings", async () => {
    const { store, saveSettings } = await settingsStore();
    store.set({ clockZone: "Pacific Standard Time" });
    store.replace({ ...DEFAULT_SETTINGS, widgetLocked: true });

    expect(store.settings.clockZone).toBe("Pacific Standard Time");
    expect(store.settings.widgetLocked).toBe(true);
    await vi.advanceTimersByTimeAsync(250);
    expect(saveSettings).toHaveBeenCalledExactlyOnceWith({ clockZone: "Pacific Standard Time" });
  });

  it("keeps a second edit that arrives while the previous edit is saving", async () => {
    const { store, saveSettings } = await settingsStore();
    let completeFirstSave: (settings: Settings) => void = () => {};
    saveSettings.mockImplementationOnce(() => new Promise((resolve) => {
      completeFirstSave = resolve;
    }));
    store.set({ clockZone: "Pacific Standard Time" });
    await vi.advanceTimersByTimeAsync(250);
    store.set({ clockEnabled: false });
    completeFirstSave({ ...DEFAULT_SETTINGS, clockZone: "Pacific Standard Time" });
    await vi.advanceTimersByTimeAsync(0);

    expect(saveSettings).toHaveBeenNthCalledWith(2, { clockEnabled: false });
    expect(store.settings.clockEnabled).toBe(false);
  });

  it("cancels a queued save when resetting settings", async () => {
    const { store, saveSettings } = await settingsStore();
    store.set({ onlineEvents: true });
    await store.reset();
    await vi.advanceTimersByTimeAsync(250);

    expect(saveSettings).not.toHaveBeenCalled();
    expect(store.settings.onlineEvents).toBe(false);
  });
});
