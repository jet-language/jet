#!/usr/bin/env node
// D-OPENTABLE1=D: generate the checked-in views of the declarations that have
// a readable Jet twin.  The declarations stay in Prelude; Rust files are
// projections and the source hash makes a stale projection a build error.
//
// Usage:
//   node scripts/agent/gen-core-tables.mjs --check
//   node scripts/agent/gen-core-tables.mjs --write
//
// Core module and dispatcher rendering is shared with
// check-core-surface-ledger.mjs.  This file owns the cross-table cutover and
// effect projections; it must not grow a second Core parser or renderer.

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import {
  coreSourceFacts,
  publicDispatcherRows,
  validatePublicDispatcher,
  validateGeneratedViews,
  writeCoreViews,
} from "./check-core-surface-ledger.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const EFFECT_SOURCE_PATH = "crates/jet-codegen/src/Prelude/Effects.jet";
const EFFECTS_PATH = "crates/jet-foundation/src/Effects.rs";
const BUILD_EFFECTS_PATH = "crates/jet-foundation/src/BuildEffects.rs";
const CORE_SOURCE_PATH = "crates/jet-codegen/src/Prelude/Core.jet";

const EFFECT_BEGIN = "// BEGIN GENERATED EFFECT DECLARATIONS";
const EFFECT_END = "// END GENERATED EFFECT DECLARATIONS";
const BUILD_EFFECT_BEGIN = "// BEGIN GENERATED BUILD EFFECT DECLARATIONS";
const BUILD_EFFECT_END = "// END GENERATED BUILD EFFECT DECLARATIONS";

// Build authority intentionally remains the existing ten-effect projection.
// FFI, Browser, and Secret are runtime authority roots, not build grants.
const BUILD_EFFECT_NAMES = [
  "Net", "FS", "IO", "DB", "Time", "Rand", "Env", "Exec", "Log", "GPU",
];
const INTERNAL_EFFECT_NAMES = ["Panic", "Mem"];

function file(rel) {
  return path.join(ROOT, rel);
}

function read(rel) {
  return readFileSync(file(rel), "utf8");
}

function sha256(source) {
  return createHash("sha256").update(source).digest("hex");
}

function fail(message) {
  throw new Error(message);
}

function parseEffects(source) {
  const declarations = [];
  const names = new Set();
  for (const [index, raw] of source.split(/\r?\n/).entries()) {
    const line = raw.replace(/\/\/.*$/, "").trim();
    if (!line) continue;
    const fields = line.split(/\s+/);
    if (fields[0] !== "effect") {
      fail(`${EFFECT_SOURCE_PATH}:${index + 1}: expected an effect declaration`);
    }
    if (fields.length < 2 || fields.length > 3) {
      fail(`${EFFECT_SOURCE_PATH}:${index + 1}: malformed effect declaration`);
    }
    const name = fields[1];
    if (!/^[A-Z][A-Za-z0-9]*(?:\.[A-Z][A-Za-z0-9]*)*$/.test(name)) {
      fail(`${EFFECT_SOURCE_PATH}:${index + 1}: invalid effect name ${name}`);
    }
    if (names.has(name)) {
      fail(`${EFFECT_SOURCE_PATH}:${index + 1}: duplicate effect declaration ${name}`);
    }
    const irreversible = fields.length === 3;
    if (irreversible && fields[2] !== "@irreversible") {
      fail(`${EFFECT_SOURCE_PATH}:${index + 1}: unknown effect annotation ${fields[2]}`);
    }
    if (name.includes(".") && !names.has(name.split(".", 1)[0])) {
      fail(`${EFFECT_SOURCE_PATH}:${index + 1}: effect leaf ${name} has no declared root`);
    }
    names.add(name);
    declarations.push({ name, irreversible });
  }
  if (!declarations.length) fail(`${EFFECT_SOURCE_PATH}: no effect declarations`);

  const roots = declarations
    .filter((declaration) => !declaration.name.includes("."))
    .map((declaration) => declaration.name);
  const rootSet = new Set(roots);
  for (const internal of INTERNAL_EFFECT_NAMES) {
    if (rootSet.has(internal)) {
      fail(`${EFFECT_SOURCE_PATH}: ${internal} is compiler-internal and cannot be declared`);
    }
  }
  for (const build of BUILD_EFFECT_NAMES) {
    if (!rootSet.has(build)) {
      fail(`${EFFECT_SOURCE_PATH}: build effect ${build} has no declared root`);
    }
  }
  const irreversible = declarations
    .filter((declaration) => declaration.irreversible)
    .map((declaration) => declaration.name);
  const expectedIrreversible = new Set(["FS.Write", "Net", "Exec", "FFI"]);
  if (irreversible.length !== expectedIrreversible.size ||
      irreversible.some((name) => !expectedIrreversible.has(name))) {
    fail(`${EFFECT_SOURCE_PATH}: irreversible declarations must be FS.Write, Net, Exec, and FFI`);
  }
  return { declarations, roots };
}

