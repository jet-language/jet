#!/usr/bin/env node
// Stage and publish signed index/cache trees without following attacker-controlled paths.
// Publication is local filesystem work only; credentials and deployment stay external.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

const repo = path.resolve(path.dirname(new URL(import.meta.url).pathname), "../..");
const MAX_CHANNEL_BYTES = 64;
const MAX_COMPONENT_BYTES = 255;
const MAX_MANIFEST_BYTES = 16 * 1024 * 1024;
const MAX_SIGNATURE_BYTES = 16 * 1024;
const MAX_COMPRESSED_BYTES = 32 * 1024 * 1024;
const MAX_DECODED_BYTES = 256 * 1024 * 1024;
const MAX_CACHE_BYTES = 16 * 1024 * 1024 * 1024;
const MAX_EXECUTABLE_BYTES = 512 * 1024 * 1024;
const MAX_TARGETS = 4096;
const MAX_RECORDS = 400000;
const MAX_DIRECTORY_ENTRIES = 65536;
const IDENTIFIER = /^[A-Za-z0-9][A-Za-z0-9._+-]{0,63}$/;
const CHANNEL = /^[a-z0-9][a-z0-9._+-]{0,63}$/;
const DIGEST = /^[0-9a-f]{64}$/;
const REVISION = /^[0-9a-f]{40}$/;
const SYSTEMS = new Set([
  "x86_64-linux",
  "aarch64-linux",
  "x86_64-darwin",
  "aarch64-darwin",
]);
const O_NOFOLLOW = fs.constants.O_NOFOLLOW;
const O_DIRECTORY = fs.constants.O_DIRECTORY;
const O_NONBLOCK = fs.constants.O_NONBLOCK ?? 0;
const FD_ROOT = process.platform === "linux" ? "/proc/self/fd" : "/dev/fd";
let temporarySequence = 0;

function usage() {
  return [
    "usage:",
    "  stage-index-cache.mjs --index-root <producer-output> --hangar-root <jetpack-root> --output <dir>",
    "    [--channel <name>] [--role <role>] [--manifest <manifest.json>] [--jetpack <binary>]",
    "  stage-index-cache.mjs publish --staging <dir> --index-destination <dir> --cache-destination <dir>",
    "    [--channel <name>]",
  ].join("\n");
}

function parseArgs(argv) {
  let mode = "stage";
  let cursor = 0;
  if (argv[0] === "publish") {
    mode = "publish";
    cursor = 1;
  }
  const values = new Map();
  while (cursor < argv.length) {
    const name = argv[cursor++];
    if (name === "--publish") {
      if (mode !== "stage") throw new Error("duplicate publish mode");
      mode = "publish";
      continue;
    }
    if (!name.startsWith("--") || cursor >= argv.length || argv[cursor].startsWith("--")) {
      throw new Error(`${usage()}\ninvalid option near ${name}`);
    }
    if (values.has(name)) throw new Error(`duplicate option ${name}`);
    values.set(name, argv[cursor++]);
  }

  const channel = values.get("--channel") ?? "nixpkgs-unstable";
  validateChannel(channel);
  if (mode === "publish") {
    const staging = required(values, "--staging");
    const indexDestination = required(values, "--index-destination");
    const cacheDestination = required(values, "--cache-destination");
    const options = {
      mode,
      staging: absoluteDirectoryPath(staging, "staging"),
      indexDestination: absoluteDirectoryPath(indexDestination, "index destination"),
      cacheDestination: absoluteDirectoryPath(cacheDestination, "cache destination"),
      channel,
    };
    rejectOverlap(options.staging, options.indexDestination, "staging and index destination overlap");
    rejectOverlap(options.staging, options.cacheDestination, "staging and cache destination overlap");
    rejectOverlap(options.indexDestination, options.cacheDestination, "index and cache destinations overlap");
    rejectUnknown(values, ["--staging", "--index-destination", "--cache-destination", "--channel"]);
    return options;
  }

  const indexRoot = required(values, "--index-root");
  const hangarRoot = required(values, "--hangar-root");
  const output = required(values, "--output");
  const role = values.get("--role") ?? "public";
  validateIdentifier(role, "cache role");
  const manifestIsDefault = !values.has("--manifest");
  const options = {
    mode,
    indexRoot: absoluteDirectoryPath(indexRoot, "index root"),
    hangarRoot: absoluteDirectoryPath(hangarRoot, "Hangar root"),
    output: absoluteDirectoryPath(output, "output"),
    channel,
    role,
    manifest: values.has("--manifest")
      ? absoluteFilePath(values.get("--manifest"), "manifest")
      : path.join(path.resolve(indexRoot), "manifest.json"),
    manifestIsDefault,
    jetpack: values.get("--jetpack")
      ? absoluteFilePath(values.get("--jetpack"), "jetpack binary")
      : path.join(repo, "target/debug/jetpack"),
  };
  rejectOverlap(options.output, options.indexRoot, "output and index root overlap");
  rejectOverlap(options.output, options.hangarRoot, "output and Hangar root overlap");
  rejectUnknown(values, ["--index-root", "--hangar-root", "--output", "--channel", "--role", "--manifest", "--jetpack"]);
  return options;
}

function required(values, name) {
  if (!values.has(name)) throw new Error(`${name} is required\n${usage()}`);
  return values.get(name);
}

function rejectUnknown(values, allowed) {
  const known = new Set(allowed);
  for (const name of values.keys()) {
    if (!known.has(name)) throw new Error(`${usage()}\nunknown option ${name}`);
  }
}

function absoluteDirectoryPath(value, label) {
  if (!value || value.includes("\0")) throw new Error(`${label} is invalid`);
  const absolute = path.resolve(value);
  if (absolute === path.parse(absolute).root) throw new Error(`${label} must not be a filesystem root`);
  return absolute;
}

function absoluteFilePath(value, label) {
  if (!value || value.includes("\0")) throw new Error(`${label} is invalid`);
  return path.resolve(value);
}

function rejectOverlap(left, right, message) {
  const relative = path.relative(left, right);
  if (relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative))) {
    throw new Error(message);
  }
}

function validateIdentifier(value, label) {
  if (typeof value !== "string" || Buffer.byteLength(value) > MAX_CHANNEL_BYTES || !IDENTIFIER.test(value)) {
    throw new Error(`${label} must be a bounded ASCII identifier`);
  }
  return value;
}

function validateChannel(value) {
  if (typeof value !== "string" || Buffer.byteLength(value) > MAX_CHANNEL_BYTES || !CHANNEL.test(value)) {
    throw new Error("channel must be a bounded lowercase ASCII identifier");
  }
  return value;
}

function validateName(name, label = "path component") {
  if (
    typeof name !== "string"
    || name.length === 0
    || name === "."
    || name === ".."
    || name.includes("\0")
    || name.includes("/")
    || name.includes("\\")
    || Buffer.byteLength(name) > MAX_COMPONENT_BYTES
  ) {
    throw new Error(`${label} contains an unsafe path component`);
  }
  return name;
}

