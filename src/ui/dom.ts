/** Tiny DOM helpers shared by the popup, settings panel and widget. */

export type Child = Node | string | null | undefined | false;

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  children?: Child | Child[],
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  append(node, children);
  return node;
}

export function append(parent: Node, children?: Child | Child[]): void {
  if (children === undefined || children === null || children === false) return;
  if (Array.isArray(children)) {
    for (const child of children) append(parent, child);
    return;
  }
  parent.appendChild(typeof children === "string" ? document.createTextNode(children) : children);
}

export function clear(parent: Node): void {
  while (parent.firstChild) parent.removeChild(parent.firstChild);
}

export function text(value: string): Text {
  return document.createTextNode(value);
}

/* ------------------------------------------------------------------ */
/* Icons (inline SVG, no external assets)                              */
/* ------------------------------------------------------------------ */

export type IconName = "chevronLeft" | "chevronRight" | "gear" | "close" | "check";

interface IconShape {
  paths: string[];
  circles?: Array<{ cx: number; cy: number; r: number }>;
}

const ICONS: Record<IconName, IconShape> = {
  chevronLeft: { paths: ["M15 18 9 12l6-6"] },
  chevronRight: { paths: ["M9 18l6-6-6-6"] },
  gear: {
    paths: [
      "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z",
    ],
    circles: [{ cx: 12, cy: 12, r: 3 }],
  },
  close: { paths: ["M18 6 6 18", "M6 6l12 12"] },
  check: { paths: ["M20 6 9 17l-5-5"] },
};

const SVG_NS = "http://www.w3.org/2000/svg";

export function icon(name: IconName): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("class", "icon");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("focusable", "false");
  const shape = ICONS[name];
  for (const path of shape.paths) {
    const node = document.createElementNS(SVG_NS, "path");
    node.setAttribute("d", path);
    svg.appendChild(node);
  }
  for (const circle of shape.circles ?? []) {
    const node = document.createElementNS(SVG_NS, "circle");
    node.setAttribute("cx", String(circle.cx));
    node.setAttribute("cy", String(circle.cy));
    node.setAttribute("r", String(circle.r));
    svg.appendChild(node);
  }
  return svg;
}

/* ------------------------------------------------------------------ */
/* Building blocks                                                     */
/* ------------------------------------------------------------------ */

export function iconButton(name: IconName, label: string, className = "icon-button"): HTMLButtonElement {
  const button = el("button", className);
  button.type = "button";
  button.setAttribute("aria-label", label);
  button.title = label;
  button.appendChild(icon(name));
  return button;
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function isActivatableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.closest("input, textarea, select, [contenteditable='true']") !== null;
}

/** Focus helper that never throws for missing/sandboxed elements. */
export function focusElement(node: HTMLElement | null): void {
  if (!node) return;
  try {
    node.focus({ preventScroll: true });
  } catch {
    /* ignore */
  }
}
