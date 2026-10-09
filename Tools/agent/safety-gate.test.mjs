import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { test } from "node:test";
import { generateCase, generateCoreCase } from "./safety-fuzz.mjs";
import { ADDITIONAL_COVERAGE, compareNative, coverageReceipt, hash, prepareCorpus, TIERS } from "./safety-gate.mjs";
import { due, INTERVAL_MS, renderJob, tick } from "./safety-schedule.mjs";

const scratch = process.env.JET_TEST_SCRATCH_DIR ?? process.env.TMPDIR ?? "/mnt/jetscratch/candidates/SafetyNet8/tests";
mkdirSync(scratch, { recursive: true });
function temporary(t) {
  const path = mkdtempSync(join(scratch, "safety-tool-test-"));
  t.after(() => rmSync(path, { recursive: true, force: true }));
  return path;
}
function nativeResult(dir, stem, verdict, stdout = "ok\n", stderr = "", status = 0) {
  mkdirSync(join(dir, "run"), { recursive: true });
  writeFileSync(join(dir, "results.tsv"), `${stem}\t${verdict}\tfixture\n`);
  const slug = stem.replace(/[^A-Za-z0-9_-]+/gu, "_");
  writeFileSync(join(dir, "run", `${slug}.stdout`), stdout);
  writeFileSync(join(dir, "run", `${slug}.stderr`), stderr);
  writeFileSync(join(dir, "run", `${slug}.status.json`), JSON.stringify({ status, signal: null, error: null }));
}

test("native O0/O1/O2 are required, with independent output/status/stderr checks", (t) => {
  assert.deepEqual(TIERS, ["O0", "O1", "O2"]);
  const dir = temporary(t);
  const cases = [{ stem: "core/case", stdout: "ok\n" }];
  assert.equal(compareNative(dir, cases)[0].verdict, "gap");
  nativeResult(dir, "core/case", "unsupported");
  assert.equal(compareNative(dir, cases)[0].verdict, "gap");
  nativeResult(dir, "core/case", "wrong");
  assert.equal(compareNative(dir, cases)[0].verdict, "fail");
  nativeResult(dir, "core/case", "pass");
  assert.equal(compareNative(dir, cases)[0].verdict, "pass");
  for (const args of [["bad\n", "", 0], ["ok\n", "bad\n", 0], ["ok\n", "", 70], ["ok\n", "", null]]) {
    nativeResult(dir, "core/case", "pass", ...args);
    assert.equal(compareNative(dir, cases)[0].verdict, "fail");
  }
});

