#!/usr/bin/env node
// Assemble the Jetpack Jet sources into one unqualified unit. `sources.list` is
// the only implementation order and `tests.list` the only test order; nothing is
// globbed into a unit.
//
// Modes:
//   check  implementation + tests as `#Test` claims, for `jet check` (tests are
//          type-checked in the fast loop, not first discovered by --tests)
//   aot    the same unit, for `jet test`
//   run    implementation + the same test bodies as functions, driven by one
//          generated `fn run()` for the default `jet run` tier (I9)
//   hosted implementation only + Jetpack/run.jet, for the standalone native
//          `jetpack` engine binary (`jet build`)
//   publication  INTERNAL Store implementation closure + the one registered
//                native runtime-pack publisher entry; no test driver
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readdir, readFile, realpath, stat, writeFile, mkdir, rm, copyFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { compilerSliceDrift } from "./compiler-slice.mjs";

const mode = process.argv[2] ?? "check";
if (!["check", "aot", "run", "hosted", "publication"].includes(mode)) fail(`unknown mode ${mode}; expected check, aot, run, hosted, or internal publication`);

const bootstrapDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(bootstrapDir, "../..");
const sourceRootDir = resolve(process.env.JETPACK_BOOTSTRAP_SOURCE_ROOT ?? repoRoot);
// Several writers may share one checkout. JETPACK_BOOTSTRAP_AREAS selects the
// `# == Area ==` sections to assemble (unset: all), and each worker's unit lives
// in its own scratch directory keyed by source root, selection and worker name.
const selectedAreas = mode === "publication"
  ? new Set(execFileSync(process.execPath, [resolve(bootstrapDir, "areas.mjs"), "closure", "Store"], {
    cwd: sourceRootDir,
    env: { ...process.env, JETPACK_BOOTSTRAP_SOURCE_ROOT: sourceRootDir },
    encoding: "utf8",
  }).trim().split(","))
  : process.env.JETPACK_BOOTSTRAP_AREAS
    ? new Set(process.env.JETPACK_BOOTSTRAP_AREAS.split(",").map((area) => area.trim()).filter(Boolean))
    : null;
if (mode === "hosted" && selectedAreas !== null) fail("hosted builds require every Jetpack area");
const workerName = process.env.JETPACK_WORKER ?? "lead";
if (!/^[A-Za-z0-9_-]+$/.test(workerName)) fail("JETPACK_WORKER must be a plain name");
const rootTag = createHash("sha256")
  .update(`${sourceRootDir}\0${selectedAreas ? [...selectedAreas].sort().join(",") : "*"}`)
  .digest("hex")
  .slice(0, 10);
const scratchDir = resolve(
  process.env.JETPACK_BOOTSTRAP_SCRATCH ?? resolve(homedir(), ".cache/jet-dev/jetpack-bootstrap", `${workerName}-${rootTag}`),
  mode,
);
const projectDir = resolve(scratchDir, "project");
const sourceDir = resolve(projectDir, "src");
const unitPath = resolve(sourceDir, "jetpack.jet");
const packagePath = resolve(projectDir, "package.jet");
const mapPath = resolve(scratchDir, "jetpack.map.json");
// Jetpack is a package/environment engine: it reads and writes the store,
// spawns builders, fetches over the network, and reads the environment.
const packageSource = [
  'name: "jetpack_bootstrap"',
  'version: "0.1.0"',
  'edition: "2028"',
  "authority: {",
  "    holds: {",
  "        allow: [Env, Exec, Exec.Args, FS, FS.Read, IO, Mem.Alloc, Mem.Rc, Net, Rand, Time, Time.Wait]",
  "    }",
  "}",
  "",
].join("\n");
// Shared dependencies reference canonical Compiler/ sources in this checkout.
const allowedPrefixes = ["Jetpack/", "Compiler/"];
const PUBLICATION_ENTRY = "Tools/packaging/native-runtime-pack-publisher.jet";

function fail(message) {
  console.error(`jetpack-bootstrap: ${message}`);
  process.exit(1);
}

function containedPath(root, candidate) {
  const fromRoot = relative(root, candidate);
  return fromRoot === "" || (!isAbsolute(fromRoot) && fromRoot !== ".." && !fromRoot.startsWith(`..${sep}`));
}

