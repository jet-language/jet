#!/usr/bin/env node
// Standalone encoder/allocation/selection witnesses; no MIR lowering needed.
// Usage: JET=/path/to/jet node run-x64-fixtures.mjs <scratch-outdir>
// JET runs `run unit.jet` in that directory as given (wrap it to pin CPUs
// or memory).
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) { console.error("usage: run-x64-fixtures.mjs <scratch-outdir>"); process.exit(64); }
const read = (path) => readFileSync(resolve(repo, path), "utf8");
const type = read("Compiler/JetFoundation/Source/MIR/MIR.jet").split("\n").find((line) => /^(pub )?struct MIRTypeID /.test(line));
if (!type) throw new Error("missing MIRTypeID");
const paths = ["Source/LIR/LIR.jet", "Source/LIR/Lint.jet", "Source/X64/Encoder.jet", "Source/X64/RegAlloc.jet", "Source/X64/Select.jet", "Tests/X64Fixtures.jet"];
const bodies = paths.map((path) => read(`Compiler/JetBackend/${path}`).replace(/^use jet_\w+.*\n/gm, ""));
// Reuse the canonical Jet ELF writer; omit only its module-linking entrypoint.
const elf = read("Compiler/JetBackend/Source/Image/ELF.jet");
const elfEnd = elf.indexOf("fn x64_start_stub(");
if (elfEnd < 0) throw new Error("missing ELF writer boundary");
// The DWARF encoders and call frame instructions Select.jet records (up to
// the section writers), and the row transfer x64_compile_module uses.
const dwarf = read("Compiler/JetBackend/Source/Image/Dwarf.jet");
const dwarfStart = dwarf.indexOf("X64_DWARF_REGISTERS :: ");
const dwarfEnd = dwarf.indexOf("fn dwarf_eh_finish(");
if (dwarfStart < 0 || dwarfEnd < dwarfStart) throw new Error("missing DWARF encoder boundary");
const collections = read("Compiler/JetFoundation/Source/Collections.jet");
const takeStart = collections.indexOf("fn compiler_take_rows<T>(");
const takeEnd = collections.indexOf("\n}\n", takeStart);
if (takeStart < 0 || takeEnd < 0) throw new Error("missing compiler_take_rows");
mkdirSync(out, { recursive: true });
writeFileSync(resolve(out, "unit.jet"), [type, elf.slice(0, elfEnd), dwarf.slice(dwarfStart, dwarfEnd), collections.slice(takeStart, takeEnd + 3), ...bodies, "fn run() { if x64_fixtures() -> print(\"X64 FIXTURES PASS\") else -> print(\"X64 FIXTURES FAIL\") }"].join("\n"));
writeFileSync(resolve(out, "package.jet"), 'name: "jet_backend_x64_fixtures"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc, FS.Write] } }\n');
const jet = process.env.JET;
if (!jet) { console.error("JET must name the compiler under test"); process.exit(64); }
const result = spawnSync(jet, ["run", "unit.jet"], { cwd: out, encoding: "utf8", maxBuffer: 1 << 28 });
writeFileSync(resolve(out, "output.log"), (result.stdout ?? "") + (result.stderr ?? ""));
process.stdout.write(result.stdout ?? ""); process.stderr.write(result.stderr ?? "");
if (result.error) console.error(result.error.message);
let failed = result.status !== 0 || !(result.stdout ?? "").split(/\r?\n/).includes("X64 FIXTURES PASS");
const witnesses = new Set();
for (const line of (result.stdout ?? "").split(/\r?\n/)) {
  const match = /^X64 EXEC ([\w-]+)$/.exec(line);
  if (match) witnesses.add(match[1]);
}
for (const name of ["parallel", "scalar", "spill", "abi-f32", "abi-f64", "entry", "memory-address", "address-arithmetic", "frame-branch", "split-loop", "fused-overflow", "fused-overflow-cold", "call-loop", "critical-edge-then", "critical-edge-else", "edge-store", "shared-slot", "int-route"]) {
  const path = resolve(out, `${name}.elf`);
  if (!witnesses.has(name) || !existsSync(path)) { console.error(`missing machine witness ${name}`); failed = true; continue; }
  const run = spawnSync(path, [], { encoding: "utf8", timeout: 10000 });
  const ok = run.status === 0 && run.stdout === "" && run.stderr === "";
  console.log(`X64 machine ${name}: ${ok ? "PASS" : "FAIL"} (rc ${run.status ?? run.signal})`);
  if (!ok) failed = true;
}
process.exit(failed ? 1 : 0);
