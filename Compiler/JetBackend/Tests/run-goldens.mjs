#!/usr/bin/env node
// Golden harness for the Jet backend: every Examples/features golden with an
// expected stdout goes through the Rust compiler's checked MIR, the Jet
// backend and the compiled runtime, and its output is compared with the
// golden.
//
//   1. mir:     `jet run <golden>` with JET_DUMP_MIR=<out>/mir/<case>.mir
//               (crates/jet-codegen/src/Codegen/TIR/mir.rs writes the checked
//               program's Debug form); the run itself is the reference.
//   2. convert: mir-debug.mjs turns the dump into the Jet MIR schema, keeps the
//               functions the entry reaches, and writes <out>/lower/<case>.mird.
//   3. lower:   a unit holding the Jet MIR schema, the backend, the generated
//               decoder and GoldenLower.jet is built once with `jet build`
//               (or, with JET_LOWER_RUN=1, run as `jet run unit.jet`, which
//               skips the rustc build of the ~1 MB unit); shards lower every
//               case to <case>.elf/<case>.o or <case>.issues (a fresh driver
//               every JET_LOWER_BATCH cases and after a crash, see runShard).
//   4. run:     each static <case>.elf (or, without one, the object linked
//               with $JET_RUNTIME_C_LIB: cc, -lpthread -ldl -lm) runs with the
//               golden's stdin and is compared with
//               Examples/features/expected/<stem>.out.
//
// Verdicts: pass, wrong (ran, output or exit differs), unsupported (lowering
// or object issue, or a runtime symbol the C library does not export),
// no-mir, convert-error, lower-crash, no-golden (expects a compile error or
// has no expected stdout).
//
// usage: [JET_RUNTIME_PACK=<jet_runtime.pack.o> | JET_RUNTIME_C_LIB=<libjet_runtime_c.a>] [JET=<jet binary>]
//        [JET_LOWER_RUN=1 | JET_LOWER_RELEASE=1] [JET_LOWER_SHARDS=<n>] [JET_LOWER_MEM=<cap>] [JET_LOWER_BATCH=<n>]
//        [JET_MIR_EMIT=1] dumps freshly checked MIR with `jet emit --rust`, without compiling/running the Rust output.
//        node Compiler/JetBackend/Tests/run-goldens.mjs <outdir> [--stage mir|convert|lower|run] [--until mir|convert|lower|run] [--case-list FILE | filter...]
// A later stage reuses the files of the earlier ones in <outdir>.
// Writes <outdir>/results.tsv and <outdir>/summary.txt.
import { createHash } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { Converter, SUPPORT_ITEMS, encode, jetDecoder, loadMirSchema, parseRustDebug } from "./mir-debug.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const args = process.argv.slice(2);
const outDir = args.shift();
if (!outDir) {
  console.error("usage: run-goldens.mjs <outdir> [--stage mir|convert|lower|run] [filter...]");
  process.exit(64);
}
let first = "mir";
let last = "run";
const filters = [];
let selectedStems = null;
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--stage") first = args[++i];
  else if (args[i] === "--until") last = args[++i];
  else if (args[i] === "--case-list") {
    selectedStems = readFileSync(args[++i], "utf8").split("\n").map((stem) => stem.trim()).filter(Boolean);
    if (!selectedStems.length || new Set(selectedStems).size !== selectedStems.length) throw new Error("case list must be nonempty and unique");
  }
  else filters.push(args[i]);
}
const STAGES = ["mir", "convert", "lower", "run"];
const from = STAGES.indexOf(first);
const until = STAGES.indexOf(last);
if (from < 0 || until < from) throw new Error(`bad stage range ${first}..${last}`);
const safeJet = `${process.env.HOME}/.cache/jet-luna/safe-jet.sh`;
const runtimeLib = process.env.JET_RUNTIME_C_LIB;
const runtimePack = process.env.JET_RUNTIME_PACK;

const { collectGoldenEntries, loadExampleStdin } = await import(pathToFileURL(join(repo, "Tools/agent/compiler-diff.mjs")));
const { copyFeatureProject, featureProjectRoot } = await import(pathToFileURL(join(repo, "Tools/agent/run-feature-examples.mjs")));
const features = join(repo, "Examples/features");
const stdinTable = loadExampleStdin();

