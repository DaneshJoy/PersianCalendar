/**
 * Dev-only local mirror for the GitHub release assets Tauri's bundler needs.
 *
 * The bundler requests `<mirror>/<original-github-url>`, e.g.
 *   http://127.0.0.1:8765/https://github.com/tauri-apps/binary-releases/...
 * This server maps that path to a local file under --root so installer bundling
 * works on networks where GitHub downloads time out. Not part of the app.
 *
 * Usage: node scripts/local-tool-mirror.mjs --root E:\Tools\_tauri-tools --port 8765
 */
import http from "node:http";
import { readFileSync, statSync } from "node:fs";
import { resolve, sep } from "node:path";

function arg(name, fallback) {
  const index = process.argv.indexOf(`--${name}`);
  return index > 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

const root = resolve(arg("root", "."));
const port = Number(arg("port", "8765"));

const server = http.createServer((request, response) => {
  const rawPath = decodeURIComponent((request.url || "/").split("?")[0]);
  let filePath = rawPath.replace(/^\/+/, "").replace(/^(https?:\/\/github\.com\/)/, "");
  const candidate = resolve(root, filePath);
  if (candidate !== root && !candidate.startsWith(root + sep)) {
    response.writeHead(403).end("outside mirror root");
    return;
  }
  try {
    const info = statSync(candidate);
    if (!info.isFile()) throw new Error("not a file");
    const data = readFileSync(candidate);
    response.writeHead(200, {
      "Content-Type": "application/octet-stream",
      "Content-Length": String(data.length),
    });
    response.end(data);
    console.log(`[mirror] 200 ${request.url} (${data.length} bytes)`);
  } catch {
    response.writeHead(404).end("not mirrored");
    console.log(`[mirror] 404 ${request.url} -> ${candidate}`);
  }
});

server.listen(port, "127.0.0.1", () => {
  console.log(`mirror serving ${root} on http://127.0.0.1:${port}`);
});
