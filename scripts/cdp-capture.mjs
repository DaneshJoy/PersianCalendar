/**
 * Capture a screenshot of one page of the running app (WebView2 via CDP) with
 * a transparent background, at an exact CSS viewport size and scale.
 *
 * Usage:
 *   node scripts/cdp-capture.mjs --port 9222 --url http://tauri.localhost/ \
 *        --width 344 --height 424 --scale 2 --out .artifacts/popup.png [--eval-file file.js]
 */
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

function parseArgs(argv) {
  const args = { port: 9222, scale: 2, wait: 400 };
  for (let i = 2; i < argv.length; i += 1) {
    const key = argv[i];
    if (!key.startsWith("--")) continue;
    const name = key.slice(2);
    const value = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : "true";
    args[name] = value;
  }
  return args;
}

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.nextId = 1;
    this.pending = new Map();
    ws.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      if (message.id && this.pending.has(message.id)) {
        const { resolve: done, reject } = this.pending.get(message.id);
        this.pending.delete(message.id);
        if (message.error) reject(new Error(message.error.message));
        else done(message.result);
      }
    });
  }

  send(method, params = {}) {
    const id = this.nextId++;
    return new Promise((resolvePromise, rejectPromise) => {
      this.pending.set(id, { resolve: resolvePromise, reject: rejectPromise });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }
}

async function main() {
  const args = parseArgs(process.argv);
  const port = Number(args.port);
  const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  const page = pages.find((entry) => entry.type === "page" && entry.url === String(args.url));
  if (!page) {
    console.error("pages:", pages.map((entry) => `${entry.title} | ${entry.url}`).join("\n"));
    throw new Error(`no page with url ${args.url}`);
  }
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((done, fail) => {
    ws.addEventListener("open", done, { once: true });
    ws.addEventListener("error", fail, { once: true });
  });
  const cdp = new Cdp(ws);
  await cdp.send("Page.enable");
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width: Number(args.width),
    height: Number(args.height),
    deviceScaleFactor: Number(args.scale),
    mobile: false,
  });
  await cdp.send("Emulation.setDefaultBackgroundColorOverride", {
    color: { r: 0, g: 0, b: 0, a: 0 },
  });
  if (args["eval-file"]) {
    await cdp.send("Runtime.evaluate", {
      expression: readFileSync(String(args["eval-file"]), "utf8"),
      awaitPromise: true,
      returnByValue: true,
    });
  }
  await sleep(Number(args.wait));
  const shot = await cdp.send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
    fromSurface: true,
  });
  const outputPath = resolve(String(args.out));
  writeFileSync(outputPath, Buffer.from(shot.data, "base64"));
  console.log("WROTE:", outputPath);
  ws.close();
}

main().catch((error) => {
  console.error("cdp-capture failed:", error.message);
  process.exit(1);
});
