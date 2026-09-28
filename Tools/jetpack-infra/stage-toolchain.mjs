#!/usr/bin/env node
// Prepare an unsigned toolchain channel tree and domain-prefixed signing
// requests. Private signing keys stay with the external release signer.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const MAX_ARTIFACT_BYTES = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES = 1024 * 1024;
const MAX_ARTIFACTS = 64;
const DEFAULT_MANIFEST_TTL_SECONDS = 30 * 24 * 60 * 60;

function usage() {
  return "usage: stage-toolchain.mjs --version <version> --sequence <n> --output <dir> [--channel <name>] [--min-version <version>] [--published-at <unix-seconds>] [--expires-at <unix-seconds>] --artifact <target>=<file> ...";
}

function parseArgs(argv) {
  const values = [];
  let version;
  let sequence;
  let channel = "stable";
  let minVersion = "1.0.0";
  let publishedAt;
  let expiresAt;
  let output;
  for (let i = 0; i < argv.length; i += 1) {
    const name = argv[i];
    if (name === "--artifact") {
      const value = argv[++i];
      const split = value?.indexOf("=");
      if (!value || split <= 0 || split === value.length - 1) {
        throw new Error("--artifact needs target=path");
      }
      values.push([value.slice(0, split), value.slice(split + 1)]);
    } else if ([
      "--version",
      "--sequence",
      "--channel",
      "--min-version",
      "--published-at",
      "--expires-at",
      "--output",
    ].includes(name)) {
      const value = argv[++i];
      if (!value || value.startsWith("--")) throw new Error(name + " needs a value");
      if (name === "--version") version = value;
      if (name === "--sequence") sequence = parsePositiveInteger(value, "sequence");
      if (name === "--channel") channel = value;
      if (name === "--min-version") minVersion = value;
      if (name === "--published-at") publishedAt = parsePositiveInteger(value, "published-at");
      if (name === "--expires-at") expiresAt = parsePositiveInteger(value, "expires-at");
      if (name === "--output") output = value;
    } else {
      throw new Error(usage() + "\nunknown option " + name);
    }
  }
  if (!version || sequence === undefined || !output || values.length === 0) {
    throw new Error(usage() + "\nversion, sequence, output, and at least one artifact are required");
  }
  validateSemver(version, "version");
  validateComponent(channel, "channel");
  validateSemver(minVersion, "min-version");
  if (values.length > MAX_ARTIFACTS) throw new Error("too many artifact inputs");
  publishedAt ??= Math.floor(Date.now() / 1000);
  expiresAt ??= publishedAt + DEFAULT_MANIFEST_TTL_SECONDS;
  if (!Number.isSafeInteger(expiresAt)) throw new Error("expires-at is outside the safe integer range");
  if (expiresAt <= publishedAt) throw new Error("expires-at must be after published-at");
  if (expiresAt - publishedAt > DEFAULT_MANIFEST_TTL_SECONDS) {
    throw new Error("expires-at exceeds the 30-day freshness window");
  }
  return {
    version,
    sequence,
    channel,
    minVersion,
    publishedAt,
    expiresAt,
    output: path.resolve(output),
    artifacts: values.map(([target, file]) => {
      validateComponent(target, "artifact target");
      return { target, file: path.resolve(file) };
    }),
  };
}

function parsePositiveInteger(value, label) {
  if (!/^[1-9][0-9]*$/.test(value)) throw new Error(label + " must be a positive integer");
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 1) {
    throw new Error(label + " is outside the safe integer range");
  }
  return parsed;
}

function validateSemver(value, label) {
  if (!/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/.test(value)) {
    throw new Error(label + " must be a SemVer major.minor.patch value");
  }
}

function validateComponent(value, label) {
  if (!value || value === "." || value === ".." || !/^[A-Za-z0-9._+-]+$/.test(value)) {
    throw new Error(label + " contains unsafe characters");
  }
}

function rejectSymlinkAncestors(target, label) {
  const absolute = path.resolve(target);
  const parsed = path.parse(absolute);
  let cursor = parsed.root;
  const components = path.relative(parsed.root, absolute).split(path.sep).filter(Boolean);
  for (let index = 0; index < components.length; index += 1) {
    cursor = path.join(cursor, components[index]);
    const stat = fs.lstatSync(cursor, { throwIfNoEntry: false });
    if (!stat) break;
    if (stat.isSymbolicLink()) throw new Error(label + " path traverses a symlink: " + cursor);
    if (index + 1 < components.length && !stat.isDirectory()) {
      throw new Error(label + " path has a non-directory ancestor: " + cursor);
    }
  }
}

function regular(file, label) {
  rejectSymlinkAncestors(file, label);
  const stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (!stat) throw new Error(label + " is missing: " + file);
  if (stat.isSymbolicLink() || !stat.isFile()) {
    throw new Error(label + " is not a regular file: " + file);
  }
  if (!Number.isSafeInteger(stat.size) || stat.size === 0) {
    throw new Error(label + " is empty");
  }
  if (stat.size > MAX_ARTIFACT_BYTES) {
    throw new Error(label + " exceeds the " + MAX_ARTIFACT_BYTES + " byte bound");
  }
  return stat;
}

