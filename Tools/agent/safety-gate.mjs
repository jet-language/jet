#!/usr/bin/env node
// #4619: strict native safety admission. Missing coverage is never a pass.
// Native producers publish native_safety in dev-artifacts.json (see safety.md).
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { generateCase, generateCoreCase } from "./safety-fuzz.mjs";
import { collectGoldenEntries } from "./compiler-diff.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
export const TIERS = ["O0", "O1", "O2"];
export const ADDITIONAL_COVERAGE = [
  "native stack redzones/use-after-return", "uninitialized-memory/undefined-behavior instrumentation",
  "data-race instrumentation", "sanitized native compiler self-build",
  "MIR alias.exclusive/view.valid/send.boundary mutation controls",
  "inline moved-payload/Option-unwrap and backedge aliases",
];
export const hash = (data) => createHash("sha256").update(data).digest("hex");
const slugOf = (stem) => stem.replace(/[^A-Za-z0-9_-]+/gu, "_");
export function compareNative(directory, cases) {
  const rowsPath = join(directory, "results.tsv");
  const rows = existsSync(rowsPath) ? new Map(readFileSync(rowsPath, "utf8").trim().split("\n").filter(Boolean).map((line) => line.split("\t"))) : new Map();
  return cases.map(({ stem, stdout, stderr = "", status = 0 }) => {
    const slug = slugOf(stem);
    const paths = ["stdout", "stderr", "status.json"].map((ext) => join(directory, "run", `${slug}.${ext}`));
    if (["wrong", "lower-crash", "convert-error"].includes(rows.get(stem))) return { stem, verdict: "fail", detail: `native result ${rows.get(stem)}` };
    if (rows.get(stem) !== "pass" || paths.some((path) => !existsSync(path))) return { stem, verdict: "gap", detail: `native result ${rows.get(stem) ?? "absent"}` };
    const actual = JSON.parse(readFileSync(paths[2], "utf8"));
    if (actual.status !== status || actual.signal || actual.error || readFileSync(paths[0], "utf8") !== stdout || readFileSync(paths[1], "utf8") !== stderr) return { stem, verdict: "fail", detail: `model/golden differs (status ${actual.status})` };
    return { stem, verdict: "pass", detail: "model/golden agrees" };
  });
}

export function coverageReceipt(input, head, coverage) {
  if (!input?.receipt || !input.sha256 || !existsSync(input.receipt)) return false;
  try {
    const bytes = readFileSync(input.receipt);
    const receipt = JSON.parse(bytes);
    return hash(bytes) === input.sha256 && receipt.head === head && receipt.coverage === coverage &&
      receipt.verdict === "PASS" && Number.isSafeInteger(receipt.checked_cases) && receipt.checked_cases > 0 &&
      Array.isArray(receipt.failures) && receipt.failures.length === 0;
  } catch { return false; }
}

export function prepareCorpus(out, seed, count) {
  if (!Number.isSafeInteger(seed) || !Number.isSafeInteger(count) || count < 1) throw new Error("seed/count must be integers; count must be positive");
  const features = join(out, "corpus/Examples/features");
  const cases = [];
  const digest = createHash("sha256");
  for (const [kind, generate] of [["fuzz", generateCase], ["core", generateCoreCase]]) {
    mkdirSync(join(features, kind), { recursive: true });
    mkdirSync(join(features, "expected", kind), { recursive: true });
    for (let index = 0; index < count; index++) {
      const stem = `${kind}/s${seed}_${String(index).padStart(4, "0")}`;
      const value = generate(seed, index);
      writeFileSync(join(features, `${stem}.jet`), value.source);
      writeFileSync(join(features, "expected", `${stem}.out`), value.stdout);
      digest.update(stem).update("\0").update(value.source).update("\0").update(value.stdout).update("\0");
      cases.push({ stem, stdout: value.stdout });
    }
  }
  const caseList = join(out, "fuzz.cases");
  writeFileSync(caseList, cases.map((c) => c.stem).join("\n") + "\n");
  return { features, cases, caseList, sha256: digest.digest("hex") };
}

