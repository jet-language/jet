#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const EXAMPLES = join(ROOT, "examples");
const UI = join(ROOT, "tests", "ui");
const CORE_CORPUS = join(ROOT, "tests", "conformance", "corpus");
const CORE_INVENTORY = join(ROOT, "scripts", "agent", "core-conformance.mjs");
const JET_ENV = join(ROOT, "scripts", "agent", "jet-env");
const HOME_CACHE = join(homedir(), ".cache");
const DEFAULT_CACHE = join(HOME_CACHE, "jet-mir-rust-metadata");
const DEFAULT_SCRATCH = join(HOME_CACHE, "jet-test-scratch");
const POSITIVE_UI_SNAPSHOT = "(no errors)\n";
const MIR_IDENTITY_SCHEMA = "mir-v1";
const MAX_CHILD_OUTPUT = 64 * 1024 * 1024;
const MAX_CACHE_BYTES = 1024 * 1024;
const MAX_SOURCES = 16_384;
const CORE_MARKER = /^\/\/ core-conformance: (\S+)$/u;
const DECLARATIONS = new Set([
  "use",
  "fn",
  "pub",
  "struct",
  "enum",
  "trait",
  "impl",
  "module",
  "derive",
  "const",
  "extern",
  "type",
  "import",
  "package",
  "workspace",
  "#",
  "@",
]);

class WitnessError extends Error {
  constructor(stage, source, message) {
    super(`${stage} source=${source}: ${message}`);
    this.stage = stage;
    this.source = source;
  }
}

function fail(stage, source, message) {
  throw new WitnessError(stage, source, message);
}

function compareLex(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function displayPath(path) {
  const absolute = resolve(path);
  const shown = relative(ROOT, absolute).replaceAll(sep, "/");
  return shown === "" ? "." : shown;
}

function safeChildEnvironment(scratch = null) {
  const environment = { ...process.env, NO_COLOR: "1" };
  if (scratch) environment.TMPDIR = scratch;
  return environment;
}
function compact(text, limit = 4096) {
  const clean = String(text ?? "").trim().replace(/\s+/gu, " ");
  return clean.length <= limit ? clean : `${clean.slice(0, limit)}...`;
}

function readDirectory(path, source) {
  try {
    return readdirSync(path, { withFileTypes: true }).sort((left, right) => compareLex(left.name, right.name));
  } catch (error) {
    fail("enumeration", source, `cannot read directory: ${error.message}`);
  }
}

function regularFile(path, source) {
  let info;
  try {
    info = lstatSync(path, { throwIfNoEntry: false });
  } catch (error) {
    fail("enumeration", source, `cannot inspect path: ${error.message}`);
  }
  if (!info) return false;
  if (info.isSymbolicLink()) fail("enumeration", source, "symbolic links are not supported");
  if (!info.isFile()) fail("enumeration", source, "expected a regular file");
  return true;
}

function walkFiles(root, predicate, source) {
  if (!existsSync(root)) fail("enumeration", source, "directory does not exist");
  const output = [];
  const visit = (directory) => {
    for (const entry of readDirectory(directory, displayPath(directory))) {
      if (entry.name.startsWith(".")) continue;
      const path = join(directory, entry.name);
      if (entry.isSymbolicLink()) fail("enumeration", displayPath(path), "symbolic links are not supported");
      if (entry.isDirectory()) {
        visit(path);
      } else if (entry.isFile() && predicate(entry.name, path)) {
        output.push(path);
      }
    }
  };
  visit(root);
  return output.sort(compareLex);
}

function jetFiles(root, source) {
  return walkFiles(root, (name) => name.endsWith(".jet") && name !== "package.jet", source);
}
function maskJet(source, file) {
  const chars = source.split("");
  const blank = (index) => {
    if (chars[index] !== "\n" && chars[index] !== "\r") chars[index] = " ";
  };
  let index = 0;
  while (index < source.length) {
    const current = source[index];
    const next = source[index + 1];
    if (current === "/" && next === "/") {
      blank(index);
      blank(index + 1);
      index += 2;
      while (index < source.length && source[index] !== "\n") {
        blank(index);
        index += 1;
      }
      continue;
    }
    if (current === "/" && next === "*") {
      blank(index);
      blank(index + 1);
      index += 2;
      let depth = 1;
      while (index < source.length && depth > 0) {
        if (source[index] === "/" && source[index + 1] === "*") {
          blank(index);
          blank(index + 1);
          depth += 1;
          index += 2;
        } else if (source[index] === "*" && source[index + 1] === "/") {
          blank(index);
          blank(index + 1);
          depth -= 1;
          index += 2;
        } else {
          blank(index);
          index += 1;
        }
      }
      if (depth !== 0) fail("enumeration", file, "unterminated block comment");
      continue;
    }
    if (current === '"') {
      const triple = source.slice(index, index + 3) === '"""';
      const opening = triple ? 3 : 1;
      for (let offset = 0; offset < opening; offset += 1) blank(index + offset);
      index += opening;
      let closed = false;
      let escaped = false;
      while (index < source.length) {
        if (!escaped && (triple ? source.slice(index, index + 3) === '"""' : source[index] === '"')) {
          const closing = triple ? 3 : 1;
          for (let offset = 0; offset < closing; offset += 1) blank(index + offset);
          index += closing;
          closed = true;
          break;
        }
        if (!triple && source[index] === "\n") {
          fail("enumeration", file, "unterminated string literal");
        }
        if (!escaped && source[index] === "\\") escaped = true;
        else escaped = false;
        blank(index);
        index += 1;
      }
      if (!closed) fail("enumeration", file, "unterminated string literal");
      continue;
    }
    if (current === "'") {
      blank(index);
      index += 1;
      let closed = false;
      let escaped = false;
      while (index < source.length && source[index] !== "\n") {
        if (!escaped && source[index] === "'") {
          blank(index);
          index += 1;
          closed = true;
          break;
        }
        if (!escaped && source[index] === "\\") escaped = true;
        else escaped = false;
        blank(index);
        index += 1;
      }
      if (!closed) fail("enumeration", file, "unterminated character literal");
      continue;
    }
    index += 1;
  }
  return chars.join("");
}

function hasFunction(masked, name) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
  return new RegExp(`\\bfn\\s+${escaped}\\b`, "u").test(masked);
}

