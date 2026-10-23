import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  collectGoldenEntries,
  comparePhase,
  goldenFailureExit,
  launcherFailure,
  loadExampleStdin,
  parseDiagnostics,
  redact,
} from "./compiler-diff.mjs";

function observed(overrides = {}) {
  return {
    status: "exit 0",
    panic: { kind: "none", message: "" },
    diagnostics: [],
    stdout: "",
    stderr: "",
    sideEffects: [],
    ...overrides,
  };
}

test("golden discovery matches the Rust loader's project-root rule", () => {
  const stems = new Set(collectGoldenEntries().map((entry) => entry.stem));
  assert.ok(stems.has("basics/onboarding"), "nested run.jet project was not discovered");
  for (const module of ["foundations/storage/cache", "foundations/tooling/load", "packages/sandbox_mathkit/sandbox_src"]) {
    assert.ok(!stems.has(module), `module/member fixture treated as an executable: ${module}`);
  }
});

test("golden failure exit follows the report contract without guessing unknown forms", () => {
  assert.equal(goldenFailureExit("unknown/stem", "Stop [E3001]: panic\n"), 70);
  assert.equal(goldenFailureExit("concurrency/deadline_context", "Error [E3003]: deadline\n"), 70);
  assert.equal(goldenFailureExit("memory/unsafe_sentries", "Runtime fault [R0802]: bad pointer\nexit: 70\n"), 70);
  assert.equal(goldenFailureExit("memory/unsafe_sentries_provenance", "Runtime fault [R0801]: bad pointer\n"), 70);
  for (const report of ["Runtime fault [R0801]: bad pointer\nexit: 1\n", "Runtime fault [R0802]: bad pointer\nexit: 101\n"]) {
    assert.equal(goldenFailureExit("memory/unsafe_sentries", report), null);
  }
  assert.equal(goldenFailureExit("io/log_fatal", ""), 1);
  assert.equal(goldenFailureExit("unknown/stem", ""), null);
  for (const stem of [
    "errors/propagation_journey", "errors/propagation_journey_direct",
    "errors/propagation_journey_one_hop", "errors/propagation_journey_core",
    "errors/error_context", "errors/default_error_conversion", "errors/default_err_edge",
    "errors/fatal_exit", "serde/decode_error_exit", "crypto/crypto_error_run",
  ]) {
    assert.equal(goldenFailureExit(stem, "source-proved entry report\n"), 1);
  }
  for (const report of ["unknown report\n", "Error: unproved return\n", "Error [CUSTOM]: unknown code\n", "[fatal] unproved exit\n", "Runtime fault [R0999]: unknown fault\n"]) {
    assert.equal(goldenFailureExit("unknown/stem", report), null);
  }
});

