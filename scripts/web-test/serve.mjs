#!/usr/bin/env node
import { createReadStream, lstatSync, realpathSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";

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
let realRoot;
try {
  realRoot = realpathSync(root);
} catch {
  console.error(`root does not exist: ${root}`);
  process.exit(2);
}

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".json": "application/json; charset=utf-8",
  ".map": "application/json; charset=utf-8",
};

function safePath(urlPath) {
  let decoded;
  try {
    decoded = decodeURIComponent(urlPath.split("?")[0]);
  } catch {
    return null;
  }
  if (decoded.includes("\\")) return null;
  const rel = decoded === "/" ? "/index.html" : decoded;
  return safeResolvedPath(resolve(realRoot, `.${rel}`));
}

function safeResolvedPath(candidate) {
  const lexical = relative(realRoot, candidate);
  if (lexical === "" || lexical === ".." || lexical.startsWith(`..${sep}`) || isAbsolute(lexical)) {
    return null;
  }
  let candidateStat;
  try {
    candidateStat = lstatSync(candidate);
  } catch {
    candidateStat = null;
  }
  let probe = candidate;
  while (true) {
    try {
      const real = realpathSync(probe);
      const realRelative = relative(realRoot, real);
      if (realRelative === ".." || realRelative.startsWith(`..${sep}`) || isAbsolute(realRelative)) {
        return null;
      }
      return candidateStat?.isSymbolicLink() ? real : candidate;
    } catch {
      if (probe === candidate && candidateStat?.isSymbolicLink()) return null;
      const parent = dirname(probe);
      if (parent === probe) return null;
      probe = parent;
    }
  }
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
    if (st.isDirectory()) path = safeResolvedPath(join(path, "index.html"));
    if (!path) throw new Error("unsafe path");
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