export function runGate({ out, artifacts, seed = 4, count = 50, prepareOnly = false }) {
  out = resolve(out);
  mkdirSync(out, { recursive: true });
  // Delete neither caller-owned artifacts nor historical receipts; every run has a unique out.
  if (existsSync(join(out, "safety-summary.json"))) throw new Error(`receipt already exists: ${out}`);
  const manifest = JSON.parse(readFileSync(artifacts, "utf8"));
  const summary = { schema: "jet-native-safety-v1", head: manifest.dev_head, artifacts_sha256: hash(readFileSync(artifacts)), seed, count, checks: [], gaps: [] };
  if (typeof manifest.dev_head !== "string" || !/^[a-f0-9]{7,40}$/u.test(manifest.dev_head)) throw new Error("artifacts must name an admitted dev_head");
  const add = (name, verdict, detail) => summary.checks.push({ name, verdict, detail });
  const command = (name, bin, args, env = {}) => {
    const result = spawnSync(bin, args, { cwd: repo, env: { ...process.env, ...env }, encoding: "utf8", timeout: 3600000, maxBuffer: 1 << 26 });
    writeFileSync(join(out, `${name}.log`), `${result.stdout ?? ""}${result.stderr ?? ""}`);
    return result;
  };
  const sourcePin = command("source-pin", "git", ["rev-parse", "HEAD"]);
  if (sourcePin.status !== 0) throw new Error("cannot identify safety source tree");
  summary.source_head = sourcePin.stdout.trim();
  if (!summary.source_head.startsWith(manifest.dev_head)) summary.gaps.push(`artifact head ${manifest.dev_head} differs from audited source ${summary.source_head}`);
  const corpus = prepareCorpus(out, seed, count);
  summary.corpus_sha256 = corpus.sha256;
  summary.corpus_features = corpus.features;
  if (prepareOnly) return summary;

  // Ratchet's lower-count mode rewrites its baseline. Use a private baseline copy.
  const baseline = join(out, "safety-baseline.md");
  writeFileSync(baseline, readFileSync(join(repo, "Docs/spec/safety.md")));
  const audit = command("core-audit", process.execPath, [join(repo, "Tools/agent/check-unsafe-ratchet.mjs"), "--baseline", baseline]);
  add("core-unsafe-audit", audit.status === 0 ? "pass" : "fail", `exit ${audit.status ?? audit.signal ?? audit.error?.code}; core-audit.log`);
  // The registry can retain an older release binary while the current head's
  // debug compiler is admitted. Core source is embedded; stale release output
  // cannot prove the audited wrappers. Native admission may bind a checker.
  const checker = manifest.native_safety?.checker ?? { path: manifest.debug_jet ?? manifest.release_jet, head: manifest.dev_head };
  const jet = checker.path;
  summary.checker = checker;
  for (const [fixture, expectedDiagnostics] of [["lowlevel_mem_direct_import_without_unsafe", 3], ["sys_posix_wrapper_without_unsafe", 2]]) {
    if (!jet || !existsSync(jet) || checker.head !== manifest.dev_head) { add(fixture, "gap", "no exact-head admitted checker"); continue; }
    const check = command(fixture, jet, ["check", join(repo, "tests/ui", `${fixture}.jet`)]);
    const diagnostics = `${check.stdout}${check.stderr}`.match(/Error \[E3103\]/gu) ?? [];
    add(fixture, check.status === 1 && diagnostics.length === expectedDiagnostics ? "pass" : "fail", `exit ${check.status ?? check.signal ?? check.error?.code}; ${diagnostics.length}/${expectedDiagnostics} E3103; ${fixture}.log`);
  }

  const goldenCases = collectGoldenEntries().filter((entry) => /^(memory|collections|strings|lowlevel|safety|concurrency)\//u.test(entry.stem)).flatMap((entry) => {
    const expected = join(repo, "Examples/features/expected", `${entry.stem}.out`);
    if (!existsSync(expected)) return [];
    const stderr = join(repo, "Examples/features/expected", `${entry.stem}.stderr.out`);
    return [{ stem: entry.stem, stdout: readFileSync(expected, "utf8"), stderr: existsSync(stderr) ? readFileSync(stderr, "utf8") : "", source: readFileSync(entry.path, "utf8") }];
  });
  const goldenHash = hash(JSON.stringify(goldenCases));
  const goldenList = join(out, "golden.cases");
  writeFileSync(goldenList, goldenCases.map((c) => c.stem).join("\n") + "\n");
  summary.golden_sha256 = goldenHash;
  summary.golden_count = goldenCases.length;
  if (!goldenCases.length) add("golden-corpus", "fail", "empty golden coverage");
  const native = manifest.native_safety;
  for (const tier of TIERS) {
    for (const [kind, cases, features, caseList, digest] of [
      ["fuzz", corpus.cases, corpus.features, corpus.caseList, corpus.sha256],
      ["golden", goldenCases, join(repo, "Examples/features"), goldenList, goldenHash],
    ]) {
      const input = native?.tiers?.[tier]?.[kind];
      const name = `${tier}-${kind}`;
      if (!input?.directory) { add(name, "gap", "no admitted native producer output"); continue; }
      try {
        const receipt = JSON.parse(readFileSync(join(input.directory, "native-safety-input.json"), "utf8"));
        if (receipt.head !== manifest.dev_head || receipt.backend !== "jet-native" || receipt.tier !== tier || receipt.corpus_sha256 !== digest || receipt.seed !== seed || receipt.count !== count) {
          add(name, "gap", "native provenance/corpus/seed mismatch"); continue;
        }
        const rows = compareNative(input.directory, cases);
        for (const row of rows) add(`${name}/${row.stem}`, row.verdict, row.detail);
        const lint = join(input.directory, "lint.tsv");
        const lintRows = existsSync(lint) ? new Map(readFileSync(lint, "utf8").trim().split("\n").map((line) => line.split("\t"))) : new Map();
        for (const c of cases) add(`${name}-lint/${c.stem}`, lintRows.get(c.stem) === "clean" ? "pass" : lintRows.has(c.stem) ? "fail" : "gap", lintRows.get(c.stem) ?? "no independent MIR Lint receipt");
        const runtime = input.runtime;
        if (!runtime?.library || !existsSync(runtime.library) || hash(readFileSync(runtime.library)) !== runtime.sha256 || runtime.head !== manifest.dev_head || runtime.instrumentation !== "address" || !runtime.negative_control?.receipt) {
          add(`${name}-runtime`, "gap", "missing exact-head instrumented runtime archive/negative control"); continue;
        }
        const negativeBytes = readFileSync(runtime.negative_control.receipt);
        const negative = JSON.parse(negativeBytes);
        if (hash(negativeBytes) !== runtime.negative_control.sha256 || negative.head !== manifest.dev_head || negative.runtime_sha256 !== runtime.sha256 || !["heap-buffer-overflow", "heap-use-after-free", "leak"].every((kind) => negative.detected?.includes(kind))) {
          add(`${name}-negative-control`, "gap", "ASan/LSan negative-control receipt incomplete or stale"); continue;
        }
        if (runtime.language !== "Jet") add(`${name}-rust-free-runtime`, "gap", `runtime language ${runtime.language ?? "unspecified"}`);
        if (receipt.frontend_language !== "Jet" || receipt.compiler_language !== "Jet") add(`${name}-rust-free-compiler`, "gap", "compiler/front-end still depends on Rust");
        const sanitize = command(`${name}-sanitize`, process.execPath, [join(repo, "Compiler/JetBackend/Tests/run-sanitized.mjs"), input.directory, "--case-list", caseList, "--runtime-library", runtime.library], { JET_GOLDEN_FEATURES: features });
        add(`${name}-sanitize`, sanitize.status === 0 ? "pass" : sanitize.status === 1 ? "fail" : "gap", `exit ${sanitize.status ?? sanitize.signal ?? sanitize.error?.code}; ${name}-sanitize.log`);
      } catch (error) { add(name, "gap", error.message); }
    }
  }
  // These are not provided by LIR/Sanitize today; never call ASan full I1 proof.
  for (const gap of ADDITIONAL_COVERAGE) {
    if (!coverageReceipt(native?.additional_coverage?.[gap], manifest.dev_head, gap)) summary.gaps.push(gap);
  }
  summary.gaps.push(...summary.checks.filter((c) => c.verdict === "gap").map((c) => `${c.name}: ${c.detail}`));
  summary.failures = summary.checks.filter((c) => c.verdict === "fail");
  summary.verdict = summary.failures.length ? "FAIL" : summary.gaps.length ? "GAP" : "PASS";
  summary.exit_code = summary.failures.length ? 1 : summary.gaps.length ? 3 : 0;
  writeFileSync(join(out, "safety-summary.json"), JSON.stringify(summary, null, 2) + "\n");
  writeFileSync(join(out, "safety-summary.txt"), [`gate: ${summary.verdict}`, `head: ${summary.head}`, `seed: ${seed}; generated cases: ${count * 2}; goldens: ${goldenCases.length}`, ...summary.failures.map((c) => `FAIL ${c.name}: ${c.detail}`), ...summary.gaps.map((gap) => `GAP ${gap}`)].join("\n") + "\n");
  return summary;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [out, artifacts, seed = "4", count = "50", mode] = process.argv.slice(2);
    if (!out || !artifacts || (mode && mode !== "--prepare")) throw new Error("usage: safety-gate.mjs OUT ARTIFACTS_JSON [SEED COUNT [--prepare]]");
    const summary = runGate({ out, artifacts, seed: Number(seed), count: Number(count), prepareOnly: mode === "--prepare" });
    console.log(JSON.stringify(summary, null, 2));
    process.exit(summary.exit_code ?? 0);
  } catch (error) { console.error(error.stack ?? String(error)); process.exit(2); }
}