const dirs = { work: join(outDir, "work"), mir: join(outDir, "mir"), lower: join(outDir, "lower"), run: join(outDir, "run") };
for (const dir of Object.values(dirs)) mkdirSync(dir, { recursive: true });

const cases = [];
const entries = collectGoldenEntries(features);
if (selectedStems) {
  if (filters.length) throw new Error("case list and substring filters are mutually exclusive");
  const available = new Set(entries.map((entry) => entry.stem));
  for (const stem of selectedStems) if (!available.has(stem)) throw new Error(`unknown case-list stem: ${stem}`);
}
const selection = selectedStems ? new Set(selectedStems) : null;
for (const entry of entries) {
  if (selection ? !selection.has(entry.stem) : filters.length && !filters.some((needle) => entry.stem.includes(needle))) continue;
  const slug = entry.stem.replace(/[^A-Za-z0-9_-]+/gu, "_");
  const expectedOut = join(features, "expected", `${entry.stem}.out`);
  const expectedErr = join(features, "expected", `${entry.stem}.err.out`);
  const verdict = existsSync(expectedErr) || !existsSync(expectedOut) ? "no-golden" : null;
  cases.push({ entry, slug, expectedOut, stdin: stdinTable.get(entry.stem) ?? null, verdict, detail: "" });
}
const open = () => cases.filter((c) => c.verdict === null);

// 1. MIR dumps (and the reference run).
function stage(c) {
  const dir = join(dirs.work, c.slug);
  rmSync(dir, { recursive: true, force: true });
  const projectRoot = featureProjectRoot(c.entry);
  if (projectRoot) copyFeatureProject(projectRoot, join(dir, relative(repo, projectRoot)));
  else if (basename(c.entry.path) === "run.jet") copyFeatureProject(dirname(c.entry.path), join(dir, relative(repo, dirname(c.entry.path))));
  else {
    const target = join(dir, relative(repo, dirname(c.entry.path)));
    mkdirSync(target, { recursive: true });
    writeFileSync(join(target, basename(c.entry.path)), readFileSync(c.entry.path));
  }
  return dir;
}

if (from <= 0 && until >= 0) {
  for (const c of open()) {
    requireProofUnpaused();
    const dump = join(dirs.mir, `${c.slug}.mir`);
    rmSync(dump, { force: true });
    const cwd = stage(c);
    const emit = process.env.JET_MIR_EMIT === "1";
    const command = emit ? ["emit", "--rust", c.entry.shown] : ["run", c.entry.shown];
    const run = spawnSync(safeJet, command, {
      cwd,
      input: c.stdin ?? "",
      encoding: "utf8",
      stdio: ["pipe", emit ? "ignore" : "pipe", "pipe"],
      env: { ...process.env, JET_DUMP_MIR: resolve(dump), SAFE_JET_TIMEOUT: process.env.SAFE_JET_TIMEOUT ?? "120" },
      maxBuffer: 1 << 28,
    });
    writeFileSync(join(dirs.mir, `${c.slug}.ref`), JSON.stringify({ mode: emit ? "emit" : "run", status: run.status, stdout: run.stdout ?? "", stderr: run.stderr ?? "" }));
    console.log(`mir ${c.slug}: ${existsSync(dump) ? "dumped" : "no dump"} (jet ${emit ? "emit" : "run"} exit ${run.status})`);
  }
}

// 2. Conversion.
const schema = loadMirSchema(repo);
const programType = { k: "named", name: "MIRProgram" };

function field(node, name) {
  return node?.k === "struct" ? node.fields.find(([field]) => field === name)?.[1] : undefined;
}

function idOf(node) {
  if (node?.k === "tuple" && node.items.length === 1) return idOf(node.items[0]);
  return node?.k === "num" ? node.v : null;
}

function entryFunction(program) {
  for (const artifact of field(program, "artifacts")?.items ?? []) {
    let entry = field(artifact, "entry");
    if (entry?.k === "tuple" && entry.name === "Some") entry = entry.items[0];
    let fn = field(entry, "function");
    if (fn?.k === "tuple" && fn.name === "Some") fn = fn.items[0];
    const id = idOf(fn);
    if (id) return id;
  }
  return null;
}