function rustName(name) {
  if (!/^[A-Z][A-Za-z0-9]*$/.test(name)) {
    fail(`effect root cannot be represented as a Rust variant: ${name}`);
  }
  return name;
}

function replaceSection(source, begin, end, replacement, rel) {
  const starts = source.split(begin).length - 1;
  const ends = source.split(end).length - 1;
  if (starts !== 1 || ends !== 1) {
    fail(`${rel}: expected exactly one generated section (${begin}, ${end})`);
  }
  const start = source.indexOf(begin);
  const finish = source.indexOf(end, start);
  if (finish < start) fail(`${rel}: generated section markers are out of order`);
  return source.slice(0, start) + replacement + source.slice(finish + end.length);
}

function readSection(source, begin, end, rel) {
  const start = source.indexOf(begin);
  const finish = source.indexOf(end, start < 0 ? 0 : start);
  if (start < 0 || finish < 0 || finish < start) {
    fail(`${rel}: generated section markers are missing or out of order`);
  }
  return source.slice(start, finish + end.length);
}

function generatedEffects(source, facts) {
  const hash = sha256(source);
  const variants = [];
  for (const root of facts.roots.map(rustName)) {
    if (root === "FFI") variants.push("Panic");
    variants.push(root);
  }
  if (!variants.includes("Panic")) variants.push("Panic");
  if (!variants.includes("Mem")) variants.push("Mem");
  const lines = [
    EFFECT_BEGIN,
    `// Source: ${EFFECT_SOURCE_PATH}`,
    `// Source SHA-256: ${hash}`,
    'pub const EFFECT_SOURCE: &str = include_str!("../../jet-codegen/src/Prelude/Effects.jet");',
    "use std::sync::LazyLock;",
    "",
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
    "pub struct EffectDeclaration {",
    "    pub name: &'static str,",
    "    pub irreversible: bool,",
    "}",
    "",
    "pub static EFFECT_DECLARATIONS: LazyLock<Vec<EffectDeclaration>> = LazyLock::new(|| vec![",
  ];
  for (const declaration of facts.declarations) {
    lines.push(`    EffectDeclaration { name: "${declaration.name}", irreversible: ${declaration.irreversible} },`);
  }
  lines.push("]);", "");
  lines.push("pub static EFFECT_ROOTS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {");
  lines.push("    EFFECT_DECLARATIONS");
  lines.push("        .iter()");
  lines.push("        .map(|declaration| declaration.name)");
  lines.push("        .filter(|root| !root.contains('.'))");
  lines.push("        .collect()");
  lines.push("});", "");

  lines.push("pub fn effect_declarations() -> &'static [EffectDeclaration] {");
  lines.push("    EFFECT_DECLARATIONS.as_slice()");
  lines.push("}", "");
  lines.push("#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]");
  lines.push("pub enum Effect {");
  for (const variant of variants) lines.push(`    ${variant},`);
  lines.push("}", "", "impl Effect {");
  lines.push("    pub const fn name(self) -> &'static str {");
  lines.push("        match self {");
  for (const variant of variants) lines.push(`            Self::${variant} => "${variant}",`);
  lines.push("        }", "    }", "");
  lines.push("    pub fn parse(root: &str) -> Option<Self> {");
  lines.push("        let canonical = crate::Authority::parse_root(root)?;");
  lines.push("        if canonical != root {");
  lines.push("            return None;");
  lines.push("        }");
  lines.push("        Some(match canonical {");
  for (const variant of variants) lines.push(`            "${variant}" => Self::${variant},`);
  lines.push("            _ => return None,", "        })", "    }");
  lines.push("", "    pub fn all() -> crate::Authority::Holds {");
  lines.push("        EFFECT_ROOTS");
  lines.push("            .iter()");
  lines.push("            .map(|root| (*root).to_string())");
  lines.push("            .collect()", "    }");
  lines.push("}", EFFECT_END);
  return lines.join("\n");
}

