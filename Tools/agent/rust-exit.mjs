#!/usr/bin/env node
// #4742 / directives #19 and #24: registry coverage and physical Rust LOC.
// node Tools/agent/rust-exit.mjs [--check|--update] [--root PATH]
// --update only lowers ceilings; registry additions always require review.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseCoreSource } from "./check-core-surface-ledger.mjs";

export const REGISTRY = "Tools/agent/rust-leaves.tsv";
export const RATCHET = "Tools/agent/rust-exit-ratchet.json";
export const CORE = "Compiler/Prelude/Core.jet";
export const HEADER = "row\trust_fn\tfile:line\tremoval_condition\trecorded_by\tscope\tdispatch_sha256";
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const cliCrates = new Set(["jet-devserver", "jet-repl", "jet-debug", "jet-net", "jet-canvas", "jetos", "jet-unicode"]);
const packCrates = new Set(["jetpack", "jet-pkg-model", "jet-env-model", "jet-nix-eval", "jet-store", "jetpack-bin"]);

// Areas partition every tracked Rust file, rather than double-counting the
// inventory's cross-references (e.g. Bootstrap/Tests.rs in B and G).
export function rustArea(file) {
  if (/(^|\/)build\.rs$/.test(file) || file.startsWith("crates/jet-foundation/build/")) return "H";
  if (file.startsWith("crates/jet-codegen/src/Prelude/") || file.startsWith("crates/jet-rt/")) return "C";
  if (file.startsWith("crates/jet-codegen/")) return "A";
  if (file.startsWith("Compiler/Bootstrap/")) return "B";
  if (file.startsWith("Core/")) return "D";
  if (file.startsWith("Source/")) return "E";
  if (file.startsWith("tests/") || file.startsWith("Tools/") || file.startsWith("Editors/")) return "G";
  if (file.startsWith("crates/")) {
    const crate = file.split("/")[1];
    if (cliCrates.has(crate)) return "E";
    if (packCrates.has(crate)) return "F";
    return "B";
  }
  // Root Rust files (the library façade and probes) are bootstrap tooling.
  return "H";
}

export function physicalLines(text) {
  return text.length === 0 ? 0 : text.split("\n").length - (text.endsWith("\n") ? 1 : 0);
}

export function rustCounts(root) {
  // Count regular source files once, like the inventory's git grep: Prelude's
  // Devtools.rs symlink points to the already-counted foundation Rust body.
  // Include nonignored new sources so staging is not a way around the gate.
  const files = execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "*.rs"], { cwd: root, encoding: "utf8" }).split("\0").filter(Boolean);
  const counts = Object.fromEntries("ABCDEFGH".split("").map((area) => [area, 0]));
  for (const file of new Set(files)) {
    if (existsSync(join(root, file)) && lstatSync(join(root, file)).isFile()) counts[rustArea(file)] += physicalLines(readFileSync(join(root, file), "utf8"));
  }
  return counts;
}

export function rowIdentity(row) {
  if (row.kind !== "receiver") return `${row.module}.${row.member}`;
  const types = row.expression.match(/&\[([^\]]*)\]/)?.[1];
  const method = row.expression.match(/&\[[^\]]*\]\s*,\s*(?:"([^"]+)"|([A-Za-z_]\w*))/);
  if (types === undefined || method === null) throw new Error(`unreadable receiver at Core.jet:${row.sourceLine}`);
  return `receiver[${types.replace(/\s/g, "")}].${method[1] ?? method[2]}`;
}

export function dispatchHash(row) {
  // Do not remove whitespace inside string literals (it can change behavior).
  const normalized = row.expression.replace(/"(?:\\.|[^"\\])*"|\s+/g, (s) => s.startsWith('"') ? s : "");
  return createHash("sha256").update(`${row.kind}:${normalized}`).digest("hex");
}

export function parseRegistry(text) {
  const lines = text.split(/\r?\n/).filter((line) => line && !line.startsWith("#"));
  if (lines.shift() !== HEADER) throw new Error(`${REGISTRY}: wrong header`);
  const entries = new Map();
  for (const line of lines) {
    const fields = line.split("\t");
    if (fields.length !== 7 || fields.some((field) => !field.trim())) throw new Error(`${REGISTRY}: malformed row ${line}`);
    const [id, symbol, location, removal, owner, scope, hash] = fields;
    if (!['leaf', 'port'].includes(scope) || !/^(?:-|[a-f0-9]{64})$/.test(hash)) throw new Error(`${REGISTRY}: invalid scope/hash for ${id}`);
    if (entries.has(id)) throw new Error(`${REGISTRY}: duplicate identity ${id}`);
    entries.set(id, { id, symbol, location, removal, owner, scope, hash });
  }
  return entries;
}

export function registryErrors(rows, entries) {
  const errors = [];
  for (const row of rows) {
    const id = rowIdentity(row), entry = entries.get(id);
    if (!entry) errors.push(`unregistered Rust dispatcher ${id} at Core.jet:${row.sourceLine}`);
    else if (entry.hash !== dispatchHash(row)) errors.push(`Rust dispatcher changed without registry review: ${id} at Core.jet:${row.sourceLine}`);
  }
  return errors;
}

export function ratchetErrors(counts, ceilings) {
  const errors = [];
  for (const area of "ABCDEFGH") {
    if (!Number.isSafeInteger(ceilings[area]) || ceilings[area] < 0) errors.push(`missing/invalid Rust ceiling for area ${area}`);
    else if (counts[area] > ceilings[area]) errors.push(`Rust area ${area} grew: ${counts[area]} > ${ceilings[area]} (+${counts[area] - ceilings[area]}); port/remove Rust, never raise the ceiling`);
  }
  return errors;
}

export function main(args = process.argv.slice(2)) {
  let root = ROOT, update = false;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--root" && args[i + 1]) root = resolve(args[++i]);
    else if (args[i] === "--update") update = true;
    else if (args[i] !== "--check") throw new Error("usage: rust-exit.mjs [--check|--update] [--root PATH]");
  }
  const entries = parseRegistry(readFileSync(join(root, REGISTRY), "utf8"));
  // Conservative coverage: a dispatcher with a Jet source wrapper may still
  // reach Rust, so ownership/census credit never exempts it. Unmapped generated
  // and implicit receiver bodies are also registered, not silently dropped.
  const rows = existsSync(join(root, CORE)) ? parseCoreSource(readFileSync(join(root, CORE), "utf8"), root).dispatcherRows : [];
  const table = JSON.parse(readFileSync(join(root, RATCHET), "utf8"));
  const counts = rustCounts(root);
  const errors = [...registryErrors(rows, entries), ...ratchetErrors(counts, table.ceilings)];
  for (const area of "ABCDEFGH") console.log(`Rust ${area}: ${counts[area]}/${table.ceilings[area]}`);
  console.log(`Rust registry: ${rows.length} dispatchers / ${entries.size} entries (registration is not Jet census credit)`);
  if (errors.length) throw new Error(errors.join("\n"));
  if (update && Object.keys(counts).some((area) => counts[area] !== table.ceilings[area])) {
    table.ceilings = counts;
    writeFileSync(join(root, RATCHET), JSON.stringify(table, null, 2) + "\n");
  }
  console.log("RUST EXIT OK");
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try { main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
