/**
 * Desktop widget window entry (`label = "widget"`).
 */
import "./styles/base.css";
import "./styles/calendar.css";
import "./styles/widget.css";

import { bootstrapServices } from "./bootstrap";
import { WidgetView } from "./ui/widget";

async function bootstrap(): Promise<void> {
  const services = await bootstrapServices("widget");
  const root = document.getElementById("app");
  if (!root) throw new Error("Missing #app root element");

  const widget = new WidgetView(root, services);
  widget.mount();
}

function showFatalError(error: unknown): void {
  console.error("widget bootstrap failed", error);
  const root = document.getElementById("app");
  if (!root) return;
  root.textContent = "";
  const message = document.createElement("div");
  message.className = "fatal-error";
  message.textContent = "خطا در بارگذاری ویجت";
  root.append(message);
}

void bootstrap().catch(showFatalError);