function ensureDescriptorFs() {
  if (process.platform === "win32" || O_NOFOLLOW === undefined || O_DIRECTORY === undefined) {
    throw new Error("secure publication requires POSIX no-follow directory descriptors");
  }
  try {
    if (!fs.statSync(FD_ROOT).isDirectory()) throw new Error("not a directory");
  } catch {
    throw new Error(`secure publication requires ${FD_ROOT}`);
  }
}

function descriptorPath(fd, name) {
  return name === undefined ? `${FD_ROOT}/${fd}` : path.join(`${FD_ROOT}/${fd}`, validateName(name));
}

function missing(error) {
  return error?.code === "ENOENT";
}

function close(fd) {
  if (fd !== undefined && fd !== null) fs.closeSync(fd);
}

function lstatAt(parentFd, name) {
  try {
    return fs.lstatSync(descriptorPath(parentFd, name), { bigint: true });
  } catch (error) {
    if (missing(error)) return null;
    throw error;
  }
}

function statIdentity(stat) {
  return `${stat.dev}:${stat.ino}`;
}

function sameIdentity(left, right) {
  return statIdentity(left) === statIdentity(right);
}

function statSnapshot(stat) {
  return [stat.dev, stat.ino, stat.nlink, stat.size, stat.mtimeNs, stat.ctimeNs].join(":");
}

function sameSnapshot(left, right) {
  return statSnapshot(left) === statSnapshot(right);
}

function checkRegular(stat, label, limit, nonempty = true) {
  if (!stat.isFile()) throw new Error(`${label} is not a regular file`);
  if (stat.nlink !== 1n) throw new Error(`${label} is hard-linked`);
  if (stat.size > BigInt(limit)) throw new Error(`${label} exceeds the ${limit} byte bound`);
  if (nonempty && stat.size === 0n) throw new Error(`${label} is empty`);
}

function openDirectoryAt(parentFd, name, label, create = false) {
  validateName(name);
  for (let attempt = 0; attempt < 2; attempt += 1) {
    let before = lstatAt(parentFd, name);
    if (!before && create) {
      try {
        fs.mkdirSync(descriptorPath(parentFd, name), { mode: 0o755 });
      } catch (error) {
        if (!missing(error) && error.code !== "EEXIST") throw error;
      }
      before = lstatAt(parentFd, name);
    }
    if (!before) throw new Error(`${label} is missing`);
    if (before.isSymbolicLink()) throw new Error(`${label} must not be a symlink`);
    if (!before.isDirectory()) throw new Error(`${label} is not a real directory`);
    let fd;
    try {
      fd = fs.openSync(
        descriptorPath(parentFd, name),
        fs.constants.O_RDONLY | O_DIRECTORY | O_NOFOLLOW,
      );
      const after = fs.fstatSync(fd, { bigint: true });
      if (!sameIdentity(before, after)) throw new Error(`${label} changed while it was opened`);
      if (after.isSymbolicLink() || !after.isDirectory()) throw new Error(`${label} is not a real directory`);
      return { fd, stat: after };
    } catch (error) {
      close(fd);
      if (error.code === "ENOENT" && create) continue;
      throw error;
    }
  }
  throw new Error(`${label} changed while it was opened`);
}

function openDirectoryPath(value, label, create = false) {
  const absolute = path.resolve(value);
  const rootFd = fs.openSync("/", fs.constants.O_RDONLY | O_DIRECTORY);
  let current = rootFd;
  try {
    const components = path.relative("/", absolute).split(path.sep).filter(Boolean);
    for (const component of components) {
      const next = openDirectoryAt(current, component, label, create);
      close(current);
      current = next.fd;
    }
    return { fd: current, path: absolute, stat: fs.fstatSync(current, { bigint: true }) };
  } catch (error) {
    close(current);
    throw new Error(`${label}: ${error.message}`);
  }
}

function openRegularAt(parentFd, name, label, limit, nonempty = true) {
  validateName(name);
  const before = lstatAt(parentFd, name);
  if (!before) throw new Error(`${label} is missing`);
  if (before.isSymbolicLink()) throw new Error(`${label} must not be a symlink`);
  checkRegular(before, label, limit, nonempty);
  let fd;
  try {
    fd = fs.openSync(
      descriptorPath(parentFd, name),
      fs.constants.O_RDONLY | O_NOFOLLOW | O_NONBLOCK,
    );
    const after = fs.fstatSync(fd, { bigint: true });
    checkRegular(after, label, limit, nonempty);
    if (!sameSnapshot(before, after)) throw new Error(`${label} changed while it was opened`);
    return { fd, stat: after };
  } catch (error) {
    close(fd);
    throw error;
  }
}

function openRegularPath(value, label, limit, nonempty = true) {
  const absolute = path.resolve(value);
  const parent = openDirectoryPath(path.dirname(absolute), `${label} parent`);
  try {
    return openRegularAt(parent.fd, path.basename(absolute), label, limit, nonempty);
  } finally {
    close(parent.fd);
  }
}

function readBounded(handle, limit, label) {
  if (handle.stat.size > BigInt(limit)) throw new Error(`${label} exceeds the ${limit} byte bound`);
  const chunks = [];
  const buffer = Buffer.allocUnsafe(64 * 1024);
  let total = 0n;
  while (true) {
    const count = fs.readSync(handle.fd, buffer, 0, buffer.length, null);
    if (count === 0) break;
    total += BigInt(count);
    if (total > BigInt(limit)) throw new Error(`${label} exceeds the ${limit} byte bound`);
    chunks.push(Buffer.from(buffer.subarray(0, count)));
  }
  const after = fs.fstatSync(handle.fd, { bigint: true });
  if (total !== handle.stat.size || !sameSnapshot(handle.stat, after)) {
    throw new Error(`${label} changed while it was read`);
  }
  if (total === 0n) throw new Error(`${label} is empty`);
  return Buffer.concat(chunks, Number(total));
}

function readRegularPath(value, label, limit) {
  const handle = openRegularPath(value, label, limit);
  try {
    return readBounded(handle, limit, label);
  } finally {
    close(handle.fd);
  }
}

function writeAll(fd, bytes) {
  let offset = 0;
  while (offset < bytes.length) {
    const written = fs.writeSync(fd, bytes, offset, bytes.length - offset, null);
    if (written === 0) throw new Error("publication write made no progress");
    offset += written;
  }
}

function syncDirectory(fd) {
  fs.fsyncSync(fd);
}

function temporaryName(prefix) {
  temporarySequence += 1;
  return `.${prefix}.partial-${process.pid}-${temporarySequence}-${crypto.randomBytes(6).toString("hex")}`;
}

