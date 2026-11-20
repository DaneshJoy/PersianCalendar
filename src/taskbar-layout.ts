const DAY_WIDTH = 36;
const CLOCK_WIDTH = 62;
const CLOCK_SECONDS_WIDTH = 88;
const GAP = 7;
const EVENT_PADDING = 17;
const MIN_EVENT_WIDTH = 34;
const MAX_CAPSULE_WIDTH = 151;

export function taskbarLayout(available: number, showCalendar: boolean, showClock: boolean, textWidth: number, seconds = false) {
  const clockWidth = seconds ? CLOCK_SECONDS_WIDTH : CLOCK_WIDTH;
  const calendar = showCalendar && available >= DAY_WIDTH;
  const clock = showClock && available >= clockWidth + (calendar ? DAY_WIDTH + GAP : 0);
  const room = available - (clock ? clockWidth + (calendar ? GAP : 0) : 0);
  const wide = Math.min(room, MAX_CAPSULE_WIDTH, DAY_WIDTH + EVENT_PADDING + textWidth);
  const event = calendar && textWidth > 0 &&
    wide >= DAY_WIDTH + EVENT_PADDING + Math.min(textWidth, MIN_EVENT_WIDTH);
  return { calendar, clock, event, capsuleWidth: calendar ? (event ? wide : DAY_WIDTH) : 0 };
}
