#!/usr/bin/env node
// Sanitizer mode for the native golden corpus (#4619 criterion 3). Native
// objects from a run-goldens.mjs output directory are linked against the same
// runtime built with AddressSanitizer, LeakSanitizer and Rust's UB
// precondition checks, and run with the golden's stdin. When run-goldens ran
// with JET_LOWER_SANITIZE=1, <out>/lower/<case>.asan.o carries ASan checks on
// every native heap load and store (Compiler/JetBackend/Source/LIR/Sanitize.jet)
// and is linked instead of <case>.o. The remaining Rust runtime and libc carry
// ASan/LSan checks; the Jet runtime object is the same required provider used
// by the native pack. Its own internals are not instrumented by rustc.
// Native load/store instrumentation is recorded separately in sanitize.tsv.
//
//   1. runtime: <runtime.rs> (the prepared runtime text the corpus's runtime
//      pack was linked from, prepare-call-runtime.mjs output) is compiled once
//      with `rustc --cfg jet_native_pack --crate-type staticlib -Zsanitizer=address -Zub-checks=yes`
//      (RUSTC_BOOTSTRAP=1; the nightly-only flags are confined to this proof
//      build) into <out>/sanitize/libjet_runtime_asan.a, reused while the
//      text, rustc and flags are unchanged.
//   2. link:    clang -fsanitize=address <case>.o jet_runtime_jet.o libjet_runtime_asan.a
//      -lpthread -ldl -lm (the hosted leg of run-goldens.mjs, plus the ASan
//      runtime).
//   3. run:     in the case's staged directory with ASAN_OPTIONS writing every
//      report to <out>/sanitize/reports/<case>.*; exitcode=0 keeps the
//      program's own exit status and buffered stdout when only leaks remain.
//
// Verdicts: clean, report (the detail names the first error kind and its
// SUMMARY line), sanitized-diverges (no report, but stdout or exit differs
// from the uninstrumented native run in <out>/run: ASan fills fresh heap
// blocks with 0xbe, so a native read of uninitialized memory shows up here),
// link-failed, no-object.
// Gate: exit 1 on any report or sanitized-diverges (leaks included unless
// --no-leaks, which the summary records).
//
// usage: JET_RUNTIME_JET_OBJECT=<fresh jet_runtime_jet.o> [RUSTC=rustc] [SANITIZE_CC=clang]
//        [JET_GOLDEN_FEATURES=<checkout>/Examples/features] (the corpus run-goldens.mjs ran)
//        node Compiler/JetBackend/Tests/run-sanitized.mjs <outdir> --runtime <runtime.rs>
//        [--case-list FILE] [--no-leaks]
// Writes <outdir>/sanitize.tsv and <outdir>/sanitize-summary.txt.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const args = process.argv.slice(2);
const outDir = args.shift();
const usage = "usage: run-sanitized.mjs <outdir> --runtime <runtime.rs> [--case-list FILE] [--no-leaks]";
if (!outDir) {
  console.error(usage);
  process.exit(64);
}
const features = process.env.JET_GOLDEN_FEATURES ? resolve(process.env.JET_GOLDEN_FEATURES) : join(repo, "Examples/features");
let runtimeSource = null;
let caseList = null;
let leaks = true;
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--runtime") runtimeSource = resolve(args[++i]);
  else if (args[i] === "--case-list") caseList = new Set(readFileSync(args[++i], "utf8").split("\n").map((stem) => stem.trim()).filter(Boolean));
  else if (args[i] === "--no-leaks") leaks = false;
  else throw new Error(`unknown option ${args[i]}\n${usage}`);
}
if (!runtimeSource) throw new Error(usage);
const jetObject = process.env.JET_RUNTIME_JET_OBJECT ? resolve(process.env.JET_RUNTIME_JET_OBJECT) : null;
if (!jetObject || !existsSync(jetObject)) throw new Error("JET_RUNTIME_JET_OBJECT must name the native pack's Jet runtime object");

const { collectGoldenEntries, loadExampleStdin } = await import(pathToFileURL(join(repo, "Tools/agent/compiler-diff.mjs")));
const { copyFeatureProject, featureProjectRoot } = await import(pathToFileURL(join(repo, "Tools/agent/run-feature-examples.mjs")));
const stdinTable = loadExampleStdin();
const entries = new Map(collectGoldenEntries(features).map((entry) => [entry.stem, entry]));
const dirs = { lower: join(outDir, "lower"), run: join(outDir, "run"), work: join(outDir, "work"), sanitize: join(outDir, "sanitize") };
const reportsDir = join(dirs.sanitize, "reports");
mkdirSync(reportsDir, { recursive: true });