function createPrivateDirectory(label) {
  const parent = openDirectoryPath(path.join(repo, ".tmp"), `${label} parent`, true);
  try {
    for (let attempt = 0; attempt < 8; attempt += 1) {
      const name = temporaryName("cache-stage");
      try {
        fs.mkdirSync(descriptorPath(parent.fd, name), { mode: 0o700 });
        const directory = openDirectoryAt(parent.fd, name, label);
        return {
          directory,
          name,
          parent,
          path: path.join(parent.path, name),
        };
      } catch (error) {
        if (error.code !== "EEXIST") throw error;
      }
    }
    throw new Error(`${label} could not allocate a unique directory`);
  } catch (error) {
    close(parent.fd);
    throw error;
  }
}

function removeTree(directory, label) {
  const { entries } = entriesAt(directory, label);
  for (const entry of entries) {
    const stat = lstatAt(directory.fd, entry.name);
    if (!stat) continue;
    if (stat.isDirectory()) {
      const child = openDirectoryAt(directory.fd, entry.name, `${label}/${entry.name}`);
      try {
        removeTree(child, `${label}/${entry.name}`);
      } finally {
        close(child.fd);
      }
      const current = lstatAt(directory.fd, entry.name);
      if (current && sameIdentity(current, stat)) fs.rmdirSync(descriptorPath(directory.fd, entry.name));
    } else {
      fs.unlinkSync(descriptorPath(directory.fd, entry.name));
    }
  }
  syncDirectory(directory.fd);
}

function removePrivateDirectory(scratch) {
  try {
    removeTree(scratch.directory, "cache stage scratch");
    close(scratch.directory.fd);
    scratch.directory.fd = undefined;
    const current = lstatAt(scratch.parent.fd, scratch.name);
    if (current && sameIdentity(current, scratch.directory.stat)) {
      fs.rmdirSync(descriptorPath(scratch.parent.fd, scratch.name));
      syncDirectory(scratch.parent.fd);
    }
  } finally {
    close(scratch.directory.fd);
    close(scratch.parent.fd);
  }
}

function createFileAt(parentFd, name, label) {
  validateName(name);
  return fs.openSync(
    descriptorPath(parentFd, name),
    fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL | O_NOFOLLOW,
    0o644,
  );
}

function removeOwnedFile(parentFd, name, identity) {
  const stat = lstatAt(parentFd, name);
  if (!stat || (identity && !sameIdentity(stat, identity))) return;
  if (!stat.isFile() && !stat.isSymbolicLink()) return;
  fs.unlinkSync(descriptorPath(parentFd, name));
}

function compareFiles(source, destination, limit, label) {
  if (source.stat.size !== destination.stat.size) {
    throw new Error(`${label} immutable bytes changed`);
  }
  const buffer = Buffer.allocUnsafe(64 * 1024);
  const other = Buffer.allocUnsafe(64 * 1024);
  let total = 0n;
  while (total < source.stat.size) {
    const wanted = Number((source.stat.size - total) < BigInt(buffer.length)
      ? source.stat.size - total
      : BigInt(buffer.length));
    const count = fs.readSync(source.fd, buffer, 0, wanted, null);
    if (count === 0) throw new Error(`${label} source changed while it was read`);
    let compared = 0;
    while (compared < count) {
      const read = fs.readSync(destination.fd, other, compared, count - compared, null);
      if (read === 0) throw new Error(`${label} destination changed while it was read`);
      if (!buffer.subarray(compared, compared + read).equals(other.subarray(compared, compared + read))) {
        throw new Error(`${label} immutable bytes changed`);
      }
      compared += read;
    }
    total += BigInt(count);
    if (total > BigInt(limit)) throw new Error(`${label} exceeds the ${limit} byte bound`);
  }
  const sourceAfter = fs.fstatSync(source.fd, { bigint: true });
  const destinationAfter = fs.fstatSync(destination.fd, { bigint: true });
  if (!sameSnapshot(source.stat, sourceAfter)) throw new Error(`${label} source changed while it was read`);
  if (!sameSnapshot(destination.stat, destinationAfter)) throw new Error(`${label} destination changed while it was read`);
}

function copyOpenFile(source, destinationParentFd, name, limit, label) {
  const existing = lstatAt(destinationParentFd, name);
  if (existing) {
    if (existing.isSymbolicLink()) throw new Error(`${label} destination must not be a symlink`);
    checkRegular(existing, label, limit);
    const destination = openRegularAt(destinationParentFd, name, label, limit);
    try {
      compareFiles(source, destination, limit, label);
    } finally {
      close(destination.fd);
    }
    return;
  }

  const temporary = temporaryName(name);
  let temporaryFd;
  let temporaryIdentity;
  try {
    temporaryFd = createFileAt(destinationParentFd, temporary, `${label} temporary file`);
    const buffer = Buffer.allocUnsafe(64 * 1024);
    let total = 0n;
    while (total < source.stat.size) {
      const wanted = Number((source.stat.size - total) < BigInt(buffer.length)
        ? source.stat.size - total
        : BigInt(buffer.length));
      const count = fs.readSync(source.fd, buffer, 0, wanted, null);
      if (count === 0) throw new Error(`${label} source changed while it was read`);
      writeAll(temporaryFd, buffer.subarray(0, count));
      total += BigInt(count);
      if (total > BigInt(limit)) throw new Error(`${label} exceeds the ${limit} byte bound`);
    }
    const sourceAfter = fs.fstatSync(source.fd, { bigint: true });
    if (total !== source.stat.size || !sameSnapshot(source.stat, sourceAfter)) {
      throw new Error(`${label} source changed while it was read`);
    }
    fs.fsyncSync(temporaryFd);
    temporaryIdentity = fs.fstatSync(temporaryFd, { bigint: true });
    close(temporaryFd);
    temporaryFd = undefined;
    try {
      fs.linkSync(
        descriptorPath(destinationParentFd, temporary),
        descriptorPath(destinationParentFd, name),
      );
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      const temporaryFile = openRegularAt(destinationParentFd, temporary, `${label} temporary file`, limit);
      const destination = openRegularAt(destinationParentFd, name, label, limit);
      try {
        compareFiles(temporaryFile, destination, limit, label);
      } finally {
        close(temporaryFile.fd);
        close(destination.fd);
      }
      removeOwnedFile(destinationParentFd, temporary, temporaryIdentity);
      return;
    }
    removeOwnedFile(destinationParentFd, temporary, temporaryIdentity);
    syncDirectory(destinationParentFd);
  } catch (error) {
    close(temporaryFd);
    if (temporaryIdentity) {
      try { removeOwnedFile(destinationParentFd, temporary, temporaryIdentity); } catch {}
    }
    throw error;
  }
}

