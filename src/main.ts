/**
 * Popup window entry (`label = "popup"`).
 */
import "./styles/base.css";
import "./styles/calendar.css";
import "./styles/popup.css";
import "./styles/settings.css";

import { bootstrapServices } from "./bootstrap";
import { PopupView } from "./ui/popup";

async function bootstrap(): Promise<void> {
  const services = await bootstrapServices("popup");
  const root = document.getElementById("app");
  if (!root) throw new Error("Missing #app root element");

  const popup = new PopupView(root, services);
  popup.mount();
}

function showFatalError(error: unknown): void {
  console.error("popup bootstrap failed", error);
  const root = document.getElementById("app");
  if (!root) return;
  root.textContent = "";
  const message = document.createElement("div");
  message.className = "fatal-error";
  message.textContent = "خطا در بارگذاری تقویم";
  root.append(message);
}

void bootstrap().catch(showFatalError);