// 1. The sanitized runtime.
const rustc = process.env.RUSTC ?? "rustc";
const cc = process.env.SANITIZE_CC ?? "clang";
const RUSTC_FLAGS = [
  "--edition", "2021", "--crate-name", "jet_runtime", "--crate-type", "staticlib", "--cfg", "jet_native_pack", "-Awarnings",
  "-Zsanitizer=address", "-Zub-checks=yes",
  "-C", "opt-level=1", "-C", "codegen-units=16", "-C", "debuginfo=line-tables-only", "-C", "force-frame-pointers=yes",
];
const runtimeLib = join(dirs.sanitize, "libjet_runtime_asan.a");
const stampPath = join(dirs.sanitize, "libjet_runtime_asan.stamp");
const rustcVersion = spawnSync(rustc, ["-vV"], { encoding: "utf8" });
if (rustcVersion.status !== 0) throw new Error(`${rustc} -vV failed: ${rustcVersion.stderr}`);
const stamp = createHash("sha256").update(readFileSync(runtimeSource)).update(rustcVersion.stdout).update(RUSTC_FLAGS.join(" ")).digest("hex");
if (!existsSync(runtimeLib) || !existsSync(stampPath) || readFileSync(stampPath, "utf8").trim() !== stamp) {
  rmSync(runtimeLib, { force: true });
  const started = Date.now();
  const build = spawnSync(rustc, [...RUSTC_FLAGS, runtimeSource, "-o", runtimeLib], {
    encoding: "utf8", env: { ...process.env, RUSTC_BOOTSTRAP: "1" }, maxBuffer: 1 << 28,
  });
  writeFileSync(join(dirs.sanitize, "runtime-build.log"), `${rustc} ${RUSTC_FLAGS.join(" ")} ${runtimeSource}\n${build.stderr ?? ""}`);
  if (build.status !== 0) throw new Error(`sanitized runtime build failed (exit ${build.status}); see ${join(dirs.sanitize, "runtime-build.log")}`);
  writeFileSync(stampPath, `${stamp}\n`);
  console.log(`sanitized runtime: built in ${Math.round((Date.now() - started) / 1000)} s`);
}
const duplicateCheck = spawnSync(process.execPath, [join(repo, "Tools/packaging/check-runtime-exports.mjs"), jetObject, runtimeLib], { encoding: "utf8" });
if (duplicateCheck.error || duplicateCheck.status !== 0) throw new Error(`sanitized runtime export check failed: ${duplicateCheck.error ?? duplicateCheck.stderr}`);

// The directory run-goldens.mjs ran the golden in (staged again the same way,
// relative to the checkout that holds the corpus, when that output directory
// has none).
const checkout = resolve(features, "../..");
function stagedDir(entry, slug) {
  const dir = join(dirs.work, slug);
  if (existsSync(dir)) return dir;
  const projectRoot = featureProjectRoot(entry);
  if (projectRoot) copyFeatureProject(projectRoot, join(dir, relative(checkout, projectRoot)));
  else if (basename(entry.path) === "run.jet") copyFeatureProject(dirname(entry.path), join(dir, relative(checkout, dirname(entry.path))));
  else {
    const target = join(dir, relative(checkout, dirname(entry.path)));
    mkdirSync(target, { recursive: true });
    writeFileSync(join(target, basename(entry.path)), readFileSync(entry.path));
  }
  return dir;
}