function writeImmutableBytes(destinationParentFd, name, bytes, limit, label) {
  if (bytes.length === 0) throw new Error(`${label} is empty`);
  if (bytes.length > limit) throw new Error(`${label} exceeds the ${limit} byte bound`);
  const existing = lstatAt(destinationParentFd, name);
  if (existing) {
    if (existing.isSymbolicLink()) throw new Error(`${label} destination must not be a symlink`);
    checkRegular(existing, label, limit);
    const current = openRegularAt(destinationParentFd, name, label, limit);
    try {
      const currentBytes = readBounded(current, limit, label);
      if (!currentBytes.equals(bytes)) throw new Error(`${label} immutable bytes changed`);
    } finally {
      close(current.fd);
    }
    return;
  }

  const temporary = temporaryName(name);
  let temporaryFd;
  let temporaryIdentity;
  try {
    temporaryFd = createFileAt(destinationParentFd, temporary, `${label} temporary file`);
    writeAll(temporaryFd, bytes);
    fs.fsyncSync(temporaryFd);
    temporaryIdentity = fs.fstatSync(temporaryFd, { bigint: true });
    close(temporaryFd);
    temporaryFd = undefined;
    try {
      fs.linkSync(
        descriptorPath(destinationParentFd, temporary),
        descriptorPath(destinationParentFd, name),
      );
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      removeOwnedFile(destinationParentFd, temporary, temporaryIdentity);
      const current = openRegularAt(destinationParentFd, name, label, limit);
      try {
        const currentBytes = readBounded(current, limit, label);
        if (!currentBytes.equals(bytes)) throw new Error(`${label} immutable bytes changed`);
      } finally {
        close(current.fd);
      }
      return;
    }
    removeOwnedFile(destinationParentFd, temporary, temporaryIdentity);
    syncDirectory(destinationParentFd);
  } catch (error) {
    close(temporaryFd);
    if (temporaryIdentity) {
      try { removeOwnedFile(destinationParentFd, temporary, temporaryIdentity); } catch {}
    }
    throw error;
  }
}

function entriesAt(directory, label) {
  const before = fs.fstatSync(directory.fd, { bigint: true });
  const entries = fs.readdirSync(descriptorPath(directory.fd), { withFileTypes: true })
    .sort((left, right) => left.name.localeCompare(right.name));
  if (entries.length > MAX_DIRECTORY_ENTRIES) throw new Error(`${label} has too many entries`);
  const after = fs.fstatSync(directory.fd, { bigint: true });
  if (!sameSnapshot(before, after)) throw new Error(`${label} changed while it was listed`);
  for (const entry of entries) validateName(entry.name, label);
  return { entries, snapshot: before };
}

function validateAuxiliaryIndexFile(parentFd, name) {
  const handle = openRegularAt(parentFd, name, `index source ${name}`, MAX_DECODED_BYTES);
  close(handle.fd);
}

function copyIndexTree(source, destination, label = "index target root", relative = [], manifestChannel = null) {
  const { entries, snapshot } = entriesAt(source, label);
  const names = new Set(entries.map((entry) => entry.name));
  const manifestDirectory = relative.length === 2
    && relative[0] === "v1"
    && relative[1] === manifestChannel;
  if (manifestDirectory && (!names.has("manifest.json") || !names.has("manifest.json.sig.json"))) {
    throw new Error("index manifest signature has no manifest");
  }
  for (const entry of entries) {
    const name = entry.name;
    if (name.endsWith(".json.zst.sig.json") && !names.has(name.slice(0, -".sig.json".length))) {
      throw new Error(`index target signature has no target: ${name}`);
    }
  }
  for (const entry of entries) {
    const name = entry.name;
    if (entry.isDirectory()) {
      const childSource = openDirectoryAt(source.fd, name, `${label}/${name}`);
      const childDestination = openDirectoryAt(destination.fd, name, `${label}/${name} destination`, true);
      try {
        copyIndexTree(childSource, childDestination, `${label}/${name}`, [...relative, name], manifestChannel);
      } finally {
        close(childSource.fd);
        close(childDestination.fd);
      }
      continue;
    }
    if (manifestDirectory && (name === "manifest.json" || name === "manifest.json.sig.json")) {
      const manifest = openRegularAt(
        source.fd,
        name,
        `index manifest ${name}`,
        name === "manifest.json" ? MAX_MANIFEST_BYTES : MAX_SIGNATURE_BYTES,
      );
      try {
        copyOpenFile(
          manifest,
          destination.fd,
          name,
          name === "manifest.json" ? MAX_MANIFEST_BYTES : MAX_SIGNATURE_BYTES,
          `index manifest ${name}`,
        );
      } finally {
        close(manifest.fd);
      }
      continue;
    }
    if (name.endsWith(".json.zst")) {
      if (!/^([0-9a-f]{64})\.json\.zst$/.test(name)) {
        throw new Error(`index target filename is not a lowercase SHA-256 object: ${name}`);
      }
      const target = openRegularAt(source.fd, name, `index target ${name}`, MAX_COMPRESSED_BYTES);
      try {
        copyOpenFile(target, destination.fd, name, MAX_COMPRESSED_BYTES, `index target ${name}`);
      } finally {
        close(target.fd);
      }
      const signatureName = `${name}.sig.json`;
      const signature = openRegularAt(source.fd, signatureName, `index target signature ${signatureName}`, MAX_SIGNATURE_BYTES);
      try {
        copyOpenFile(signature, destination.fd, signatureName, MAX_SIGNATURE_BYTES, `index target signature ${signatureName}`);
      } finally {
        close(signature.fd);
      }
      continue;
    }
    if (name.endsWith(".json.zst.sig.json")) continue;
    if (name.endsWith(".json.zst.sig.request") || name.endsWith(".json.zst.generation-report.json")) {
      validateAuxiliaryIndexFile(source.fd, name);
      continue;
    }
    throw new Error(`unexpected index source file: ${name}`);
  }
  if (manifestDirectory && (!names.has("manifest.json") || !names.has("manifest.json.sig.json"))) {
    throw new Error("index manifest is missing");
  }
  const after = fs.fstatSync(source.fd, { bigint: true });
  if (!sameSnapshot(snapshot, after)) throw new Error(`${label} changed during publication`);
}

function copyCacheTree(source, destination, label = "cache source") {
  const { entries, snapshot } = entriesAt(source, label);
  for (const entry of entries) {
    const name = entry.name;
    if (name.startsWith(".") || name.includes(".partial-")) {
      throw new Error(`cache source contains transient file: ${name}`);
    }
    if (entry.isDirectory()) {
      const childSource = openDirectoryAt(source.fd, name, `${label}/${name}`);
      const childDestination = openDirectoryAt(destination.fd, name, `${label}/${name} destination`, true);
      try {
        copyCacheTree(childSource, childDestination, `${label}/${name}`);
      } finally {
        close(childSource.fd);
        close(childDestination.fd);
      }
      continue;
    }
    const file = openRegularAt(source.fd, name, `cache object ${name}`, MAX_CACHE_BYTES);
    try {
      copyOpenFile(file, destination.fd, name, MAX_CACHE_BYTES, `cache object ${name}`);
    } finally {
      close(file.fd);
    }
  }
  const after = fs.fstatSync(source.fd, { bigint: true });
  if (!sameSnapshot(snapshot, after)) throw new Error(`${label} changed during publication`);
}