// Keep the functions the entry reaches through any function id they mention.
function prune(program, entryId) {
  const functions = field(program, "functions");
  const byId = new Map(functions.items.map((fn) => [idOf(field(fn, "id")), fn]));
  const keep = new Set([entryId]);
  const work = [entryId];
  while (work.length) {
    const fn = byId.get(work.pop());
    if (!fn) continue;
    const visit = (node) => {
      if (!node || typeof node !== "object") return;
      if (node.k === "tuple" && node.name === "MirFunctionId") {
        const id = idOf(node);
        if (id && !keep.has(id)) {
          keep.add(id);
          work.push(id);
        }
        return;
      }
      if (node.k === "struct") node.fields.forEach(([, value]) => visit(value));
      else if (node.k === "tuple" || node.k === "list") node.items.forEach(visit);
      else if (node.k === "map") node.entries.forEach(([key, value]) => (visit(key), visit(value)));
    };
    visit(fn);
  }
  functions.items = functions.items.filter((fn) => keep.has(idOf(field(fn, "id"))));
  pruneTables(program);
}

// Tables the lowering reads by id (types, field rows, Prelude and core call
// rows) keep the rows the kept functions reach, directly or through a kept
// row; type instances keep those naming only kept types. The interpreted
// lowering decodes and searches every row it is given.
const TABLES = [
  ["types", "MirTypeId"],
  ["fields", "MirFieldId"],
  ["prelude_calls", "MirPreludeCallId"],
  ["core_calls", "MirCoreCallId"],
];
function typeIds(node, out = []) {
  if (!node || typeof node !== "object") return out;
  if (node.k === "tuple" && node.name === "MirTypeId") out.push(idOf(node));
  else if (node.k === "struct") node.fields.forEach(([, value]) => typeIds(value, out));
  else if (node.k === "tuple" || node.k === "list") node.items.forEach((item) => typeIds(item, out));
  return out;
}
function pruneTables(program) {
  const refs = new Map(TABLES.map(([, idName]) => [idName, new Set()]));
  const rows = new Map(TABLES.map(([table, idName]) => [idName, new Map((field(program, table)?.items ?? []).map((row) => [idOf(field(row, "id")), row]))]));
  const work = [...field(program, "functions").items];
  while (work.length) {
    const visit = (node) => {
      if (!node || typeof node !== "object") return;
      if (node.k === "tuple" && refs.has(node.name)) {
        const id = idOf(node);
        const seen = refs.get(node.name);
        if (id && !seen.has(id)) {
          seen.add(id);
          const row = rows.get(node.name).get(id);
          if (row) work.push(row);
        }
        return;
      }
      if (node.k === "struct") node.fields.forEach(([, value]) => visit(value));
      else if (node.k === "tuple" || node.k === "list") node.items.forEach(visit);
      else if (node.k === "map") node.entries.forEach(([key, value]) => (visit(key), visit(value)));
    };
    visit(work.pop());
  }
  for (const [table, idName] of TABLES) {
    const list = field(program, table);
    if (list) list.items = list.items.filter((row) => refs.get(idName).has(idOf(field(row, "id"))));
  }
  const instances = field(program, "type_instances");
  if (instances) instances.items = instances.items.filter((ty) => typeIds(ty).every((id) => refs.get("MirTypeId").has(id)));
}

