#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readdir, readFile, realpath, stat, writeFile, mkdir } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { loadMirSchema } from "../JetBackend/Tests/mir-debug.mjs";
import { jetWriter } from "../JetBackend/Tests/front-feed-codec.mjs";

const bootstrapDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(bootstrapDir, "../..");
const sourceRootDir = resolve(process.env.JET_BOOTSTRAP_SOURCE_ROOT ?? repoRoot);
const manifestPath = resolve(bootstrapDir, "sources.list");
const scratchDir = resolve(homedir(), ".cache/jet-luna/compiler-bootstrap");
const projectDir = resolve(scratchDir, "project");
const sourceDir = resolve(projectDir, "src");
const unitPath = resolve(sourceDir, "compiler.jet");
const packagePath = resolve(projectDir, "package.jet");
const mapPath = resolve(scratchDir, "compiler.map.json");
const packageSource = [
  'name: "compiler_bootstrap"',
  'version: "0.0.1"',
  'edition: "2028"',
  "",
].join("\n");
const sourceRoots = [
  "Compiler/JetLexer/Source",
  "Compiler/JetBuildSession/Source",
  "Compiler/JetFoundation/Source",
  "Compiler/JetBackend/Source",
  "Compiler/JetOptimizer/Source",
  "Compiler/JetParser/Source/Parser",
  "Compiler/JetSema/Source",
  "Compiler/JetCodegen/Source/Codegen",
  "Compiler/JetCodegen/Source/Emit",
  "Compiler/JetRepl/Source",
  "Compiler/JetEval/Source",
  "Compiler/JetDriver/Source",
  "Compiler/JetCli/Source",
  "Compiler/Bootstrap",
];
// Command ports with no CLI caller yet (Compiler/JetCli/Commands) and the REPL
// console, notebook and history-host ports (Compiler/JetRepl/Console) are their own
// packages, outside these source roots and the compiler assembly closure.
const excludedSourceRoots = [
  // Native host glue remains a Rust/backend concern, not aggregate Jet input.
  "Compiler/Bootstrap/Host",
];

function fail(message) {
  console.error(`jet-bootstrap: ${message}`);
  process.exit(1);
}

function containedPath(root, candidate) {
  const pathFromRoot = relative(root, candidate);
  return pathFromRoot === "" || (!isAbsolute(pathFromRoot) && pathFromRoot !== ".." && !pathFromRoot.startsWith(`..${sep}`));
}

function isExcludedSourcePath(sourcePath) {
  return excludedSourceRoots.some((root) => sourcePath === root || sourcePath.startsWith(`${root}/`));
}

