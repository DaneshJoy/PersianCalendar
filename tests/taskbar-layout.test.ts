import { expect, it } from "vitest";
import { taskbarLayout } from "../src/taskbar-layout";

it("fits short names and caps long names instead of filling the taskbar slot", () => {
  expect(taskbarLayout(220, true, true, 58)).toEqual({ calendar: true, clock: true, event: true, capsuleWidth: 111 });
  expect(taskbarLayout(220, true, true, 21)).toEqual({ calendar: true, clock: true, event: true, capsuleWidth: 74 });
  expect(taskbarLayout(220, true, true, 400).capsuleWidth).toBe(151);
  expect(taskbarLayout(220, true, true, 0).capsuleWidth).toBe(36);
});

it("reserves a stable clock width with seconds and still fits the event", () => {
  expect(taskbarLayout(246, true, true, 400, true)).toEqual({ calendar: true, clock: true, event: true, capsuleWidth: 151 });
  expect(taskbarLayout(131, true, true, 0, true).clock).toBe(true);
  expect(taskbarLayout(130, true, true, 0, true).clock).toBe(false);
});

it("keeps the calendar in a crowded bar and fits the full clock when there is enough room", () => {
  expect(taskbarLayout(58, true, true, 200)).toEqual({ calendar: true, clock: false, event: false, capsuleWidth: 36 });
  expect(taskbarLayout(105, true, true, 200)).toEqual({ calendar: true, clock: true, event: false, capsuleWidth: 36 });
  expect(taskbarLayout(62, false, true, 0)).toEqual({ calendar: false, clock: true, event: false, capsuleWidth: 0 });
  expect(taskbarLayout(61, false, true, 0).clock).toBe(false);
});
