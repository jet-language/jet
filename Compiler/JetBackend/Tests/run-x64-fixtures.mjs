#!/usr/bin/env node
// Standalone encoder/allocation/selection witnesses; no MIR lowering needed.
// Usage: JET=/path/to/jet node run-x64-fixtures.mjs <scratch-outdir>
import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = process.argv[2] && resolve(process.argv[2]);
if (!out) { console.error("usage: run-x64-fixtures.mjs <scratch-outdir>"); process.exit(64); }
const read = (path) => readFileSync(resolve(repo, path), "utf8");
const type = read("Compiler/JetFoundation/Source/MIR/MIR.jet").split("\n").find((line) => line.startsWith("pub struct MIRTypeID "));
if (!type) throw new Error("missing MIRTypeID");
const paths = ["Source/LIR/LIR.jet", "Source/LIR/Lint.jet", "Source/X64/Encoder.jet", "Source/X64/RegAlloc.jet", "Source/X64/Select.jet", "Tests/X64Fixtures.jet"];
const bodies = paths.map((path) => read(`Compiler/JetBackend/${path}`).replace(/^use jet_\w+.*\n/gm, ""));
mkdirSync(out, { recursive: true });
writeFileSync(resolve(out, "unit.jet"), [type, ...bodies, "fn run() { if x64_fixtures() -> print(\"X64 FIXTURES PASS\") else -> print(\"X64 FIXTURES FAIL\") }"].join("\n"));
writeFileSync(resolve(out, "package.jet"), 'name: "jet_backend_x64_fixtures"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n');
const env = readFileSync(resolve(homedir(), ".cache/jet-dev/scratch/NativeLead4/release-52303d4e2.env"), "utf8");
const jet = process.env.JET ?? env.split("\n").find((line) => line.startsWith("PQ_BIN_jet=")).slice("PQ_BIN_jet=".length);
const result = spawnSync(resolve(homedir(), ".cache/jet-dev/cheap.sh"), [jet, "run", "unit.jet"], { cwd: out, encoding: "utf8", maxBuffer: 1 << 28 });
writeFileSync(resolve(out, "output.log"), (result.stdout ?? "") + (result.stderr ?? ""));
process.stdout.write(result.stdout ?? ""); process.stderr.write(result.stderr ?? "");
if (result.error) console.error(result.error.message);
let failed = result.status !== 0 || !(result.stdout ?? "").split(/\r?\n/).includes("X64 FIXTURES PASS");
const witnesses = new Map();
for (const line of (result.stdout ?? "").split(/\r?\n/)) {
  const match = /^X64 EXEC ([\w-]+): ([\d,]+)$/.exec(line);
  if (match) witnesses.set(match[1], Buffer.from(match[2].split(",").filter(Boolean).map(Number)));
}
for (const name of ["parallel", "scalar", "spill"]) {
  const code = witnesses.get(name);
  if (!code) { console.error(`missing machine witness ${name}`); failed = true; continue; }
  // One RX PT_LOAD containing the ELF header, program header and entry code.
  const image = Buffer.alloc(120 + code.length);
  image.set([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1]);
  image.writeUInt16LE(2, 16); image.writeUInt16LE(62, 18); image.writeUInt32LE(1, 20);
  image.writeBigUInt64LE(0x400078n, 24); image.writeBigUInt64LE(64n, 32);
  image.writeUInt16LE(64, 52); image.writeUInt16LE(56, 54); image.writeUInt16LE(1, 56);
  image.writeUInt32LE(1, 64); image.writeUInt32LE(5, 68);
  image.writeBigUInt64LE(0x400000n, 80); image.writeBigUInt64LE(0x400000n, 88);
  image.writeBigUInt64LE(BigInt(image.length), 96); image.writeBigUInt64LE(BigInt(image.length), 104);
  image.writeBigUInt64LE(4096n, 112); code.copy(image, 120);
  const path = resolve(out, `${name}.elf`);
  writeFileSync(path, image); chmodSync(path, 0o755);
  const run = spawnSync(path, [], { encoding: "utf8", timeout: 10000 });
  const ok = run.status === 0 && run.stdout === "" && run.stderr === "";
  console.log(`X64 machine ${name}: ${ok ? "PASS" : "FAIL"} (rc ${run.status ?? run.signal})`);
  if (!ok) failed = true;
}
process.exit(failed ? 1 : 0);
