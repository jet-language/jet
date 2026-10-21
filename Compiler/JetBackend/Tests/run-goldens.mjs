#!/usr/bin/env node
// Golden execution through the admitted production Jet binary, never a Rust
// MIR dump or factory front end. JET must name the compiler under test.
// Default: build each staged example, then compare its real executable's exit
// and streams with the committed golden. All artifacts stay in OUTDIR.
// Registry transport may use --stage lower --until lower with
// JET_LOWER_ASSEMBLE_ONLY=1 and externally checked .mird inputs, then --stage run.
// usage: JET=BIN node run-goldens.mjs OUTDIR
//        [--stage build|lower|run] [--until build|lower|run]
//        [--case-list FILE | filter...]
import { createHash } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { SUPPORT_ITEMS, jetDecoder, loadMirSchema } from "./mir-debug.mjs";
import { nativeTransportEnvironment } from "../../../Tools/agent/compiler-native-transport.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const args = process.argv.slice(2);
const requestedOut = args.shift();
if (!requestedOut) {
  console.error("usage: JET=BIN run-goldens.mjs <outdir> [--stage build|lower|run] [filter...]");
  process.exit(64);
}
const outDir = resolve(requestedOut);
if (outDir === repo || outDir.startsWith(`${repo}/`) || outDir === "/tmp" || outDir.startsWith("/tmp/")) throw new Error("golden output must be external and disk-backed");
let first = "build";
let last = "run";
const filters = [];
let front = process.env.JET_GOLDEN_NATIVE_MIR === "1" ? "native" : "jet";
let selectedStems = null;
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--stage") first = args[++i];
  else if (args[i] === "--until") last = args[++i];
  else if (args[i] === "--front") front = args[++i];
  else if (args[i] === "--case-list") {
    selectedStems = readFileSync(args[++i], "utf8").split("\n").map((stem) => stem.trim()).filter(Boolean);
    if (!selectedStems.length || new Set(selectedStems).size !== selectedStems.length) throw new Error("case list must be nonempty and unique");
  }
  else filters.push(args[i]);
}
const STAGES = ["build", "lower", "run"];
const from = STAGES.indexOf(first);
const until = STAGES.indexOf(last);
if (from < 0 || until < from) throw new Error(`bad stage range ${first}..${last}`);
if (!["jet", "native"].includes(front)) throw new Error("--front must be jet or native");
if (front === "native" && from < 1) throw new Error("externally checked native MIR requires --stage lower or --stage run");
if (front === "jet" && (first === "lower" || last === "lower")) throw new Error("lower is only an externally checked native MIR transport stage");
const jet = process.env.JET;
if (!jet) throw new Error("JET must name the admitted production compiler binary");
const runtimePack = process.env.JET_RUNTIME_PACK;
if (front === "native" && !runtimePack) throw new Error("JET_RUNTIME_PACK must explicitly name an admitted fresh Jet pack");

const { collectGoldenEntries, goldenFailureExit, loadExampleStdin } = await import(pathToFileURL(join(repo, "Tools/agent/compiler-diff.mjs")));
const { copyFeatureProject, featureProjectRoot } = await import(pathToFileURL(join(repo, "Tools/agent/run-feature-examples.mjs")));
// JET_GOLDEN_FEATURES=<checkout>/Examples/features runs another corpus laid
// out like the repository's (Tools/agent/safety-fuzz.mjs writes one); cases
// are staged relative to that checkout so `jet run` sees the same paths.
const features = process.env.JET_GOLDEN_FEATURES ? resolve(process.env.JET_GOLDEN_FEATURES) : join(repo, "Examples/features");
const checkout = resolve(features, "../..");
const stdinTable = loadExampleStdin();