function validateCacheTree(source, label = "cache source") {
  const { entries, snapshot } = entriesAt(source, label);
  for (const entry of entries) {
    const name = entry.name;
    if (name.startsWith(".") || name.includes(".partial-")) {
      throw new Error(`cache source contains transient file: ${name}`);
    }
    if (entry.isDirectory()) {
      const child = openDirectoryAt(source.fd, name, `${label}/${name}`);
      try { validateCacheTree(child, `${label}/${name}`); } finally { close(child.fd); }
    } else {
      const file = openRegularAt(source.fd, name, `cache object ${name}`, MAX_CACHE_BYTES);
      close(file.fd);
    }
  }
  const after = fs.fstatSync(source.fd, { bigint: true });
  if (!sameSnapshot(snapshot, after)) throw new Error(`${label} changed while it was checked`);
}

function validateIndexTargetTree(source, label = "index source", relative = [], manifestChannel = null) {
  const { entries, snapshot } = entriesAt(source, label);
  const names = new Set(entries.map((entry) => entry.name));
  const manifestDirectory = relative.length === 2
    && relative[0] === "v1"
    && relative[1] === manifestChannel;
  if (manifestDirectory && (!names.has("manifest.json") || !names.has("manifest.json.sig.json"))) {
    throw new Error("index manifest signature has no manifest");
  }
  for (const entry of entries) {
    const name = entry.name;
    if (entry.isDirectory()) {
      const child = openDirectoryAt(source.fd, name, `${label}/${name}`);
      try {
        validateIndexTargetTree(child, `${label}/${name}`, [...relative, name], manifestChannel);
      } finally {
        close(child.fd);
      }
    } else if (manifestDirectory && (name === "manifest.json" || name === "manifest.json.sig.json")) {
      const manifest = openRegularAt(
        source.fd,
        name,
        `index manifest ${name}`,
        name === "manifest.json" ? MAX_MANIFEST_BYTES : MAX_SIGNATURE_BYTES,
      );
      close(manifest.fd);
    } else if (name.endsWith(".json.zst")) {
      if (!/^([0-9a-f]{64})\.json\.zst$/.test(name)) {
        throw new Error(`index target filename is not a lowercase SHA-256 object: ${name}`);
      }
      const target = openRegularAt(source.fd, name, `index target ${name}`, MAX_COMPRESSED_BYTES);
      close(target.fd);
      const signature = openRegularAt(source.fd, `${name}.sig.json`, `index target signature ${name}.sig.json`, MAX_SIGNATURE_BYTES);
      close(signature.fd);
    } else if (name.endsWith(".json.zst.sig.json")) {
      if (!names.has(name.slice(0, -".sig.json".length))) throw new Error(`index target signature has no target: ${name}`);
    } else if (name.endsWith(".json.zst.sig.request") || name.endsWith(".json.zst.generation-report.json")) {
      validateAuxiliaryIndexFile(source.fd, name);
    } else {
      throw new Error(`unexpected index source file: ${name}`);
    }
  }
  if (manifestDirectory && (!names.has("manifest.json") || !names.has("manifest.json.sig.json"))) {
    throw new Error("index manifest is missing");
  }
  const after = fs.fstatSync(source.fd, { bigint: true });
  if (!sameSnapshot(snapshot, after)) throw new Error(`${label} changed while it was checked`);
}

function compareTree(source, destination, fileLimit, label, indexTree = false) {
  const sourceListed = entriesAt(source, label);
  const destinationListed = entriesAt(destination, `${label} destination`);
  const sourceNames = new Set(sourceListed.entries.map((entry) => entry.name));
  const destinationNames = new Set(destinationListed.entries.map((entry) => entry.name));
  for (const name of sourceNames) {
    if (!destinationNames.has(name)) throw new Error(`${label} immutable generation is missing ${name}`);
  }
  for (const name of destinationNames) {
    if (!sourceNames.has(name)) throw new Error(`${label} immutable generation has unexpected ${name}`);
  }
  for (const entry of sourceListed.entries) {
    const name = entry.name;
    if (entry.isDirectory()) {
      const sourceChild = openDirectoryAt(source.fd, name, `${label}/${name}`);
      const destinationChild = openDirectoryAt(destination.fd, name, `${label}/${name} destination`);
      try {
        compareTree(sourceChild, destinationChild, fileLimit, `${label}/${name}`, indexTree);
      } finally {
        close(sourceChild.fd);
        close(destinationChild.fd);
      }
    } else {
      if (indexTree && name.endsWith(".json.zst")) {
        if (!/^([0-9a-f]{64})\.json\.zst$/.test(name)) throw new Error(`invalid immutable index target ${name}`);
      }
      const sourceFile = openRegularAt(source.fd, name, `${label}/${name}`, fileLimit(name));
      const destinationFile = openRegularAt(destination.fd, name, `${label}/${name} destination`, fileLimit(name));
      try {
        compareFiles(sourceFile, destinationFile, fileLimit(name), `${label}/${name}`);
      } finally {
        close(sourceFile.fd);
        close(destinationFile.fd);
      }
    }
  }
  const sourceAfter = fs.fstatSync(source.fd, { bigint: true });
  if (!sameSnapshot(sourceListed.snapshot, sourceAfter)) throw new Error(`${label} changed while it was checked`);
}

function indexFileLimit(name) {
  if (name === "manifest.json") return MAX_MANIFEST_BYTES;
  if (name.endsWith(".json.zst")) return MAX_COMPRESSED_BYTES;
  if (name.endsWith(".sig.json")) return MAX_SIGNATURE_BYTES;
  return MAX_DECODED_BYTES;
}

function cacheFileLimit() {
  return MAX_CACHE_BYTES;
}

function positiveSafeInteger(value, label) {
  if (!Number.isSafeInteger(value) || value < 1) throw new Error(`${label} must be a positive safe integer`);
  return value;
}

function nonnegativeSafeInteger(value, label) {
  if (!Number.isSafeInteger(value) || value < 0) throw new Error(`${label} must be a nonnegative safe integer`);
  return value;
}

function validateUrl(value, label, expectedSuffix) {
  if (typeof value !== "string" || value.length > 4096 || /[\u0000-\u0020\u007f]/.test(value)) {
    throw new Error(`${label} is malformed`);
  }
  let parsed;
  try { parsed = new URL(value); } catch { throw new Error(`${label} is malformed`); }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") throw new Error(`${label} must use HTTP or HTTPS`);
  if (parsed.search || parsed.hash || parsed.pathname !== expectedSuffix) {
    throw new Error(`${label} does not name its canonical target`);
  }
}

function requireExactFields(value, fields, label) {
  const expected = [...fields].sort();
  const actual = Object.keys(value).sort();
  if (actual.length !== expected.length || actual.some((field, index) => field !== expected[index])) {
    throw new Error(`${label} has an unexpected or missing field`);
  }
}

