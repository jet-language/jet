import test from "node:test";
import assert from "node:assert/strict";
import {
  classifyEmit,
  classifyRun,
  collectFeatureExamples,
  nativeRunIsWebTarget,
  parseArgs,
  runArgs,
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

test("argument filters and concurrency options parse", () => {
  assert.deepEqual(parseArgs(["net", "--jobs", "2", "--timeout", "60", "--no-env", "--json"]), {
    filters: ["net"], jobs: 2, timeoutSec: 60, list: false, json: true, noEnv: true,
  });
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
