#!/usr/bin/env node
import { constants, realpathSync } from "node:fs";
import { lstat, open, realpath } from "node:fs/promises";
import { createServer } from "node:http";
import { isAbsolute, join, relative, resolve, sep } from "node:path";

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

const O_RDONLY = constants.O_RDONLY;
const O_CLOEXEC = constants.O_CLOEXEC || 0;
const O_DIRECTORY = constants.O_DIRECTORY || 0;
const O_NOFOLLOW = constants.O_NOFOLLOW || 0;
const O_NONBLOCK = constants.O_NONBLOCK || 0;

function descriptorPath(fd) {
  return `/proc/self/fd/${fd}`;
}

function descriptorChildPath(fd, name) {
  return `${descriptorPath(fd)}/${name}`;
}

class UnsafePathError extends Error {}

function unsafePath(message) {
  return new UnsafePathError(message);
}

function hasIdentity(stat) {
  return (
    typeof stat.dev === "bigint" &&
    typeof stat.ino === "bigint" &&
    stat.dev !== 0n &&
    stat.ino !== 0n
  );
}

function sameIdentity(left, right) {
  return hasIdentity(left) && hasIdentity(right) && left.dev === right.dev && left.ino === right.ino;
}

function isContained(candidate) {
  const lexical = relative(realRoot, candidate);
  return (
    lexical !== "" &&
    lexical !== ".." &&
    !lexical.startsWith(`..${sep}`) &&
    !isAbsolute(lexical)
  );
}

function safePath(urlPath) {
  let decoded;
  try {
    decoded = decodeURIComponent(urlPath.split("?")[0]);
  } catch {
    return null;
  }
  if (decoded.includes("\\") || decoded.includes("\0")) return null;
  const pathPart = decoded.startsWith("/") ? decoded.slice(1) : decoded;
  if (/^[A-Za-z]:/.test(pathPart) || pathPart.startsWith("/") || pathPart.startsWith("\\")) {
    return null;
  }
  const rel = decoded === "/" ? "/index.html" : decoded;
  const candidate = resolve(realRoot, `.${rel}`);
  if (!isContained(candidate)) return null;
  const relativePath = relative(realRoot, candidate);
  if (!relativePath) return null;
  return { candidate, relativePath };
}

function components(relativePath) {
  const parts = relativePath.split(sep);
  if (
    parts.length === 0 ||
    parts.some((part) => part === "" || part === "." || part === "..")
  ) {
    throw unsafePath("static path is not normalized");
  }
  return parts;
}

function hasSingleLink(stat) {
  return typeof stat.nlink === "bigint" && stat.nlink === 1n;
}

async function closeQuietly(handle) {
  if (!handle) return;
  try {
    await handle.close();
  } catch {
    // The descriptor may already have been closed by its stream.
  }
}