if (from <= 1 && until >= 1) {
  for (const c of open()) {
    const dump = join(dirs.mir, `${c.slug}.mir`);
    const mird = join(dirs.lower, `${c.slug}.mird`);
    rmSync(mird, { force: true });
    rmSync(join(dirs.lower, `${c.slug}.convert-error`), { force: true });
    if (!existsSync(dump)) continue;
    try {
      const program = parseRustDebug(readFileSync(dump, "utf8"));
      const entryId = entryFunction(program);
      if (!entryId) throw new Error("no artifact names an entry function");
      prune(program, entryId);
      const converter = new Converter(schema);
      const value = converter.convert(program, programType, "program");
      const lines = [BigInt(entryId).toString()];
      encode(value, programType, schema, lines);
      writeFileSync(mird, `${lines.join("\n")}\n`);
    } catch (error) {
      writeFileSync(join(dirs.lower, `${c.slug}.convert-error`), `${error.stack}\n`);
    }
  }
}
for (const c of open()) {
  if (!existsSync(join(dirs.mir, `${c.slug}.mir`))) {
    c.verdict = "no-mir";
    const ref = join(dirs.mir, `${c.slug}.ref`);
    if (existsSync(ref)) {
      const reference = JSON.parse(readFileSync(ref, "utf8"));
      c.detail = `jet ${reference.mode ?? "run"} exit ${reference.status}`;
    }
  } else if (existsSync(join(dirs.lower, `${c.slug}.convert-error`))) {
    c.verdict = "convert-error";
    c.detail = readFileSync(join(dirs.lower, `${c.slug}.convert-error`), "utf8").split("\n")[0];
  }
}

// 3. Lowering, in one interpreted unit.
const read = (path) => readFileSync(join(repo, path), "utf8");
function item(path, name) {
  const lines = read(path).split("\n");
  const start = lines.findIndex((line) => new RegExp(`^(pub )?(struct|enum) ${name}\\b`).test(line));
  if (start < 0) throw new Error(`${path}: no declaration ${name}`);
  if (lines[start].trimEnd().endsWith("}")) return lines[start];
  const end = lines.findIndex((line, index) => index > start && line === "}");
  return lines.slice(start, end + 1).join("\n");
}
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
    kept.push(line);
  }
  return `// [unit source: ${path}]\n${kept.join("\n")}\n`;
}

// The backend exactly as run-lower-fixtures.mjs assembles it: every lowering
// file in name order, then the generator and the image writers.
const sortedJet = (dir) => readdirSync(join(repo, dir)).filter((name) => name.endsWith(".jet")).sort().map((name) => `${dir}/${name}`);
const BACKEND = [
  "Compiler/JetFoundation/Source/Text/RustDebug.jet",
  "Compiler/JetBackend/Source/LIR/LIR.jet",
  "Compiler/JetBackend/Source/LIR/Lint.jet",
  "Compiler/JetBackend/Source/LIR/Print.jet",
  ...sortedJet("Compiler/JetBackend/Source/Lower"),
  "Compiler/JetBackend/Source/X64/Encoder.jet",
  "Compiler/JetBackend/Source/X64/RegAlloc.jet",
  "Compiler/JetBackend/Source/X64/Select.jet",
  "Compiler/JetBackend/Source/Image/Link.jet",
  "Compiler/JetBackend/Source/Image/Runtime.jet",
  "Compiler/JetBackend/Source/Image/RuntimeStrings.jet",
  "Compiler/JetBackend/Source/Image/ELF.jet",
  "Compiler/JetBackend/Source/Image/Object.jet",
  "Compiler/JetBackend/Source/Image/Archive.jet",
  "Compiler/JetBackend/Source/Image/StaticLink.jet",
  "Compiler/JetBackend/Source/Image/RuntimePack.jet",
];

function assembleUnit() {
  const support = [];
  for (const [path, names] of SUPPORT_ITEMS) for (const name of names) support.push(item(path, name));
  const zone = read("Compiler/JetFoundation/Source/Types/Types.jet").split("\n");
  const zoneStart = zone.findIndex((line) => line.startsWith("pub fn param_zone_name("));
  support.push(zone.slice(zoneStart, zone.findIndex((line, index) => index > zoneStart && line === "}") + 1).join("\n"));
  const parts = [
    "// [unit source: Foundation items named by the Jet MIR schema]",
    ...support,
    "",
    body("Compiler/JetFoundation/Source/MIR/MIR.jet"),
    ...BACKEND.map(body),
    body("Compiler/JetBackend/Tests/GoldenLower.jet"),
    "// [unit source: decoder generated by Compiler/JetBackend/Tests/mir-debug.mjs]",
    jetDecoder(schema),
  ];
  writeFileSync(join(dirs.lower, "unit.jet"), [...coreUses, "", ...parts].join("\n"));
  writeFileSync(join(dirs.lower, "package.jet"), 'name: "jet_backend_goldens"\nversion: "0.1.0"\nauthority: { holds: { allow: [FS.Read, FS.Write, IO, Mem.Alloc, Time] } }\n');
}

