#!/usr/bin/env node
// Turn Compiler/ into a Jet package graph (card #3862, D-MOD-CYCLE1=A).
//
// Every Compiler/<Pkg>/ folder is one package: its files share one namespace,
// so no import is written between files of the same package. For each package,
// in dependency order, this script:
//   1. writes Compiler/<Pkg>/package.jet with the dependencies its sources use;
//   2. marks `pub` every top-level item another package uses;
//   3. rewrites each file's cross-package import lines, one
//      `use <package>.[names]` per dependency package, listing exactly the
//      names that file uses.
// Names never change. The script owns only `use <compiler package>…` lines,
// `pub` on used items, and package.jet, so a second run changes nothing.
//
//   node Tools/compiler-modules/migrate.mjs [--package JetFoundation] [--check]
//
// --check reports what would change and exits 1 if anything would.
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { inventory, packageOf, PACKAGES, stronglyConnected } from "./inventory.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

export const PACKAGE_NAMES = {
  JetFoundation: "jet_foundation",
  JetLexer: "jet_lexer",
  JetParser: "jet_parser",
  JetOptimizer: "jet_optimizer",
  JetSema: "jet_sema",
  JetCodegen: "jet_codegen",
  JetEval: "jet_eval",
  JetDriver: "jet_driver",
  Bootstrap: "compiler_bootstrap",
};
const aliasPackages = new Set(Object.values(PACKAGE_NAMES));

const args = process.argv.slice(2);
const check = args.includes("--check");
const only = args.includes("--package") ? args[args.indexOf("--package") + 1] : null;
if (only && !PACKAGES.includes(only)) throw new Error(`unknown package ${only}; expected one of ${PACKAGES.join(", ")}`);

const { facts, edges } = inventory();
const factOf = new Map(facts.map((fact) => [fact.file, fact]));

// Package dependency graph from name uses; it must be acyclic (E0604 on packages).
const packageDeps = new Map(PACKAGES.map((pkg) => [pkg, new Set()]));
for (const [from, out] of edges) {
  for (const to of out.keys()) {
    if (packageOf(from) !== packageOf(to)) packageDeps.get(packageOf(from)).add(packageOf(to));
  }
}
const cycles = stronglyConnected(PACKAGES, (pkg) => packageDeps.get(pkg)).filter((c) => c.length > 1);
if (cycles.length > 0) throw new Error(`package cycle: ${cycles.map((c) => c.join(" <-> ")).join("; ")}`);
const order = [];
const placed = new Set();
const place = (pkg) => {
  if (placed.has(pkg)) return;
  placed.add(pkg);
  for (const dep of [...packageDeps.get(pkg)].sort()) place(dep);
  order.push(pkg);
};
for (const pkg of PACKAGES) place(pkg);

// Items used by another package need `pub`.
const exported = new Set();
for (const [from, out] of edges) {
  for (const [to, names] of out) {
    if (packageOf(from) === packageOf(to)) continue;
    for (const name of names) exported.add(`${to}\0${name}`);
  }
}

function packageManifest(pkg) {
  const deps = [...packageDeps.get(pkg)].sort((a, b) => order.indexOf(a) - order.indexOf(b));
  const lines = [
    `name: "${PACKAGE_NAMES[pkg]}"`,
    'version: "0.1.0"',
    'edition: "2028"',
  ];
  if (deps.length > 0) {
    lines.push("deps: {");
    for (const dep of deps) lines.push(`    ${PACKAGE_NAMES[dep]}: ../${dep},`);
    lines.push("}");
  }
  return `${lines.join("\n")}\n`;
}

// Byte offsets of the `use <compiler package>` statements this script owns.
function ownedImports(tokens) {
  const ranges = [];
  for (let k = 0; k < tokens.length; k += 1) {
    if (tokens[k].kind !== "keyword.use") continue;
    const head = tokens[k + 1];
    if (!head || head.kind !== "identifier" || !aliasPackages.has(head.text)) continue;
    let nest = 0;
    let end = k;
    while (end + 1 < tokens.length) {
      const kind = tokens[end + 1].kind;
      if (kind === "punctuation.left_bracket") nest += 1;
      else if (kind === "punctuation.right_bracket") nest -= 1;
      else if ((kind === "terminator" || kind === "eof") && nest === 0) break;
      end += 1;
    }
    ranges.push([tokens[k].span.start, tokens[end].span.end]);
    k = end;
  }
  return ranges;
}