async function jetFilesUnder(directory) {
  const files = [];
  const entries = await readdir(directory, { withFileTypes: true }).catch(() => []);
  entries.sort((left, right) => (left.name < right.name ? -1 : left.name > right.name ? 1 : 0));
  for (const entry of entries) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await jetFilesUnder(path)));
    else if (entry.isFile() && entry.name.endsWith(".jet")) files.push(path);
  }
  return files;
}

// One opt-in flag freezes dependency manifests and bytes to HEAD. By default
// every reference reads its live canonical path. The proved area's own sources
// always remain live when JETPACK_BOOTSTRAP_OWN_AREA selects one.
const depsFromHead = process.env.JETPACK_BOOTSTRAP_DEPS_FROM_HEAD === "1";
function headFile(path) {
  try {
    return execFileSync("git", ["show", `HEAD:${path}`], { cwd: sourceRootDir, maxBuffer: 256 * 1024 * 1024 });
  } catch (error) {
    fail(`--deps-from-head: ${path} is not in HEAD (${error.message.split("\n")[0]})`);
  }
}
function mergedManifest(workingText, name) {
  if (!depsFromHead) return workingText;
  const own = process.env.JETPACK_BOOTSTRAP_OWN_AREA;
  const headSections = new Map();
  let current = null;
  for (const line of headFile(`Jetpack/Bootstrap/${name}`).toString("utf8").split(/\r?\n/)) {
    const header = /^#\s*==\s*([^=(]+?)\s*(?:\(.*\))?\s*==\s*$/.exec(line.trim());
    if (header) current = header[1];
    else if (current !== null) headSections.set(current, [...(headSections.get(current) ?? []), line]);
  }
  const out = [];
  current = null;
  for (const line of workingText.split(/\r?\n/)) {
    const header = /^#\s*==\s*([^=(]+?)\s*(?:\(.*\))?\s*==\s*$/.exec(line.trim());
    if (header) {
      current = header[1];
      out.push(line);
      if (current !== own) out.push(...(headSections.get(current) ?? []));
    } else if (current === null || current === own) {
      out.push(line);
    }
  }
  return out.join("\n");
}

// `embed_file("rel")` is written relative to its own source file, as in any
// package. The assembled unit lives elsewhere and an embed may not leave its
// project (E0957), so each embedded file is copied into the unit's project as
// `src/embedded/<repository path>.embed` and the call is rebased onto that
// copy. The suffix keeps an embedded `.jet` asset from becoming a package
// source file; the embedded file itself must stay inside the source root.
const embeddedFiles = new Map();
function rebaseEmbeds(text, sourcePath, where) {
  return text.replace(/embed_file\("([^"\\{}]+)"\)/g, (whole, rel) => {
    if (isAbsolute(rel)) fail(`${where}: ${sourcePath}: embed_file paths are relative to their source file`);
    const target = resolve(sourceRootDir, dirname(sourcePath), rel);
    if (!containedPath(sourceRootDir, target)) fail(`${where}: ${sourcePath}: embed_file("${rel}") resolves outside the source root`);
    const copy = `embedded/${relative(sourceRootDir, target).split(sep).join("/")}.embed`;
    embeddedFiles.set(copy, target);
    return `embed_file("${copy}")`;
  });
}