let rootHandle;
let rootIdentity;
let descriptorRoot = false;
try {
  rootHandle = await open(realRoot, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
  rootIdentity = await rootHandle.stat({ bigint: true });
  if (!rootIdentity.isDirectory() || !hasIdentity(rootIdentity)) {
    throw new Error("static root is not a directory");
  }
  if (process.platform === "linux") {
    try {
      descriptorRoot = resolve(realpathSync(descriptorPath(rootHandle.fd))) === resolve(realRoot);
    } catch {
      descriptorRoot = false;
    }
  }
} catch {
  console.error(`root cannot be opened: ${root}`);
  process.exit(2);
}

async function ensureRootCurrent() {
  const current = await lstat(realRoot, { bigint: true });
  if (!current.isDirectory() || !sameIdentity(rootIdentity, current)) {
    throw unsafePath("static root changed");
  }
}

async function openEntry(parent, candidate, name, requireDirectory) {
  const before = await lstat(candidate, { bigint: true });
  if (before.isSymbolicLink()) throw unsafePath("symbolic link in static path");
  if (requireDirectory && !before.isDirectory()) {
    throw new Error("static path component is not a directory");
  }

  const path = descriptorRoot ? descriptorChildPath(parent.fd, name) : candidate;
  const flags =
    O_RDONLY |
    O_CLOEXEC |
    O_NONBLOCK |
    (requireDirectory ? O_DIRECTORY : 0) |
    (descriptorRoot ? 0 : O_NOFOLLOW);
  let handle;
  try {
    handle = await open(path, flags);
  } catch (error) {
    if (error?.code === "ELOOP") throw unsafePath("symbolic link in static path");
    throw error;
  }
  try {
    const opened = await handle.stat({ bigint: true });
    const after = await lstat(candidate, { bigint: true });
    if (
      after.isSymbolicLink() ||
      !sameIdentity(before, opened) ||
      !sameIdentity(after, opened)
    ) {
      throw unsafePath("static path changed while opening");
    }
    const actual = descriptorRoot
      ? await realpath(descriptorPath(handle.fd))
      : await realpath(candidate);
    if (!isContained(actual)) throw unsafePath("static path escaped root");
    if (requireDirectory && !opened.isDirectory()) {
      throw new Error("static path component is not a directory");
    }
    return { handle, stat: opened };
  } catch (error) {
    await closeQuietly(handle);
    throw error;
  }
}

async function openStaticFile(selection) {
  await ensureRootCurrent();
  const parts = components(selection.relativePath);
  const heldDirectories = [];
  let parent = rootHandle;
  let target;
  try {
    for (let index = 0; index < parts.length - 1; index += 1) {
      const name = parts[index];
      const candidate = join(realRoot, ...parts.slice(0, index + 1));
      const opened = await openEntry(parent, candidate, name, true);
      heldDirectories.push(opened.handle);
      parent = opened.handle;
    }

    const finalName = parts[parts.length - 1];
    const finalCandidate = join(realRoot, ...parts);
    target = await openEntry(parent, finalCandidate, finalName, false);
    let servedPath = finalCandidate;
    if (target.stat.isDirectory()) {
      const directory = target;
      try {
        const indexCandidate = join(finalCandidate, "index.html");
        target = await openEntry(directory.handle, indexCandidate, "index.html", false);
        servedPath = indexCandidate;
      } finally {
        await closeQuietly(directory.handle);
      }
    }
    if (!target.stat.isFile() || !hasSingleLink(target.stat)) {
      throw unsafePath("static target is not a single-link regular file");
    }
    return { handle: target.handle, stat: target.stat, servedPath };
  } catch (error) {
    await closeQuietly(target?.handle);
    throw error;
  } finally {
    for (const directory of heldDirectories) {
      await closeQuietly(directory);
    }
  }
}

async function serve(req, res) {
  const selection = safePath(req.url || "/");
  if (!selection) {
    res.writeHead(400);
    res.end("bad path");
    return;
  }

  let opened;
  try {
    opened = await openStaticFile(selection);
    const current = await opened.handle.stat({ bigint: true });
    if (!current.isFile() || !hasSingleLink(current) || !sameIdentity(opened.stat, current)) {
      throw unsafePath("static target changed before streaming");
    }
    const ext = opened.servedPath.slice(opened.servedPath.lastIndexOf("."));
    res.writeHead(200, { "content-type": MIME[ext] || "application/octet-stream" });
    const stream = opened.handle.createReadStream({ autoClose: true });
    stream.on("error", () => {
      if (!res.destroyed) res.destroy();
    });
    stream.pipe(res);
  } catch (error) {
    await closeQuietly(opened?.handle);
    if (res.headersSent) {
      res.destroy();
      return;
    }
    res.writeHead(error instanceof UnsafePathError ? 400 : 404);
    res.end(error instanceof UnsafePathError ? "bad path" : "not found");
  }
}

createServer((req, res) => {
  void serve(req, res);
}).listen(port, "127.0.0.1", () => {
  process.stdout.write(`serving ${root} on ${port}\n`);
});
