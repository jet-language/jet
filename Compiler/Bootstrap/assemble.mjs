#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readdir, readFile, realpath, stat, writeFile, mkdir } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

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
  'version: "0.1.0"',
  'edition: "2028"',
  "",
].join("\n");
const sourceRoots = [
  "Compiler/JetLexer/Source",
  "Compiler/JetFoundation/Source",
  "Compiler/JetMirPasses/Source",
  "Compiler/JetAst/Source",
  "Compiler/JetParser/Source/Parser",
  "Compiler/JetSema/Source",
  "Compiler/JetCodegen/Source/Codegen",
  "Compiler/JetCodegen/Source/Emit",
  "Compiler/RuntimeBridge/Source",
  "Compiler/JetEval/Source",
  "Compiler/JetDriver/Source",
  "Compiler/Bootstrap",
];
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
    } else if (entry.isFile() && entry.name.endsWith(".jet")) {
      files.push(path);
    }
  }
  return files;
}

const manifestText = await readFile(manifestPath, "utf8").catch((error) => {
  fail(`cannot read source manifest ${relative(repoRoot, manifestPath)}: ${error.message}`);
});


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

const inventoried = new Set();
for (const sourceRoot of sourceRoots) {
  const absoluteRoot = resolve(sourceRootDir, sourceRoot);
  const paths = await jetFilesUnder(absoluteRoot).catch((error) => {
    fail(`cannot inventory ${sourceRoot}: ${error.message}`);
  });
  for (const path of paths) inventoried.add(relative(sourceRootDir, path).split(sep).join("/"));
}
const unlisted = [...inventoried].filter((sourcePath) => !seen.has(sourcePath) && !isExcludedSourcePath(sourcePath)).sort();
if (unlisted.length > 0) {
  fail(`Jet compiler sources are not in ${relative(repoRoot, manifestPath)}; add them in explicit order:\n${unlisted.map((path) => `  ${path}`).join("\n")}`);
}

let output = "";
let generatedByte = 0;
let nextLine = 1;
const mappedSources = [];
for (const { sourcePath, bytes, text } of entries) {
  // Keep source bytes unchanged; provenance markers are outside each byte range.
  const marker = `// [jet-bootstrap source: ${sourcePath}]\n`;
  output += marker;
  generatedByte += Buffer.byteLength(marker, "utf8");
  nextLine += 1;

  const generatedStartByte = generatedByte;
  const generatedStartLine = nextLine;
  output += text;
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

await mkdir(sourceDir, { recursive: true });
await writeFile(unitPath, output, "utf8");
await writeFile(packagePath, packageSource, "utf8");
await writeFile(mapPath, `${JSON.stringify({
  schema: "jet-bootstrap-source-map/v2",
  module: "Compiler",
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
