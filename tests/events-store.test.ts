import { expect, it, vi } from "vitest";

import type { Bridge } from "../src/bridge";
import { EventsStore } from "../src/events";
import type { MonthEvents } from "../src/types";

const builtin: MonthEvents = {
  year: 1405, month: 8, source: "builtin", stale: false, days: {}, error: null,
};
const network: MonthEvents = { ...builtin, source: "network" };

it("replaces bundled month data when a download becomes available", async () => {
  const getMonthEvents = vi.fn().mockResolvedValueOnce(builtin).mockResolvedValueOnce(network);
  const store = new EventsStore({ getMonthEvents } as unknown as Bridge);

  expect((await store.load(1405, 8))?.source).toBe("builtin");
  expect(store.lookup(1405, 8)?.source).toBe("builtin");
  expect((await store.load(1405, 8))?.source).toBe("network");
  expect(store.lookup(1405, 8)?.source).toBe("network");
});

it("keeps an older response from overwriting a month refreshed by an update event", async () => {
  let completeFirstResponse: (value: MonthEvents) => void = () => {};
  const pendingResponse = new Promise<MonthEvents>((resolve) => {
    completeFirstResponse = resolve;
  });
  const getMonthEvents = vi.fn().mockReturnValueOnce(pendingResponse).mockResolvedValueOnce(network);
  const store = new EventsStore({ getMonthEvents } as unknown as Bridge);

  const firstLoad = store.load(1405, 8);
  store.invalidate(1405, 8);
  await store.load(1405, 8);
  completeFirstResponse(builtin);
  await firstLoad;

  expect(store.lookup(1405, 8)?.source).toBe("network");
});
