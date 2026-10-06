#!/usr/bin/env node
// Stages golden cases as generated-compiler projects.
// usage: golden-cases.mjs REPO OUTDIR SELECTOR
//   SELECTOR  all            every Examples/features golden with an expected
//                            stream (<stem>.out or <stem>.err.out)
//             everything     every golden, also the compile+build-only ones
//             <file>         a list file (see below)
//             SUB[,SUB...]   goldens whose stem contains any SUB
// Writes OUTDIR/cases.tsv: stem, source root, entry (relative to the root),
// expectation (out | err | none), stdin file (or -), expected base (the
// expected streams are <base>.out, <base>.err.out, <base>.stderr.out).
//
// A list file has one case per line (# starts a comment):
//   <stem>                               an Examples/features golden, exact stem
//   fixture <name> <source> <expected>   a Compiler/Bootstrap/Tests.rs-style
//                                        source fixture (stem fixture/<name>)
// <source> and <expected> are one of
//   repo:<path>      a file of REPO
//   const:<NAME>     a `const NAME: &str` of REPO/Compiler/Bootstrap/Tests.rs
//                    (raw string, string literal or include_str!)
//   text:"..."       a JSON string
//
// Discovery and stdin answers come from the repo's own comparator
// (Tools/agent/compiler-diff.mjs, a port of tests/golden.rs), so the corpus is
// the canonical one. A generated compiler needs a package root: project
// examples keep their own root and package.jet; a single-file example is
// staged alone (its file name is part of its expected diagnostics) with the
// fixture manifest the stage-zero harness uses (SOURCE_FIXTURE_MANIFEST). A
// source fixture is staged exactly as write_source_fixture_project does: that
// manifest plus the source as main.jet (SMALL_ENTRY_RELATIVE).
import { copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [repoArg, outDir, selector] = process.argv.slice(2);
if (!repoArg || !outDir || !selector) {
  console.error("usage: golden-cases.mjs REPO OUTDIR SELECTOR");
  process.exit(64);
}
// run-feature-examples.mjs stops its package-root search at its own FEATURES
// path, which Node resolves through symlinks (~/.cache/jet-luna -> jet-dev).
// The entry paths must be resolved the same way, or the search climbs out of
// the repo and stages whatever directory above it holds a package.jet.
const repo = realpathSync(repoArg);
const { collectGoldenEntries, loadExampleStdin } = await import(pathToFileURL(join(repo, "Tools/agent/compiler-diff.mjs")));
const { copyFeatureProject, featureProjectRoot } = await import(pathToFileURL(join(repo, "Tools/agent/run-feature-examples.mjs")));

const MANIFEST = 'name: "bootstrap_source_fixture"\nversion: "0.1.0"\nedition: "2028"\noutputs: { app: .Executable{ entry: run } }\n';
const features = join(repo, "Examples/features");
const testsRs = join(repo, "Compiler/Bootstrap/Tests.rs");
const stdinTable = loadExampleStdin(readFileSync(join(repo, "tests/common/mod.rs"), "utf8"));
const projects = join(outDir, "projects");
rmSync(projects, { recursive: true, force: true });
mkdirSync(projects, { recursive: true });

const fail = (message) => {
  console.error(`golden-cases: ${message}`);
  process.exit(2);
};
const slugOf = (stem) => stem.replace(/[^A-Za-z0-9_-]+/gu, "_");
const expectationOf = (base) => (existsSync(`${base}.err.out`) ? "err" : existsSync(`${base}.out`) ? "out" : "none");

// const:NAME in Tests.rs: a raw string, a plain string literal or include_str!.
let testsText = null;
function testsConst(name) {
  testsText ??= readFileSync(testsRs, "utf8");
  const head = `const ${name}: &str =`;
  const at = testsText.indexOf(head);
  if (at < 0) fail(`${testsRs} has no \`${head}\``);
  const rest = testsText.slice(at + head.length).trimStart();
  let match = /^r(#*)"/u.exec(rest);
  if (match) {
    const close = `"${match[1]};`;
    const end = rest.indexOf(close, match[0].length);
    if (end < 0) fail(`unterminated raw string for ${name}`);
    return rest.slice(match[0].length, end);
  }
  match = /^"((?:[^"\\]|\\.)*)";/su.exec(rest);
  if (match) return JSON.parse(`"${match[1].replaceAll("\n", "\\n")}"`);
  match = /^include_str!\("([^"]+)"\)\s*;/u.exec(rest);
  if (match) return readFileSync(resolve(dirname(testsRs), match[1]), "utf8");
  fail(`const ${name} is not a string literal, raw string or include_str!`);
}