async function jetFilesUnder(directory) {
  const files = [];
  const entries = await readdir(directory, { withFileTypes: true });
  entries.sort((left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0);
  for (const entry of entries) {
    const path = resolve(directory, entry.name);
    if (entry.isSymbolicLink()) fail(`source inventory contains a symlink: ${relative(sourceRootDir, path)}`);
    if (entry.isDirectory()) {
      files.push(...await jetFilesUnder(path));
    } else if (entry.isFile() && entry.name.endsWith(".jet") && entry.name !== "package.jet") {
      files.push(path);
    }
  }
  return files;
}

// D-MOD-CYCLE1=A: each Compiler/<Pkg>/ is a package, and its files import the
// packages they depend on with `use <package>.[names]`. The aggregate unit is
// one namespace, so those imports are blanked to spaces of the same length:
// every byte offset and line in the source map still matches the original.
// Repeated Core imports share one binding inside the aggregate. Grouped
// member imports are deduplicated per member, including overlapping groups,
// without shifting any surviving token, byte offset, or source line.
const PACKAGE_IMPORT = /^use (?:jet_foundation|jet_lexer|jet_parser|jet_optimizer|jet_sema|jet_codegen|jet_eval|jet_repl|jet_driver|jet_backend|jet_cli|compiler_bootstrap|jet_build_session|jetpack_foundation|jetpack_trust_root|jet_pkg_model|jet_env_model|jetpack_trust|jetpack_artifact_store)\.(?:\[[^\]]*\]|[A-Za-z_][A-Za-z0-9_]*[ \t]*$)/gm;
const CORE_IMPORT = /^use core\.[^\[\n]*$/gm;
const CORE_MEMBER_IMPORT = /^use (core(?:\.[A-Za-z_][A-Za-z0-9_]*)*)\.\[([^\]]*)\]/gm;
const seenCoreImports = new Set();
const seenCoreMembers = new Set();
const blank = (block) => block.replace(/[^\n]/g, " ");
function blankRepeatedCoreMembers(importText, module, members) {
  const parts = members.split(",");
  const bindings = parts.map((part) => part.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)(?:\s+as\s+([A-Za-z_][A-Za-z0-9_]*))?\s*$/));
  // Leave unsupported or commented forms to the parser; do not guess names.
  if (bindings.some((binding, index) => !binding && parts[index].trim() !== "")) return importText;
  let removed = false;
  const kept = bindings.map((binding) => {
    if (!binding) return false;
    const key = `${module}.${binding[1]} as ${binding[2] ?? binding[1]}`;
    if (seenCoreMembers.has(key)) { removed = true; return false; }
    seenCoreMembers.add(key);
    return true;
  });
  if (!removed) return importText;
  const last = kept.lastIndexOf(true);
  if (last < 0) return blank(importText);
  const rewritten = parts.map((part, index) =>
    (kept[index] ? part : blank(part)) +
    (index === parts.length - 1 ? "" : kept[index] && index < last ? "," : " ")
  ).join("");
  return importText.slice(0, importText.indexOf("[") + 1) + rewritten + "]";
}
function blankPackageImports(text) {
  return text.replace(PACKAGE_IMPORT, blank)
    .replace(CORE_MEMBER_IMPORT, blankRepeatedCoreMembers)
    .replace(CORE_IMPORT, (line) => {
    const key = line.trimEnd();
    if (!seenCoreImports.has(key)) {
      seenCoreImports.add(key);
      return line;
    }
    return blank(line);
  });
}

const manifestText = await readFile(manifestPath, "utf8").catch((error) => {
  fail(`cannot read source manifest ${relative(repoRoot, manifestPath)}: ${error.message}`);
});
const runtimeManifestPath = resolve(sourceRootDir, "Compiler/JetRuntime/sources.list");
const runtimeManifestText = await readFile(runtimeManifestPath, "utf8").catch((error) => {
  fail(`cannot read runtime source manifest ${relative(sourceRootDir, runtimeManifestPath)}: ${error.message}`);
});
const runtimeSources = new Set(runtimeManifestText.split(/\r?\n/)
  .map((line) => line.trim()).filter((line) => line !== "" && !line.startsWith("#")));


