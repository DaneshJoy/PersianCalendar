/**
 * Evaluate JavaScript inside a page exposed by a remote-debugging endpoint
 * (WebView2 or Chrome). Dev tool, no npm dependencies.
 *
 * Usage:
 *   node scripts/cdp-eval.mjs --port 9222 --target "تقویم" --eval "document.title"
 *   node scripts/cdp-eval.mjs --port 9222 --url "http://tauri.localhost/" --file scripts/probe.js
 *   node scripts/cdp-eval.mjs --port 9222 --target widget.html --eval "JSON.stringify(...)" --json
 */
import { readFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";

function parseArgs(argv) {
  const args = { port: 9222, target: "", eval: "", wait: 0 };
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
      /* retry */
    }
    await sleep(150);
  }
  throw new Error(`timed out waiting for ${url}`);
}

async function main() {
  const args = parseArgs(process.argv);
  const port = Number(args.port);
  const pages = await waitForJson(`http://127.0.0.1:${port}/json/list`);
  const target = String(args.target);
  const candidates = pages.filter((entry) => entry.type === "page");
  let page = null;
  if (args.url) {
    page = candidates.find((entry) => entry.url === String(args.url)) || null;
  }
  if (!page && target) {
    page = candidates.find((entry) => entry.title === target) || null;
  }
  if (!page && target) {
    page = candidates.find((entry) => entry.title.includes(target)) || null;
  }
  if (!page && target) {
    page = candidates.find((entry) => entry.url.includes(target)) || null;
  }
  if (!page) {
    console.error("available pages:", pages.map((entry) => `${entry.title} | ${entry.url}`).join("\n"));
    throw new Error(`no page matching target ${JSON.stringify(target)} on port ${port}`);
  }
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((done, fail) => {
    ws.addEventListener("open", done, { once: true });
    ws.addEventListener("error", fail, { once: true });
  });
  const cdp = new Cdp(ws);
  await cdp.send("Runtime.enable");
  if (Number(args.wait) > 0) await sleep(Number(args.wait));
  const expression = args.file ? readFileSync(String(args.file), "utf8") : String(args.eval);
  const result = await cdp.send("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  if (result.exceptionDetails) {
    console.error("EVAL_THREW:", JSON.stringify(result.exceptionDetails));
    process.exitCode = 1;
  } else {
    const value = result.result && result.result.value;
    console.log(typeof value === "string" ? value : JSON.stringify(value));
  }
  ws.close();
}

main().catch((error) => {
  console.error("cdp-eval failed:", error.message);
  process.exit(1);
});
