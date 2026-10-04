#!/usr/bin/env node
// Generate the private payload for Foundation's canonical embedded-Core
// accessor from the frozen native metadata and the source files it embeds.
// Public rows and private parts retain native order and carry real body text.
// CLI, Driver and tools consume Foundation; no client owns another table.
// usage: node Compiler/Bootstrap/generate-core-sources.mjs [--check]
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const bootstrapDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(bootstrapDir, "../..");
const exportsPath = "crates/jet-foundation/src/CoreModuleExports.rs";
const partsPath = "crates/jet-foundation/src/CoreSourceParts.rs";
const effectsPath = "crates/jet-foundation/src/Effects.rs";
const declarationPath = "crates/jet-codegen/src/Prelude/Core.jet";
const tagsPath = "crates/jet-foundation/src/Syntax/effects_surface.rs";
const outputPath = resolve(repoRoot, "Compiler/JetFoundation/Source/Registry/EmbeddedCoreData.jet");

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
const exportsSource = await read(exportsPath);
const partsSource = await read(partsPath);
const effectsRust = await read(effectsPath);
const declarationSource = await read(declarationPath);
const tagsSource = await read(tagsPath);

const members = (list) => [...list.matchAll(/"([^"]*)"/g)].map((match) => match[1]);

// Public rows, in table order.
const tableStart = exportsSource.indexOf("pub const CORE_SOURCE_MODULES: &[CoreSourceModule] = &[");
if (tableStart < 0) fail(`${exportsPath} has no CORE_SOURCE_MODULES table`);
const tableEnd = exportsSource.indexOf("\n];", tableStart);
if (tableEnd < 0) fail(`${exportsPath}: CORE_SOURCE_MODULES is not closed`);
const rowPattern = /CoreSourceModule \{ module: "([^"]*)", alias: "([^"]*)", path: "([^"]*)", owned_members: &\[([^\]]*)\] \}/g;
const rows = [];
for (const match of exportsSource.slice(tableStart, tableEnd).matchAll(rowPattern)) {
  rows.push({ module: match[1], alias: match[2], path: match[3], owner: "", members: members(match[4]) });
}
const tableRowCount = (exportsSource.slice(tableStart, tableEnd).match(/CoreSourceModule \{/g) ?? []).length;
if (rows.length === 0 || rows.length !== tableRowCount) fail(`${exportsPath}: parsed ${rows.length} of ${tableRowCount} CORE_SOURCE_MODULES rows`);
const publicRowCount = rows.length;

// Private parts, in CORE_PRIVATE_SOURCE_PARTS order. Their fields name
// string constants declared in the same file.
const constants = new Map();
for (const match of partsSource.matchAll(/pub const ([A-Z_]+): &str = "([^"]*)";/g)) constants.set(match[1], match[2]);
const field = (body, name, context) => {
  const match = body.match(new RegExp(`\\b${name}: (?:"([^"]*)"|([A-Z_]+))`));
  if (!match) fail(`${partsPath}: ${context} has no \`${name}\``);
  if (match[1] !== undefined) return match[1];
  if (!constants.has(match[2])) fail(`${partsPath}: ${context} names unknown constant ${match[2]}`);
  return constants.get(match[2]);
};
const partsList = partsSource.match(/pub const CORE_PRIVATE_SOURCE_PARTS: &\[CorePrivateSourcePart\] =\s*&\[([^\]]*)\];/);
if (!partsList) fail(`${partsPath} has no CORE_PRIVATE_SOURCE_PARTS list`);
for (const name of partsList[1].split(",").map((part) => part.trim()).filter(Boolean)) {
  const declaration = partsSource.match(new RegExp(`pub const ${name}: CorePrivateSourcePart = CorePrivateSourcePart \\{([\\s\\S]*?)\\n\\};`));
  if (!declaration) fail(`${partsPath}: private part ${name} is not declared`);
  const body = declaration[1];
  const owned = body.match(/owned_members: &\[([^\]]*)\]/);
  if (!owned) fail(`${partsPath}: ${name} has no \`owned_members\``);
  rows.push({
    module: field(body, "module", name),
    alias: field(body, "alias", name),
    path: field(body, "path", name),
    owner: field(body, "owner", name),
    members: members(owned[1]),
  });
}

// The canonical effect source is the file Effects.rs embeds.
const effectsInclude = effectsRust.match(/pub const EFFECT_SOURCE: &str = include_str!\("([^"]+)"\);/);
if (!effectsInclude) fail(`${effectsPath}: EFFECT_SOURCE is not an include_str! of one file`);
const effectSourcePath = resolve(dirname(resolve(repoRoot, effectsPath)), effectsInclude[1]);
const effectSourceDisplay = effectSourcePath.replace(`${repoRoot}/`, "");
const effectSource = await readFile(effectSourcePath, "utf8").catch((error) => fail(`cannot read ${effectSourceDisplay}: ${error.message}`));