const dirs = { work: join(outDir, "work"), lower: join(outDir, "lower"), run: join(outDir, "run") };
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
  const expectedStderr = join(features, "expected", `${entry.stem}.stderr.out`);
  if (!existsSync(expectedOut) && !existsSync(expectedErr)) continue;
  // Golden suffix rule, shared with Tools/testkit/Golden.jet: `.err.out` pins stderr of a
  // non-zero exit (70 panic, 1 uncaught Err); `.stderr.out` pins the stderr of
  // a successful run beside its `.out`.
  const expect = existsSync(expectedErr)
    ? { failure: true, stdout: existsSync(expectedOut) ? readFileSync(expectedOut) : Buffer.alloc(0), stderr: readFileSync(expectedErr) }
    : { failure: false, stdout: readFileSync(expectedOut), stderr: existsSync(expectedStderr) ? readFileSync(expectedStderr) : null };
  cases.push({ entry, slug, expect, stdin: stdinTable.get(entry.stem) ?? null, verdict: null, detail: "" });
}
if (!cases.length) throw new Error("no executable goldens selected");
const open = () => cases.filter((c) => c.verdict === null);

// 1. Build each staged project with the production compiler.
function stage(c) {
  const dir = join(dirs.work, c.slug);
  rmSync(dir, { recursive: true, force: true });
  const projectRoot = featureProjectRoot(c.entry);
  if (projectRoot) copyFeatureProject(projectRoot, join(dir, relative(checkout, projectRoot)));
  else if (basename(c.entry.path) === "run.jet") copyFeatureProject(dirname(c.entry.path), join(dir, relative(checkout, dirname(c.entry.path))));
  else {
    const target = join(dir, relative(checkout, dirname(c.entry.path)));
    mkdirSync(target, { recursive: true });
    writeFileSync(join(target, basename(c.entry.path)), readFileSync(c.entry.path));
  }
  return dir;
}

if (from === 0) {
  for (const c of open()) {
    requireProofUnpaused();
    const cwd = stage(c);
    const built = spawnSync(jet, ["build", c.entry.shown, "--color=never"], {
      cwd, encoding: "utf8", timeout: Number(process.env.JET_GOLDEN_BUILD_TIMEOUT_MS ?? 120000),
      env: { ...nativeTransportEnvironment(), JET_STORE_DIR: join(outDir, "store", c.slug) }, maxBuffer: 1 << 28,
    });
    writeFileSync(join(dirs.lower, `${c.slug}.build.stdout`), built.stdout ?? "");
    writeFileSync(join(dirs.lower, `${c.slug}.build.stderr`), built.stderr ?? "");
    const file = join(cwd, relative(checkout, c.entry.path));
    const stem = basename(file, ".jet");
    const reported = (built.stdout ?? "").match(/(?:built|artifact:)\s+([^\n]+)/i)?.[1]?.trim();
    const candidates = [join(dirname(file), ".jet/build", stem), join(cwd, ".jet/build", stem)];
    const root = featureProjectRoot(c.entry);
    if (root) candidates.push(join(cwd, relative(checkout, root), ".jet/build", stem));
    if (reported) candidates.push(resolve(cwd, reported));
    const executable = candidates.find(path => path.startsWith(`${cwd}/`) && existsSync(path));
    if (built.error || built.signal || built.status !== 0 || !executable) {
      c.verdict = "build-fail";
      c.detail = `exit ${built.status ?? built.signal}: ${built.error?.message ?? (built.stderr ?? "").split("\n")[0]}`;
    } else {
      c.executable = executable;
      writeFileSync(join(dirs.lower, `${c.slug}.executable`), `${executable}\n`);
    }
    console.log(`build ${c.slug}: ${c.verdict ?? "built"}`);
  }
}

const schema = front === "native" ? loadMirSchema(repo) : null;

