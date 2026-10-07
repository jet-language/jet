#!/usr/bin/env node
// Assemble the audited OS layer and Jet runtime library, dump its checked MIR,
// and lower precisely MIRArtifactPlan.exports into jet_runtime_jet.o.
// usage: JET=<compiler> node run-runtime.mjs <outdir> [--stage assemble|mir|convert|lower] [--until assemble|mir|convert|lower]
// --until assemble prepares both units for cheap checks without invoking a build.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { Converter, SUPPORT_ITEMS, encode, jetDecoder, loadMirSchema, parseRustDebug } from "./mir-debug.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const args = process.argv.slice(2);
const output = args.shift();
if (!output) throw new Error("usage: run-runtime.mjs <outdir> [--stage assemble|mir|convert|lower] [--until assemble|mir|convert|lower]");
const out = resolve(output);
const stages = ["assemble", "mir", "convert", "lower"];
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
  const runtimeFiles = sorted("Compiler/JetRuntime/Source");
  if (!runtimeFiles.length) throw new Error("Jet runtime library has no source files");
  const runtime = [body("Compiler/JetBackend/Source/OS/Linux.jet"), ...runtimeFiles.map(body)];
  mkdirSync(join(library, "Source"), { recursive: true });
  writeFileSync(join(library, "Source/Runtime.jet"), [...coreUses, "", ...runtime].join("\n"));
  writeFileSync(join(library, "package.jet"), read("Compiler/JetRuntime/package.jet"));
  coreUses.clear();
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
  ];
  // Reuse the golden stream reader verbatim rather than a second wire decoder.
  const golden = "Compiler/JetBackend/Tests/GoldenLower.jet";
  const readers = [item(golden, /^struct MIRDReader /), ...["next", "int", "u64", "bool", "float", "hex", "string"].map((name) => item(golden, new RegExp(`^fn mird_${name}\\(`)))];
  const parts = [...support, body("Compiler/JetFoundation/Source/MIR/MIR.jet"), ...backend.map(body), ...readers, body("Compiler/JetBackend/Tests/RuntimeLower.jet"), jetDecoder(schema)];
  writeFileSync(join(out, "unit.jet"), ["use core.math as math", ...coreUses, "", ...parts].join("\n"));
  writeFileSync(join(out, "package.jet"), 'name: "jet_runtime_lower"\nversion: "0.1.0"\nauthority: { holds: { allow: [FS.Read, FS.Write, IO, Mem.Alloc] } }\n');
}
function invoke(command, cwd, extra = {}) {
  const jet = process.env.JET;
  if (!jet) throw new Error("set JET to the compiler for this tree");
  const run = spawnSync(join(repo, "Tools/agent/jet-env"), [jet, ...command], {
    cwd, env: { ...process.env, ...extra }, encoding: "utf8", maxBuffer: 1 << 28,
  });
  writeFileSync(join(out, `${command[0]}.stderr`), run.stderr ?? "");
  if (run.error || run.status !== 0) throw new Error(`${command.join(" ")} failed (${run.status}): ${run.error ?? run.stderr}`);
  return run.stdout;
}
if (from <= 1 && until >= 1) {
  const dump = join(out, "runtime.mir");
  rmSync(dump, { force: true });
  invoke(["emit", "--rust"], library, { JET_DUMP_MIR: dump });
  if (!existsSync(dump)) throw new Error("library front end produced no MIR dump; use the Jet front feed (no Rust fix)");
}
if (from <= 2 && until >= 2) {
  const ast = parseRustDebug(readFileSync(join(out, "runtime.mir"), "utf8"));
  const converted = new Converter(schema).convert(ast, { k: "named", name: "MIRProgram" }, "program");
  const lines = [];
  encode(converted, { k: "named", name: "MIRProgram" }, schema, lines);
  writeFileSync(join(out, "runtime.mird"), `${lines.join("\n")}\n`);
}
if (from <= 3 && until >= 3) {
  rmSync(join(out, "jet_runtime_jet.o"), { force: true });
  invoke(["run", "unit.jet"], out);
  if (!existsSync(join(out, "jet_runtime_jet.o"))) throw new Error("runtime lowering produced no object");
}
console.log(join(out, until === 3 ? "jet_runtime_jet.o" : "unit.jet"));