// Exercise the actual run-only harness with real ELF child fixtures. These
// test stream/status comparisons, not the compiler or runtime implementation.
function runGoldenFixture(stem, overrides = {}) {
  const repo = fileURLToPath(new URL("../../", import.meta.url));
  const expected = join(repo, "Examples/features/expected", stem);
  const stdoutExpected = existsSync(`${expected}.out`) ? readFileSync(`${expected}.out`) : Buffer.alloc(0);
  const failure = existsSync(`${expected}.err.out`);
  const stderrExpected = failure ? readFileSync(`${expected}.err.out`) : Buffer.alloc(0);
  const statusExpected = failure ? goldenFailureExit(stem, stderrExpected.toString("utf8")) : 0;
  assert.notEqual(statusExpected, null, `ambiguous fixture exit: ${stem}`);
  const stdout = Buffer.from(overrides.stdout ?? stdoutExpected);
  const stderr = Buffer.from(overrides.stderr ?? stderrExpected);
  const status = overrides.status ?? statusExpected;
  const base = process.env.JET_TEST_SCRATCH_DIR || join(homedir(), ".cache/jet-dev/scratch");
  mkdirSync(base, { recursive: true });
  const dir = mkdtempSync(join(base, "golden-failure-test-"));
  try {
    const lower = join(dir, "lower");
    mkdirSync(lower);
    const slug = stem.replace(/[^A-Za-z0-9_-]+/gu, "_");
    writeFileSync(join(dir, "stdout.data"), stdout);
    writeFileSync(join(dir, "stderr.data"), stderr);
    const assembly = [
      ".text", ".globl _start", "_start:",
      "mov $1, %eax", "mov $1, %edi", "lea stdout_data(%rip), %rsi", `mov $${stdout.length}, %edx`, "syscall",
      "mov $1, %eax", "mov $2, %edi", "lea stderr_data(%rip), %rsi", `mov $${stderr.length}, %edx`, "syscall",
      "mov $60, %eax", `mov $${status}, %edi`, "syscall",
      ".section .rodata", "stdout_data:", '.incbin "stdout.data"', "stderr_data:", '.incbin "stderr.data"',
      '.section .note.GNU-stack,"",@progbits', "",
    ].join("\n");
    writeFileSync(join(dir, "child.s"), assembly);
    for (const [tool, args] of [
      ["as", ["--64", "-o", join(lower, `${slug}.o`), "child.s"]],
      ["ld", ["-o", join(lower, `${slug}.elf`), join(lower, `${slug}.o`)]],
    ]) {
      const built = spawnSync(tool, args, { cwd: dir, encoding: "utf8", timeout: 10000 });
      assert.equal(built.status, 0, `${tool}: ${built.error?.message ?? built.stderr}`);
    }
    writeFileSync(join(lower, `${slug}.executable`), `${join(lower, `${slug}.elf`)}\n`);
    const list = join(dir, "cases");
    writeFileSync(list, `${stem}\n`);
    const checked = spawnSync(process.execPath, [
      join(repo, "Compiler/JetBackend/Tests/run-goldens.mjs"), dir,
      "--stage", "run", "--until", "run", "--case-list", list,
    ], {
      cwd: repo, encoding: "utf8", timeout: 30000,
      env: { ...process.env, JET: process.execPath, JET_RUNTIME_PACK: "", JET_RUNTIME_C_LIB: "", JET_GOLDEN_FEATURES: join(repo, "Examples/features") },
    });
    const row = readFileSync(join(dir, "results.tsv"), "utf8").trim().split("\t").slice(1);
    assert.equal(checked.status, row[0] === "pass" ? 0 : 1, checked.stdout + checked.stderr);
    assert.deepEqual(readFileSync(join(dir, "run", `${slug}.stdout`)), stdout);
    assert.deepEqual(readFileSync(join(dir, "run", `${slug}.stderr`)), stderr);
    assert.equal(JSON.parse(readFileSync(join(dir, "run", `${slug}.status.json`))).status, status);
    return row;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

for (const stem of ["memory/release_defer_panic", "memory/scope_close_safety_net_panic"]) {
  test(`golden failure preserves the existing Close stdout oracle: ${stem}`, () => {
    assert.equal(runGoldenFixture(stem)[0], "pass");
    const [verdict, detail] = runGoldenFixture(stem, { stdout: "" });
    assert.equal(verdict, "wrong");
    assert.match(detail, /stdout differs/u);
  });
}

test("golden failure rejects unexpected stdout when the oracle is absent", () => {
  const [verdict, detail] = runGoldenFixture("errors/panic", { stdout: "unexpected output\n" });
  assert.equal(verdict, "wrong");
  assert.match(detail, /stdout differs/u);
});

test("golden failure rejects the other former wildcard exit", () => {
  const [verdict, detail] = runGoldenFixture("memory/release_defer_panic", { status: 1 });
  assert.equal(verdict, "wrong");
  assert.match(detail, /exit 1 \(expected 70\)/u);
});

test("golden failure rejects different stderr with correct stdout and exit", () => {
  const [verdict, detail] = runGoldenFixture("memory/release_defer_panic", { stderr: "different report\n" });
  assert.equal(verdict, "wrong");
  assert.match(detail, /stderr differs/u);
});

test("interactive answers come from the example .stdin goldens", () => {
  const table = loadExampleStdin();
  assert.equal(table.get("io/stdin_input"), "alpha\nbeta\n");
  assert.equal(table.get("io/terminal"), "h");
});

test("the most specific differing field is reported first", () => {
  const reference = observed({
    status: "exit 1",
    diagnostics: parseDiagnostics("Error [E0109]: Text values aren't joined\n  --> a.jet:4:14\n"),
    stderr: "Error [E0109]: Text values aren't joined\n  --> a.jet:4:14\n",
  });
  const candidate = observed({
    status: "exit 101",
    panic: { kind: "panic", message: "index out of bounds" },
    stderr: "thread 'main' panicked at <LOC>:\nindex out of bounds\n",
  });
  const { differing } = comparePhase(reference, candidate);
  assert.deepEqual(differing.map((d) => d.field), ["panic", "status", "diagnostics", "stderr"]);
  assert.equal(differing[2].span, "a.jet:4:14");
});

test("a timeout is inconclusive only when both sides time out", () => {
  assert.ok(comparePhase(observed({ status: "timeout" }), observed({ status: "timeout" })).inconclusive);
  const { differing } = comparePhase(observed(), observed({ status: "timeout" }));
  assert.deepEqual(differing.map((d) => d.field), ["status"]);
});

test("a missing candidate is unavailable, never a match", () => {
  const run = spawnSync(process.execPath, [
    fileURLToPath(new URL("./compiler-diff.mjs", import.meta.url)),
    "--reference", process.execPath,
    "--candidate", "/nonexistent/jet-candidate",
    "--corpus", "differential",
  ], { encoding: "utf8", env: { ...process.env, JET_CANDIDATE_COMPILER: "" } });
  assert.equal(run.status, 2);
  assert.match(run.stderr, /unavailable: candidate compiler \/nonexistent\/jet-candidate is unavailable/u);
  assert.doesNotMatch(run.stdout, /summary:/u);
});

// Snapshot rotation can delete or replace a pinned binary mid-run. Both sides
// then fail identically, which must never compare as a match.
for (const [mutation, script] of [
  ["deleted", 'rm -f "$0"\necho "no problems"\n'],
  ["replaced", 'printf "#!/bin/sh\\necho replaced\\n" > "$0"\necho "no problems"\n'],
]) {
  test(`a compiler ${mutation} mid-run is unavailable, never a match`, () => {
    const base = process.env.JET_TEST_SCRATCH_DIR || join(homedir(), ".cache/jet-dev/scratch");
    mkdirSync(base, { recursive: true });
    const dir = mkdtempSync(join(base, "compiler-diff-test-"));
    try {
      const compilers = ["reference", "candidate"].map((role) => {
        const path = join(dir, `jet-${role}`);
        writeFileSync(path, `#!/bin/sh\n${script}`, { mode: 0o755 });
        return path;
      });
      const run = spawnSync(process.execPath, [
        fileURLToPath(new URL("./compiler-diff.mjs", import.meta.url)),
        "--reference", compilers[0],
        "--candidate", compilers[1],
        "--corpus", "differential",
        "--phases", "check",
        "--limit", "1",
        "--mem-limit", "none",
        "--root", join(dir, "root"),
      ], { encoding: "utf8" });
      assert.equal(run.status, 2, run.stdout + run.stderr);
      assert.match(run.stderr, /unavailable: reference compiler .*jet-reference changed or disappeared during /u);
      assert.doesNotMatch(run.stdout, /summary:/u);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
}

test("systemd-run's own exec failure is a launcher failure, not compiler output", () => {
  const failed = (stderr, overrides = {}) => ({ code: 1, stdout: "", stderr, ...overrides });
  const missing = failed("Failed to find executable /pinned/jet: No such file or directory\n");
  assert.match(launcherFailure(missing, "/pinned/jet", "6G"), /Failed to find executable/u);
  assert.match(launcherFailure(failed("Failed to execute: Permission denied\n"), "/pinned/jet", "6G"), /Permission denied/u);
  // Uncapped runs have no launcher; a compiler or program that prints the same
  // words, names another path, or writes more than that one line is observed.
  assert.equal(launcherFailure(missing, "/pinned/jet", "none"), null);
  assert.equal(launcherFailure(missing, "/other/jet", "6G"), null);
  assert.equal(launcherFailure(failed("Failed to execute: Permission denied\nmore\n"), "/pinned/jet", "6G"), null);
  assert.equal(launcherFailure(failed("Failed to execute: x\n", { code: 2 }), "/pinned/jet", "6G"), null);
});

test("evidence redacts credentials", () => {
  const text = redact("token=abc123 ghp_0123456789abcdefghijABCDEFGHIJ ok");
  assert.equal(text, "token=<REDACTED> <REDACTED> ok");
});

// These are comparator-input fixtures, not observed runtime receipts.
function runDifferentialFixture({ missingReference = false, missingNativeStatus = false, invalidStdout = false } = {}) {
  const repo = fileURLToPath(new URL("../../", import.meta.url));
  const stem = "strings/from_bytes";
  const slug = "strings_from_bytes";
  const expected = readFileSync(join(repo, "Examples/features/expected", `${stem}.out`));
  const replacement = expected.indexOf(Buffer.from("\uFFFD"));
  assert.ok(replacement >= 0);
  const invalid = Buffer.concat([expected.subarray(0, replacement), Buffer.from([0x80]), expected.subarray(replacement + 3)]);
  assert.equal(invalid.toString("utf8"), expected.toString("utf8"));
  const base = process.env.JET_TEST_SCRATCH_DIR || join(homedir(), ".cache/jet-dev/scratch");
  mkdirSync(base, { recursive: true });
  const dir = mkdtempSync(join(base, "golden-differential-test-"));
  try {
    mkdirSync(join(dir, "mir"));
    mkdirSync(join(dir, "run"));
    writeFileSync(join(dir, "results.tsv"), `${stem}\twrong\tstdout differs\n`);
    writeFileSync(join(dir, "mir", `${slug}.ref`), JSON.stringify({ mode: "run", status: 0 }));
    if (!missingReference) {
      writeFileSync(join(dir, "mir", `${slug}.stdout`), expected);
      writeFileSync(join(dir, "mir", `${slug}.stderr`), Buffer.alloc(0));
    }
    writeFileSync(join(dir, "run", `${slug}.stdout`), invalidStdout ? invalid : expected);
    if (!missingNativeStatus) writeFileSync(join(dir, "run", `${slug}.status.json`), JSON.stringify({ status: 0, signal: null, error: null }));
    const checked = spawnSync(process.execPath, [
      join(repo, "Compiler/JetBackend/Tests/run-differential.mjs"), dir,
    ], {
      cwd: repo, encoding: "utf8", timeout: 10000,
      env: { ...process.env, JET_GOLDEN_FEATURES: join(repo, "Examples/features") },
    });
    assert.equal(checked.status, invalidStdout ? 1 : 0, checked.stdout + checked.stderr);
    return readFileSync(join(dir, "differential.tsv"), "utf8").trim().split("\t").slice(1);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("golden failure differential rejects invalid UTF-8 replacement equivalence", () => {
  const [verdict, , detail] = runDifferentialFixture({ invalidStdout: true });
  assert.equal(verdict, "native-diverges");
  assert.match(detail, /bytes differ/u);
});

test("golden failure differential requires raw reference artifacts", () => {
  const [verdict, , detail] = runDifferentialFixture({ missingReference: true });
  assert.equal(verdict, "rust-absent");
  assert.match(detail, /missing raw reference streams/u);
});

test("golden failure differential never guesses an absent native exit status", () => {
  assert.equal(runDifferentialFixture({ missingNativeStatus: true })[0], "native-absent");
});

test("golden failure rejects invalid UTF-8 that displays like a successful stdout oracle", () => {
  const repo = fileURLToPath(new URL("../../", import.meta.url));
  const expected = readFileSync(join(repo, "Examples/features/expected/strings/from_bytes.out"));
  const replacement = expected.indexOf(Buffer.from("\uFFFD"));
  assert.ok(replacement >= 0);
  const invalid = Buffer.concat([expected.subarray(0, replacement), Buffer.from([0x80]), expected.subarray(replacement + 3)]);
  assert.equal(invalid.toString("utf8"), expected.toString("utf8"));
  const [verdict, detail] = runGoldenFixture("strings/from_bytes", { stdout: invalid });
  assert.equal(verdict, "wrong");
  assert.match(detail, /stdout differs/u);
});