function canonicalManifestBytes(manifest) {
  const targets = [...manifest.targets].sort((left, right) => {
    const revision = left.revision < right.revision ? -1 : left.revision > right.revision ? 1 : 0;
    if (revision !== 0) return revision;
    return left.system < right.system ? -1 : left.system > right.system ? 1 : 0;
  });
  return Buffer.from(JSON.stringify({
    schema: manifest.schema,
    channel: manifest.channel,
    generation: manifest.generation,
    issued_unix: manifest.issued_unix,
    expires_unix: manifest.expires_unix,
    targets: targets.map((target) => ({
      revision: target.revision,
      system: target.system,
      url: target.url,
      signature_url: target.signature_url,
      sha256: target.sha256,
      compressed_length: target.compressed_length,
      decoded_length: target.decoded_length,
      record_count: target.record_count,
      index_signature_sha256: target.index_signature_sha256,
      discoverable: target.discoverable,
    })),
  }));
}

function validateSignature(bytes, label) {
  let signature;
  try { signature = JSON.parse(bytes.toString("utf8")); } catch { throw new Error(`${label} is not valid JSON`); }
  if (!signature || Array.isArray(signature) || typeof signature !== "object") {
    throw new Error(`${label} must be an object`);
  }
  requireExactFields(signature, ["schema", "key_id", "algorithm", "signature"], label);
  if (signature.schema !== 1 || typeof signature.key_id !== "string" || signature.key_id.length === 0
      || signature.algorithm !== "ed25519" || typeof signature.signature !== "string"
      || signature.signature.length === 0) {
    throw new Error(`${label} has an unsupported signature shape`);
  }
  if (!Buffer.from(JSON.stringify(signature)).equals(bytes)) {
    throw new Error(`${label} bytes are not canonical`);
  }
}

function validateManifest(bytes, channel) {
  let manifest;
  try { manifest = JSON.parse(bytes.toString("utf8")); } catch { throw new Error("index manifest is not valid JSON"); }
  if (!manifest || Array.isArray(manifest) || typeof manifest !== "object") {
    throw new Error("index manifest must be an object");
  }
  requireExactFields(manifest, ["schema", "channel", "generation", "issued_unix", "expires_unix", "targets"], "index manifest");
  if (manifest.schema !== 1) throw new Error("index manifest schema is unsupported");
  if (manifest.channel !== channel) throw new Error("index manifest channel disagrees with --channel");
  positiveSafeInteger(manifest.generation, "index manifest generation");
  nonnegativeSafeInteger(manifest.issued_unix, "index manifest issued-unix");
  nonnegativeSafeInteger(manifest.expires_unix, "index manifest expires-unix");
  if (manifest.expires_unix <= manifest.issued_unix) throw new Error("index manifest expiry is not after issue time");
  if (!Array.isArray(manifest.targets) || manifest.targets.length > MAX_TARGETS) {
    throw new Error("index manifest targets are outside their bound");
  }
  const seen = new Set();
  for (const target of manifest.targets) {
    if (!target || typeof target !== "object" || Array.isArray(target)) throw new Error("index manifest target is malformed");
    requireExactFields(target, [
      "revision",
      "system",
      "url",
      "signature_url",
      "sha256",
      "compressed_length",
      "decoded_length",
      "record_count",
      "index_signature_sha256",
      "discoverable",
    ], "index manifest target");
    if (typeof target.revision !== "string" || !REVISION.test(target.revision)) throw new Error("index manifest revision is malformed");
    if (typeof target.system !== "string" || !SYSTEMS.has(target.system)) throw new Error("index manifest system is malformed");
    if (typeof target.sha256 !== "string" || !DIGEST.test(target.sha256)) throw new Error("index manifest target digest is malformed");
    if (typeof target.index_signature_sha256 !== "string" || !DIGEST.test(target.index_signature_sha256)) {
      throw new Error("index manifest signature digest is malformed");
    }
    positiveSafeInteger(target.compressed_length, "index target compressed length");
    positiveSafeInteger(target.decoded_length, "index target decoded length");
    if (target.compressed_length > MAX_COMPRESSED_BYTES || target.decoded_length > MAX_DECODED_BYTES) {
      throw new Error("index manifest target exceeds its bound");
    }
    nonnegativeSafeInteger(target.record_count, "index target record count");
    if (target.record_count > MAX_RECORDS) throw new Error("index target record count exceeds its bound");
    if (typeof target.discoverable !== "boolean") throw new Error("index manifest discoverable flag is malformed");
    const key = `${target.revision}/${target.system}`;
    if (seen.has(key)) throw new Error("index manifest has duplicate targets");
    seen.add(key);
    const suffix = `/index-v1/${target.revision}/${target.system}/${target.sha256}.json.zst`;
    validateUrl(target.url, "index target URL", suffix);
    if (target.signature_url !== `${target.url}.sig.json`) throw new Error("index target signature URL is not canonical");
  }
  if (!canonicalManifestBytes(manifest).equals(bytes)) {
    throw new Error("index manifest bytes are not canonical");
  }
  return { bytes, manifest };
}

function validateManifestTargets(targetRoot, manifestInfo) {
  for (const target of manifestInfo.manifest.targets) {
    const revision = openDirectoryAt(targetRoot.fd, target.revision, "index target revision");
    const system = openDirectoryAt(revision.fd, target.system, "index target system");
    try {
      const targetName = `${target.sha256}.json.zst`;
      const targetFile = openRegularAt(system.fd, targetName, `index target ${targetName}`, MAX_COMPRESSED_BYTES);
      try {
        if (targetFile.stat.size !== BigInt(target.compressed_length)) throw new Error(`index target ${targetName} length disagrees with manifest`);
        const targetBytes = readBounded(targetFile, MAX_COMPRESSED_BYTES, `index target ${targetName}`);
        if (crypto.createHash("sha256").update(targetBytes).digest("hex") !== target.sha256) {
          throw new Error(`index target ${targetName} digest disagrees with manifest`);
        }
      } finally {
        close(targetFile.fd);
      }
      const signatureName = `${targetName}.sig.json`;
      const signatureFile = openRegularAt(system.fd, signatureName, `index target signature ${signatureName}`, MAX_SIGNATURE_BYTES);
      try {
        const signatureBytes = readBounded(signatureFile, MAX_SIGNATURE_BYTES, `index target signature ${signatureName}`);
        validateSignature(signatureBytes, `index target signature ${signatureName}`);
        if (crypto.createHash("sha256").update(signatureBytes).digest("hex") !== target.index_signature_sha256) {
          throw new Error(`index target signature ${signatureName} digest disagrees with manifest`);
        }
      } finally {
        close(signatureFile.fd);
      }
    } finally {
      close(system.fd);
      close(revision.fd);
    }
  }
}