test("seed-4 model cannot exceed String bounds; Core API programs deterministic and distinct", () => {
  for (let index = 0; index < 200; index++) {
    const c = generateCase(4, index);
    assert.match(c.source, /fn run\(/u);
    assert.ok(c.stdout.length < 2_000_000);
  }
  const c = generateCoreCase(4, 0);
  assert.deepEqual(c, generateCoreCase(4, 0));
  assert.notDeepEqual(c, generateCoreCase(5, 0));
  assert.match(c.source, /String\.from_bytes/u);
  assert.match(c.source, /m\.pop/u);
  assert.match(c.source, /saved\d+ :: xs\.copy\(\)/u);
  assert.equal(c.stdout.split("\n").filter((line) => line === "invalid").length, 6);
});

test("corpus digest binds model, source, seed and case count", (t) => {
  const a = prepareCorpus(temporary(t), 4, 3);
  const b = prepareCorpus(temporary(t), 4, 3);
  const c = prepareCorpus(temporary(t), 5, 3);
  assert.equal(a.cases.length, 6);
  assert.equal(a.sha256, b.sha256);
  assert.notEqual(a.sha256, c.sha256);
  assert.equal(readFileSync(a.caseList, "utf8").trim().split("\n").length, 6);
  assert.throws(() => prepareCorpus(temporary(t), 4, 0), /positive/u);
  assert.throws(() => prepareCorpus(temporary(t), 4, 1.5), /integers/u);
});

test("standing cadence: first artifact, changed admission, every six hours", () => {
  const now = 100000000;
  assert.equal(due(null, "a", now), true);
  assert.equal(due({ artifacts_sha256: "a", at: now }, "a", now + INTERVAL_MS - 1), false);
  assert.equal(due({ artifacts_sha256: "a", at: now }, "b", now), true);
  assert.equal(due({ artifacts_sha256: "a", at: now }, "a", now + INTERVAL_MS), true);
  const text = renderJob("echo test", { WT: "/a'b", SHA: "123456789" });
  assert.match(text, /proofq: mem=8G time=120M kind=test/u);
  assert.equal(spawnSync("bash", ["-n"], { input: text, encoding: "utf8" }).status, 0);
  assert.equal(hash("a"), hash(Buffer.from("a")));
});

test("standing gate respects pause and all SafetyNet jobs, including gate-held jobs", (t) => {
  const root = temporary(t);
  const options = { stateDir: join(root, "state"), queueRoot: root };
  writeFileSync(join(root, "PAUSE"), "");
  assert.equal(tick(options), "proofq paused");
  rmSync(join(root, "PAUSE"));
  mkdirSync(join(root, "queue-gate-hold"));
  writeFileSync(join(root, "queue-gate-hold/SafetyNet7-held.sh"), "");
  assert.equal(tick(options), "standing safety job already queued/running");
});

test("additional coverage requires exact-head hashed nonempty passing evidence", (t) => {
  const path = join(temporary(t), "coverage.json");
  const receipt = { head: "abcdef123", coverage: "stack", verdict: "PASS", checked_cases: 2, failures: [] };
  writeFileSync(path, JSON.stringify(receipt));
  const input = { receipt: path, sha256: hash(readFileSync(path)) };
  assert.equal(coverageReceipt(input, "abcdef123", "stack"), true);
  assert.equal(coverageReceipt(input, "old", "stack"), false);
  assert.equal(coverageReceipt(input, "abcdef123", "races"), false);
  assert.equal(coverageReceipt({ ...input, sha256: "wrong" }, "abcdef123", "stack"), false);
  receipt.checked_cases = 0;
  writeFileSync(path, JSON.stringify(receipt));
  assert.equal(coverageReceipt({ ...input, sha256: hash(readFileSync(path)) }, "abcdef123", "stack"), false);
});

test("#4754 whole inline moved-payload/Option-unwrap/backedge class stays a required gate", (t) => {
  const coverage = "inline moved-payload/Option-unwrap and backedge aliases";
  assert.ok(ADDITIONAL_COVERAGE.includes(coverage));
  assert.equal(coverageReceipt(null, "abcdef123", coverage), false);
  const path = join(temporary(t), "payload-class.json");
  const receipt = { head: "abcdef123", coverage, verdict: "FAIL", checked_cases: 2, failures: ["native UAF"] };
  writeFileSync(path, JSON.stringify(receipt));
  assert.equal(coverageReceipt({ receipt: path, sha256: hash(readFileSync(path)) }, "abcdef123", coverage), false);
  receipt.verdict = "PASS";
  receipt.failures = [];
  writeFileSync(path, JSON.stringify(receipt));
  assert.equal(coverageReceipt({ receipt: path, sha256: hash(readFileSync(path)) }, "abcdef123", coverage), true);
});

test("sanitizer cannot green on empty/missing/uninstrumented/link-failed native corpus", (t) => {
  const root = temporary(t);
  const features = join(root, "Examples/features");
  mkdirSync(join(features, "core"), { recursive: true });
  mkdirSync(join(features, "expected/core"), { recursive: true });
  for (const stem of ["present", "missing"]) {
    writeFileSync(join(features, "core", `${stem}.jet`), 'fn run() { print("ok") }\n');
    writeFileSync(join(features, "expected/core", `${stem}.out`), "ok\n");
  }
  const out = join(root, "output");
  mkdirSync(join(out, "lower"), { recursive: true });
  const runtime = join(root, "runtime.a");
  writeFileSync(runtime, "!<arch>\n");
  const jetObject = join(root, "runtime.o");
  const cc = process.env.SANITIZE_CC ?? "clang";
  const compile = spawnSync(cc, ["-x", "c", "-c", "-", "-o", jetObject], { input: "", encoding: "utf8" });
  assert.equal(compile.status, 0, compile.error?.message ?? compile.stderr);
  const invoke = (extra = []) => spawnSync(process.execPath, [resolve("Compiler/JetBackend/Tests/run-sanitized.mjs"), out, "--runtime-library", runtime, ...extra], {
    cwd: resolve("."), encoding: "utf8", env: { ...process.env, JET_GOLDEN_FEATURES: features, JET_RUNTIME_JET_OBJECT: jetObject, SANITIZE_CC: cc, RUSTC: "/nonexistent-rustc" },
  });
  writeFileSync(join(out, "results.tsv"), "");
  assert.equal(invoke().status, 1);
  assert.match(readFileSync(join(out, "sanitize-summary.txt"), "utf8"), /empty-corpus/u);
  writeFileSync(join(out, "results.tsv"), "core/present\tpass\n");
  writeFileSync(join(out, "lower/core_present.o"), "invalid native object");
  const caseList = join(root, "cases");
  writeFileSync(caseList, "core/present\ncore/missing\n");
  assert.equal(invoke(["--case-list", caseList]).status, 1);
  assert.match(readFileSync(join(out, "sanitize.tsv"), "utf8"), /core\/present\tuninstrumented/u);
  assert.match(readFileSync(join(out, "sanitize.tsv"), "utf8"), /core\/missing\tno-object/u);
  writeFileSync(join(out, "lower/core_present.asan.o"), "invalid native object");
  assert.equal(invoke().status, 1);
  assert.match(readFileSync(join(out, "sanitize.tsv"), "utf8"), /link-failed/u);
});