const entries = [];
const seen = new Set();
for (const [index, rawLine] of manifestText.split(/\r?\n/).entries()) {
  const sourcePath = rawLine.trim();
  if (sourcePath === "" || sourcePath.startsWith("#")) continue;
  const absolutePath = resolve(sourceRootDir, sourcePath);
  const normalizedPath = relative(sourceRootDir, absolutePath).split(sep).join("/");
  if (isAbsolute(sourcePath) || !containedPath(sourceRootDir, absolutePath) || !sourcePath.endsWith(".jet")) {
    fail(`${relative(repoRoot, manifestPath)}:${index + 1}: expected a repository-relative .jet source path`);
  }
  if (sourcePath !== normalizedPath) fail(`${relative(repoRoot, manifestPath)}:${index + 1}: use the canonical path ${normalizedPath}`);
  if (seen.has(sourcePath)) fail(`${relative(repoRoot, manifestPath)}:${index + 1}: duplicate source ${sourcePath}`);
  seen.add(sourcePath);
  let canonicalPath;
  try {
    canonicalPath = await realpath(absolutePath);
  } catch (error) {
    fail(`${relative(repoRoot, manifestPath)}:${index + 1}: missing source ${sourcePath}: ${error.message}`);
  }
  if (!containedPath(sourceRootDir, canonicalPath)) fail(`${sourcePath}: source resolves outside the source root`);
  const metadata = await stat(canonicalPath);
  if (!metadata.isFile()) fail(`${sourcePath}: source is not a regular file`);
  const bytes = await readFile(canonicalPath);
  const text = bytes.toString("utf8");
  if (!Buffer.from(text, "utf8").equals(bytes)) fail(`${sourcePath}: source is not valid UTF-8`);
  entries.push({ sourcePath, bytes, text });
}
if (entries.length === 0) fail(`${relative(repoRoot, manifestPath)} contains no sources`);
// `--task-roots` adds the task-root fixture after the manifest sources, as the
// self-compile harness's `task-roots` mode and the fixed point's
// KEEP_TASK_ROOTS=true need. It is a jet_eval package file outside the source
// roots, so it is assembled like one but never inventoried.
const taskRoots = process.argv.slice(2).includes("--task-roots");
if (taskRoots) {
  const sourcePath = "Compiler/JetEval/Tests/TaskRoots.jet";
  const bytes = await readFile(resolve(sourceRootDir, sourcePath)).catch((error) => {
    fail(`cannot read the task-root fixture ${sourcePath}: ${error.message}`);
  });
  const text = bytes.toString("utf8");
  if (!Buffer.from(text, "utf8").equals(bytes)) fail(`${sourcePath}: source is not valid UTF-8`);
  entries.push({ sourcePath, bytes, text });
}

// A focused compiler fixture still checks the full canonical unit. Append
// exactly one explicit Tests source using the same import blanking and source
// provenance as production files; never check a package leaf in isolation.
const fixtureArguments = process.argv.slice(2);
const fixtureAt = fixtureArguments.indexOf("--fixture");
if (fixtureAt !== -1) {
  if (fixtureArguments.lastIndexOf("--fixture") !== fixtureAt) fail("--fixture accepts exactly one Tests source per unit");
  const sourcePath = fixtureArguments[fixtureAt + 1] ?? "";
  if (!/^Compiler\/[A-Za-z0-9_-]+\/Tests\/(?:[A-Za-z0-9_-]+\/)*[A-Za-z0-9_-]+\.jet$/.test(sourcePath)) {
    fail("--fixture needs a canonical Compiler/<Package>/Tests/<name>.jet path");
  }
  const absolutePath = resolve(sourceRootDir, sourcePath);
  const canonicalPath = await realpath(absolutePath).catch((error) => {
    fail(`cannot read fixture ${sourcePath}: ${error.message}`);
  });
  if (!containedPath(sourceRootDir, canonicalPath)) fail(`${sourcePath}: fixture resolves outside the source root`);
  const metadata = await stat(canonicalPath);
  if (!metadata.isFile()) fail(`${sourcePath}: fixture is not a regular file`);
  const bytes = await readFile(canonicalPath);
  const text = bytes.toString("utf8");
  if (!Buffer.from(text, "utf8").equals(bytes)) fail(`${sourcePath}: source is not valid UTF-8`);
  if (entries.some((entry) => entry.sourcePath === sourcePath)) fail(`${sourcePath}: fixture already appears in the unit`);
  entries.push({ sourcePath, bytes, text });
}

const inventoried = new Set();
for (const sourceRoot of sourceRoots) {
  const absoluteRoot = resolve(sourceRootDir, sourceRoot);
  const paths = await jetFilesUnder(absoluteRoot).catch((error) => {
    fail(`cannot inventory ${sourceRoot}: ${error.message}`);
  });
  for (const path of paths) {
    const sourcePath = relative(sourceRootDir, path).split(sep).join("/");
    // Runtime-only ABI units belong to their own aggregate. Explicit shared
    // sources (for example OS/Linux.jet) still join the compiler inventory.
    if (!runtimeSources.has(sourcePath) || seen.has(sourcePath)) inventoried.add(sourcePath);
  }
}
const unlisted = [...inventoried].filter((sourcePath) => !seen.has(sourcePath) && !isExcludedSourcePath(sourcePath)).sort();
if (unlisted.length > 0) {
  fail(`Jet compiler sources are not in ${relative(repoRoot, manifestPath)}; add them in explicit order:\n${unlisted.map((path) => `  ${path}`).join("\n")}`);
}

