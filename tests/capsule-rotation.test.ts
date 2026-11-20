import { expect, it } from "vitest";
import { rotationFrame } from "../src/capsule-rotation";

it("does not schedule a change for zero or one occasion", () => {
  expect(rotationFrame(0, 100_000, 60)).toEqual({ index: 0, nextIn: null });
  expect(rotationFrame(1, 100_000, 60)).toEqual({ index: 0, nextIn: null });
});

it("rotates all ten items, wraps, and keeps the next boundary after a sleep", () => {
  expect(rotationFrame(10, 59_999, 60)).toEqual({ index: 0, nextIn: 1 });
  expect(rotationFrame(10, 60_000, 60)).toEqual({ index: 1, nextIn: 60_000 });
  expect(rotationFrame(10, 620_000, 60)).toEqual({ index: 0, nextIn: 40_000 });
});

it("honors 30 seconds and 30 minutes", () => {
  expect(rotationFrame(3, 31_000, 30)).toEqual({ index: 1, nextIn: 29_000 });
  expect(rotationFrame(3, 1_800_000, 1800)).toEqual({ index: 1, nextIn: 1_800_000 });
});