function directory(dir, label) {
  const absolute = path.resolve(dir);
  const parsed = path.parse(absolute);
  let cursor = parsed.root;
  const components = path.relative(parsed.root, absolute).split(path.sep).filter(Boolean);
  for (const component of components) {
    cursor = path.join(cursor, component);
    let stat = fs.lstatSync(cursor, { throwIfNoEntry: false });
    if (!stat) {
      fs.mkdirSync(cursor);
      stat = fs.lstatSync(cursor);
    }
    if (stat.isSymbolicLink() || !stat.isDirectory()) {
      throw new Error(label + " is not a real directory: " + cursor);
    }
  }
}

function readBounded(file, stat, label, limit = MAX_ARTIFACT_BYTES) {
  const fd = fs.openSync(file, "r");
  const chunks = [];
  let total = 0;
  const buffer = Buffer.allocUnsafe(64 * 1024);
  try {
    while (true) {
      const count = fs.readSync(fd, buffer, 0, buffer.length, null);
      if (count === 0) break;
      total += count;
      if (total > limit) {
        throw new Error(label + " exceeds the " + limit + " byte bound");
      }
      chunks.push(Buffer.from(buffer.subarray(0, count)));
    }
  } finally {
    fs.closeSync(fd);
  }
  if (total !== stat.size) throw new Error(label + " changed while it was read");
  if (total === 0) throw new Error(label + " is empty");
  return Buffer.concat(chunks, total);
}

function syncDirectory(dir) {
  if (process.platform === "win32") return;
  const fd = fs.openSync(dir, "r");
  try { fs.fsyncSync(fd); } finally { fs.closeSync(fd); }
}

function writeAll(fd, bytes) {
  let offset = 0;
  while (offset < bytes.length) {
    const written = fs.writeSync(fd, bytes, offset, bytes.length - offset);
    if (written === 0) throw new Error("publication write made no progress");
    offset += written;
  }
}

function immutable(file, bytes) {
  rejectSymlinkAncestors(file, "publication");
  const stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (stat?.isSymbolicLink() || (stat && !stat.isFile())) {
    throw new Error("publication path is not a regular file: " + file);
  }
  if (stat) {
    if (!Number.isSafeInteger(stat.size) || stat.size !== bytes.length
      || !readBounded(file, stat, "existing publication file", bytes.length).equals(bytes)) {
      throw new Error("immutable publication file changed: " + file);
    }
    return;
  }
  directory(path.dirname(file), "publication parent");
  rejectSymlinkAncestors(file, "publication");
  const partial = file + ".partial-" + process.pid;
  rejectSymlinkAncestors(partial, "publication temporary file");
  const partialStat = fs.lstatSync(partial, { throwIfNoEntry: false });
  if (partialStat) throw new Error("publication temporary file is already present: " + partial);
  let created = false;
  try {
    const fd = fs.openSync(partial, "wx", 0o644);
    created = true;
    try {
      writeAll(fd, bytes);
      fs.fsyncSync(fd);
    } finally {
      fs.closeSync(fd);
    }
    fs.renameSync(partial, file);
    syncDirectory(path.dirname(file));
  } catch (error) {
    if (created) {
      try { fs.unlinkSync(partial); } catch {}
    }
    throw error;
  }
}

function main(argv) {
  const options = parseArgs(argv);
  directory(options.output, "publication output");
  const seen = new Set();
  const artifacts = options.artifacts
    .sort((left, right) => left.target < right.target ? -1 : left.target > right.target ? 1 : 0)
    .map(({ target, file }) => {
      if (seen.has(target)) throw new Error("duplicate artifact target " + target);
      seen.add(target);
      const stat = regular(file, "artifact input");
      const bytes = readBounded(file, stat, "artifact input");
      const relative = "v1/" + options.channel + "/" + options.version
        + "/jet-" + options.version + "-" + target;
      return {
        target,
        path: relative,
        sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
        size: bytes.length,
        signature: relative + ".sig.json",
        bytes,
      };
    });
  const manifest = {
    schema: 1,
    channel: options.channel,
    version: options.version,
    sequence: options.sequence,
    published_at: options.publishedAt,
    expires_at: options.expiresAt,
    min_version: options.minVersion,
    artifacts: artifacts.map(({ target, path: relative, sha256, size, signature }) => ({
      target,
      path: relative,
      sha256,
      size,
      signature,
    })),
  };
  const manifestBytes = Buffer.from(JSON.stringify(manifest) + "\n");
  if (manifestBytes.length > MAX_MANIFEST_BYTES) {
    throw new Error("manifest exceeds the " + MAX_MANIFEST_BYTES + " byte bound");
  }
  const manifestPath = path.join(options.output, "v1", options.channel, "manifest.json");
  immutable(manifestPath, manifestBytes);
  immutable(manifestPath + ".sig.request", Buffer.concat([
    Buffer.from("jet-toolchain-channel-v1\n"),
    manifestBytes,
  ]));
  for (const artifact of artifacts) {
    const artifactPath = path.join(options.output, artifact.path);
    immutable(artifactPath, artifact.bytes);
    immutable(artifactPath + ".sig.request", Buffer.concat([
      Buffer.from("jet-toolchain-artifact-v1\n"),
      artifact.bytes,
    ]));
  }
  console.log("prepared toolchain channel at " + options.output
    + "; external signatures and hosting remain pending");
}

try {
  main(process.argv.slice(2));
} catch (error) {
  console.error("stage-toolchain: " + error.message);
  process.exit(1);
}