// 2. Optional native MIR lowering for registry transport.
const read = (path) => readFileSync(join(repo, path), "utf8");
function item(path, name) {
  const lines = read(path).split("\n");
  const start = lines.findIndex((line) => new RegExp(`^(?:pub )?(struct|enum) ${name}\\b`).test(line));
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
  "Compiler/JetFoundation/Source/Encoding/Json.jet",
  "Compiler/JetBackend/Source/LIR/LIR.jet",
  "Compiler/JetBackend/Source/LIR/Lint.jet",
  "Compiler/JetBackend/Source/LIR/Print.jet",
  "Compiler/JetBackend/Source/LIR/Sanitize.jet",
  ...sortedJet("Compiler/JetBackend/Source/Lower"),
  "Compiler/JetBackend/Source/Opt/Simplify.jet",
  "Compiler/JetBackend/Source/Opt/Strength.jet",
  "Compiler/JetBackend/Source/Opt/Inline.jet",
  "Compiler/JetBackend/Source/Opt/Facts.jet",
  "Compiler/JetBackend/Source/Opt/CSE.jet",
  "Compiler/JetBackend/Source/Opt/LICM.jet",
  "Compiler/JetBackend/Source/Opt/Scalar.jet",
  "Compiler/JetBackend/Source/Opt/Opt.jet",
  "Compiler/JetBackend/Source/X64/Encoder.jet",
  "Compiler/JetBackend/Source/X64/InlineAsm.jet",
  "Compiler/JetBackend/Source/X64/RegAlloc.jet",
  "Compiler/JetBackend/Source/X64/Select.jet",
  "Compiler/JetBackend/Source/Image/Link.jet",
  "Compiler/JetBackend/Source/Image/Runtime.jet",
  "Compiler/JetBackend/Source/Image/RuntimeStrings.jet",
  "Compiler/JetBackend/Source/Image/ELF.jet",
  "Compiler/JetBackend/Source/Image/Object.jet",
  "Compiler/JetBackend/Source/Image/RuntimeObject.jet",
  "Compiler/JetBackend/Source/Image/RuntimeLibrary.jet",
  "Compiler/JetBackend/Source/Image/Archive.jet",
  "Compiler/JetBackend/Source/Image/StaticLink.jet",
  "Compiler/JetBackend/Source/Image/RuntimePack.jet",
];

