#!/usr/bin/env node
// Lay out the signed index targets and Hangar cache into one local tree.
// This script performs no network publication and rejects symlinks.
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

const repo = path.resolve(path.dirname(new URL(import.meta.url).pathname), "../..");

function usage() {
  return "usage: stage-index-cache.mjs --index-root <producer-output> --hangar-root <jetpack-root> --output <dir> [--channel <name>] [--role <role>] [--manifest <manifest.json>] [--jetpack <binary>]";
}

function parseArgs(argv) {
  const values = new Map();
  for (let i = 0; i < argv.length; i += 1) {
    const name = argv[i];
    if (!name.startsWith("--") || i + 1 >= argv.length || argv[i + 1].startsWith("--")) {
      throw new Error(`${usage()}\ninvalid option near ${name}`);
    }
    if (values.has(name)) throw new Error(`duplicate option ${name}`);
    values.set(name, argv[++i]);
  }
  for (const name of ["--index-root", "--hangar-root", "--output"]) {
    if (!values.has(name)) throw new Error(`${name} is required\n${usage()}`);
  }
  return {
    indexRoot: path.resolve(values.get("--index-root")),
    hangarRoot: path.resolve(values.get("--hangar-root")),
    output: path.resolve(values.get("--output")),
    channel: values.get("--channel") ?? "nixpkgs-unstable",
    role: values.get("--role") ?? "public",
    manifest: values.has("--manifest")
      ? path.resolve(values.get("--manifest"))
      : path.resolve(values.get("--index-root"), "manifest.json"),
    jetpack: values.get("--jetpack") ?? path.join(repo, "target/debug/jetpack"),
  };
}

function lstatRegular(file, label) {
  const stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (!stat) throw new Error(`${label} is missing: ${file}`);
  if (stat.isSymbolicLink()) throw new Error(`${label} must not be a symlink: ${file}`);
  return stat;
}

function prepareDirectory(dir, label) {
  const stat = fs.lstatSync(dir, { throwIfNoEntry: false });
  if (stat?.isSymbolicLink() || (stat && !stat.isDirectory())) {
    throw new Error(`${label} is not a real directory: ${dir}`);
  }
  fs.mkdirSync(dir, { recursive: true });
}

function writeImmutable(destination, bytes) {
  const stat = fs.lstatSync(destination, { throwIfNoEntry: false });
  if (stat?.isSymbolicLink() || (stat && !stat.isFile())) {
    throw new Error(`publication path is not a regular file: ${destination}`);
  }
  if (stat) {
    if (!fs.readFileSync(destination).equals(bytes)) {
      throw new Error(`immutable publication file changed: ${destination}`);
    }
    return;
  }
  prepareDirectory(path.dirname(destination), "publication parent");
  const partial = `${destination}.partial-${process.pid}`;
  try {
    fs.writeFileSync(partial, bytes, { flag: "wx", mode: 0o644 });
    fs.renameSync(partial, destination);
  } catch (error) {
    try { fs.unlinkSync(partial); } catch {}
    throw error;
  }
}

function copyTree(source, destination, include) {
  const stat = lstatRegular(source, "source path");
  if (stat.isDirectory()) {
    prepareDirectory(destination, "staging directory");
    for (const name of fs.readdirSync(source).sort()) {
      copyTree(path.join(source, name), path.join(destination, name), include);
    }
    return;
  }
  if (!include(source)) return;
  writeImmutable(destination, fs.readFileSync(source));
}

function copyManifest(manifest, output, channel) {
  lstatRegular(manifest, "index manifest");
  const destination = path.join(output, "v1", channel, "manifest.json");
  for (const suffix of ["", ".sig.json", ".sig.request"]) {
    const source = `${manifest}${suffix}`;
    if (fs.lstatSync(source, { throwIfNoEntry: false })) {
      copyTree(source, `${destination}${suffix}`, () => true);
    }
  }
}

function main(argv) {
  const options = parseArgs(argv);
  const indexTargets = path.join(options.indexRoot, "index-v1");
  lstatRegular(indexTargets, "index target root");
  prepareDirectory(options.output, "output");
  copyTree(
    indexTargets,
    path.join(options.output, "index-v1"),
    (file) => file.endsWith(".json.zst") || file.endsWith(".json.zst.sig.json"),
  );
  copyManifest(options.manifest, options.output, options.channel);

  lstatRegular(options.jetpack, "jetpack binary");
  const cache = path.join(options.output, "cache");
  const result = spawnSync(
    options.jetpack,
    ["hangar", "cache", "stage", "--role", options.role, "--to", cache, "--yes"],
    {
      env: { ...process.env, JETPACK_ROOT: options.hangarRoot },
      stdio: "inherit",
    },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`jetpack cache staging failed with status ${result.status}`);
  console.log(`prepared index/cache tree at ${options.output}; publication remains pending`);
}

try {
  main(process.argv.slice(2));
} catch (error) {
  console.error(`stage-index-cache: ${error.message}`);
  process.exit(1);
}