const tagsMatch = tagsSource.match(/pub const BUILTIN_TAGS: &\[&str\] = &\[([^\]]*)\];/);
if (!tagsMatch) fail(`${tagsPath}: missing BUILTIN_TAGS`);
const tags = members(tagsMatch[1]);
const effects = [];
for (const line of effectSource.split("\n")) {
  const text = line.trim();
  if (!text || text.startsWith("//")) continue;
  const match = text.match(/^effect ([A-Za-z_][A-Za-z0-9_.]*)( \$irreversible)?$/);
  if (!match) fail(`${effectSourceDisplay}: invalid declaration ${text}`);
  effects.push({ name: match[1], irreversible: Boolean(match[2]) });
}
if (!effects.length || !tags.length) fail("empty effect or tag vocabulary");
const digest = createHash("sha256");
for (const text of [exportsSource, partsSource, declarationSource, effectSource, tagsSource]) digest.update(text);
for (const row of rows) {
  row.source = await read(row.path);
  if (!row.source.trim()) fail(`${row.path}: empty Core source body`);
  digest.update(row.source);
}
const sourceHash = digest.digest("hex");
const list = (values) => `[String]{${values.map((value) => jetStringLiteral(value, exportsPath)).join(", ")}}`;
const renderRow = (row, index) => [
  `fn foundation_embedded_core_source_${index}() -> FoundationCoreSource Never! {`,
  `    FoundationCoreSource{module_name: ${jetStringLiteral(row.module, exportsPath)}, alias: ${jetStringLiteral(row.alias, exportsPath)}, path: ${jetStringLiteral(row.path, exportsPath)}, owner: ${jetStringLiteral(row.owner, partsPath)}, owned_members: ${list(row.members)}, source: ${jetStringLiteral(row.source, row.path)}}`,
  "}",
  "",
].join("\n");
const sourceList = (name, start, end) => [
  `fn foundation_embedded_core_${name}_sources() -> [FoundationCoreSource] Never! {`,
  "    [FoundationCoreSource]{",
  ...rows.slice(start, end).map((_, index) => `        foundation_embedded_core_source_${start + index}(),`),
  "    }",
  "}",
  "",
].join("\n");
const output = [
  `// Sources: ${exportsPath}, ${partsPath}, ${declarationPath}, ${effectSourceDisplay}, ${tagsPath}, and their Core source bodies.`,
  `// Source SHA-256: ${sourceHash}`,
  "// Generated by Compiler/Bootstrap/generate-core-sources.mjs; do not edit.",
  "// Private payload of FoundationEmbeddedCore, never a client-owned registry.",
  "",
  "fn foundation_embedded_core_declaration_text() -> String Never! {",
  `    ${jetStringLiteral(declarationSource, declarationPath)}`,
  "}",
  "",
  "fn foundation_embedded_core_effect_source() -> String Never! {",
  `    ${jetStringLiteral(effectSource, effectSourceDisplay)}`,
  "}",
  "",
  "fn foundation_embedded_core_builtin_tags() -> [String] Never! {",
  `    ${list(tags)}`,
  "}",
  "",
  "fn foundation_embedded_core_effect_declarations() -> [FoundationCoreEffectDeclaration] Never! {",
  "    [FoundationCoreEffectDeclaration]{",
  ...effects.map((effect) => `        FoundationCoreEffectDeclaration{name: ${jetStringLiteral(effect.name, effectSourceDisplay)}, irreversible: ${effect.irreversible}},`),
  "    }",
  "}",
  "",
  sourceList("public", 0, publicRowCount),
  sourceList("private", publicRowCount, rows.length),
  ...rows.map(renderRow),
].join("\n");

if (process.argv.includes("--check")) {
  const current = await readFile(outputPath, "utf8").catch(() => "");
  if (current !== output) {
    console.error(`${outputPath.replace(`${repoRoot}/`, "")} is stale; run node Compiler/Bootstrap/generate-core-sources.mjs`);
    process.exit(1);
  }
  console.log(`embedded Core payload is current (${rows.length} source bodies, source ${sourceHash})`);
} else {
  await writeFile(outputPath, output, "utf8");
  console.log(`generated ${outputPath.replace(`${repoRoot}/`, "")} (${rows.length} source bodies, source ${sourceHash})`);
}