// Private host provenance is a file header, never an incidental source comment.
// Account for it in the map without changing any original source byte range.
const privateHostHeader = entries.some(({ sourcePath }) => sourcePath === "Compiler/Bootstrap/Host/Entry.jet")
  ? "// [jet-bootstrap private host: Compiler/Bootstrap/Host/Entry.jet]\n"
  : "";
let output = privateHostHeader;
let generatedByte = Buffer.byteLength(privateHostHeader, "utf8");
let nextLine = privateHostHeader === "" ? 1 : 2;
const mappedSources = [];
for (const { sourcePath, bytes, text } of entries) {
  // Keep source offsets unchanged; provenance markers are outside each byte range.
  const marker = `// [jet-bootstrap source: ${sourcePath}]\n`;
  output += marker;
  generatedByte += Buffer.byteLength(marker, "utf8");
  nextLine += 1;

  const generatedStartByte = generatedByte;
  const generatedStartLine = nextLine;
  output += blankPackageImports(text);
  generatedByte += bytes.length;
  const newlineCount = (text.match(/\n/g) ?? []).length;
  const sourceLineCount = text.length === 0 ? 0 : newlineCount + (text.endsWith("\n") ? 0 : 1);
  const lineStarts = [0];
  for (let byteIndex = 0; byteIndex < bytes.length; byteIndex += 1) {
    if (bytes[byteIndex] === 10) lineStarts.push(byteIndex + 1);
  }
  nextLine += newlineCount;
  if (!text.endsWith("\n")) {
    output += "\n";
    generatedByte += 1;
    nextLine += 1;
  }

  mappedSources.push({
    path: sourcePath,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    original_bytes: bytes.length,
    original_lines: sourceLineCount,
    line_starts: lineStarts,
    generated_start_byte: generatedStartByte,
    generated_end_byte_exclusive: generatedStartByte + bytes.length,
    generated_start_line: generatedStartLine,
    generated_end_line: generatedStartLine + sourceLineCount - 1,
  });
}
// Diagnostic observation only; native lowering still consumes the live MIR.
// Keep generated helper spans separate from original-source provenance.
const observer = jetWriter(loadMirSchema(sourceRootDir));
const observerMarker = "// [jet-bootstrap generated: MIR observation writer]\n";
const observerStart = generatedByte + Buffer.byteLength(observerMarker, "utf8");
output += observerMarker + observer;
const observerMap = {
  generator: "Compiler/JetBackend/Tests/front-feed-codec.mjs",
  generated: { start: observerStart, end: observerStart + Buffer.byteLength(observer, "utf8") },
  sha256: createHash("sha256").update(observer).digest("hex"),
};

await mkdir(sourceDir, { recursive: true });
await writeFile(unitPath, output, "utf8");
await writeFile(packagePath, packageSource, "utf8");
await writeFile(mapPath, `${JSON.stringify({
  schema: "jet-bootstrap-source-map/v2",
  module: "Compiler",
  mir_observer: observerMap,
  fallback_source_file: "Compiler",
  // `generated` uses aggregate Parser spans; line starts are fragment-local byte offsets.
  source_segments: mappedSources.map(({ path, generated_start_byte, generated_end_byte_exclusive, line_starts }) => ({
    generated: { start: generated_start_byte, end: generated_end_byte_exclusive },
    source_file: path,
    line_starts: line_starts,
  })),
  files: mappedSources,
}, null, 2)}\n`, "utf8");
console.log(`assembled ${entries.length} canonical Jet sources into ${unitPath}`);
console.log(`source provenance: ${mapPath}`);
