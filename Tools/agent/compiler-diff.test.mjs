import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  collectGoldenEntries,
  comparePhase,
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

test("interactive answers come from the example_stdin table", () => {
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

test("evidence redacts credentials", () => {
  const text = redact("token=abc123 ghp_0123456789abcdefghijABCDEFGHIJ ok");
  assert.equal(text, "token=<REDACTED> <REDACTED> ok");
});