function importBlock(file) {
  const byPackage = new Map();
  for (const [target, names] of edges.get(file)) {
    const pkg = packageOf(target);
    if (pkg === packageOf(file)) continue;
    if (!byPackage.has(pkg)) byPackage.set(pkg, new Set());
    for (const name of names) byPackage.get(pkg).add(name);
  }
  const lines = [];
  for (const pkg of order.filter((p) => byPackage.has(p))) {
    const names = [...byPackage.get(pkg)].sort();
    lines.push(`use ${PACKAGE_NAMES[pkg]}.[`);
    for (const name of names) lines.push(`    ${name},`);
    lines.push("]");
  }
  return lines;
}

// Where a new import block goes: after the leading comment lines and after
// any existing `use` lines at the top of the file.
function importInsertLine(lines) {
  let at = 0;
  while (at < lines.length && lines[at].startsWith("//")) at += 1;
  let last = -1;
  for (let i = at; i < lines.length; i += 1) {
    if (lines[i].startsWith("use ")) last = i;
    else if (lines[i].trim() !== "" && !lines[i].startsWith("//")) break;
  }
  return last >= 0 ? last + 1 : at;
}

function migrateFile(file) {
  const fact = factOf.get(file);
  const path = join(repo, file);
  const original = readFileSync(path, "utf8");
  // 1. `pub` on exported items (edits sorted from the end keep offsets valid).
  const edits = [];
  for (const item of fact.items) {
    if (item.kind === "impl" || item.pub || !exported.has(`${file}\0${item.name}`)) continue;
    const at = fact.tokens.findIndex((t) => t.span.start === item.start);
    const keyword = item.kind === "const" ? fact.tokens[at] : fact.tokens[at - 1];
    edits.push({ start: keyword.span.start, end: keyword.span.start, text: "pub " });
  }
  // 2. Drop owned import statements with their line break; the blank line
  //    after a block stays and is reused when the block is written again.
  const bytes = Buffer.from(original, "utf8");
  for (const [start, end] of ownedImports(fact.tokens)) {
    const stop = bytes[end] === 0x0a ? end + 1 : end;
    edits.push({ start, end: stop, text: "" });
  }
  // Token spans are UTF-8 byte offsets, so edit bytes, not UTF-16 text.
  edits.sort((a, b) => b.start - a.start);
  let edited = bytes;
  for (const edit of edits) {
    edited = Buffer.concat([edited.subarray(0, edit.start), Buffer.from(edit.text, "utf8"), edited.subarray(edit.end)]);
  }
  let text = edited.toString("utf8");
  // 3. Insert the fresh import block.
  const block = importBlock(file);
  if (block.length > 0) {
    const lines = text.split("\n");
    const at = importInsertLine(lines);
    const before = lines.slice(0, at);
    const after = lines.slice(at);
    while (after.length > 0 && after[0].trim() === "") after.shift();
    text = [...before, ...block, "", ...after].join("\n");
  }
  return { original, text, path };
}

let changed = 0;
for (const pkg of order) {
  if (only && pkg !== only) continue;
  const manifestPath = join(repo, "Compiler", pkg, "package.jet");
  const manifest = packageManifest(pkg);
  if (!existsSync(manifestPath) || readFileSync(manifestPath, "utf8") !== manifest) {
    changed += 1;
    console.log(`${check ? "would write" : "wrote"} Compiler/${pkg}/package.jet`);
    if (!check) writeFileSync(manifestPath, manifest);
  }
  for (const fact of facts.filter((f) => packageOf(f.file) === pkg)) {
    const { original, text, path } = migrateFile(fact.file);
    if (text === original) continue;
    changed += 1;
    console.log(`${check ? "would update" : "updated"} ${fact.file}`);
    if (!check) writeFileSync(path, text);
  }
}
console.log(`package order: ${order.join(" -> ")}`);
console.log(`${changed} file(s) ${check ? "need changes" : "changed"}`);
if (check && changed > 0) process.exit(1);
