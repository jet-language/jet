import test from "node:test";
import assert from "node:assert/strict";
import {
  asRunCommand,
  buildArgs,
  classifyEmit,
  classifyRun,
  collectFeatureExamples,
  nativeRunIsWebTarget,
  parseArgs,
  runArgs,
  runProcess,
  warningOnlyStderr,
  withoutWarningDiagnostics,
} from "./run-feature-examples.mjs";

test("warning-only stderr is non-fatal", () => {
  const warning = "Warning [L0103] (unused_import): Import is never used.\n  --> example.jet:1:1\n\n";
  assert.equal(warningOnlyStderr(warning), true);
  assert.equal(withoutWarningDiagnostics(warning).trim(), "");
  assert.equal(classifyRun({ stem: "example", path: "example.jet" }, {
    code: 0,
    stdout: "ok\n",
    stderr: warning,
    timedOut: false,
    timeoutSec: 60,
  }, { out: "ok\n", err: null, stderrOut: null }).status, "passed");
});

test("emit tier goldens compare exact stdout and exit", () => {
  const ok = { code: 0, stdout: "fn run() -> Unit\n", stderr: "", timedOut: false, timeoutSec: 60 };
  assert.equal(classifyEmit("mir", ok, "fn run() -> Unit\n"), null);
  assert.equal(classifyEmit("mir", ok, "fn other() -> Unit\n").status, "no-match");
  assert.equal(classifyEmit("lir", { ...ok, code: 1 }, "").detail, "emit --lir exit 1");
  assert.equal(classifyEmit("ast", { ...ok, timedOut: true }, "").status, "timeout");
});

test("warning plus a compiler error remains a failure", () => {
  const stderr = "Warning [L0103] (unused_import): Import is never used.\n\nError [E0003]: bad syntax\n";
  assert.equal(warningOnlyStderr(stderr), false);
  assert.match(withoutWarningDiagnostics(stderr), /Error \[E0003\]/u);
  assert.equal(classifyRun({ stem: "example", path: "example.jet" }, {
    code: 1,
    stdout: "",
    stderr,
    timedOut: false,
    timeoutSec: 60,
  }, { out: "", err: null, stderrOut: null }).status, "failed");
});

test("expected failure comparison ignores warning blocks", () => {
  const expected = "Error [E0003]: bad syntax\n";
  const actual = "Warning [L0103] (unused_import): Import is never used.\n\nError [E0003]: bad syntax\n";
  assert.equal(classifyRun({ stem: "example", path: "example.jet" }, {
    code: 1,
    stdout: "",
    stderr: actual,
    timedOut: false,
    timeoutSec: 60,
  }, { out: null, err: expected, stderrOut: null }).status, "passed");
});

test("timeouts are distinct from failures", () => {
  assert.equal(classifyRun({ stem: "example", path: "example.jet" }, {
    code: null,
    stdout: "",
    stderr: "",
    timedOut: true,
    timeoutSec: 5,
  }, { out: "", err: null, stderrOut: null }).status, "timeout");
});

test("argument filters, tier and concurrency options parse", () => {
  assert.deepEqual(parseArgs(["net", "--jobs", "2", "--timeout", "60", "--no-env", "--json"]), {
    filters: ["net"], tier: "run", jobs: 2, timeoutSec: 60, list: false, json: true, noEnv: true,
  });
  assert.equal(parseArgs(["--tier", "build"]).tier, "build");
  assert.throws(() => parseArgs(["--tier", "emit"]), /--tier expects one of run, build/u);
});

test("feature discovery excludes expected and hidden trees", () => {
  const entries = collectFeatureExamples();
  assert.ok(entries.length > 700);
  assert.equal(entries.some((entry) => entry.shown.includes("/expected/")), false);
  assert.equal(entries.some((entry) => entry.shown.includes("/.jet/")), false);
});