function valueOf(spec) {
  if (spec.startsWith("repo:")) return readFileSync(join(repo, spec.slice(5)), "utf8");
  if (spec.startsWith("const:")) return testsConst(spec.slice(6));
  if (spec.startsWith("text:")) return JSON.parse(spec.slice(5));
  fail(`unknown fixture value \`${spec}\` (repo:, const: or text:)`);
}

const rows = [];
function stageGolden(entry) {
  const expectedBase = join(features, "expected", entry.stem);
  const expect = expectationOf(expectedBase);
  const stage = join(projects, slugOf(entry.stem));
  // A package example (package.jet) or a run.jet project keeps its whole tree.
  // A fixture-state directory is not a package: its siblings are other
  // examples, so only the entry and its state (as .jet/) are staged.
  const featureRoot = featureProjectRoot(entry);
  const packageRoot = featureRoot && existsSync(join(featureRoot, "package.jet")) ? featureRoot
    : basename(entry.path) === "run.jet" ? dirname(entry.path) : null;
  let root;
  let entryRel;
  if (packageRoot) {
    root = join(stage, basename(packageRoot));
    copyFeatureProject(packageRoot, root);
    if (!existsSync(join(root, "package.jet"))) writeFileSync(join(root, "package.jet"), MANIFEST);
    entryRel = relative(packageRoot, entry.path);
  } else {
    root = join(stage, "src");
    entryRel = relative(featureRoot ?? dirname(entry.path), entry.path);
    mkdirSync(dirname(join(root, entryRel)), { recursive: true });
    copyFileSync(entry.path, join(root, entryRel));
    if (featureRoot && existsSync(join(featureRoot, "fixture-state"))) {
      cpSync(join(featureRoot, "fixture-state"), join(root, ".jet"), { recursive: true });
    }
    // An inline `package { }` is the example's manifest; a package.jet beside
    // it would conflict (E1363).
    const inlinePackage = readFileSync(entry.path, "utf8").split("\n").some((line) => line.startsWith("package {"));
    if (!inlinePackage) writeFileSync(join(root, "package.jet"), MANIFEST);
  }
  let stdinFile = "-";
  if (stdinTable.has(entry.stem)) {
    stdinFile = join(stage, "stdin");
    writeFileSync(stdinFile, stdinTable.get(entry.stem));
  }
  rows.push([entry.stem, root, entryRel, expect, stdinFile, expectedBase]);
}

function stageFixture(name, source, expected) {
  const stem = `fixture/${name}`;
  const stage = join(projects, slugOf(stem));
  const root = join(stage, "project");
  mkdirSync(root, { recursive: true });
  writeFileSync(join(root, "package.jet"), MANIFEST);
  writeFileSync(join(root, "main.jet"), valueOf(source));
  writeFileSync(join(stage, "expected.out"), valueOf(expected));
  rows.push([stem, root, "main.jet", "out", "-", join(stage, "expected")]);
}

const goldens = collectGoldenEntries(features);
const isFile = existsSync(selector) && statSync(selector).isFile();
if (selector === "all" || selector === "everything") {
  for (const entry of goldens) {
    if (selector === "all" && expectationOf(join(features, "expected", entry.stem)) === "none") continue;
    stageGolden(entry);
  }
} else if (isFile) {
  const byStem = new Map(goldens.map((entry) => [entry.stem, entry]));
  const value = String.raw`(text:"(?:[^"\\]|\\.)*"|\S+)`;
  const fixtureLine = new RegExp(String.raw`^fixture\s+(\S+)\s+${value}\s+${value}$`, "u");
  for (const [index, raw] of readFileSync(selector, "utf8").split("\n").entries()) {
    const line = raw.replace(/^\s*#.*$/u, "").trim();
    if (!line) continue;
    if (line.startsWith("fixture ")) {
      const match = fixtureLine.exec(line);
      if (!match) fail(`${selector}:${index + 1}: expected \`fixture <name> <source> <expected>\``);
      stageFixture(match[1], match[2], match[3]);
    } else {
      const entry = byStem.get(line);
      if (!entry) fail(`${selector}:${index + 1}: no Examples/features golden with stem \`${line}\``);
      stageGolden(entry);
    }
  }
} else {
  const needles = selector.split(",").filter(Boolean);
  for (const entry of goldens) {
    if (needles.some((needle) => entry.stem.includes(needle))) stageGolden(entry);
  }
}
const stems = new Set();
for (const row of rows) {
  if (stems.has(row[0])) fail(`case ${row[0]} is listed twice`);
  stems.add(row[0]);
}
writeFileSync(join(outDir, "cases.tsv"), rows.length ? `${rows.map((row) => row.join("\t")).join("\n")}\n` : "");
console.log(`${rows.length} golden case(s) staged under ${projects}`);
