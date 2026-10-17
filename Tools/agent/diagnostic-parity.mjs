#!/usr/bin/env node
// Rust-exit diagnostic parity census and guard (#4766).
//
// For every registered diagnostic code (Compiler/JetFoundation/Source/Registry/
// DiagnosticRows.jet) the census records where the Rust front references it,
// where the Jet front references it (or MISSING), and the UI snapshot that
// exercises it. A reference is the code as a quoted literal in non-test source
// outside comments; registry and explain-only data tables do not count.
//
// The guard keeps one ratchet, Tools/agent/diagnostic-parity-ratchet.json: the
// active codes the Rust front emits and the Jet front does not. It fails when
//   - a new active code is Rust-only (port it, or register it as retired), or
//   - a ratchet code lost its Rust reference without gaining a Jet one while it
//     is still active: that is a silent drop at the Rust exit.
// `--update` only removes codes the Jet front now references or the registry
// retired; it never adds one. With no ratchet file it records the current set.
//
// Usage: node Tools/agent/diagnostic-parity.mjs --census | --check | --update [--root PATH]

import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REGISTRY = "Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet";
const RATCHET = "Tools/agent/diagnostic-parity-ratchet.json";
// Jet data tables that name codes without emitting them.
const JET_DATA_TABLES = new Set([REGISTRY, "Compiler/JetCli/Source/Cli/SyntaxDictionaryData.jet"]);
const RUST_ROOTS = ["crates", "Source", "Compiler/Bootstrap"];
const JET_ROOTS = ["Compiler", "Core", "Jetpack", "Tools"];
const UI_ROOTS = [["tests/ui", ".stderr"], ["tests/ui_lint", ".warn"]];
const SKIP_DIRECTORIES = new Set([".git", ".jet", "node_modules", "target", "build", "Tests", "tests", "benches", "fixtures"]);

function usage() {
  return "usage: diagnostic-parity.mjs --census | --check | --update [--root PATH]";
}

function parseArgs(argv) {
  const options = { root: resolve(dirname(fileURLToPath(import.meta.url)), "../.."), mode: null };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--census" || arg === "--check" || arg === "--update") {
      if (options.mode) throw new Error(`choose one mode\n${usage()}`);
      options.mode = arg.slice(2);
    } else if (arg === "--root") {
      const value = argv[++index];
      if (!value) throw new Error("--root needs a path");
      options.root = resolve(value);
    } else if (arg === "--help") {
      console.log(usage());
      process.exit(0);
    } else {
      throw new Error(`unknown option ${arg}\n${usage()}`);
    }
  }
  if (!options.mode) throw new Error(usage());
  return options;
}

function files(root, start, extension) {
  const out = [];
  function visit(directory) {
    if (!existsSync(directory)) return;
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))) {
      if (entry.isSymbolicLink()) continue;
      const path = join(directory, entry.name);
      if (entry.isDirectory()) {
        if (!SKIP_DIRECTORIES.has(entry.name)) visit(path);
      } else if (entry.isFile() && entry.name.endsWith(extension)) {
        out.push(relative(root, path).split("\\").join("/"));
      }
    }
  }
  visit(join(root, start));
  return out;
}

function registry(root) {
  const text = readFileSync(join(root, REGISTRY), "utf8");
  const rows = new Map();
  for (const match of text.matchAll(/code: "([^"]+)",[\s\S]*?status: DiagnosticRegistryStatus\.(\w+),/g)) {
    if (rows.has(match[1])) throw new Error(`${REGISTRY}: duplicate code ${match[1]}`);
    rows.set(match[1], match[2]);
  }
  if (rows.size === 0) throw new Error(`${REGISTRY}: no registry rows found`);
  return rows;
}

// The line with `//` comments removed; a `//` inside a string literal stays.
function codeText(line) {
  let quoted = false;
  for (let index = 0; index < line.length; index += 1) {
    const char = line[index];
    if (char === "\\" && quoted) index += 1;
    else if (char === '"') quoted = !quoted;
    else if (!quoted && char === "/" && line[index + 1] === "/") return line.slice(0, index);
  }
  return line;
}