function readManifestFromDirectories(indexRoot, channel) {
  const v1 = openDirectoryAt(indexRoot.fd, "v1", "staged index v1");
  const channelDirectory = openDirectoryAt(v1.fd, channel, "staged index channel");
  try {
    const manifest = openRegularAt(channelDirectory.fd, "manifest.json", "index manifest", MAX_MANIFEST_BYTES);
    const signature = openRegularAt(channelDirectory.fd, "manifest.json.sig.json", "index manifest signature", MAX_SIGNATURE_BYTES);
    try {
      const bytes = readBounded(manifest, MAX_MANIFEST_BYTES, "index manifest");
      const signatureBytes = readBounded(signature, MAX_SIGNATURE_BYTES, "index manifest signature");
      validateSignature(signatureBytes, "index manifest signature");
      return { ...validateManifest(bytes, channel), signatureBytes };
    } finally {
      close(manifest.fd);
      close(signature.fd);
    }
  } finally {
    close(channelDirectory.fd);
    close(v1.fd);
  }
}

function readManifestFromPath(manifestPath, channel) {
  const bytes = readRegularPath(manifestPath, "index manifest", MAX_MANIFEST_BYTES);
  const signatureBytes = readRegularPath(`${manifestPath}.sig.json`, "index manifest signature", MAX_SIGNATURE_BYTES);
  validateSignature(signatureBytes, "index manifest signature");
  return { ...validateManifest(bytes, channel), signatureBytes };
}

function readManifestFromIndexRoot(indexRoot, channel) {
  const manifest = openRegularAt(indexRoot.fd, "manifest.json", "index manifest", MAX_MANIFEST_BYTES);
  const signature = openRegularAt(indexRoot.fd, "manifest.json.sig.json", "index manifest signature", MAX_SIGNATURE_BYTES);
  try {
    const bytes = readBounded(manifest, MAX_MANIFEST_BYTES, "index manifest");
    const signatureBytes = readBounded(signature, MAX_SIGNATURE_BYTES, "index manifest signature");
    validateSignature(signatureBytes, "index manifest signature");
    return { ...validateManifest(bytes, channel), signatureBytes };
  } finally {
    close(manifest.fd);
    close(signature.fd);
  }
}

function validateStagedIndex(indexRoot, manifestInfo, label) {
  const current = readManifestFromDirectories(indexRoot, manifestInfo.manifest.channel);
  if (!current.bytes.equals(manifestInfo.bytes)) {
    throw new Error(`${label} manifest changed after it was read`);
  }
  if (!current.signatureBytes.equals(manifestInfo.signatureBytes)) {
    throw new Error(`${label} manifest signature changed after it was read`);
  }
  const targets = openDirectoryAt(indexRoot.fd, "index-v1", `${label} target root`);
  try {
    validateManifestTargets(targets, manifestInfo);
  } finally {
    close(targets.fd);
  }
}

function manifestGenerationId(manifestInfo) {
  const digest = crypto.createHash("sha256").update(manifestInfo.bytes).digest("hex");
  return `g${manifestInfo.manifest.generation}-${digest}`;
}

function parseGenerationId(name) {
  const match = /^g([1-9][0-9]*)-([0-9a-f]{64})$/.exec(name);
  if (!match) return null;
  const generation = Number(match[1]);
  if (!Number.isSafeInteger(generation)) throw new Error(`generation identifier is outside the safe range: ${name}`);
  return { id: name, generation, digest: match[2] };
}

function inspectGenerationHistory(generations, generationId, generation) {
  const { entries } = entriesAt(generations, "publication generations");
  let highest = 0;
  for (const entry of entries) {
    if (entry.name.startsWith(".")) {
      if (entry.isSymbolicLink() || (!entry.isDirectory() && !entry.isFile())) {
        throw new Error(`publication generation entry is not safe: ${entry.name}`);
      }
      continue;
    }
    const parsed = parseGenerationId(entry.name);
    if (!parsed) throw new Error(`publication generations contains an unexpected entry: ${entry.name}`);
    if (!entry.isDirectory()) throw new Error(`publication generation is not a directory: ${entry.name}`);
    highest = Math.max(highest, parsed.generation);
    if (parsed.generation === generation && parsed.id !== generationId) {
      throw new Error("publication refuses an equal-generation fork");
    }
  }
  if (generation < highest) throw new Error("publication refuses a generation rollback");
}

function currentGeneration(root, generations) {
  const current = lstatAt(root.fd, "current");
  if (!current) return null;
  if (!current.isSymbolicLink()) throw new Error("publication current pointer must be a symlink");
  const target = fs.readlinkSync(descriptorPath(root.fd, "current"));
  const match = /^generations\/(g[1-9][0-9]*-[0-9a-f]{64})$/.exec(target);
  if (!match) throw new Error("publication current pointer has an unsafe target");
  const parsed = parseGenerationId(match[1]);
  const generation = openDirectoryAt(generations.fd, parsed.id, "current generation");
  close(generation.fd);
  return parsed;
}

function acquirePublicationLock(root) {
  const name = ".publish.lock";
  let fd;
  try {
    fd = fs.openSync(
      descriptorPath(root.fd, name),
      fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL | O_NOFOLLOW,
      0o600,
    );
    writeAll(fd, Buffer.from(`${process.pid}\n`));
    fs.fsyncSync(fd);
    return { fd, stat: fs.fstatSync(fd, { bigint: true }), name };
  } catch (error) {
    close(fd);
    if (error.code === "EEXIST") throw new Error("publication root is busy");
    throw error;
  }
}

function releasePublicationLock(root, lock) {
  close(lock.fd);
  const current = lstatAt(root.fd, lock.name);
  if (current && sameIdentity(current, lock.stat)) {
    fs.unlinkSync(descriptorPath(root.fd, lock.name));
    syncDirectory(root.fd);
  }
}

function syncTree(directory) {
  const { entries } = entriesAt(directory, "publication generation");
  for (const entry of entries) {
    if (entry.isDirectory()) {
      const child = openDirectoryAt(directory.fd, entry.name, "publication generation directory");
      try { syncTree(child); } finally { close(child.fd); }
    }
  }
  syncDirectory(directory.fd);
}

function advanceCurrent(root, generations, generationId, generation) {
  const existing = currentGeneration(root, generations);
  if (existing && existing.id === generationId) return;
  if (existing && existing.generation >= generation) {
    throw new Error("publication current pointer would move backward");
  }
  const temporary = temporaryName("current");
  try {
    fs.symlinkSync(`generations/${generationId}`, descriptorPath(root.fd, temporary), "dir");
    fs.renameSync(descriptorPath(root.fd, temporary), descriptorPath(root.fd, "current"));
    syncDirectory(root.fd);
  } catch (error) {
    try { fs.unlinkSync(descriptorPath(root.fd, temporary)); } catch {}
    throw error;
  }
}