function assembleUnit() {
  const support = [];
  // SUPPORT_ITEMS also owns the canonical shared DerivationTypes declarations.
  for (const [path, names] of SUPPORT_ITEMS) for (const name of names) support.push(item(path, name));
  const zone = read("Compiler/JetFoundation/Source/Types/Types.jet").split("\n");
  const zoneStart = zone.findIndex((line) => /^(?:pub )?fn param_zone_name\(/.test(line));
  if (zoneStart < 0) throw new Error("Types.jet: no declaration param_zone_name");
  support.push(zone.slice(zoneStart, zone.findIndex((line, index) => index > zoneStart && line === "}") + 1).join("\n"));
  const parts = [
    "// [unit source: Foundation items named by the Jet MIR schema]",
    ...support,
    "",
    body("Compiler/JetFoundation/Source/MIR/MIR.jet"),
    ...BACKEND.map(body),
    body("Compiler/JetBackend/Tests/Fragments.fixture/TestPack.jet"),
    body("Compiler/JetBackend/Tests/Fragments.fixture/GoldenLower.jet"),
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
    return [jet, "run", join(dirs.lower, "unit.jet")];
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
  const build = spawnSync(jet, ["build", ...(release ? ["--release"] : []), "unit.jet"], {
    cwd: dirs.lower, encoding: "utf8", timeout: Number(process.env.JET_GOLDEN_BUILD_TIMEOUT_MS ?? 600000),
    env: nativeTransportEnvironment(), maxBuffer: 1 << 28,
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
  if (process.env.JET_LOWER_SANITIZE === "1") writeFileSync(join(dir, "sanitize.txt"), "1\n");
  else rmSync(join(dir, "sanitize.txt"), { force: true });
  if (process.env.JET_LOWER_OPT) writeFileSync(join(dir, "opt-level.txt"), `${Number(process.env.JET_LOWER_OPT)}\n`);
  else rmSync(join(dir, "opt-level.txt"), { force: true });
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

if (front === "native" && from <= 1 && until >= 1) {
  const resume = process.env.JET_LOWER_RESUME === "1";
  if (resume && !existsSync(join(dirs.lower, "unit.jet"))) throw new Error("resume requires the original frozen lower/unit.jet");
  if (!resume) assembleUnit();
  if (process.env.JET_LOWER_ASSEMBLE_ONLY === "1") process.exit(0);
  const pending = open().filter((c) => {
    if (!existsSync(join(dirs.lower, `${c.slug}.mird`))) return false;
    if (!resume) return true;
    const issues = join(dirs.lower, `${c.slug}.issues`);
    return !existsSync(join(dirs.lower, `${c.slug}.o`)) && !(existsSync(issues) && readFileSync(issues, "utf8").trim());
  });
  if (!resume) for (const c of pending) {
    for (const suffix of [".o", ".asan.o", ".exe", ".elf", ".issues", ".asan-issues", ".pack-issues", ".static-issues"]) rmSync(join(dirs.lower, `${c.slug}${suffix}`), { force: true });
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
for (const c of until >= 2 ? open() : []) {
  const object = join(dirs.lower, `${c.slug}.o`);
  const issues = join(dirs.lower, `${c.slug}.issues`);
  if (existsSync(issues)) {
    c.verdict = "unsupported";
    c.detail = readFileSync(issues, "utf8").split("\n")[0];
    continue;
  }
  if (front === "native" && !existsSync(object)) {
    c.verdict = "lower-crash";
    continue;
  }
  c.leg = front === "native" ? "pack" : "production-jet";
  const locator = join(dirs.lower, `${c.slug}.executable`);
  const program = front === "native" ? join(dirs.lower, `${c.slug}.exe`)
    : c.executable ?? (existsSync(locator) ? readFileSync(locator, "utf8").trim() : "");
  const packIssues = join(dirs.lower, `${c.slug}.pack-issues`);
  if (!existsSync(program)) {
    const text = existsSync(packIssues) ? readFileSync(packIssues, "utf8") : "no linked executable";
    const symbols = [...new Set([...text.matchAll(/undefined symbol `([^`]+)`/g)].map((m) => m[1]))];
    for (const symbol of symbols) missing.set(symbol, (missing.get(symbol) ?? 0) + 1);
    c.verdict = "unsupported";
    c.detail = symbols.length ? `runtime pack lacks ${symbols.join(" ")}` : `pack link: ${text.split("\n")[0]}`;
    continue;
  }
  // Run beside the staged fixture data, including when build ran earlier.
  const cwd = existsSync(join(dirs.work, c.slug)) ? join(dirs.work, c.slug) : stage(c);
  const run = spawnSync(program, [], { input: c.stdin ?? "", encoding: null, timeout: 20000, cwd, maxBuffer: 1 << 26 });
  writeFileSync(join(dirs.run, `${c.slug}.stdout`), run.stdout ?? "");
  writeFileSync(join(dirs.run, `${c.slug}.stderr`), run.stderr ?? "");
  writeFileSync(join(dirs.run, `${c.slug}.status.json`), JSON.stringify({ status: run.status, signal: run.signal, error: run.error ? { code: run.error.code, message: run.error.message } : null }) + "\n");
  const { expect } = c;
  const expectedExit = expect.failure ? goldenFailureExit(c.entry.stem, expect.stderr.toString("utf8")) : 0;
  const exitOk = expectedExit !== null && run.status === expectedExit;
  const stdoutOk = run.stdout?.equals(expect.stdout) ?? false;
  const stderrOk = expect.stderr === null || (run.stderr?.equals(expect.stderr) ?? false);
  if (exitOk && stdoutOk && stderrOk) c.verdict = "pass";
  else {
    c.verdict = "wrong";
    c.detail = !exitOk
      ? expectedExit === null ? "ambiguous failure exit report" : `exit ${run.status ?? run.signal} (expected ${expectedExit}): ${(run.stderr?.toString("utf8") ?? "").split("\n")[0]}`
      : !stdoutOk ? "stdout differs" : "stderr differs";
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
  `front: ${front}`,
  ...[...counts].sort((a, b) => b[1] - a[1]).map(([verdict, count]) => `${verdict}: ${count}`),
  "",
  "first blocker per unsupported case (count):",
  ...[...reasons].sort((a, b) => b[1] - a[1]).slice(0, 40).map(([reason, count]) => `${count}\t${reason}`),
  "",
  "runtime symbols missing from the explicit runtime pack (cases):",
  ...[...missing].sort((a, b) => b[1] - a[1]).slice(0, 40).map(([symbol, count]) => `${count}\t${symbol}`),
];
writeFileSync(join(outDir, "summary.txt"), summary.join("\n") + "\n");
console.log(summary.join("\n"));
process.exitCode = cases.length > 0 && cases.every(c => until < 2 ? c.verdict === null : c.verdict === "pass") ? 0 : 1;
