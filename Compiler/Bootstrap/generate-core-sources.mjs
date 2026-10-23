#!/usr/bin/env node
// Generate Foundation's shared source-ownership rows and the CLI's effect
// text from the Core declaration sources: every public Core source module
// (`source_module` rows of Compiler/Prelude/Core.jet), then the
// compiler-private parts (the rows of
// Compiler/Prelude/CoreSourceParts.tsv), then the runtime units
// (Core/*/Runtime/**/*.jet), and the canonical effect source
// (Compiler/Prelude/Effects.jet). The rows carry no body text:
// the CLI reads `<toolchain root>/<path>` at run time, so a Core edit needs no
// regeneration unless the row set changes.
// usage: node Compiler/Bootstrap/generate-core-sources.mjs [--check]
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { coreRuntimeParts, coreSourceFacts } from "../../Tools/agent/check-core-surface-ledger.mjs";

const bootstrapDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(bootstrapDir, "../..");
const corePath = "Compiler/Prelude/Core.jet";
const partsPath = "Compiler/Prelude/CoreSourceParts.tsv";
const runtimePath = "Core/*/Runtime/**/*.jet";
const effectSourceDisplay = "Compiler/Prelude/Effects.jet";
const outputPath = resolve(repoRoot, "Compiler/JetCli/Source/Cli/CoreSources.jet");
const ownershipPath = resolve(repoRoot, "Compiler/JetFoundation/Source/Registry/CoreSourceOwned.jet");

function fail(message) {
  throw new Error(`core-source-generator: ${message}`);
}

function jetStringLiteral(value, context) {
  let literal = '"';
  for (const character of value) {
    const codepoint = character.codePointAt(0);
    if (codepoint < 0x20 && character !== "\n" && character !== "\t") {
      fail(`control character U+${codepoint.toString(16)} in ${context}`);
    }
    if (character === "\\") literal += "\\\\";
    else if (character === '"') literal += '\\"';
    else if (character === "\n") literal += "\\n";
    else if (character === "\t") literal += "\\t";
    else if (character === "{") literal += "{{";
    else if (character === "}") literal += "}}";
    else literal += character;
  }
  return `${literal}"`;
}

const read = (path) => readFile(resolve(repoRoot, path), "utf8").catch((error) => fail(`cannot read ${path}: ${error.message}`));
const coreSource = await read(corePath);
const partsSource = await read(partsPath);
const effectSource = await read(effectSourceDisplay);
const declarations = coreSourceFacts();

const names = (list) => list.split(",").map((name) => name.trim()).filter(Boolean);

// Public rows, in Core.jet order.
const rows = declarations.sourceModules.map((entry) => ({
  module: entry.module, alias: entry.alias, path: entry.path, owner: "", members: entry.ownedMembers, implTypes: entry.implTypes, runtime: false,
}));
if (rows.length === 0) fail(`${corePath} declares no source modules`);

// Private parts, in CoreSourceParts.tsv order.
const partModules = new Set();
for (const [index, line] of partsSource.split("\n").entries()) {
  if (line === "" || line.startsWith("#")) continue;
  const fields = line.split("\t");
  if (fields.length !== 6 || fields.slice(0, 4).some((field) => field === "")) fail(`${partsPath}:${index + 1}: expected module, owner, path, alias, owns, impls`);
  const [module, owner, path, alias, owns, impls] = fields;
  if (partModules.has(module)) fail(`${partsPath}:${index + 1}: duplicate source part ${module}`);
  partModules.add(module);
  rows.push({ module, alias, path, owner, members: names(owns), implTypes: names(impls), runtime: false });
}
if (partModules.size === 0) fail(`${partsPath} declares no source parts`);

// Runtime units, in directory order.
const runtimeParts = coreRuntimeParts();
for (const part of runtimeParts) {
  rows.push({ module: part.module, alias: part.alias, path: part.path, owner: "", members: [], implTypes: [], runtime: true });
}

const digest = createHash("sha256");
for (const text of [coreSource, partsSource, runtimeParts.map((part) => part.path).join("\n"), effectSource]) digest.update(text);
const sourceHash = digest.digest("hex");
const list = (values) => `[String]{${values.map((value) => jetStringLiteral(value, corePath)).join(", ")}}`;
// Type exports come from the module's Core.jet declaration.
const typeNames = row => declarations.modules.find(module => module.module === row.module)?.types.map(type => type.name) ?? [];
const renderRow = row =>
  `    CoreSourceOwnedRow{module_name: ${jetStringLiteral(row.module, corePath)}, alias: ${jetStringLiteral(row.alias, corePath)}, path: ${jetStringLiteral(row.path, corePath)}, owner: ${jetStringLiteral(row.owner, partsPath)}, owned_members: ${list(row.members)}, type_exports: ${list(typeNames(row))}, impl_types: ${list(row.implTypes)}, runtime: ${row.runtime}},`;
const header = [
  `// Sources: ${corePath} (source_module rows), ${partsPath} (private parts), ${runtimePath} (runtime units), ${effectSourceDisplay} (effect source)`,
  `// Source SHA-256: ${sourceHash}`,
  "// This file is generated by Compiler/Bootstrap/generate-core-sources.mjs; do not edit.",
];
const ownership = [
  ...header,
  "struct CoreSourceOwnedRow {",
  "    module_name: String",
  "    alias: String",
  "    path: String",
  "    owner: String",
  "    owned_members: [String]",
  "    type_exports: [String]",
  "    impl_types: [String]",
  "    runtime: Bool",
  "}",
  "",
  "// A nominal is source-owned only when this registration row and a loaded",
  "// source declaration agree. Opaque impl ownership uses the module's row.",
  "CORE_SOURCE_OWNED_ROWS :: comptime { [CoreSourceOwnedRow]{",
  ...rows.map(renderRow),
  "} }",
  "",
].join("\n");
const output = [
  ...header,
  "",
  "// The canonical effect source, Compiler/Prelude/Effects.jet.",
  `JET_CLI_EFFECT_SOURCE :: comptime { ${jetStringLiteral(effectSource, effectSourceDisplay)} }`,
  "",
  "",
].join("\n");

if (process.argv.includes("--check")) {
  const current = await readFile(outputPath, "utf8").catch(() => "");
  const currentOwnership = await readFile(ownershipPath, "utf8").catch(() => "");
  if (current !== output || currentOwnership !== ownership) {
    console.error("Core source ownership views are stale; run node Compiler/Bootstrap/generate-core-sources.mjs");
    process.exit(1);
  }
  console.log(`core source rows are current (${rows.length} rows, source ${sourceHash})`);
} else {
  await writeFile(outputPath, output, "utf8");
  await writeFile(ownershipPath, ownership, "utf8");
  console.log(`generated ${outputPath.replace(`${repoRoot}/`, "")} (${rows.length} rows, source ${sourceHash})`);
}