// The unit is built once (`jet build`, or `jet build --release` with
// JET_LOWER_RELEASE=1) and the binary is reused while unit.jet is unchanged;
// with JET_LOWER_RUN=1 every shard runs `jet run unit.jet` instead.
// Cases are split over JET_LOWER_SHARDS (default 4) concurrent runs; each
// shard runs in its own directory and names its cases `../<slug>`, so the
// objects and issues land beside the .mird files.
function driverCommand() {
  if (process.env.JET_LOWER_RUN === "1") {
    const jet = process.env.JET ?? `${process.env.HOME}/.cache/jet-dev/scratch/jet-current`;
    return [join(repo, "Tools/agent/jet-env"), jet, "run", join(dirs.lower, "unit.jet")];
  }
  return [buildDriver()];
}

function requireProofUnpaused() {
  if (existsSync(join(process.env.HOME, ".cache/jet-dev/proofq/PAUSE"))) throw new Error("proofq paused: retain partial outputs and resume after the pause lifts");
}
function buildDriver() {
  requireProofUnpaused();
  const unit = readFileSync(join(dirs.lower, "unit.jet"), "utf8");
  const release = process.env.JET_LOWER_RELEASE === "1";
  const stamp = `${release ? "release" : "dev"}\n${createHash("sha256").update(unit).digest("hex")}\n`;
  const binary = join(dirs.lower, ".jet/build/unit");
  const stampPath = join(dirs.lower, "driver.stamp");
  if (existsSync(binary) && existsSync(stampPath) && readFileSync(stampPath, "utf8") === stamp) return binary;
  rmSync(stampPath, { force: true });
  const started = Date.now();
  const build = spawnSync(safeJet, ["build", ...(release ? ["--release"] : []), "unit.jet"], {
    cwd: dirs.lower,
    encoding: "utf8",
    env: { ...process.env, SAFE_JET_TIMEOUT: process.env.SAFE_JET_TIMEOUT ?? "7200", SAFE_JET_MEM: process.env.SAFE_JET_MEM ?? "12G" },
    maxBuffer: 1 << 28,
  });
  writeFileSync(join(dirs.lower, "build.log"), `exit ${build.status}\n${build.stdout}\n${build.stderr}`);
  if (build.status !== 0 || !existsSync(binary)) throw new Error(`driver build failed (exit ${build.status}); see ${join(dirs.lower, "build.log")}`);
  console.log(`lower: driver built in ${Math.round((Date.now() - started) / 1000)} s`);
  writeFileSync(stampPath, stamp);
  return binary;
}

