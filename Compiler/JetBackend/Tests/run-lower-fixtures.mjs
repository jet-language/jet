#!/usr/bin/env node
// Assemble the MIR -> LIR -> x86-64 fixture unit (Tests/LowerFixtures.jet)
// with the sources it needs, as one package-free Jet file, and the in-process
// loader unit (Tests/LoadImages.jet) beside it.
//
// usage: node Compiler/JetBackend/Tests/run-lower-fixtures.mjs <outdir>
// Append --abi to assemble just the ABI/alignment witnesses.
// then:  cd <outdir> && jet run unit.jet        (writes <fixture>.elf files)
// then:  node Compiler/JetBackend/Tests/check-native-fixtures.mjs <outdir>
//        (with JET_RUNTIME_C_LIB, it also runs every image through load.jet)
//
// MIR.jet names three Foundation items (Span, Effect, ParamZone with
// param_zone_name) whose files pull in the whole diagnostics registry; their
// exact definitions are copied out of those files instead.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const outDir = process.argv[2];
if (!outDir) {
  console.error("usage: run-lower-fixtures.mjs <outdir>");
  process.exit(64);
}
// While other edits are in flight: FX_HEAD (comma-separated repo paths) reads
// those files as committed at HEAD, and FX_EXCLUDE (comma-separated file
// names without `.jet`) leaves those Lower and Fixtures files out.
const headPaths = new Set((process.env.FX_HEAD ?? "").split(",").filter(Boolean));
const excluded = new Set((process.env.FX_EXCLUDE ?? "").split(",").filter(Boolean).map((name) => `${name}.jet`));
// FX_ONLY (comma-separated fixture names) keeps only those one-line
// fx_report* calls in run(), for a small probe of one fixture.
const only = new Set((process.env.FX_ONLY ?? "").split(",").filter(Boolean));
const read = (path) => headPaths.has(path)
  ? execFileSync("git", ["show", `HEAD:${path}`], { cwd: repo, encoding: "utf8", maxBuffer: 1 << 28 })
  : readFileSync(`${repo}/${path}`, "utf8");

// The top-level item starting at the line that begins with `head`, through its
// closing `}` at column 0 (or the line itself for a one-line item).
function item(path, head) {
  const lines = read(path).split("\n");
  const start = lines.findIndex((line) => line.startsWith(head));
  if (start < 0) throw new Error(`${path}: no item starting with ${JSON.stringify(head)}`);
  if (!lines[start].trimEnd().endsWith("{")) return lines[start];
  const end = lines.findIndex((line, index) => index > start && line === "}");
  return lines.slice(start, end + 1).join("\n");
}

// Units are single-package: drop `use jet_*` items, one line or a bracketed
// block; `use core.*` items are collected for the head of the unit.
const coreUses = new Set();
function body(path) {
  const kept = [];
  let skipping = false;
  for (const line of read(path).split("\n")) {
    if (skipping) {
      if (line.startsWith("]")) skipping = false;
      continue;
    }
    if (/^use jet_\w+\.\[\s*$/.test(line)) {
      skipping = true;
      continue;
    }
    if (/^use jet_\w+/.test(line)) continue;
    if (/^use core\./.test(line)) {
      coreUses.add(line);
      continue;
    }
    const report = /fx_report\w*\("([\w-]+)"/.exec(line);
    if (only.size > 0 && report && !only.has(report[1])) continue;
    kept.push(line);
  }
  return `// [unit source: ${path}]\n${kept.join("\n")}\n`;
}

// Every lowering file (Lower.jet and its sibling family files) and every
// fixture file under Tests/Fixtures, in name order.
const sorted = (dir) => existsSync(`${repo}/${dir}`) ? readdirSync(`${repo}/${dir}`).filter((name) => name.endsWith(".jet") && !excluded.has(name)).sort().map((name) => `${dir}/${name}`) : [];
const abiOnly = process.argv.includes("--abi");
const parts = [
  ...(abiOnly ? [
    item("Compiler/JetFoundation/Source/MIR/MIR.jet", "pub struct MIRTypeID "),
  ] : [
    "// [unit source: Foundation items named by MIR.jet]",
    item("Compiler/JetFoundation/Source/Diagnostics/Diagnostic.jet", "pub struct Span {"),
    item("Compiler/JetFoundation/Source/Registry/CoreCalls.jet", "pub enum Effect "),
    item("Compiler/JetFoundation/Source/Types/Types.jet", "pub enum ParamZone {"),
    item("Compiler/JetFoundation/Source/Types/Types.jet", "pub fn param_zone_name("),
    body("Compiler/JetFoundation/Source/MIR/MIR.jet"),
    body("Compiler/JetFoundation/Source/Text/RustDebug.jet"),
  ]),
  body("Compiler/JetBackend/Source/LIR/LIR.jet"),
  body("Compiler/JetBackend/Source/LIR/Lint.jet"),
  ...(!abiOnly ? [
    body("Compiler/JetBackend/Source/LIR/Print.jet"),
    ...sorted("Compiler/JetBackend/Source/Lower").map(body),
  ] : []),
  body("Compiler/JetBackend/Source/X64/Encoder.jet"),
  body("Compiler/JetBackend/Source/X64/RegAlloc.jet"),
  body("Compiler/JetBackend/Source/X64/Select.jet"),
  body("Compiler/JetBackend/Source/Image/Link.jet"),
  body("Compiler/JetBackend/Source/Image/Runtime.jet"),
  body("Compiler/JetBackend/Source/Image/RuntimeStrings.jet"),
  body("Compiler/JetBackend/Source/Image/ELF.jet"),
  body("Compiler/JetBackend/Source/Image/Object.jet"),
  ...(!abiOnly ? [body("Compiler/JetBackend/Source/Image/Memory.jet")] : []),
  body("Compiler/JetBackend/Tests/AbiFixtures.jet"),
  ...(abiOnly ? ["fn run() { abi_fixtures() }"] : [body("Compiler/JetBackend/Tests/LowerFixtures.jet"), ...sorted("Compiler/JetBackend/Tests/Fixtures").map(body)]),
];
mkdirSync(outDir, { recursive: true });
writeFileSync(`${outDir}/unit.jet`, [...coreUses, "", ...parts].join("\n"));

// The loader unit needs only the image type, the OS layer and the loader. It
// is its own package (load/), run from <outdir> so it finds the images.
coreUses.clear();
const loadParts = [
  "// [unit source: Compiler/JetBackend/Source/Image/Memory.jet]",
  item("Compiler/JetBackend/Source/Image/Memory.jet", "pub struct X64MemoryImage {"),
  "",
  body("Compiler/JetBackend/Source/OS/Linux.jet"),
  body("Compiler/JetBackend/Source/Image/Loader.jet"),
  body("Compiler/JetBackend/Tests/LoadImages.jet"),
];
const packageSource = (name) => `name: "${name}"\nversion: "0.1.0"\nauthority: { holds: { allow: [Exec, Exec.Args, FFI, FS.Read, FS.Write, IO, Mem.Alloc] } }\n`;
writeFileSync(`${outDir}/package.jet`, packageSource("jet_backend_lower_fixtures"));
mkdirSync(`${outDir}/load`, { recursive: true });
writeFileSync(`${outDir}/load/load.jet`, [...coreUses, "", ...loadParts].join("\n"));
writeFileSync(`${outDir}/load/package.jet`, packageSource("jet_backend_load_images"));
console.log(`${outDir}/unit.jet`);
console.log(`${outDir}/load/load.jet`);
