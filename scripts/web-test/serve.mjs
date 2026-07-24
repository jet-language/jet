#!/usr/bin/env node
import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import { join, normalize } from "node:path";

function arg(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : null;
}

const port = Number(arg("--port"));
const root = arg("--root");
if (!port || !root) {
  console.error("usage: serve.mjs --port <port> --root <dir>");
  process.exit(2);
}

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".json": "application/json; charset=utf-8",
};

function safePath(urlPath) {
  const decoded = decodeURIComponent(urlPath.split("?")[0]);
  const rel = decoded === "/" ? "/index.html" : decoded;
  const joined = normalize(join(root, rel));
  if (!joined.startsWith(normalize(root))) return null;
  return joined;
}

createServer((req, res) => {
  const file = safePath(req.url || "/");
  if (!file) {
    res.writeHead(400);
    res.end("bad path");
    return;
  }
  let path = file;
  try {
    const st = statSync(path);
    if (st.isDirectory()) path = join(path, "index.html");
    statSync(path);
  } catch {
    res.writeHead(404);
    res.end("not found");
    return;
  }
  const ext = path.slice(path.lastIndexOf("."));
  res.writeHead(200, { "content-type": MIME[ext] || "application/octet-stream" });
  createReadStream(path).pipe(res);
}).listen(port, "127.0.0.1", () => {
  process.stdout.write(`serving ${root} on ${port}\n`);
});