test("web-targeted sources are skipped for native execution", () => {
  const entry = collectFeatureExamples().find((candidate) => candidate.stem === "web/web_wasm_map");
  assert.ok(entry);
  assert.equal(nativeRunIsWebTarget(entry), true);
});

test("a single `jet run` or `built` case step supplies the run flags or program arguments", () => {
  const entry = { stem: "effects/authority_allow_flag", shown: "Examples/features/effects/authority_allow_flag.jet", path: "/repo/Examples/features/effects/authority_allow_flag.jet" };
  const shown = entry.shown;
  assert.deepEqual(runArgs(entry, null), ["run", shown]);
  assert.deepEqual(runArgs(entry, "step: jet run --allow=Env,Time authority_allow_flag.jet\n"), ["run", "--allow=Env,Time", shown]);
  assert.deepEqual(runArgs(entry, "normalize: argv0\n"), ["run", shown]);
  assert.deepEqual(runArgs(entry, "step: built alpha beta\n"), ["run", shown, "--", "alpha", "beta"]);
  assert.deepEqual(runArgs(entry, "step: build\nstep: built alpha\n"), ["run", shown, "--", "alpha"]);
  assert.deepEqual(runArgs(entry, "step: built\n"), ["run", shown]);
  assert.deepEqual(runArgs(entry, "step: jet run --allow=Env other.jet\n"), ["run", shown]);
  assert.deepEqual(runArgs(entry, "step: build\nstep: jet run -A authority_allow_flag.jet\n"), ["run", "-A", shown]);
  assert.deepEqual(runArgs(entry, "step: built a\nstep: built b\n"), ["run", shown]);
});

test("the build tier builds with the case's run flags", () => {
  const entry = { stem: "effects/authority_allow_flag", shown: "Examples/features/effects/authority_allow_flag.jet", path: "/repo/Examples/features/effects/authority_allow_flag.jet" };
  assert.deepEqual(buildArgs(entry, null), ["build", entry.shown]);
  assert.deepEqual(buildArgs(entry, "step: jet run --allow=Env,Time authority_allow_flag.jet\n"), ["build", "--allow=Env,Time", entry.shown]);
  assert.deepEqual(buildArgs(entry, "step: built alpha beta\n"), ["build", entry.shown]);
});

test("the build tier compares compile-time diagnostics with the `jet run` command word", () => {
  const fix = (command) => `Error [E1803]: Application authority is undecided for \`FS\`\n Fix: run \`${command} --allow=FS example.jet\` to allow it for this run\n`;
  assert.equal(asRunCommand(fix("jet build")), fix("jet run"));
  const entry = { stem: "example", path: "example.jet" };
  const built = { code: 1, stdout: "", stderr: fix("jet build"), timedOut: false, timeoutSec: 60 };
  const expected = { out: null, err: fix("jet run"), stderrOut: null };
  assert.equal(classifyRun(entry, built, expected, "build").status, "passed");
  assert.equal(classifyRun(entry, built, expected).status, "no-match");
});

test("a timeout does not wait for a descendant that keeps the output pipes open", async () => {
  const started = Date.now();
  const run = await runProcess("sh", ["-c", "setsid sleep 30 & echo started; sleep 30"], { cwd: process.cwd(), timeoutMs: 500, scratch: process.cwd() });
  assert.equal(run.timedOut, true);
  assert.match(run.stdout, /started/);
  assert.ok(Date.now() - started < 5000, "the timed-out step settled within the drain window");
});

test("an exited program is not held by a background child that keeps the pipes open", async () => {
  const started = Date.now();
  const run = await runProcess("sh", ["-c", "setsid sleep 30 & echo done"], { cwd: process.cwd(), timeoutMs: 20000, scratch: process.cwd() });
  assert.equal(run.timedOut, false);
  assert.equal(run.code, 0);
  assert.equal(run.stdout, "done\n");
  assert.ok(Date.now() - started < 5000, "the finished step settled within the drain window");
});