async function readManifest(name) {
  const manifestPath = resolve(sourceRootDir, "Jetpack/Bootstrap", name);
  const label = relative(sourceRootDir, manifestPath);
  const workingText = await readFile(manifestPath, "utf8").catch((error) => fail(`cannot read ${label}: ${error.message}`));
  const text = mergedManifest(workingText, name);
  const entries = [];
  // Section headers look like `# == PackageModel ==`; the name is the text before any `(`.
  let section = null;
  for (const [index, rawLine] of text.split(/\r?\n/).entries()) {
    const sourcePath = rawLine.trim();
    const header = /^#\s*==\s*([^=(]+?)\s*(?:\(.*\))?\s*==\s*$/.exec(sourcePath);
    if (header !== null) {
      section = header[1];
      continue;
    }
    if (sourcePath === "" || sourcePath.startsWith("#")) continue;
    const where = `${label}:${index + 1}`;
    const absolutePath = resolve(sourceRootDir, sourcePath);
    const normalized = relative(sourceRootDir, absolutePath).split(sep).join("/");
    if (isAbsolute(sourcePath) || !containedPath(sourceRootDir, absolutePath) || !sourcePath.endsWith(".jet")) {
      fail(`${where}: expected a repository-relative .jet source path`);
    }
    if (sourcePath !== normalized) fail(`${where}: use the canonical path ${normalized}`);
    const publicationEntry = sourcePath === PUBLICATION_ENTRY && name === "sources.list" && section === "Store";
    if (!publicationEntry && !allowedPrefixes.some((prefix) => sourcePath.startsWith(prefix))) {
      fail(`${where}: sources must live under ${allowedPrefixes.join(" or ")}`);
    }
    const area = /^Jetpack\/([^/]+)\//.exec(sourcePath)?.[1];
    if (area !== undefined && area !== section) fail(`${where}: ${sourcePath} belongs under the \`# == ${area} ==\` section`);
    if (section === null) fail(`${where}: every source belongs to a \`# == Area ==\` section`);
    if (selectedAreas !== null && !selectedAreas.has(section)) continue;
    // Dependency references have the same live/HEAD rule, never a per-file
    // fallback to a different implementation.
    const fromHead = depsFromHead && section !== process.env.JETPACK_BOOTSTRAP_OWN_AREA;
    let bytes;
    if (fromHead) {
      bytes = headFile(sourcePath);
    } else {
      let canonicalPath;
      try {
        canonicalPath = await realpath(absolutePath);
      } catch (error) {
        fail(`${where}: missing source ${sourcePath}: ${error.message}`);
      }
      if (!containedPath(sourceRootDir, canonicalPath)) fail(`${sourcePath}: source resolves outside the source root`);
      if (!(await stat(canonicalPath)).isFile()) fail(`${sourcePath}: source is not a regular file`);
      bytes = await readFile(canonicalPath);
    }
    const sourceText = bytes.toString("utf8");
    if (!Buffer.from(sourceText, "utf8").equals(bytes)) fail(`${sourcePath}: source is not valid UTF-8`);
    entries.push({ sourcePath, bytes, text: rebaseEmbeds(sourceText, sourcePath, where), where, section });
  }
  return entries;
}

