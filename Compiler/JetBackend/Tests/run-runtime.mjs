#!/usr/bin/env node
// Assemble the audited runtime-only unit. NativeFeed's Jet compiler hands its
// checked MIR directly to the library-object lowering hook; no Rust MIR feed.
// usage: JET_FRONT_JETC0=<candidate> node run-runtime.mjs <outdir>
//        [--stage assemble|lower] [--until assemble|lower]
// --until assemble prepares the runtime and narrow RuntimeLower check units.
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { SUPPORT_ITEMS, jetDecoder, loadMirSchema } from "./mir-debug.mjs";
import { candidateJetc0, frontReceipt } from "./jet-front.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const args = process.argv.slice(2);
const output = args.shift();
if (!output) throw new Error("usage: run-runtime.mjs <outdir> [--stage assemble|lower] [--until assemble|lower]");
const out = resolve(output);
const stages = ["assemble", "lower"];
let first = "assemble", last = "lower";
for (let index = 0; index < args.length; index++) {
  if (args[index] === "--stage") first = args[++index];
  else if (args[index] === "--until") last = args[++index];
  else throw new Error(`unknown argument: ${args[index]}`);
}
const from = stages.indexOf(first), until = stages.indexOf(last);
if (from < 0 || until < from) throw new Error(`bad stage range ${first}..${last}`);
const library = join(out, "library");
mkdirSync(library, { recursive: true });
const read = (path) => readFileSync(join(repo, path), "utf8");
const sorted = (dir) => readdirSync(join(repo, dir)).filter((name) => name.endsWith(".jet")).sort().map((name) => `${dir}/${name}`);
const coreUses = new Set();
function body(path) {
  const kept = [];
  let skipping = false;
  for (const line of read(path).split("\n")) {
    if (skipping) { if (line.startsWith("]")) skipping = false; continue; }
    if (/^use jet_\w+\.\[\s*$/.test(line)) { skipping = true; continue; }
    if (/^use jet_\w+/.test(line)) continue;
    if (/^use core\./.test(line)) { coreUses.add(line); continue; }
    kept.push(line);
  }
  return `// [unit source: ${path}]\n${kept.join("\n")}\n`;
}
function item(path, head) {
  const lines = read(path).split("\n");
  const start = lines.findIndex((line) => head.test(line));
  if (start < 0) throw new Error(`${path}: missing declaration ${head}`);
  if (lines[start].trimEnd().endsWith("}")) return lines[start];
  const end = lines.findIndex((line, index) => index > start && line === "}");
  if (end < 0) throw new Error(`${path}: unterminated declaration ${head}`);
  return lines.slice(start, end + 1).join("\n");
}
const schema = loadMirSchema(repo);
if (from <= 0 && until >= 0) {
  execFileSync(process.execPath, [join(repo, "Compiler/JetRuntime/Tests/check-runtime.mjs"), library, "--assemble-only"], { stdio: "inherit" });
  mkdirSync(join(library, "Source"), { recursive: true });
  renameSync(join(library, "unit.jet"), join(library, "Source/Runtime.jet"));
  const support = [];
  for (const [path, names] of SUPPORT_ITEMS) for (const name of names) support.push(item(path, new RegExp(`^(pub )?(struct|enum) ${name}\\b`)));
  support.push(item("Compiler/JetFoundation/Source/Types/Types.jet", /^(pub )?fn param_zone_name\(/));
  const backend = [
    "Compiler/JetFoundation/Source/Text/RustDebug.jet",
    "Compiler/JetBackend/Source/LIR/LIR.jet", "Compiler/JetBackend/Source/LIR/Lint.jet",
    ...sorted("Compiler/JetBackend/Source/Lower"),
    "Compiler/JetBackend/Source/X64/Encoder.jet",
    ...(existsSync(join(repo, "Compiler/JetBackend/Source/X64/InlineAsm.jet")) ? ["Compiler/JetBackend/Source/X64/InlineAsm.jet"] : []),
    "Compiler/JetBackend/Source/X64/RegAlloc.jet", "Compiler/JetBackend/Source/X64/Select.jet",
    "Compiler/JetBackend/Source/Image/Link.jet", "Compiler/JetBackend/Source/Image/Runtime.jet",
    "Compiler/JetBackend/Source/Image/RuntimeStrings.jet", "Compiler/JetBackend/Source/Image/ELF.jet",
    "Compiler/JetBackend/Source/Image/Object.jet", "Compiler/JetBackend/Source/Image/RuntimeObject.jet",
    "Compiler/JetBackend/Source/Image/RuntimeLibrary.jet",
  ];
  // Reuse the golden stream reader verbatim rather than a second wire decoder.
  const golden = "Compiler/JetBackend/Tests/GoldenLower.jet";
  const readers = [item(golden, /^struct MIRDReader /), ...["next", "int", "u64", "bool", "float", "hex", "string"].map((name) => item(golden, new RegExp(`^fn mird_${name}\\(`)))];
  const parts = [...support, body("Compiler/JetFoundation/Source/MIR/MIR.jet"), ...backend.map(body), ...readers, body("Compiler/JetBackend/Tests/RuntimeLower.jet"), jetDecoder(schema)];
  writeFileSync(join(out, "unit.jet"), ["use core.math as math", ...coreUses, "", ...parts].join("\n"));
  writeFileSync(join(out, "package.jet"), 'name: "jet_runtime_lower"\nversion: "0.1.0"\nauthority: { holds: { allow: [FS.Read, FS.Write, IO, Mem.Alloc] } }\n');
}
if (until === 1) {
  const candidate = candidateJetc0();
  const prefix = join(out, "jet_runtime_jet");
  const receipt = join(out, "front.receipt");
  for (const suffix of [".o", ".exports", ".issues"]) rmSync(prefix + suffix, { force: true });
  rmSync(receipt, { force: true });
  const run = spawnSync(candidate, [], {
    cwd: repo,
    env: {
      ...process.env,
      JET_BOOTSTRAP_MODE: "factory", JET_BOOTSTRAP_FACTORY_TIER: "aot",
      JET_BOOTSTRAP_SOURCE_ROOT: library, JET_BOOTSTRAP_ENTRY: relative(library, join(library, "Source/Runtime.jet")),
      JET_BOOTSTRAP_OUTPUT: join(out, "unused.output"), JET_BOOTSTRAP_RECEIPT: receipt,
      JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix, JET_FRONT_LIBRARY_EXPORTS: join(repo, "Compiler/JetRuntime/exports.list"),
      JET_STORE_DIR: resolve(process.env.JET_STORE_DIR ?? join(out, "store")), RUST_MIN_STACK: "8388608",
    },
    encoding: "utf8", timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 120000), maxBuffer: 1 << 27,
  });
  writeFileSync(join(out, "front.log"), `${run.stdout ?? ""}${run.stderr ?? ""}`);
  const problem = frontReceipt(existsSync(receipt) ? readFileSync(receipt, "utf8") : "", run);
  if (problem) throw new Error(problem.detail);
  if (existsSync(prefix + ".issues")) throw new Error(readFileSync(prefix + ".issues", "utf8"));
  if (!existsSync(prefix + ".o")) throw new Error("candidate lacks the Jet library-object hook; rebuild it from this tree");
}
console.log(join(out, until === 1 ? "jet_runtime_jet.o" : "unit.jet"));
