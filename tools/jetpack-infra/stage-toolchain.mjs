#!/usr/bin/env node
// Prepare an unsigned toolchain channel tree and domain-prefixed signing
// requests. Private signing keys stay with the external release signer.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

function usage() {
  return "usage: stage-toolchain.mjs --version <version> --output <dir> [--channel <name>] --artifact <target>=<file> ...";
}

function parseArgs(argv) {
  const values = [];
  let version;
  let channel = "stable";
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
    } else if (name === "--version" || name === "--channel" || name === "--output") {
      const value = argv[++i];
      if (!value || value.startsWith("--")) throw new Error(name + " needs a value");
      if (name === "--version") version = value;
      if (name === "--channel") channel = value;
      if (name === "--output") output = value;
    } else {
      throw new Error(usage() + "\nunknown option " + name);
    }
  }
  if (!version || !output || values.length === 0) {
    throw new Error(usage() + "\nversion, output, and at least one artifact are required");
  }
  validateComponent(version, "version");
  validateComponent(channel, "channel");
  return {
    version,
    channel,
    output: path.resolve(output),
    artifacts: values.map(([target, file]) => {
      validateComponent(target, "artifact target");
      return { target, file: path.resolve(file) };
    }),
  };
}

function validateComponent(value, label) {
  if (!value || value === "." || value === ".." || !/^[A-Za-z0-9._+-]+$/.test(value)) {
    throw new Error(label + " contains unsafe characters");
  }
}

function regular(file, label) {
  const stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (!stat) throw new Error(label + " is missing: " + file);
  if (stat.isSymbolicLink() || !stat.isFile()) {
    throw new Error(label + " is not a regular file: " + file);
  }
  return stat;
}

function directory(dir, label) {
  const stat = fs.lstatSync(dir, { throwIfNoEntry: false });
  if (stat?.isSymbolicLink() || (stat && !stat.isDirectory())) {
    throw new Error(label + " is not a real directory: " + dir);
  }
  fs.mkdirSync(dir, { recursive: true });
}

function immutable(file, bytes) {
  const stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (stat?.isSymbolicLink() || (stat && !stat.isFile())) {
    throw new Error("publication path is not a regular file: " + file);
  }
  if (stat) {
    if (!fs.readFileSync(file).equals(bytes)) {
      throw new Error("immutable publication file changed: " + file);
    }
    return;
  }
  directory(path.dirname(file), "publication parent");
  const partial = file + ".partial-" + process.pid;
  try {
    fs.writeFileSync(partial, bytes, { flag: "wx", mode: 0o644 });
    fs.renameSync(partial, file);
  } catch (error) {
    try { fs.unlinkSync(partial); } catch {}
    throw error;
  }
}

function main(argv) {
  const options = parseArgs(argv);
  const seen = new Set();
  const artifacts = options.artifacts
    .sort(([left], [right]) => left.localeCompare(right))
    .map(({ target, file }) => {
      if (seen.has(target)) throw new Error("duplicate artifact target " + target);
      seen.add(target);
      regular(file, "artifact input");
      const bytes = fs.readFileSync(file);
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
    artifacts: artifacts.map(({ target, path: relative, sha256, size, signature }) => ({
      target,
      path: relative,
      sha256,
      size,
      signature,
    })),
  };
  const manifestBytes = Buffer.from(JSON.stringify(manifest));
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