// The interpreted driver's memory grows across cases (#4585), so one run
// lowers at most JET_LOWER_BATCH cases (default 25) and a fresh process takes
// the rest. A crash blames its case only when that case ran first in a fresh
// process; otherwise the case is retried first in the next one.
function runShard(command, index, shard) {
  const dir = join(dirs.lower, `shard-${index}`);
  mkdirSync(dir, { recursive: true });
  if (runtimePack) writeFileSync(join(dir, "runtime-pack.txt"), `${resolve(runtimePack)}\n`);
  else rmSync(join(dir, "runtime-pack.txt"), { force: true });
  const size = Math.max(1, Number(process.env.JET_LOWER_BATCH ?? "25"));
  return new Promise((done) => {
    let pending = shard;
    const next = () => {
      if (pending.length === 0) return done();
      requireProofUnpaused();
      const batch = pending.slice(0, size);
      const rest = pending.slice(size);
      writeFileSync(join(dir, "cases.txt"), batch.map((c) => `../${c.slug}`).join("\n") + "\n");
      // jet-env keeps nix's temporary files on disk (AGENTS.md: never /tmp).
      const env = { ...process.env, TMPDIR: process.env.JET_LOWER_TMPDIR ?? `${process.env.HOME}/.cache/jet-dev/scratch`, JET_NIX_TMP_CLEANED: "1" };
      const run = spawn("systemd-run", ["--user", "--slice=jetwork.slice", "--scope", "-q", "-p", `MemoryMax=${process.env.JET_LOWER_MEM ?? "4G"}`, "-p", "MemorySwapMax=0", "timeout", process.env.JET_LOWER_TIMEOUT ?? "3600", ...command], { cwd: dir, env });
      let stdout = "";
      let stderr = "";
      run.stdout.on("data", (chunk) => (stdout += chunk));
      run.stderr.on("data", (chunk) => (stderr += chunk));
      run.on("close", (status) => {
        writeFileSync(join(dir, `lower-${Date.now()}.log`), `exit ${status}\n${stdout}\n${stderr}`);
        const finished = new Set();
        for (const line of stdout.split("\n")) {
          const [name, verdict] = line.split("\t");
          if (verdict) finished.add(name.replace(/^\.\.\//, ""));
        }
        const left = batch.filter((c) => !finished.has(c.slug));
        if (left.length && status !== 0) {
          if (left[0] === batch[0]) {
            // The first case of a fresh driver crashed it: blame it and go on.
            left[0].verdict = "lower-crash";
            left[0].detail = (stderr.split("\n").find((line) => /error|panic/i.test(line)) ?? `exit ${status}`).slice(0, 200);
            console.log(`lower: ${left[0].slug} crashed (${left[0].detail})`);
            pending = [...left.slice(1), ...rest];
          } else pending = [...left, ...rest];
        } else pending = rest;
        next();
      });
    };
    next();
  });
}

if (from <= 2 && until >= 2) {
  const resume = process.env.JET_LOWER_RESUME === "1";
  if (resume && !existsSync(join(dirs.lower, "unit.jet"))) throw new Error("resume requires the original frozen lower/unit.jet");
  if (!resume) assembleUnit();
  const pending = open().filter((c) => {
    if (!existsSync(join(dirs.lower, `${c.slug}.mird`))) return false;
    if (!resume) return true;
    const issues = join(dirs.lower, `${c.slug}.issues`);
    return !existsSync(join(dirs.lower, `${c.slug}.o`)) && !(existsSync(issues) && readFileSync(issues, "utf8").trim());
  });
  if (!resume) for (const c of pending) {
    for (const suffix of [".o", ".exe", ".elf", ".issues", ".pack-issues", ".static-issues"]) rmSync(join(dirs.lower, `${c.slug}${suffix}`), { force: true });
  }
  const command = driverCommand();
  const count = Math.max(1, Number(process.env.JET_LOWER_SHARDS ?? "4"));
  const shards = Array.from({ length: count }, () => []);
  pending.forEach((c, index) => shards[index % count].push(c));
  const started = Date.now();
  await Promise.all(shards.map((shard, index) => runShard(command, index, shard)));
  console.log(`lower: ${pending.length} cases in ${Math.round((Date.now() - started) / 1000)} s`);
}

// 4. Link, run, compare.
const missing = new Map();
for (const c of until >= 3 ? open() : []) {
  const object = join(dirs.lower, `${c.slug}.o`);
  const issues = join(dirs.lower, `${c.slug}.issues`);
  if (existsSync(issues)) {
    c.verdict = "unsupported";
    c.detail = readFileSync(issues, "utf8").split("\n")[0];
    continue;
  }
  if (!existsSync(object)) {
    c.verdict = "lower-crash";
    continue;
  }
  // With JET_RUNTIME_PACK the object is linked with that runtime pack by the
  // in-process linker during lowering, exactly as native `jet build` links
  // (<case>.exe). Otherwise the freestanding static executable runs; without
  // one, the object is linked with the compiled runtime's C library.
  let program;
  if (runtimePack) {
    c.leg = "pack";
    program = join(dirs.lower, `${c.slug}.exe`);
    const packIssues = join(dirs.lower, `${c.slug}.pack-issues`);
    if (!existsSync(program)) {
      const text = existsSync(packIssues) ? readFileSync(packIssues, "utf8") : "no linked executable";
      const symbols = [...new Set([...text.matchAll(/undefined symbol `([^`]+)`/g)].map((m) => m[1]))];
      for (const symbol of symbols) missing.set(symbol, (missing.get(symbol) ?? 0) + 1);
      c.verdict = "unsupported";
      c.detail = symbols.length ? `runtime pack lacks ${symbols.join(" ")}` : `pack link: ${text.split("\n")[0]}`;
      continue;
    }
  } else if (existsSync(join(dirs.lower, `${c.slug}.elf`))) {
    c.leg = "static";
    program = join(dirs.lower, `${c.slug}.elf`);
  } else {
    c.leg = "hosted";
    if (!runtimeLib) {
      c.verdict = "object";
      const staticIssues = join(dirs.lower, `${c.slug}.static-issues`);
      if (existsSync(staticIssues)) c.detail = readFileSync(staticIssues, "utf8").split("\n")[0];
      continue;
    }
    program = join(dirs.run, c.slug);
    const link = spawnSync(process.env.CC ?? "cc", [object, runtimeLib, "-lpthread", "-ldl", "-lm", "-o", program], { encoding: "utf8" });
    if (link.status !== 0) {
      const symbols = [...new Set([...link.stderr.matchAll(/undefined reference to `([^']+)'/g)].map((m) => m[1]))];
      for (const symbol of symbols) missing.set(symbol, (missing.get(symbol) ?? 0) + 1);
      c.verdict = "unsupported";
      c.detail = symbols.length ? `runtime lacks ${symbols.join(" ")}` : `link failed: ${link.stderr.split("\n")[0]}`;
      continue;
    }
  }
  // The golden runs where its `jet run` reference ran (staged again when the
  // mir stage ran in another output directory).
  const cwd = existsSync(join(dirs.work, c.slug)) ? join(dirs.work, c.slug) : stage(c);
  const run = spawnSync(program, [], { input: c.stdin ?? "", encoding: "utf8", timeout: 20000, cwd, maxBuffer: 1 << 26 });
  const expected = readFileSync(c.expectedOut, "utf8");
  writeFileSync(join(dirs.run, `${c.slug}.stdout`), run.stdout ?? "");
  if (run.stdout === expected && run.status === 0) c.verdict = "pass";
  else {
    c.verdict = "wrong";
    c.detail = run.status !== 0 ? `exit ${run.status ?? run.signal}: ${(run.stderr ?? "").split("\n")[0]}` : "stdout differs";
  }
  c.detail = `${c.leg}${c.detail ? `: ${c.detail}` : ""}`;
}

// Summary.
const counts = new Map();
for (const c of cases) counts.set(c.verdict, (counts.get(c.verdict) ?? 0) + 1);
writeFileSync(join(outDir, "results.tsv"), cases.map((c) => `${c.entry.stem}\t${c.verdict}\t${c.detail}`).join("\n") + "\n");
// Every distinct blocker of a case counts once for that case, so the top rows
// name what unblocks the most goldens.
const reasons = new Map();
const reasonKey = (line) => line.replace(/^[^\t]*\t/, "").replace(/-?[0-9]{2,}/g, "N").replace(/v[0-9]+/g, "vN").slice(0, 140);
for (const c of cases.filter((c) => c.verdict === "unsupported" || c.verdict === "convert-error")) {
  const issues = join(dirs.lower, `${c.slug}.issues`);
  const lines = existsSync(issues) ? readFileSync(issues, "utf8").split("\n").filter(Boolean) : [c.detail];
  for (const key of new Set(lines.map(reasonKey))) reasons.set(key, (reasons.get(key) ?? 0) + 1);
}
const summary = [
  `goldens: ${cases.length}`,
  ...[...counts].sort((a, b) => b[1] - a[1]).map(([verdict, count]) => `${verdict}: ${count}`),
  "",
  "first blocker per unsupported case (count):",
  ...[...reasons].sort((a, b) => b[1] - a[1]).slice(0, 40).map(([reason, count]) => `${count}\t${reason}`),
  "",
  `runtime symbols missing from the ${runtimePack ? "runtime pack" : "C library"} (cases):`,
  ...[...missing].sort((a, b) => b[1] - a[1]).slice(0, 40).map(([symbol, count]) => `${count}\t${symbol}`),
];
writeFileSync(join(outDir, "summary.txt"), summary.join("\n") + "\n");
console.log(summary.join("\n"));