function publishEndpoint(source, destinationPath, generationInfo, manifestInfo, kind) {
  const root = openDirectoryPath(destinationPath, `${kind} publication root`, true);
  let lock;
  try {
    lock = acquirePublicationLock(root);
    const generations = openDirectoryAt(root.fd, "generations", `${kind} generations`, true);
    try {
      inspectGenerationHistory(generations, generationInfo.id, generationInfo.generation);
      const existing = lstatAt(generations.fd, generationInfo.id);
      if (existing) {
        if (existing.isSymbolicLink() || !existing.isDirectory()) {
          throw new Error(`${kind} immutable generation is not a real directory`);
        }
        const finalGeneration = openDirectoryAt(generations.fd, generationInfo.id, `${kind} immutable generation`);
        try {
          if (kind === "index") {
            validateStagedIndex(source, manifestInfo, "staged index");
            validateIndexTargetTree(source, "staged index", [], generationInfo.channel);
            compareTree(source, finalGeneration, indexFileLimit, "index immutable generation", true);
          } else {
            validateCacheTree(source, "staged cache");
            compareTree(source, finalGeneration, cacheFileLimit, "cache immutable generation");
          }
        } finally {
          close(finalGeneration.fd);
        }
      } else {
        const temporary = temporaryName(`generation-${generationInfo.generation}`);
        const partial = openDirectoryAt(generations.fd, temporary, `${kind} partial generation`, true);
        try {
          if (kind === "index") copyIndexTree(source, partial, "staged index", [], generationInfo.channel);
          else copyCacheTree(source, partial, "staged cache");
          if (kind === "index") validateStagedIndex(partial, manifestInfo, "partial index generation");
          syncTree(partial);
        } finally {
          close(partial.fd);
        }
        try {
          fs.renameSync(
            descriptorPath(generations.fd, temporary),
            descriptorPath(generations.fd, generationInfo.id),
          );
          syncDirectory(generations.fd);
        } catch (error) {
          if (error.code !== "EEXIST" && error.code !== "ENOTEMPTY") {
            throw error;
          }
          const finalGeneration = openDirectoryAt(generations.fd, generationInfo.id, `${kind} immutable generation`);
          try {
            if (kind === "index") {
              validateStagedIndex(source, manifestInfo, "staged index");
              compareTree(source, finalGeneration, indexFileLimit, "index immutable generation", true);
            }
            else compareTree(source, finalGeneration, cacheFileLimit, "cache immutable generation");
          } finally {
            close(finalGeneration.fd);
          }
        }
      }
      advanceCurrent(root, generations, generationInfo.id, generationInfo.generation);
    } finally {
      close(generations.fd);
    }
  } finally {
    if (lock) releasePublicationLock(root, lock);
    close(root.fd);
  }
}

function stage(options) {
  const producer = openDirectoryPath(options.indexRoot, "index root");
  const output = openDirectoryPath(options.output, "output", true);
  const hangar = openDirectoryPath(options.hangarRoot, "Hangar root");
  const jetpack = openRegularPath(options.jetpack, "jetpack binary", MAX_EXECUTABLE_BYTES);
  try {
    for (const name of ["v1", "index-v1", "generations", "current"]) {
      if (lstatAt(output.fd, name)) throw new Error(`output contains legacy publication path: ${name}`);
    }
    const manifestInfo = options.manifestIsDefault
      ? readManifestFromIndexRoot(producer, options.channel)
      : readManifestFromPath(options.manifest, options.channel);
    const producerTargets = openDirectoryAt(producer.fd, "index-v1", "index target root");
    try {
      validateManifestTargets(producerTargets, manifestInfo);
      const index = openDirectoryAt(output.fd, "index", "index publication root", true);
      try {
        const targetDestination = openDirectoryAt(index.fd, "index-v1", "index target destination", true);
        try { copyIndexTree(producerTargets, targetDestination); } finally { close(targetDestination.fd); }
        const v1 = openDirectoryAt(index.fd, "v1", "index manifest v1 destination", true);
        try {
          const channelDirectory = openDirectoryAt(v1.fd, options.channel, "index manifest channel destination", true);
          try {
            writeImmutableBytes(channelDirectory.fd, "manifest.json", manifestInfo.bytes, MAX_MANIFEST_BYTES, "index manifest");
            writeImmutableBytes(channelDirectory.fd, "manifest.json.sig.json", manifestInfo.signatureBytes, MAX_SIGNATURE_BYTES, "index manifest signature");
          } finally {
            close(channelDirectory.fd);
          }
        } finally {
          close(v1.fd);
        }
        validateStagedIndex(index, manifestInfo, "staged index");
      } finally {
        close(index.fd);
      }
    } finally {
      close(producerTargets.fd);
    }

    const cache = openDirectoryAt(output.fd, "cache", "cache publication root", true);
    const scratch = createPrivateDirectory("cache stage scratch");
    try {
      const result = spawnSync(
        descriptorPath(5),
        ["hangar", "cache", "stage", "--role", options.role, "--to", scratch.path, "--yes"],
        {
          cwd: descriptorPath(3),
          env: { ...process.env, JETPACK_ROOT: "." },
          stdio: ["ignore", "inherit", "inherit", hangar.fd, scratch.directory.fd, jetpack.fd],
        },
      );
      if (result.error) throw result.error;
      if (result.status !== 0) throw new Error(`jetpack cache staging failed with status ${result.status}`);
      const scratchPath = lstatAt(scratch.parent.fd, scratch.name);
      if (!scratchPath || !sameIdentity(scratchPath, scratch.directory.stat)) {
        throw new Error("cache staging scratch ancestor changed");
      }
      validateCacheTree(scratch.directory, "cache stage output");
      const cachePath = lstatAt(output.fd, "cache");
      if (!cachePath || !sameIdentity(cachePath, cache.stat)) throw new Error("cache staging output ancestor changed");
      copyCacheTree(scratch.directory, cache, "cache stage output");
      validateCacheTree(cache);
    } finally {
      close(cache.fd);
      removePrivateDirectory(scratch);
    }
    console.log(`prepared static index and cache trees at ${options.output}; publication remains pending`);
  } finally {
    close(jetpack.fd);
    close(hangar.fd);
    close(output.fd);
    close(producer.fd);
  }
}

function publish(options) {
  const staging = openDirectoryPath(options.staging, "staging");
  try {
    const index = openDirectoryAt(staging.fd, "index", "staged index root");
    const cache = openDirectoryAt(staging.fd, "cache", "staged cache root");
    try {
      const manifestInfo = readManifestFromDirectories(index, options.channel);
      validateStagedIndex(index, manifestInfo, "staged index");
      const generationInfo = {
        id: manifestGenerationId(manifestInfo),
        generation: manifestInfo.manifest.generation,
        channel: options.channel,
      };
      publishEndpoint(index, options.indexDestination, generationInfo, manifestInfo, "index");
      publishEndpoint(cache, options.cacheDestination, generationInfo, manifestInfo, "cache");
      console.log(`published generation ${generationInfo.id}; prior generations retained`);
    } finally {
      close(cache.fd);
      close(index.fd);
    }
  } finally {
    close(staging.fd);
  }
}

try {
  ensureDescriptorFs();
  const options = parseArgs(process.argv.slice(2));
  if (options.mode === "publish") publish(options);
  else stage(options);
} catch (error) {
  console.error(`stage-index-cache: ${error.message}`);
  process.exit(1);
}