function hasImplicitEntry(masked) {
  let braces = 0;
  let start = 0;
  while (start <= masked.length) {
    const end = masked.indexOf("\n", start);
    const lineEnd = end < 0 ? masked.length : end;
    const line = masked.slice(start, lineEnd);
    if (braces === 0 && line.trim() !== "") {
      const match = line.match(/^\s*([A-Za-z_][A-Za-z0-9_]*|[#@]|.)/u);
      if (match && !DECLARATIONS.has(match[1])) return true;
    }
    for (let index = start; index < lineEnd; index += 1) {
      if (masked[index] === "{") braces += 1;
      else if (masked[index] === "}") {
        braces -= 1;
        if (braces < 0) return false;
      }
    }
    if (end < 0) break;
    start = end + 1;
  }
  return false;
}

function addRecord(records, path, kind) {
  const absolute = resolve(path);
  const shown = displayPath(absolute);
  const previous = records.get(shown);
  if (previous) {
    previous.kinds.add(kind);
    return previous;
  }
  const record = { absolute, shown, kinds: new Set([kind]) };
  records.set(shown, record);
  return record;
}

function packageName(manifest, source) {
  let text;
  try {
    text = readFileSync(manifest, "utf8");
  } catch (error) {
    fail("enumeration", source, `cannot read package manifest: ${error.message}`);
  }
  const matches = [...text.matchAll(/^name\s*:\s*"([^"]+)"\s*$/gmu)];
  if (matches.length !== 1) fail("enumeration", source, "package manifest must define exactly one top-level name");
  return { text, name: matches[0][1] };
}

function packageOutputNames(text, manifest) {
  const masked = maskJet(text, manifest);
  const names = new Set();
  for (const match of masked.matchAll(/\bentry\s*:\s*([A-Za-z_][A-Za-z0-9_.]*)/gu)) {
    names.add(match[1]);
  }
  return [...names].sort(compareLex);
}

function packageOwner(path, packageRoots) {
  let owner = null;
  for (const candidate of packageRoots) {
    const rel = relative(candidate, path);
    if (rel === "" || rel.startsWith(`..${sep}`) || isAbsolute(rel)) continue;
    if (!owner || candidate.length > owner.length) owner = candidate;
  }
  return owner;
}

function collectExamples(records) {
  const allFiles = jetFiles(EXAMPLES, displayPath(EXAMPLES));
  const manifests = walkFiles(EXAMPLES, (name) => name === "package.jet", displayPath(EXAMPLES));
  const packageData = manifests.map((manifest) => {
    const root = dirname(manifest);
    const { text, name } = packageName(manifest, displayPath(manifest));
    return { manifest, root, name, outputNames: packageOutputNames(text, displayPath(manifest)) };
  });
  const packageRoots = packageData.map((entry) => entry.root);
  const sources = allFiles.map((absolute) => {
    let source;
    try {
      source = readFileSync(absolute, "utf8");
    } catch (error) {
      fail("enumeration", displayPath(absolute), `cannot read source: ${error.message}`);
    }
    return {
      absolute,
      source,
      masked: maskJet(source, displayPath(absolute)),
      owner: packageOwner(absolute, packageRoots),
    };
  });
  const byPackage = new Map();
  for (const source of sources) {
    if (!source.owner) continue;
    if (!byPackage.has(source.owner)) byPackage.set(source.owner, []);
    byPackage.get(source.owner).push(source);
  }

  let entries = 0;
  for (const source of sources) {
    if (!hasFunction(source.masked, "run") && !hasFunction(source.masked, "main") && !hasImplicitEntry(source.masked)) {
      continue;
    }
    addRecord(records, source.absolute, source.owner ? "example-package-entry" : "example-entry");
    entries += 1;
  }

  for (const packageEntry of packageData) {
    const owned = byPackage.get(packageEntry.root) ?? [];
    for (const outputName of packageEntry.outputNames) {
      const functionName = outputName.slice(outputName.lastIndexOf(".") + 1);
      const matches = owned.filter((source) => hasFunction(source.masked, functionName));
      if (matches.length === 0) {
        fail(
          "enumeration",
          displayPath(packageEntry.manifest),
          `declared output entry ${outputName} has no source function witness`,
        );
      }
      if (matches.length > 1) {
        fail(
          "enumeration",
          displayPath(packageEntry.manifest),
          `declared output entry ${outputName} resolves ambiguously: ${matches
            .map((source) => displayPath(source.absolute))
            .sort(compareLex)
            .join(", ")}`,
        );
      }
      const before = records.has(displayPath(matches[0].absolute));
      addRecord(records, matches[0].absolute, "example-declared-output-entry");
      if (!before) entries += 1;
    }
  }
  return { files: allFiles.length, entries };
}

function uiEntries() {
  const entries = [];
  for (const entry of readDirectory(UI, displayPath(UI))) {
    if (entry.name.startsWith(".")) continue;
    const path = join(UI, entry.name);
    if (entry.isSymbolicLink()) fail("enumeration", displayPath(path), "symbolic links are not supported");
    if (entry.isFile() && entry.name.endsWith(".jet") && !entry.name.includes(".fixed.")) {
      entries.push({ source: path, expected: `${path.slice(0, -4)}.stderr` });
      continue;
    }
    if (!entry.isDirectory()) continue;
    const run = join(path, "run.jet");
    const main = join(path, "main.jet");
    const workspace = join(path, "workspace.jet");
    if (regularFile(run, displayPath(run))) {
      entries.push({ source: run, expected: join(path, "stderr") });
    } else if (regularFile(main, displayPath(main))) {
      entries.push({ source: main, expected: join(path, "stderr") });
    } else if (regularFile(workspace, displayPath(workspace))) {
      const packageDir = dirname(path);
      entries.push({
        source: workspace,
        expected: join(dirname(packageDir), `${packageDir.split(sep).at(-1)}.stderr`),
      });
    }
  }
  return entries.sort((left, right) => compareLex(displayPath(left.source), displayPath(right.source)));
}

function collectUi(records) {
  const entries = uiEntries();
  if (entries.length === 0) fail("enumeration", displayPath(UI), "no UI cases were enumerated");
  let positives = 0;
  for (const entry of entries) {
    if (!existsSync(entry.expected)) {
      fail("enumeration", displayPath(entry.source), `expected snapshot is missing: ${displayPath(entry.expected)}`);
    }
    let expected;
    try {
      expected = readFileSync(entry.expected, "utf8");
    } catch (error) {
      fail("enumeration", displayPath(entry.source), `cannot read expected snapshot: ${error.message}`);
    }
    if (expected === POSITIVE_UI_SNAPSHOT) {
      addRecord(records, entry.source, "ui-positive");
      positives += 1;
    } else if (expected.trim() === "") {
      fail("enumeration", displayPath(entry.source), "empty UI snapshot is ambiguous; expected the exact positive marker");
    }
  }
  return { cases: entries.length, positives };
}

function runCoreInventory() {
  const result = spawnSync(process.execPath, [CORE_INVENTORY, "--inventory"], {
    cwd: ROOT,
    env: safeChildEnvironment(),
    encoding: "utf8",
    maxBuffer: MAX_CHILD_OUTPUT,
  });
  if (result.error) fail("enumeration", displayPath(CORE_INVENTORY), `inventory command failed: ${result.error.message}`);
  if (result.status !== 0) {
    fail(
      "enumeration",
      displayPath(CORE_INVENTORY),
      `inventory command exited ${result.status ?? "without a status"}: ${compact(result.stderr)}`,
    );
  }
  if (compact(result.stderr) !== "") {
    fail("enumeration", displayPath(CORE_INVENTORY), `inventory command wrote stderr: ${compact(result.stderr)}`);
  }
  let rows;
  try {
    rows = JSON.parse(result.stdout);
  } catch (error) {
    fail("enumeration", displayPath(CORE_INVENTORY), `inventory output is not JSON: ${error.message}`);
  }
  if (!Array.isArray(rows) || rows.length === 0 || rows.some((row) => typeof row !== "string" || !/^[A-Za-z_][A-Za-z0-9_.-]*$/u.test(row))) {
    fail("enumeration", displayPath(CORE_INVENTORY), "inventory must be a non-empty array of valid row names");
  }
  const sorted = [...rows].sort(compareLex);
  if (rows.some((row, index) => row !== sorted[index])) fail("enumeration", displayPath(CORE_INVENTORY), "inventory order is not deterministic");
  if (new Set(rows).size !== rows.length) fail("enumeration", displayPath(CORE_INVENTORY), "inventory contains duplicate rows");
  return rows;
}

function coreKey(path) {
  const rel = relative(CORE_CORPUS, path).replaceAll(sep, "/");
  if (rel.startsWith("../") || rel === ".." || !rel.endsWith(".jet")) {
    fail("enumeration", displayPath(path), "Core witness lies outside the corpus");
  }
  return rel
    .slice(0, -4)
    .split("/")
    .filter(Boolean)
    .join(".");
}

function collectCore(records) {
  const rows = runCoreInventory();
  const files = jetFiles(CORE_CORPUS, displayPath(CORE_CORPUS));
  const witnesses = new Map();
  for (const path of files) {
    let source;
    try {
      source = readFileSync(path, "utf8");
    } catch (error) {
      fail("enumeration", displayPath(path), `cannot read source: ${error.message}`);
    }
    const firstLine = source.split(/\r?\n/u, 1)[0];
    const match = firstLine.match(CORE_MARKER);
    if (!match) fail("enumeration", displayPath(path), "Core witness is missing its first-line row marker");
    const row = match[1];
    const derived = coreKey(path);
    if (row !== derived) {
      fail("enumeration", displayPath(path), `row marker ${row} does not match canonical row ${derived}`);
    }
    if (witnesses.has(row)) fail("enumeration", displayPath(path), `duplicate witness for Core row ${row}`);
    witnesses.set(row, path);
  }
  for (const row of rows) {
    const path = witnesses.get(row);
    if (!path) fail("enumeration", row, "Core conformance row has no witness source");
    addRecord(records, path, "core-conformance");
  }
  for (const row of witnesses.keys()) {
    if (!rows.includes(row)) fail("enumeration", displayPath(witnesses.get(row)), `witness row ${row} is absent from the canonical inventory`);
  }
  return { rows: rows.length, files: files.length };
}

function enumerateSources() {
  const records = new Map();
  const examples = collectExamples(records);
  const ui = collectUi(records);
  const core = collectCore(records);
  const sources = [...records.values()].sort((left, right) => compareLex(left.shown, right.shown));
  if (sources.length === 0) fail("enumeration", "<all inputs>", "no witness sources were enumerated");
  if (sources.length > MAX_SOURCES) fail("enumeration", "<all inputs>", `source count exceeds ${MAX_SOURCES}`);
  return { records: sources, examples, ui, core };
}

function assertSafePath(path, label) {
  const resolved = resolve(path);
  const target = resolve(ROOT, "target");
  if (resolved === "/tmp" || resolved.startsWith(`/tmp${sep}`)) fail("cache", label, "path may not use /tmp");
  if (resolved === target || resolved.startsWith(`${target}${sep}`)) fail("cache", label, "path may not use target/");
  return resolved;
}

function cacheDirectory() {
  const value = process.env.JET_MIR_METADATA_CACHE_DIR || DEFAULT_CACHE;
  const path = assertSafePath(value, "JET_MIR_METADATA_CACHE_DIR");
  const homeCache = resolve(HOME_CACHE);
  if (path !== homeCache && !path.startsWith(`${homeCache}${sep}`)) {
    fail("cache", displayPath(path), "cache must live under the established user cache");
  }
  try {
    mkdirSync(path, { recursive: true });
  } catch (error) {
    fail("cache", displayPath(path), `cannot create cache directory: ${error.message}`);
  }
  return path;
}

function scratchDirectory() {
  const value = process.env.JET_MIR_METADATA_SCRATCH_DIR || process.env.JET_TEST_SCRATCH_DIR || DEFAULT_SCRATCH;
  const path = assertSafePath(value, "JET_MIR_METADATA_SCRATCH_DIR");
  const repoScratch = resolve(ROOT, ".tmp");
  if (
    path !== resolve(HOME_CACHE) &&
    !path.startsWith(`${resolve(HOME_CACHE)}${sep}`) &&
    path !== repoScratch &&
    !path.startsWith(`${repoScratch}${sep}`)
  ) {
    fail("cache", displayPath(path), "scratch must live under the established user cache or ignored .tmp/");
  }
  try {
    mkdirSync(path, { recursive: true });
  } catch (error) {
    fail("cache", displayPath(path), `cannot create scratch directory: ${error.message}`);
  }
  return path;
}

function sha256(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

function runRustcIdentity(scratch) {
  const result = spawnSync(JET_ENV, ["full", "rustc", "-vV"], {
    cwd: ROOT,
    env: safeChildEnvironment(scratch),
    encoding: "utf8",
    maxBuffer: MAX_CHILD_OUTPUT,
  });
  if (result.error) fail("metadata", "<rustc -vV>", `rustc identity command failed: ${result.error.message}`);
  if (result.status !== 0) {
    fail(
      "metadata",
      "<rustc -vV>",
      `rustc identity command exited ${result.status ?? "without a status"}: ${compact(result.stderr)}`,
    );
  }
  if (!/^rustc\s+\S+/mu.test(result.stdout)) fail("metadata", "<rustc -vV>", "rustc identity output is empty or malformed");
  return { text: result.stdout, digest: sha256(result.stdout) };
}

function parseMirIdentity(record, lines) {
  const identityLines = lines.filter((line) => line.startsWith("// jet-mir-identity:"));
  if (identityLines.length !== 1) {
    fail("emit", record.shown, "emitted Rust must contain exactly one canonical mir-v1 identity marker");
  }
  const prefix = "// jet-mir-identity:";
  const canonical = identityLines[0].slice(prefix.length).trim();
  let identity;
  try {
    identity = JSON.parse(canonical);
  } catch (error) {
    fail("identity", record.shown, `mir-v1 identity is not JSON: ${error.message}`);
  }
  if (JSON.stringify(identity) !== canonical) {
    fail("identity", record.shown, "mir-v1 identity is not canonical JSON");
  }
  if (
    identity?.schema !== MIR_IDENTITY_SCHEMA ||
    !/^[0-9a-f]{64}$/u.test(identity.semantic_hash ?? "") ||
    identity.semantic_hash !== identity.optimized_hash ||
    !/^[0-9a-f]{64}$/u.test(identity.identity_digest ?? "") ||
    !Array.isArray(identity.function_ids) ||
    !Array.isArray(identity.core_ids) ||
    !Array.isArray(identity.source_map) ||
    !Array.isArray(identity.target_facts)
  ) {
    fail("identity", record.shown, "mir-v1 identity is malformed or has inconsistent program digests");
  }
  return { ...identity, canonical };
}

function emitRust(record, scratch) {
  const result = spawnSync(JET_ENV, ["full", "jet", "emit", "--rust", "--metadata", record.shown], {
    cwd: ROOT,
    env: safeChildEnvironment(scratch),
    encoding: "utf8",
    maxBuffer: MAX_CHILD_OUTPUT,
  });
  if (result.error) fail("emit", record.shown, `jet emit --rust failed to start: ${result.error.message}`);
  if (result.status !== 0) {
    const detail = compact(result.stderr || result.stdout);
    fail("emit", record.shown, `jet emit --rust exited ${result.status ?? "without a status"}${detail ? `: ${detail}` : ""}`);
  }
  const identity = parseMirIdentity(record, result.stdout.split(/\r?\n/u));
  return {
    source: result.stdout,
    semantic: identity.semantic_hash,
    optimized: identity.optimized_hash,
    identity,
    mirHash: identity.optimized_hash,
    rustDigest: sha256(result.stdout),
  };
}


function cachePath(cache, marker, rustc) {
  return join(cache, `${marker.mirHash}-${rustc.digest}.json`);
}

function readCache(path, marker, rustc, record) {
  if (!existsSync(path)) return false;
  let raw;
  try {
    raw = readFileSync(path, "utf8");
  } catch (error) {
    fail("cache", record.shown, `cannot read cache entry ${path}: ${error.message}`);
  }
  if (Buffer.byteLength(raw, "utf8") > MAX_CACHE_BYTES) fail("cache", record.shown, `cache entry is larger than ${MAX_CACHE_BYTES} bytes`);
  let entry;
  try {
    entry = JSON.parse(raw);
  } catch (error) {
    fail("cache", record.shown, `cache entry is not JSON: ${error.message}`);
  }
  if (
    entry?.schema !== 3 ||
    entry.status !== "pass" ||
    entry.mir_schema !== MIR_IDENTITY_SCHEMA ||
    entry.mir_program_digest !== marker.mirHash ||
    entry.mir_identity !== marker.identity.canonical ||
    entry.semantic !== marker.semantic ||
    entry.optimized !== marker.optimized ||
    entry.rustc_digest !== rustc.digest ||
    entry.rustc_identity !== rustc.text ||
    entry.rust_source_digest !== marker.rustDigest
  ) {
    fail("cache", record.shown, `cache entry is inconsistent: ${path}`);
  }
  return true;
}

function publishCache(path, marker, rustc, record) {
  const parent = dirname(path);
  let staging;
  try {
    staging = mkdtempSync(join(parent, ".publish-"));
    const staged = join(staging, "entry.json");
    writeFileSync(
      staged,
      `${JSON.stringify({
        schema: 3,
        status: "pass",
        mir_schema: MIR_IDENTITY_SCHEMA,
        mir_program_digest: marker.mirHash,
        mir_identity: marker.identity.canonical,
        semantic: marker.semantic,
        optimized: marker.optimized,
        rustc_digest: rustc.digest,
        rustc_identity: rustc.text,
        rust_source_digest: marker.rustDigest,
      })}\n`,
      "utf8",
    );
    renameSync(staged, path);
  } catch (error) {
    fail("cache", record.shown, `cannot publish cache entry ${path}: ${error.message}`);
  } finally {
    if (staging) rmSync(staging, { recursive: true, force: true });
  }
}

function compileMetadata(record, marker, rustc, scratch, cache) {
  const path = cachePath(cache, marker, rustc);
  if (readCache(path, marker, rustc, record)) return "hit";
  let run;
  try {
    run = mkdtempSync(join(scratch, "mir-rust-metadata-witness-"));
  } catch (error) {
    fail("cache", record.shown, `cannot create metadata scratch directory: ${error.message}`);
  }
  try {
    const sourcePath = join(run, "witness.rs");
    const metadataPath = join(run, "witness.rmeta");
    try {
      writeFileSync(sourcePath, marker.source, "utf8");
    } catch (error) {
      fail("metadata", record.shown, `cannot write Rust scratch source: ${error.message}`);
    }
    const result = spawnSync(
      JET_ENV,
      [
        "full",
        "rustc",
        "--edition",
        "2021",
        "--emit=metadata",
        "--crate-type",
        "lib",
        "--crate-name",
        "jet_mir_metadata_witness",
        sourcePath,
        "-o",
        metadataPath,
      ],
      {
        cwd: ROOT,
        env: safeChildEnvironment(scratch),
        encoding: "utf8",
        maxBuffer: MAX_CHILD_OUTPUT,
      },
    );
    if (result.error) fail("metadata", record.shown, `rustc --emit=metadata failed to start: ${result.error.message}`);
    if (result.status !== 0) {
      const detail = compact(result.stderr || result.stdout);
      fail(
        "metadata",
        record.shown,
        `rustc --emit=metadata exited ${result.status ?? "without a status"}${detail ? `: ${detail}` : ""}`,
      );
    }
    if (!regularFile(metadataPath, record.shown) || statSync(metadataPath).size === 0) {
      fail("metadata", record.shown, "rustc reported success but emitted no metadata artifact");
    }
    publishCache(path, marker, rustc, record);
    return "miss";
  } finally {
    rmSync(run, { recursive: true, force: true });
  }
}

function run() {
  const enumeration = enumerateSources();
  const cache = cacheDirectory();
  const scratch = scratchDirectory();
  const rustc = runRustcIdentity(scratch);
  let hits = 0;
  let misses = 0;
  for (const record of enumeration.records) {
    const marker = emitRust(record, scratch);
    const result = compileMetadata(record, marker, rustc, scratch, cache);
    if (result === "hit") hits += 1;
    else misses += 1;
  }
  console.log(
    [
      "mir-rust-metadata-witness: PASS",
      `schema=${MIR_IDENTITY_SCHEMA}`,
      `sources=${enumeration.records.length}`,
      `examples=${enumeration.examples.entries}`,
      `ui_positive=${enumeration.ui.positives}`,
      `core_rows=${enumeration.core.rows}`,
      `cache_hits=${hits}`,
      `metadata_checks=${misses}`,
      `rustc=${rustc.digest}`,
    ].join(" "),
  );
}

try {
  run();
} catch (error) {
  if (error instanceof WitnessError) {
    console.error(`mir-rust-metadata-witness: FAIL ${error.message}`);
  } else {
    console.error(`mir-rust-metadata-witness: FAIL source=<internal>: ${error.message}`);
  }
  process.exitCode = 1;
}
