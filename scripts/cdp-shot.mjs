/**
 * CDP screenshot helper (dev-only, no npm dependency).
 *
 * Launches headless Chrome with remote debugging, emulates an exact CSS
 * viewport / theme, optionally runs JS and then writes a PNG.
 *
 * Usage:
 *   node scripts/cdp-shot.mjs --url http://127.0.0.1:4173/index.html \
 *        --width 344 --height 424 --theme dark --out .artifacts/popup.png \
 *        [--scale 2] [--wait 800] [--eval "document.title"] [--click selector]
 *        [--transparent] [--init-file seed.js]
 *
 * `--init-file` runs the given script in every new document before the app
 * loads (handy to seed the dev-bridge localStorage), and `--transparent`
 * captures with a transparent page background for the docs composer.
 */
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

const CHROME = process.env.CHROME_PATH || "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";

function parseArgs(argv) {
  const args = { scale: 2, wait: 900, port: 9333, theme: "dark" };
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

async function waitForJson(url, timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return await response.json();
    } catch {
      /* not up yet */
    }
    await sleep(200);
  }
  throw new Error(`timed out waiting for ${url}`);
}

async function main() {
  const args = parseArgs(process.argv);
  if (!args.url || !args.out) {
    console.error("usage: node scripts/cdp-shot.mjs --url <url> --out <file> [--width --height --theme --scale]");
    process.exit(2);
  }
  const width = Number(args.width || 344);
  const height = Number(args.height || 424);
  const scale = Number(args.scale);
  const port = Number(args.port);
  const profile = mkdtempSync(join(tmpdir(), "cdp-shot-"));

  const chrome = spawn(
    CHROME,
    [
      "--headless=new",
      "--no-sandbox",
      "--disable-gpu",
      "--disable-dev-shm-usage",
      "--no-first-run",
      "--no-default-browser-check",
      `--remote-debugging-port=${port}`,
      `--user-data-dir=${profile}`,
      "about:blank",
    ],
    { stdio: "ignore" },
  );

  try {
    const version = await waitForJson(`http://127.0.0.1:${port}/json/version`);
    const pages = await waitForJson(`http://127.0.0.1:${port}/json/list`);
    const pageInfo = pages.find((entry) => entry.type === "page");
    const pageWs = new WebSocket(pageInfo.webSocketDebuggerUrl);
    await new Promise((done, fail) => {
      pageWs.addEventListener("open", done, { once: true });
      pageWs.addEventListener("error", fail, { once: true });
    });
    const pageCdp = new Cdp(pageWs);
    void version;

    await pageCdp.send("Page.enable");
    await pageCdp.send("Emulation.setDeviceMetricsOverride", {
      width,
      height,
      deviceScaleFactor: scale,
      mobile: false,
    });
    await pageCdp.send("Emulation.setEmulatedMedia", {
      features: [{ name: "prefers-color-scheme", value: String(args.theme) }],
    });
    if (args.transparent) {
      await pageCdp.send("Emulation.setDefaultBackgroundColorOverride", {
        color: { r: 0, g: 0, b: 0, a: 0 },
      });
    }
    if (args["init-file"]) {
      await pageCdp.send("Page.addScriptToEvaluateOnNewDocument", {
        source: readFileSync(resolve(String(args["init-file"])), "utf8"),
      });
    }

    const loaded = new Promise((done) => {
      pageWs.addEventListener("message", (event) => {
        const message = JSON.parse(event.data);
        if (message.method === "Page.loadEventFired") done();
      });
    });
    await pageCdp.send("Page.navigate", { url: String(args.url) });
    await Promise.race([loaded, sleep(10000)]);
    await sleep(Number(args.wait));

    if (args.click) {
      await pageCdp.send("Runtime.evaluate", {
        expression: `document.querySelector(${JSON.stringify(String(args.click))})?.click(); true`,
      });
      await sleep(500);
    }

    if (args.eval) {
      const result = await pageCdp.send("Runtime.evaluate", {
        expression: `JSON.stringify((() => { try { return (${args.eval}); } catch (error) { return String(error); } })())`,
        returnByValue: true,
      });
      console.log("EVAL:", result.result && result.result.value);
    }

    const metrics = await pageCdp.send("Runtime.evaluate", {
      expression: `JSON.stringify({ innerWidth: window.innerWidth, innerHeight: window.innerHeight, scrollWidth: document.documentElement.scrollWidth, scrollHeight: document.documentElement.scrollHeight, theme: document.documentElement.dataset.theme, bodyBg: getComputedStyle(document.body).backgroundColor })`,
      returnByValue: true,
    });
    console.log("METRICS:", metrics.result.value);

    const shot = await pageCdp.send("Page.captureScreenshot", {
      format: "png",
      captureBeyondViewport: false,
    });
    const outputPath = resolve(String(args.out));
    writeFileSync(outputPath, Buffer.from(shot.data, "base64"));
    console.log("WROTE:", outputPath);
    pageWs.close();
  } finally {
    chrome.kill();
  }
}

main().catch((error) => {
  console.error("cdp-shot failed:", error);
  process.exit(1);
});
