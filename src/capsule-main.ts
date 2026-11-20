import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ClockInfo } from "./types";
import { rotationFrame } from "./capsule-rotation";
import { taskbarLayout } from "./taskbar-layout";
import "./styles/capsule.css";

interface CapsuleData {
  date: string;
  day: string;
  dayColor: string | null;
  calendarEnabled: boolean;
  inlineClock: boolean;
  clock: ClockInfo | null;
  hourColor: string | null;
  minuteColor: string | null;
  secondColor: string | null;
  events: string[];
  intervalSeconds: number;
  dark: boolean;
}

const row = document.querySelector<HTMLElement>(".taskbar-surfaces")!;
const capsule = document.querySelector<HTMLButtonElement>(".calendar-capsule")!;
const day = document.querySelector<HTMLElement>(".calendar-capsule__day")!;
const viewport = document.querySelector<HTMLElement>(".calendar-capsule__window")!;
const eventLabel = document.querySelector<HTMLElement>(".calendar-capsule__event")!;
const clock = document.querySelector<HTMLButtonElement>(".taskbar-clock")!;
const hour = document.querySelector<HTMLElement>(".taskbar-clock__hour")!;
const minute = document.querySelector<HTMLElement>(".taskbar-clock__minute")!;
const second = document.querySelector<HTMLElement>(".taskbar-clock__second")!;
const secondSeparator = document.querySelector<HTMLElement>(".taskbar-clock__second-separator")!;
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
const native = "__TAURI_INTERNALS__" in window;

let data: CapsuleData | undefined;
let latestClock: ClockInfo | null = null;
let clockVersion = 0;
let startedAt = Date.now();
let rotationTimer: number | undefined;
let layoutTimer: number | undefined;
let marquee: Animation | undefined;
let slide: Animation | undefined;
let requestNumber = 0;
let lastSurface = "";
let layoutFrame: number | undefined;

function setColor(property: string, color: string | null): void {
  if (color) row.style.setProperty(property, color);
  else row.style.removeProperty(property);
}

