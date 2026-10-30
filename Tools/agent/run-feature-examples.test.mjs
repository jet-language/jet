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
  runCwd,
  runProcess,
  skipReason,
  unmetRequirement,
  untilExpected,
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
  assert.deepEqual(runArgs(entry, "step: jet run --allow=Env,Time authority_allow_flag.jet\n"), ["run", "--allow=Env,Time", "authority_allow_flag.jet"]);
  assert.deepEqual(runArgs(entry, "normalize: argv0\n"), ["run", shown]);
  assert.deepEqual(runArgs(entry, "step: built alpha beta\n"), ["run", shown, "--", "alpha", "beta"]);
  assert.deepEqual(runArgs(entry, "step: build\nstep: built alpha\n"), ["run", shown, "--", "alpha"]);
  assert.deepEqual(runArgs(entry, "step: built\n"), ["run", shown]);
  assert.deepEqual(runArgs(entry, "step: jet run --allow=Env other.jet\n"), ["run", shown]);
  assert.deepEqual(runArgs(entry, "step: build\nstep: jet run -A authority_allow_flag.jet\n"), ["run", "-A", "authority_allow_flag.jet"]);
  assert.deepEqual(runArgs(entry, "step: built a\nstep: built b\n"), ["run", shown]);
});

test("a case's own `jet run` step runs from the example's directory", () => {
  const entry = { stem: "effects/authority_core_method", shown: "Examples/features/effects/authority_core_method.jet", path: "/repo/Examples/features/effects/authority_core_method.jet" };
  assert.equal(runCwd(entry, "step: jet run authority_core_method.jet\n", "/stage"), "/stage/Examples/features/effects");
  assert.equal(runCwd(entry, "step: built\n", "/stage"), "/stage");
  assert.equal(runCwd(entry, null, "/stage"), "/stage");
  assert.ok(runCwd(entry, "step: jet run authority_core_method.jet\n", undefined).endsWith("/Examples/features/effects"));
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

test("a `run: none` example passes once its output is the expected output", async () => {
  const expected = { out: "true\n", err: null, stderrOut: null };
  assert.equal(untilExpected("step: build\n", expected), null);
  assert.equal(untilExpected("run: none\n", { ...expected, out: null }), null);
  const until = untilExpected("# serves\nrun: none\n", expected);
  const started = Date.now();
  const run = await runProcess("sh", ["-c", "echo true; sleep 30"], { cwd: process.cwd(), timeoutMs: 20000, scratch: process.cwd(), until });
  assert.equal(run.timedOut, false);
  assert.equal(run.stopped, true);
  assert.equal(classifyRun({ stem: "net/serve", path: "serve.jet" }, run, expected).status, "passed");
  assert.ok(Date.now() - started < 5000, "the serving example was stopped at its expected output");
});

test("a case whose host requirement is missing is skipped, as testkit SKIP-HOST", () => {
  const host = { family: "unix", hasTool: (name) => name === "javac" || name === "sh", hasGtk4: () => false };
  assert.equal(unmetRequirement(null, host), null);
  assert.equal(unmetRequirement("requires: tool:go\nstep: jet check run.jet\n", host), "tool:go");
  assert.equal(unmetRequirement("requires: tool:java|javac\n", host), null);
  assert.equal(unmetRequirement("requires: tool:sh\nrequires: tool:bwrap\n", host), "tool:bwrap");
  assert.equal(unmetRequirement("requires: gtk4\n", host), "gtk4");
  assert.equal(unmetRequirement("requires: os:windows\n", host), "os:windows");
  assert.equal(unmetRequirement("requires: os:unix\n", host), null);
  assert.equal(unmetRequirement("requires: os:!windows\n", host), null);
  assert.equal(unmetRequirement("requires: os:linux|unix\n", host), null);
  assert.equal(unmetRequirement("requires: env:JET_RAYLIB_DISPLAY=1\n", host), null);
  assert.equal(unmetRequirement("requires: probe something\nrequires: tool:go\n", host), "tool:go");
  const entry = { stem: "lowlevel/polyglot_go", path: "/nonexistent/run.jet" };
  const expected = { out: "ok\n", err: null, stderrOut: null };
  assert.equal(skipReason(entry, expected, "requires: tool:go\n", host), "requires tool:go");
  assert.equal(skipReason(entry, expected, "requires: tool:javac\n", host), null);
  assert.equal(skipReason(entry, { out: null, err: null, stderrOut: null }, "requires: tool:go\n", host), "no native golden");
});