// Registration/inventory still sees the fixed entry in ordinary modes, but
// only the internal publication artifact emits it.
const registeredSources = await readManifest("sources.list");
// The compiler slice is closed under the compiler's packages; a use that only an
// unlisted slice-package member file declares fails the assembly.
const sliceDrift = compilerSliceDrift(sourceRootDir);
const sliceFailures = sliceDrift.filter((problem) => !problem.includes("outside the compiler slice"));
if (sliceFailures.length > 0) fail(`the Jetpack compiler slice drifted from the compiler's packages:\n${sliceFailures.map((problem) => `  ${problem}`).join("\n")}`);
for (const problem of sliceDrift.filter((problem) => problem.includes("outside the compiler slice"))) console.log(`note: ${problem}`);
const sources = registeredSources.filter((entry) => mode === "publication" || entry.sourcePath !== PUBLICATION_ENTRY);
if (mode === "publication" && registeredSources.filter((entry) => entry.sourcePath === PUBLICATION_ENTRY).length !== 1) {
  fail(`internal publication requires exactly one Store registration of ${PUBLICATION_ENTRY}`);
}
const tests = await readManifest("tests.list");
if (selectedAreas !== null) {
  const known = new Set((await readFile(resolve(sourceRootDir, "Jetpack/Bootstrap/sources.list"), "utf8"))
    .split(/\r?\n/)
    .map((line) => /^#\s*==\s*([^=(]+?)\s*(?:\(.*\))?\s*==\s*$/.exec(line.trim())?.[1])
    .filter(Boolean));
  for (const area of selectedAreas) if (!known.has(area)) fail(`JETPACK_BOOTSTRAP_AREAS names unknown section ${area}`);
}
const seen = new Set();
for (const entry of [...registeredSources, ...tests]) {
  if (seen.has(entry.sourcePath)) fail(`${entry.where}: duplicate source ${entry.sourcePath}`);
  seen.add(entry.sourcePath);
}
for (const entry of registeredSources) {
  if (/\/Tests\//.test(entry.sourcePath)) fail(`${entry.where}: test sources belong in tests.list`);
}
for (const entry of tests) {
  if (!/^(?:Jetpack\/[^/]+|Compiler\/JetFoundation)\/Tests\//.test(entry.sourcePath)) fail(`${entry.where}: tests.list names only Jetpack/<Area>/Tests or shared Compiler/JetFoundation/Tests sources`);
}

// Every Jetpack source and test file must be listed; an unlisted file is
// either dead code or an ordering decision nobody made. When one area is being
// proved (JETPACK_BOOTSTRAP_OWN_AREA), only that area must be complete: its
// dependency areas may hold another writer's unlisted work in progress, which
// is left out of the unit and noted, never a reason to fail this proof.
const ownArea = mode === "publication" ? null : process.env.JETPACK_BOOTSTRAP_OWN_AREA ?? null;
const inventoried = [];
const areas = await readdir(resolve(sourceRootDir, "Jetpack"), { withFileTypes: true }).catch(() => []);
for (const area of areas) {
  if (!area.isDirectory() || area.name === "Bootstrap") continue;
  if (selectedAreas !== null && !selectedAreas.has(area.name)) continue;
  for (const kind of ["Source", "Tests"]) {
    for (const path of await jetFilesUnder(resolve(sourceRootDir, "Jetpack", area.name, kind))) {
      inventoried.push({ area: area.name, path: relative(sourceRootDir, path).split(sep).join("/") });
    }
  }
}
const unlisted = inventoried.filter(({ path }) => !seen.has(path));
const blocking = unlisted.filter(({ area }) => ownArea === null || area === ownArea).map(({ path }) => path).sort();
const skipped = unlisted.filter(({ area }) => ownArea !== null && area !== ownArea).map(({ path }) => path).sort();
if (blocking.length > 0) {
  fail(`Jetpack sources are not listed in Jetpack/Bootstrap/sources.list or tests.list; add them in explicit order:\n${blocking.map((path) => `  ${path}`).join("\n")}`);
}
if (skipped.length > 0) {
  console.log(`note: ${skipped.length} unlisted file(s) in dependency areas are not part of this unit (another writer's work in progress)`);
}

// A top-level test claim starts at column zero; its body is ordinary function
// statements. The run tier renders the same body as a plain function. Claim
// names are plain text (no interpolation holes or escapes) so both tiers
// print the identical claim ledger.
const claimHead = /^#Test\("([^"\\{}]+)"\)\s*\{\s*$/;
const testNames = [];
function renderTests(text, sourcePath) {
  // `check` type-checks test claims exactly as the AOT harness will see them.
  const lines = text.split("\n");
  let claims = 0;
  for (const [index, line] of lines.entries()) {
    const match = claimHead.exec(line);
    if (match === null) {
      if (/^\s*#Test\b/.test(line)) fail(`${sourcePath}:${index + 1}: write test claims as a column-zero \`#Test("plain name") {\` line; names cannot contain quotes, backslashes, or braces`);
      if (/^\s+\.(setup|expect_fail|timeout|skip|measure)\b/.test(line)) {
        fail(`${sourcePath}:${index + 1}: Jetpack tests run on every tier; test scope members are not available under jet run`);
      }
      continue;
    }
    claims += 1;
    if (testNames.some((claim) => claim.name === match[1])) fail(`${sourcePath}:${index + 1}: duplicate test claim "${match[1]}"`);
    const fnName = `jetpack_test_claim_${testNames.length}`;
    testNames.push({ name: match[1], fnName });
    if (mode === "run") lines[index] = `fn ${fnName}() {`;
  }
  // Shared test helpers live in `*Support.jet` files: helpers only, never claims.
  const support = sourcePath.endsWith("Support.jet");
  if (support && claims > 0) fail(`${sourcePath}: a *Support.jet test helper file must not declare #Test claims`);
  if (!support && claims === 0) fail(`${sourcePath}: a tests.list entry must declare at least one #Test claim (shared helpers go in a *Support.jet file)`);
  return lines.join("\n");
}

// Proving one area runs only that area's tests; each dependency area proves its
// own tests in its own unit.
const unitTests = mode === "publication" || mode === "hosted" ? [] : ownArea === null ? tests : tests.filter((entry) => entry.section === ownArea);
const units = [...sources, ...unitTests];
if (mode === "hosted") {
  const sourcePath = "Jetpack/run.jet";
  const bytes = await readFile(resolve(sourceRootDir, sourcePath));
  units.push({ sourcePath, bytes, text: bytes.toString("utf8") });
}
if (units.length === 0) fail("Jetpack/Bootstrap/sources.list contains no sources");
const testPaths = new Set(unitTests.map((entry) => entry.sourcePath));

let output = "";
let generatedByte = 0;
let nextLine = 1;
const mapped = [];
function append(text) {
  output += text;
  generatedByte += Buffer.byteLength(text, "utf8");
  nextLine += (text.match(/\n/g) ?? []).length;
}
// D-MOD-CYCLE1=A: the listed Compiler/
// front-end files import their sibling packages with `use <package>.[names]`
// or `use <package>.name`. This unit is one namespace, so those imports are
// blanked to spaces of the same length (every byte offset and line in the
// source map still matches). A repeated single-line Core import after its first
// occurrence is blanked the same way (a duplicate import name is E0105), and
// grouped Core member imports are deduplicated per member.
const PACKAGE_IMPORT = /^use (?:jet_foundation|jet_lexer|jet_parser|jet_sema|jet_eval|jet_repl|jet_driver|jet_backend|jet_cli|compiler_bootstrap|jetpack_foundation|jetpack_trust_root|jet_pkg_model|jet_env_model|jetpack_trust|jetpack_artifact_store|jetpack_nix_eval|jetpack_nix|jetpack_store|jetpack_publication|jetpack_environment)\.(?:\[[^\]]*\]|[A-Za-z_][A-Za-z0-9_]*[ \t]*$)/gm;
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
for (const entry of units) {
  append(`// [jet-bootstrap source: ${entry.sourcePath}]\n`);
  const text = blankPackageImports(entry.text);
  const rendered = testPaths.has(entry.sourcePath) ? renderTests(text, entry.sourcePath) : text;
  const body = rendered ?? text;
  const bytes = Buffer.from(body, "utf8");
  const startByte = generatedByte;
  const startLine = nextLine;
  append(body);
  if (!body.endsWith("\n")) append("\n");
  const lineStarts = [0];
  for (let index = 0; index < bytes.length; index += 1) if (bytes[index] === 10) lineStarts.push(index + 1);
  const lineCount = body.length === 0 ? 0 : (body.match(/\n/g) ?? []).length + (body.endsWith("\n") ? 0 : 1);
  mapped.push({
    path: entry.sourcePath,
    sha256: createHash("sha256").update(entry.bytes).digest("hex"),
    original_bytes: entry.bytes.length,
    original_lines: lineCount,
    line_starts: lineStarts,
    generated_start_byte: startByte,
    generated_end_byte_exclusive: startByte + bytes.length,
    generated_start_line: startLine,
    generated_end_line: startLine + lineCount - 1,
  });
}

if (mode === "run") {
  // One driver prints the same claim ledger the AOT harness proves, so a
  // failure on either tier names the same claim.
  append("// [jetpack-bootstrap generated: default-tier test driver]\nfn run() {\n");
  for (const { name, fnName } of testNames) append(`    ${fnName}()\n    print("pass ${name}")\n`);
  append(`    print("jetpack-bootstrap: ${testNames.length} claims passed")\n}\n`);
}
if (mode === "publication") {
  // Preserve the publisher's typed failure for the canonical entry reporter.
  append("// [jetpack-bootstrap generated: internal native runtime-pack publisher]\nfn run() NativeRuntimePackError! {\n    native_runtime_pack_publisher()\n}\n");
}

await rm(projectDir, { recursive: true, force: true });
await mkdir(sourceDir, { recursive: true });
for (const [copy, target] of embeddedFiles) {
  const destination = resolve(sourceDir, copy);
  await mkdir(dirname(destination), { recursive: true });
  await copyFile(target, destination).catch((error) => fail(`cannot copy embedded file ${relative(sourceRootDir, target)}: ${error.message}`));
}
await writeFile(unitPath, output, "utf8");
await writeFile(packagePath, packageSource, "utf8");
await writeFile(mapPath, `${JSON.stringify({
  schema: "jet-bootstrap-source-map/v2",
  module: "Jetpack",
  mode,
  fallback_source_file: "Jetpack",
  source_segments: mapped.map(({ path, generated_start_byte, generated_end_byte_exclusive, line_starts }) => ({
    generated: { start: generated_start_byte, end: generated_end_byte_exclusive },
    source_file: path,
    line_starts,
  })),
  files: mapped,
  tests: testNames.map(({ name }) => name),
}, null, 2)}\n`, "utf8");
console.log(`assembled ${units.length} Jet sources (${mode}, ${testNames.length} test claims) into ${unitPath}`);
console.log(`source provenance: ${mapPath}`);