function reportSurface(force: boolean): void {
  if (!native) return;
  const rect = row.getBoundingClientRect();
  const bounds = rect.width && rect.height
    ? { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
    : null;
  const detail = { bounds, calendar: !capsule.hidden, clock: !clock.hidden };
  const signature = JSON.stringify(detail);
  if (!force && lastSurface === signature) return;
  lastSurface = signature;
  void invoke("set_taskbar_surface", detail).catch((error) => {
    if (lastSurface === signature) lastSurface = "";
    console.error("taskbar surface registration failed", error);
  });
}

function scheduleMarquee(): void {
  clearTimeout(layoutTimer);
  marquee?.cancel();
  layoutTimer = window.setTimeout(() => {
    if (!data || reducedMotion.matches || viewport.clientWidth <= 0 || !capsule.classList.contains("has-event")) return;
    const distance = eventLabel.offsetWidth - viewport.clientWidth;
    if (distance <= 3) return;
    const duration = Math.min(data.intervalSeconds * 700, Math.max(5_500, distance * 36 + 3_400));
    // The label is right-aligned. Moving right brings its left edge into view.
    marquee = eventLabel.animate([
      { transform: "translateX(0)" },
      { transform: "translateX(0)", offset: 0.22 },
      { transform: `translateX(${distance}px)`, offset: 0.78 },
      { transform: `translateX(${distance}px)` },
    ], { duration, delay: 1_200, fill: "forwards" });
  }, 250);
}

function layout(forceSurface = false): void {
  if (!data) return;
  const text = eventLabel.textContent ?? "";
  // Re-measure when a previously crowded taskbar makes room for the capsule.
  capsule.hidden = !data.calendarEnabled;
  const textWidth = text ? eventLabel.scrollWidth : 0;
  const config = taskbarLayout(document.body.clientWidth, data.calendarEnabled, data.inlineClock && latestClock !== null, textWidth, clock.classList.contains("has-seconds"));
  capsule.hidden = !config.calendar;
  clock.hidden = !config.clock;
  capsule.style.width = `${config.capsuleWidth}px`;
  capsule.classList.toggle("has-event", config.event);
  capsule.title = text ? `${data.date} · ${text}` : data.date;
  reportSurface(forceSurface);
}

function updateClock(next: ClockInfo | null): void {
  const [hours = "", minutes = "", seconds = ""] = next?.time.split(":") ?? [];
  const hasSeconds = seconds !== "";
  const resize = (latestClock === null) !== (next === null) || hasSeconds !== clock.classList.contains("has-seconds");
  latestClock = next;
  clock.classList.toggle("has-seconds", hasSeconds);
  second.hidden = secondSeparator.hidden = !hasSeconds;
  if (hour.textContent !== hours) hour.textContent = hours;
  if (minute.textContent !== minutes) minute.textContent = minutes;
  if (second.textContent !== seconds) second.textContent = seconds;
  const title = next?.date ?? "";
  if (clock.title !== title) clock.title = title;
  if (resize) scheduleLayout();
}

function scheduleLayout(force = false): void {
  if (layoutFrame !== undefined) cancelAnimationFrame(layoutFrame);
  layoutFrame = requestAnimationFrame(() => {
    layoutFrame = undefined;
    layout(force);
  });
}

function render(): void {
  if (!data) return;
  clearTimeout(rotationTimer);
  const step = rotationFrame(data.events.length, Date.now() - startedAt, data.intervalSeconds);
  const nextEvent = data.events[step.index] ?? "";
  const changed = eventLabel.textContent !== nextEvent;
  if (changed) eventLabel.textContent = nextEvent;
  layout();
  if (changed) {
    slide?.cancel();
    if (nextEvent && capsule.classList.contains("has-event") && !reducedMotion.matches) {
      slide = viewport.animate([
        { opacity: 0, transform: "translateY(3px)" },
        { opacity: 1, transform: "translateY(0)" },
      ], { duration: 185, easing: "ease-out" });
    }
    scheduleMarquee();
  }
  if (step.nextIn !== null) rotationTimer = window.setTimeout(render, step.nextIn);
}

function apply(next: CapsuleData, replaceClock: boolean): void {
  if (!data || data.date !== next.date || data.intervalSeconds !== next.intervalSeconds ||
      data.events.length !== next.events.length || data.events.some((event, index) => event !== next.events[index])) {
    startedAt = Date.now();
  }
  data = next;
  if (replaceClock) updateClock(next.clock);
  document.documentElement.dataset.theme = next.dark ? "dark" : "light";
  setColor("--day-color", next.dayColor);
  setColor("--hour-color", next.hourColor);
  setColor("--minute-color", next.minuteColor);
  setColor("--second-color", next.secondColor);
  day.textContent = next.day;
  render();
}

async function refresh(): Promise<void> {
  const request = ++requestNumber;
  const revision = clockVersion;
  const next = await invoke<CapsuleData>("get_capsule_data");
  if (request === requestNumber) apply(next, revision === clockVersion);
}

function refreshSoon(): void {
  void refresh().catch((error) => console.error("taskbar data unavailable", error));
}

if (native) {
  for (const name of ["today-changed", "settings-changed", "theme-changed", "events-updated"]) {
    void listen(name, refreshSoon).catch((error) => console.error("taskbar updates unavailable", error));
  }
  void listen<ClockInfo | null>("clock-changed", (event) => {
    clockVersion++;
    updateClock(event.payload);
  }).catch((error) => console.error("clock updates unavailable", error));
  void listen<boolean>("taskbar-placement", (event) => {
    document.body.classList.toggle("tray-left", !event.payload);
    // A WebView may have measured its surface before its native window became visible.
    layout(true);
  }).catch((error) => console.error("taskbar placement updates unavailable", error));

  for (const button of [clock, capsule]) {
    button.addEventListener("click", () => {
      void invoke("open_taskbar_calendar").catch((error) => console.error("cannot open calendar", error));
    });
    button.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      void invoke("open_taskbar_menu").catch((error) => console.error("cannot open calendar menu", error));
    });
  }
  refreshSoon();
} else if (import.meta.env.DEV) {
  const options = new URLSearchParams(location.search);
  apply({
    date: "1405/08/29",
    day: "۲۹",
    dayColor: "#2f6fed",
    calendarEnabled: !options.has("clock-only"),
    inlineClock: !options.has("no-clock"),
    clock: { time: options.has("seconds") ? "۱۸:۲۰:۰۵" : "۱۸:۲۰", date: "۱۴۰۵/۰۸/۲۹" },
    hourColor: "#b38400",
    minuteColor: "#d9485f",
    secondColor: "#8263bd",
    events: options.has("empty") ? [] : [options.has("long") ? "جشن برداشت زعفران در روستاهای خراسان جنوبی" : "روز جهانی کودک"],
    intervalSeconds: 30,
    dark: matchMedia("(prefers-color-scheme: dark)").matches,
  }, true);
}

new ResizeObserver(scheduleMarquee).observe(viewport);
new ResizeObserver(() => scheduleLayout()).observe(document.body);
void document.fonts.ready.then(() => { scheduleLayout(); scheduleMarquee(); });
reducedMotion.addEventListener("change", scheduleMarquee);
document.addEventListener("visibilitychange", () => {
  if (!document.hidden) { render(); layout(true); }
});
