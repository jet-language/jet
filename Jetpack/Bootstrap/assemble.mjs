#!/usr/bin/env node
// Assemble the Jetpack Jet sources into one unqualified unit, mirroring
// Compiler/Bootstrap/assemble.mjs. `sources.list` is the only implementation
// order and `tests.list` the only test order; nothing is globbed into a unit.
//
// Modes:
//   check  implementation sources only                 -> src/jetpack.jet
//   aot    implementation + tests as `#Test` claims     -> src/jetpack.jet
//   run    implementation + the same test bodies as functions, driven by one
//          generated `fn run()` for the default `jet run` tier (I9)
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readdir, readFile, realpath, stat, writeFile, mkdir, rm } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const mode = process.argv[2] ?? "check";
if (!["check", "aot", "run"].includes(mode)) fail(`unknown mode ${mode}; expected check, aot, or run`);

const bootstrapDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(bootstrapDir, "../..");
const sourceRootDir = resolve(process.env.JETPACK_BOOTSTRAP_SOURCE_ROOT ?? repoRoot);
// Several writers may share one checkout. JETPACK_BOOTSTRAP_AREAS selects the
// `# == Area ==` sections to assemble (unset: all), and each worker's unit lives
// in its own scratch directory keyed by source root, selection and worker name.
const selectedAreas = process.env.JETPACK_BOOTSTRAP_AREAS
  ? new Set(process.env.JETPACK_BOOTSTRAP_AREAS.split(",").map((area) => area.trim()).filter(Boolean))
  : null;
const workerName = process.env.JETPACK_WORKER ?? "lead";
if (!/^[A-Za-z0-9_-]+$/.test(workerName)) fail("JETPACK_WORKER must be a plain name");
const rootTag = createHash("sha256")
  .update(`${sourceRootDir}\0${selectedAreas ? [...selectedAreas].sort().join(",") : "*"}`)
  .digest("hex")
  .slice(0, 10);
const scratchDir = resolve(
  process.env.JETPACK_BOOTSTRAP_SCRATCH ?? resolve(homedir(), ".cache/jet-luna/jetpack-bootstrap", `${workerName}-${rootTag}`),
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
  "        allow: [Env, Exec, FS, IO, Mem.Alloc, Net, Panic, Time]",
  "    }",
  "}",
  "",
].join("\n");
// Jetpack reuses the Compiler's Jet front end instead of porting a second copy,
// so manifests may name Compiler sources as well as Jetpack sources.
const allowedPrefixes = ["Jetpack/", "Compiler/"];

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

// With JETPACK_BOOTSTRAP_DEPS_FROM_HEAD=1 while proving one area, dependency
// areas come from the last commit (list lines and file bytes), so another
// writer's unfinished edits in a dependency cannot break this proof. The proved
// area itself always comes from the working copy.
const depsFromHead = process.env.JETPACK_BOOTSTRAP_DEPS_FROM_HEAD === "1" && process.env.JETPACK_BOOTSTRAP_OWN_AREA;
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
    if (!allowedPrefixes.some((prefix) => sourcePath.startsWith(prefix))) {
      fail(`${where}: sources must live under ${allowedPrefixes.join(" or ")}`);
    }
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
    const area = /^Jetpack\/([^/]+)\//.exec(sourcePath)?.[1];
    if (area !== undefined && area !== section) fail(`${where}: ${sourcePath} belongs under the \`# == ${area} ==\` section`);
    if (section === null) fail(`${where}: every source belongs to a \`# == Area ==\` section`);
    if (selectedAreas !== null && !selectedAreas.has(section)) continue;
    entries.push({ sourcePath, bytes, text: sourceText, where, section });
  }
  return entries;
}

const sources = await readManifest("sources.list");
const tests = await readManifest("tests.list");
if (selectedAreas !== null) {
  const known = new Set((await readFile(resolve(sourceRootDir, "Jetpack/Bootstrap/sources.list"), "utf8"))
    .split(/\r?\n/)
    .map((line) => /^#\s*==\s*([^=(]+?)\s*(?:\(.*\))?\s*==\s*$/.exec(line.trim())?.[1])
    .filter(Boolean));
  for (const area of selectedAreas) if (!known.has(area)) fail(`JETPACK_BOOTSTRAP_AREAS names unknown section ${area}`);
}
const seen = new Set();
for (const entry of [...sources, ...tests]) {
  if (seen.has(entry.sourcePath)) fail(`${entry.where}: duplicate source ${entry.sourcePath}`);
  seen.add(entry.sourcePath);
}
for (const entry of sources) {
  if (/\/Tests\//.test(entry.sourcePath)) fail(`${entry.where}: test sources belong in tests.list`);
}
for (const entry of tests) {
  if (!/^Jetpack\/[^/]+\/Tests\//.test(entry.sourcePath)) fail(`${entry.where}: tests.list names only Jetpack/<Area>/Tests sources`);
}

// Every Jetpack source and test file must be listed; an unlisted file is
// either dead code or an ordering decision nobody made. When one area is being
// proved (JETPACK_BOOTSTRAP_OWN_AREA), only that area must be complete: its
// dependency areas may hold another writer's unlisted work in progress, which
// is left out of the unit and noted, never a reason to fail this proof.
const ownArea = process.env.JETPACK_BOOTSTRAP_OWN_AREA ?? null;
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
  if (mode === "check") return null;
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
  if (claims === 0) fail(`${sourcePath}: a tests.list entry must declare at least one #Test claim`);
  return lines.join("\n");
}

// Proving one area runs only that area's tests; each dependency area proves its
// own tests in its own unit.
const unitTests = ownArea === null ? tests : tests.filter((entry) => entry.section === ownArea);
const units = mode === "check" ? sources : [...sources, ...unitTests];
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
for (const entry of units) {
  append(`// [jetpack-bootstrap source: ${entry.sourcePath}]\n`);
  const rendered = testPaths.has(entry.sourcePath) ? renderTests(entry.text, entry.sourcePath) : entry.text;
  const body = rendered ?? entry.text;
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

await rm(projectDir, { recursive: true, force: true });
await mkdir(sourceDir, { recursive: true });
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
