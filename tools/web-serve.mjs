#!/usr/bin/env node
// Tiny, dependency-free static file server for `jet build --target=web` output
// (build/index.html, app.js, app.wasm, jet_dom_runtime.js). ES module imports
// and WASM need a real http:// origin — `file://` won't work (CORS blocks the
// module import). Usage:
//
//   node tools/web-serve.mjs [dir] [port]
//
// Defaults: dir=build, port=8080.

import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { join, extname, normalize } from "node:path";

const dir = process.argv[2] || "build";
const port = Number(process.argv[3] || 8080);

const MIME = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".json": "application/json",
  ".wasm": "application/wasm",
  ".css": "text/css",
};

const server = createServer(async (req, res) => {
  let p = req.url.split("?")[0];
  if (p === "/") p = "/index.html";
  const file = join(process.cwd(), dir, normalize(p).replace(/^(\.\.[/\\])+/, ""));
  try {
    const s = await stat(file);
    if (!s.isFile()) throw new Error("not a file");
    const data = await readFile(file);
    res.writeHead(200, { "content-type": MIME[extname(file)] || "application/octet-stream" });
    res.end(data);
  } catch {
    res.writeHead(404);
    res.end(`not found: ${p}`);
  }
});

server.listen(port, () => {
  console.log(`serving ./${dir} at http://localhost:${port}`);
});
