#!/usr/bin/env node
// Assemble the in-process linker unit (Tests/LinkRuntime.jet with
// Image/Archive.jet, Image/StaticLink.jet, Image/RuntimePack.jet and the ELF
// writer helpers they share with ELF.jet and Object.jet) as one
// package-free Jet file.
//
// usage: node Compiler/JetBackend/Tests/run-link.mjs <outdir>
// then:  cd <outdir> && jet run unit.jet -- [--pack] <output> <input>...
// Or: node Compiler/JetBackend/Tests/run-link.mjs <outdir> --indices
// then: cd <outdir> && jet run unit.jet (reserved-index round-trip tests).
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const outDir = process.argv[2];
if (!outDir) {
  console.error("usage: node Compiler/JetBackend/Tests/run-link.mjs <outdir>");
  process.exit(64);
}
const read = (path) => readFileSync(`${repo}/${path}`, "utf8");
const indices = process.argv[3] === "--indices";
if (process.argv[3] && !indices) throw new Error("unknown linker unit mode");

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

// Units are single-package: drop `use jet_*` items; `use core.*` items are
// collected for the head of the unit.
const coreUses = new Set();
function body(path) {
  const kept = [];
  for (const line of read(path).split("\n")) {
    if (/^use jet_\w+/.test(line)) continue;
    if (/^use core\./.test(line)) {
      coreUses.add(line);
      continue;
    }
    kept.push(line);
  }
  return `// [unit source: ${path}]\n${kept.join("\n")}\n`;
}

const elf = "Compiler/JetBackend/Source/Image/ELF.jet";
const object = "Compiler/JetBackend/Source/Image/Object.jet";
const parts = [
  `// [unit source: ELF writer items of ${elf} and ${object}]`,
  item(elf, "pub struct X64ExecutableResult {"),
  item(elf, "fn elf64_u16("),
  item(elf, "fn elf64_u32("),
  item(elf, "fn elf64_u64("),
  item(object, "fn elf64_string("),
  item(object, "fn elf64_pad("),
  item(object, "fn elf64_section_header("),
  "",
  body("Compiler/JetBackend/Source/Image/Archive.jet"),
  body("Compiler/JetBackend/Source/Image/StaticLink.jet"),
  body("Compiler/JetBackend/Source/Image/RuntimePack.jet"),
  body(`Compiler/JetBackend/Tests/${indices ? "LinkIndexes" : "LinkRuntime"}.jet`),
];
mkdirSync(outDir, { recursive: true });
writeFileSync(`${outDir}/unit.jet`, [...coreUses, "", ...parts].join("\n"));
writeFileSync(`${outDir}/package.jet`, `name: "jet_backend_link_runtime"\nversion: "0.1.0"\nauthority: { holds: { allow: [Exec, Exec.Args, FS.Read, FS.Write, IO, Mem.Alloc, Time] } }\n`);
console.log(`${outDir}/unit.jet`);
