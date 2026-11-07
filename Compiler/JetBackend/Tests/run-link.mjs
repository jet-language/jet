#!/usr/bin/env node
// Assemble the in-process linker unit (Tests/Fragments.fixture/LinkRuntime.jet
// with Image/Archive.jet, Image/StaticLink.jet, Image/RuntimePack.jet,
// Image/MemoryLink.jet, Image/DebugPrune.jet, the fault report bytes of
// X64/FaultReport.jet that each image link writes, the ELF writer helpers they
// share with ELF.jet and Object.jet, and the compiler-thread items of
// JetFoundation's Threads.jet) as one package-free Jet file.
//
// usage: node Compiler/JetBackend/Tests/run-link.mjs <outdir>
// then:  cd <outdir> && jet run unit.jet -- [--pack | --image | --bind] <output> <input>...
//        (or -- --image-compare <runtime pack> <runtime image>)
// Or: node Compiler/JetBackend/Tests/run-link.mjs <outdir> --indices
// then: cd <outdir> && jet run unit.jet (reserved-index round-trip tests).
// Or: node Compiler/JetBackend/Tests/run-link.mjs <outdir> --pack-roots
// then: run unit.jet beside the inputs from run-runtime-pack-roots.mjs.
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { toolJet } from "../../../Tools/agent/roots.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const outDir = process.argv[2];
if (!outDir) {
  console.error("usage: node Compiler/JetBackend/Tests/run-link.mjs <outdir>");
  process.exit(64);
}
const read = (path) => readFileSync(`${repo}/${path}`, "utf8");
const mode = process.argv[3];
const fixture = mode === "--indices" ? "LinkIndexes" : mode === "--pack-roots" ? "LinkPackRoots" : "LinkRuntime";
if (mode && mode !== "--indices" && mode !== "--pack-roots") throw new Error("unknown linker unit mode");

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

// Each file's text with its `use { … }` blocks as one `use` line per row
// (Tools/agent/CompilerPackages.jet unblock, run by roots.mjs toolJet).
const unblocked = (paths) => execFileSync(toolJet(), [join(repo, "Tools/agent/CompilerPackages.jet"), "--root", repo, "unblock", ...paths], { maxBuffer: 1 << 30 }).toString("utf8").split("\0").slice(0, paths.length);

// Units are single-package: drop `use jet_*` items; `use core.*` items are
// collected for the head of the unit.
const coreUses = new Set();
function body(path, text) {
  const kept = [];
  for (const line of text.split("\n")) {
    if (/^use jet_\w+/.test(line)) continue;
    if (/^use (?:\w+: )?core\./.test(line)) {
      coreUses.add(line);
      continue;
    }
    kept.push(line);
  }
  return `// [unit source: ${path}]\n${kept.join("\n")}\n`;
}

const elf = "Compiler/JetBackend/Source/Image/ELF.jet";
const object = "Compiler/JetBackend/Source/Image/Object.jet";
const threads = "Compiler/JetFoundation/Source/Threads.jet";
const dwarf = "Compiler/JetBackend/Source/Image/Dwarf.jet";
const buildOptions = "Compiler/JetFoundation/Source/Registry/BuildOptions.jet";
const bodies = [
  "Compiler/JetFoundation/Source/Hash/SHA256.jet",
  "Compiler/JetBackend/Source/Image/Archive.jet",
  "Compiler/JetBackend/Source/Image/StaticLink.jet",
  "Compiler/JetBackend/Source/Image/RuntimePack.jet",
  "Compiler/JetBackend/Source/Image/MemoryLink.jet",
  "Compiler/JetBackend/Source/Image/DebugPrune.jet",
  "Compiler/JetBackend/Source/X64/FaultReport.jet",
  `Compiler/JetBackend/Tests/Fragments.fixture/${fixture}.jet`,
];
const bodyTexts = unblocked(bodies);
const parts = [
  `// [unit source: ELF writer items of ${elf} and ${object}]`,
  item(elf, "X64ExecutableResult: struct {"),
  item(elf, "elf64_u16: fn("),
  item(elf, "elf64_u32: fn("),
  item(elf, "elf64_u64: fn("),
  item(object, "elf64_string: fn("),
  item(object, "elf64_pad: fn("),
  item(object, "elf64_section_header: fn("),
  `// [unit source: compiler-thread items of ${threads}]`,
  item(threads, "CompilerThreads: struct {"),
  item(threads, "compiler_threads_single: fn("),
  item(threads, "compiler_threads_admit: fn("),
  item(threads, "compiler_threads_partition: fn("),
  item(threads, "compiler_threads_failure_text: fn("),
  `// [unit source: the DWARF, build-option and hash items the link reads]`,
  item(dwarf, "dwarf_uleb: fn("),
  item(dwarf, "dwarf_sleb: fn("),
  item(dwarf, "dwarf_patch_u32: fn("),
  item(dwarf, "dwarf_patch_length: fn("),
  item(buildOptions, "CHECKED_ALLOC_RUNTIME_EXPORTS: "),
  "",
  ...bodies.map((path, index) => body(path, bodyTexts[index])),
];
mkdirSync(outDir, { recursive: true });
writeFileSync(`${outDir}/unit.jet`, [...coreUses, "", ...parts].join("\n"));
writeFileSync(`${outDir}/package.jet`, `name: "jet_backend_link_runtime"\nversion: "0.1.0"\nauthority: { allow: [Exec, Exec.Args, FS.Read, FS.Write, IO, Mem.Alloc, Time] }\n`);
console.log(`${outDir}/unit.jet`);