// First reference of each registered code as a quoted literal in `paths`.
function references(root, paths, codes, { rustTests }) {
  const found = new Map();
  for (const path of paths) {
    const lines = readFileSync(join(root, path), "utf8").split("\n");
    let block = false;
    for (let index = 0; index < lines.length; index += 1) {
      let line = lines[index];
      // Rust unit-test modules sit after `#[cfg(test)]`; they exercise, not emit.
      if (rustTests && /^\s*#\[cfg\(test\)\]/.test(line)) break;
      if (block) {
        const close = line.indexOf("*/");
        if (close < 0) continue;
        line = line.slice(close + 2);
        block = false;
      }
      const open = line.indexOf("/*");
      if (open >= 0 && line.indexOf("*/", open) < 0) {
        line = line.slice(0, open);
        block = true;
      }
      for (const match of codeText(line).matchAll(/"([A-Z][A-Z0-9-]*[0-9A-Z])"/g)) {
        if (codes.has(match[1]) && !found.has(match[1])) found.set(match[1], `${path}:${index + 1}`);
      }
    }
  }
  return found;
}

function snapshots(root, codes) {
  const found = new Map();
  for (const [start, extension] of UI_ROOTS) {
    for (const path of files(root, start, extension)) {
      for (const match of readFileSync(join(root, path), "utf8").matchAll(/\[([A-Z][A-Z0-9-]*[0-9A-Z])\]/g)) {
        if (codes.has(match[1]) && !found.has(match[1])) found.set(match[1], path);
      }
    }
  }
  return found;
}

function census(root) {
  const rows = registry(root);
  const codes = new Set(rows.keys());
  const rustPaths = RUST_ROOTS.flatMap((start) => files(root, start, ".rs"))
    .filter((path) => !/(^|\/)(tests?|.*_tests?)\.rs$/.test(path));
  const jetPaths = JET_ROOTS.flatMap((start) => files(root, start, ".jet")).filter((path) => !JET_DATA_TABLES.has(path));
  const rust = references(root, rustPaths, codes, { rustTests: true });
  const jet = references(root, jetPaths, codes, { rustTests: false });
  const ui = snapshots(root, codes);
  return [...rows].sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0)).map(([code, status]) => ({
    code,
    status,
    rust: rust.get(code) ?? null,
    jet: jet.get(code) ?? null,
    ui: ui.get(code) ?? null,
  }));
}

function rustOnly(rows) {
  return rows.filter((row) => row.status === "Active" && row.rust && !row.jet).map((row) => row.code);
}

function readRatchet(root) {
  const path = join(root, RATCHET);
  if (!existsSync(path)) return null;
  const data = JSON.parse(readFileSync(path, "utf8"));
  if (data?.schema !== 1 || !Array.isArray(data.rustOnly) || data.rustOnly.some((code) => typeof code !== "string")) {
    throw new Error(`${RATCHET} must be {"schema": 1, "rustOnly": [code, ...]}`);
  }
  return data.rustOnly;
}

function writeRatchet(root, codes) {
  const sorted = [...codes].sort();
  mkdirSync(dirname(join(root, RATCHET)), { recursive: true });
  writeFileSync(join(root, RATCHET), `${JSON.stringify({ schema: 1, rustOnly: sorted }, null, 2)}\n`);
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const rows = census(options.root);
  if (options.mode === "census") {
    console.log("code\tstatus\trust\tjet\tui");
    for (const row of rows) console.log([row.code, row.status, row.rust ?? "-", row.jet ?? "MISSING", row.ui ?? "-"].join("\t"));
    return;
  }
  const byCode = new Map(rows.map((row) => [row.code, row]));
  const current = rustOnly(rows);
  const baseline = readRatchet(options.root);
  if (baseline === null) {
    if (options.mode !== "update") throw new Error(`missing ${RATCHET}; run --update to record the current Rust-only codes`);
    writeRatchet(options.root, current);
    console.log(`diagnostic parity: recorded ${current.length} Rust-only active code(s)`);
    return;
  }
  const recorded = new Set(baseline);
  const added = current.filter((code) => !recorded.has(code));
  const drops = [];
  const resolved = [];
  for (const code of baseline) {
    if (current.includes(code)) continue;
    const row = byCode.get(code);
    if (!row || row.status !== "Active" || row.jet) resolved.push(code);
    else drops.push(code);
  }
  const problems = [
    ...added.map((code) => `  - ${code}: emitted by the Rust front only (${byCode.get(code).rust}); port it to the Jet front or retire it in the registry`),
    ...drops.map((code) => `  - ${code}: silent drop: no Rust or Jet reference remains while the registry keeps it active; port it or retire it`),
  ];
  if (problems.length) {
    console.error(["diagnostic parity failed:", ...problems].join("\n"));
    process.exitCode = 1;
  }
  if (options.mode === "update" && resolved.length) {
    writeRatchet(options.root, baseline.filter((code) => !resolved.includes(code)));
    console.log(`diagnostic parity: removed ${resolved.length} resolved code(s): ${resolved.join(", ")}`);
  } else if (resolved.length) {
    console.error(`diagnostic parity: ${resolved.length} recorded code(s) are now resolved (${resolved.join(", ")}); lower the ratchet with --update`);
    process.exitCode = 1;
  }
  if (!process.exitCode) console.log(`diagnostic parity: ${current.length} Rust-only active code(s), none new`);
}

try {
  main();
} catch (error) {
  console.error(error.message);
  process.exitCode = 2;
}