const symbolizer = spawnSync("sh", ["-c", "command -v llvm-symbolizer || command -v addr2line"], { encoding: "utf8" }).stdout.trim();
const resultsPath = join(outDir, "results.tsv");
if (!existsSync(resultsPath)) throw new Error(`${resultsPath} missing: run run-goldens.mjs through the lower stage first`);
const cases = [];
for (const line of readFileSync(resultsPath, "utf8").split("\n").filter(Boolean)) {
  const [stem] = line.split("\t");
  if (caseList && !caseList.has(stem)) continue;
  const entry = entries.get(stem);
  if (!entry) throw new Error(`results.tsv names a golden missing from ${features}: ${stem}`);
  const slug = stem.replace(/[^A-Za-z0-9_-]+/gu, "_");
  const c = { stem, verdict: "", detail: "" };
  cases.push(c);
  // run-goldens.mjs with JET_LOWER_SANITIZE=1 also writes the object with
  // ASan checks on every native heap access (LIR/Sanitize.jet); it is
  // preferred over the uninstrumented one.
  const instrumented = join(dirs.lower, `${slug}.asan.o`);
  const object = existsSync(instrumented) ? instrumented : join(dirs.lower, `${slug}.o`);
  c.instrumented = object === instrumented;
  if (!existsSync(object) || existsSync(join(dirs.lower, `${slug}.issues`))) {
    c.verdict = "no-object";
    continue;
  }
  // 2. Link.
  const program = join(dirs.sanitize, slug);
  const link = spawnSync(cc, ["-fsanitize=address", "-fno-omit-frame-pointer", object, jetObject, runtimeLib, "-lpthread", "-ldl", "-lm", "-o", program], { encoding: "utf8" });
  if (link.status !== 0) {
    c.verdict = "link-failed";
    const symbols = [...new Set([...link.stderr.matchAll(/undefined reference to `([^']+)'/g)].map((m) => m[1]))];
    c.detail = symbols.length ? `runtime lacks ${symbols.join(" ")}` : link.stderr.split("\n")[0];
    continue;
  }
  // 3. Run.
  for (const name of readdirSync(reportsDir)) if (name.startsWith(`${slug}.`)) rmSync(join(reportsDir, name));
  const options = [
    `log_path=${join(reportsDir, slug)}`, "exitcode=0", `detect_leaks=${leaks ? 1 : 0}`, "abort_on_error=0",
    "handle_segv=1", "handle_abort=1",
    ...(symbolizer ? [`external_symbolizer_path=${symbolizer}`] : []),
  ].join(":");
  const run = spawnSync(program, [], {
    input: stdinTable.get(stem) ?? "", encoding: "utf8", timeout: 60000, cwd: stagedDir(entry, slug), maxBuffer: 1 << 26,
    env: { ...process.env, ASAN_OPTIONS: options },
  });
  const reports = readdirSync(reportsDir).filter((name) => name.startsWith(`${slug}.`)).map((name) => readFileSync(join(reportsDir, name), "utf8"));
  if (reports.length) {
    const text = reports.join("\n");
    const first = text.match(/ERROR: (AddressSanitizer|LeakSanitizer): (\S+)/);
    const kind = !first ? "unknown" : first[1] === "LeakSanitizer" ? "leak" : first[2];
    const summary = text.match(/^SUMMARY: (.*)$/m)?.[1] ?? "";
    c.verdict = "report";
    c.detail = `${kind}: ${summary.slice(0, 160)}`;
    continue;
  }
  // Without a report the instrumented run must reproduce the plain native run.
  const plainStdout = join(dirs.run, `${slug}.stdout`);
  const plainStatus = join(dirs.run, `${slug}.status.json`);
  if (existsSync(plainStdout)) {
    const status = existsSync(plainStatus) ? JSON.parse(readFileSync(plainStatus, "utf8")).status : null;
    if (run.stdout !== readFileSync(plainStdout, "utf8") || (status !== null && run.status !== status)) {
      c.verdict = "sanitized-diverges";
      c.detail = `exit ${run.status ?? run.signal ?? run.error?.code}${status !== null ? ` (plain ${status})` : ""}`;
      continue;
    }
  }
  c.verdict = "clean";
  rmSync(program, { force: true });
}

const counts = new Map();
for (const c of cases) counts.set(c.verdict, (counts.get(c.verdict) ?? 0) + 1);
writeFileSync(join(outDir, "sanitize.tsv"), cases.map((c) => `${c.stem}\t${c.verdict}\t${c.instrumented ? "native-checked" : "runtime-only"}\t${c.detail}`).join("\n") + "\n");
const reported = cases.filter((c) => c.verdict === "report");
const failures = cases.filter((c) => c.verdict === "report" || c.verdict === "sanitized-diverges");
const kinds = new Map();
for (const c of reported) {
  const kind = c.detail.split(":")[0];
  kinds.set(kind, (kinds.get(kind) ?? 0) + 1);
}
const summary = [
  `cases: ${cases.length}`,
  `mode: address+ub-checks runtime, leaks ${leaks ? "on" : "off"}; native code checked in ${cases.filter((c) => c.instrumented).length} of ${cases.length} cases`,
  ...["clean", "report", "sanitized-diverges", "link-failed", "no-object"].filter((verdict) => counts.has(verdict)).map((verdict) => `${verdict}: ${counts.get(verdict)}`),
  "",
  "reports by kind:",
  ...[...kinds].sort((a, b) => b[1] - a[1]).map(([kind, count]) => `${count}\t${kind}`),
  "",
  `gate: ${failures.length ? `FAIL (${failures.length}; report text in ${reportsDir})` : "PASS"}`,
  ...failures.map((c) => `${c.verdict}\t${c.stem}\t${c.detail}`),
];
writeFileSync(join(outDir, "sanitize-summary.txt"), summary.join("\n") + "\n");
console.log(summary.join("\n"));
process.exit(failures.length ? 1 : 0);