function generatedBuildEffects(source, facts) {
  const hash = sha256(source);
  const lines = [
    BUILD_EFFECT_BEGIN,
    `// Source: ${EFFECT_SOURCE_PATH}`,
    `// Source SHA-256: ${hash}`,
    "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]",
    "pub enum BuildEffect {",
  ];
  for (const name of BUILD_EFFECT_NAMES) lines.push(`    ${rustName(name)},`);
  lines.push("}", "", "impl BuildEffect {");
  lines.push(`    pub const ALL: [Self; ${BUILD_EFFECT_NAMES.length}] = [`);
  for (const name of BUILD_EFFECT_NAMES) lines.push(`        Self::${rustName(name)},`);
  lines.push("    ];", "");
  lines.push("    pub const fn name(self) -> &'static str {");
  lines.push("        match self {");
  for (const name of BUILD_EFFECT_NAMES) lines.push(`            Self::${rustName(name)} => "${name}",`);
  lines.push("        }", "    }", "");
  lines.push("    pub const fn flag(self) -> &'static str {");
  lines.push("        match self {");
  for (const name of BUILD_EFFECT_NAMES) {
    lines.push(`            Self::${rustName(name)} => "${name.toLowerCase()}",`);
  }
  lines.push("        }", "    }", "");
  lines.push("    pub fn parse(value: &str) -> Option<Self> {");
  lines.push("        if value.contains('.') {");
  lines.push("            return None;");
  lines.push("        }");
  lines.push("        let canonical = crate::Authority::parse_root(value)?;");
  lines.push("        Self::ALL");
  lines.push("            .into_iter()");
  lines.push("            .find(|effect| canonical == effect.name())", "    }", "}");
  // Keep facts as an explicit input to make it impossible to silently emit a
  // build projection from a source that was parsed but not root-checked.
  if (!facts.roots.some((root) => BUILD_EFFECT_NAMES.includes(root))) {
    fail(`${EFFECT_SOURCE_PATH}: no build roots available`);
  }
  lines.push(BUILD_EFFECT_END);
  return lines.join("\n");
}

function ensureCoreNames() {
  const core = coreSourceFacts();
  if (!core.modules.some((entry) => entry.module === "core.tasks")) {
    fail(`${CORE_SOURCE_PATH}: missing canonical core.tasks module`);
  }
  if (core.modules.some((entry) => entry.module === "core.task")) {
    fail(`${CORE_SOURCE_PATH}: retired core.task alias is still declared`);
  }
  validatePublicDispatcher(core, publicDispatcherRows());
  return core;
}

function checkEffects(source, facts) {
  const expected = generatedEffects(source, facts);
  const actual = read(EFFECTS_PATH);
  const projection = readSection(actual, EFFECT_BEGIN, EFFECT_END, EFFECTS_PATH);
  if (projection !== expected) {
    fail(`${EFFECTS_PATH} is stale; run --write to regenerate from ${EFFECT_SOURCE_PATH}`);
  }
  const expectedBuild = generatedBuildEffects(source, facts);
  const actualBuild = read(BUILD_EFFECTS_PATH);
  const buildProjection = readSection(
    actualBuild,
    BUILD_EFFECT_BEGIN,
    BUILD_EFFECT_END,
    BUILD_EFFECTS_PATH,
  );
  if (buildProjection !== expectedBuild) {
    fail(`${BUILD_EFFECTS_PATH} is stale; run --write to regenerate from ${EFFECT_SOURCE_PATH}`);
  }
}

function writeEffects(source, facts) {
  const effects = replaceSection(
    read(EFFECTS_PATH),
    EFFECT_BEGIN,
    EFFECT_END,
    generatedEffects(source, facts),
    EFFECTS_PATH,
  );
  writeFileSync(file(EFFECTS_PATH), effects);
  const buildEffects = replaceSection(
    read(BUILD_EFFECTS_PATH),
    BUILD_EFFECT_BEGIN,
    BUILD_EFFECT_END,
    generatedBuildEffects(source, facts),
    BUILD_EFFECTS_PATH,
  );
  writeFileSync(file(BUILD_EFFECTS_PATH), buildEffects);
}

function check() {
  const source = read(EFFECT_SOURCE_PATH);
  const facts = parseEffects(source);
  const core = ensureCoreNames();
  checkEffects(source, facts);
  const coreSource = read(CORE_SOURCE_PATH);
  validateGeneratedViews(coreSource, core);
  process.stdout.write("open tables: generated views are current\n");
}

function write() {
  const source = read(EFFECT_SOURCE_PATH);
  const facts = parseEffects(source);
  ensureCoreNames();
  writeEffects(source, facts);
  // CoreModuleExports.rs and RingLayer.rs use the existing Core generator;
  // this coordinator never reimplements that schema.
  writeCoreViews();
  process.stdout.write("wrote effect and Core generated views\n");
}

function usage() {
  process.stdout.write("usage: gen-core-tables.mjs --check|--write\n");
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const args = process.argv.slice(2);
  try {
    if (args.includes("--check")) check();
    else if (args.includes("--write")) write();
    else if (args.includes("--help")) usage();
    else fail("usage: gen-core-tables.mjs --check|--write");
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}

export {
  parseEffects,
  generatedEffects,
  generatedBuildEffects,
  replaceSection,
};
