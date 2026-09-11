import test from "node:test";
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";
import { collectServiceTierTrace, collectTierTrace } from "./run.mjs";

function producer(facts, output = "") {
  const script = [
    `require("node:fs").writeFileSync(process.env.JET_TRACE_TIERS_PATH, ${JSON.stringify(JSON.stringify(facts))});`,
    `process.stdout.write(${JSON.stringify(output)});`,
  ].join(" ");
  return ["sh", "-c", `${process.execPath} -e ${JSON.stringify(script)}`];
}

function facts(rows, wholeProgramDeopt = false) {
  return {
    rows,
    native_rows: rows.filter((row) => row.tier === "native").length,
    interp_rows: rows.filter((row) => row.tier === "interp").length,
    whole_program_deopt: wholeProgramDeopt,
  };
}

const nativeRow = { function: "main", tier: "native", reason: null, millis: 1 };
const interpRow = { function: "scheduled", tier: "interp", reason: "scheduled deopt", millis: 1 };

async function withTempDir(fn) {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "jet-tier-trace-test-"));
  try {
    return await fn(dir);
  } finally {
    await fs.rm(dir, { recursive: true, force: true });
  }
}

test("tier trace collector rejects a stale compiler-owned sidecar", async () => {
  await withTempDir(async (cwd) => {
    await fs.writeFile(path.join(cwd, ".jet-tier-trace.json"), JSON.stringify(facts([nativeRow])));
    const result = await collectTierTrace(cwd, [
      ["sh", "-c", "printf ok"],
    ], Buffer.from("ok"));
    assert.equal(result.status, "failed");
    assert.equal(result.channel, "compiler_owned_sidecar");
    assert.match(result.reason, /sidecar is absent/);
  });
});

test("tier trace collector rejects malformed compiler-owned facts", async () => {
  await withTempDir(async (cwd) => {
    const command = [
      "sh",
      "-c",
      `${process.execPath} -e ${JSON.stringify("require(\"node:fs\").writeFileSync(process.env.JET_TRACE_TIERS_PATH, \"{\"); process.stdout.write(\"ok\")")}`,
    ];
    const result = await collectTierTrace(cwd, [command], Buffer.from("ok"));
    assert.equal(result.status, "failed");
    assert.match(result.reason, /sidecar is malformed/);
  });
});

test("tier trace collector aggregates earlier interpreter plans", async () => {
  await withTempDir(async (cwd) => {
    const result = await collectTierTrace(cwd, [
      producer(facts([interpRow], true), "a"),
      producer(facts([nativeRow]), "b"),
    ], Buffer.from("ab"));
    assert.equal(result.status, "passed");
    assert.equal(result.native_rows, 1);
    assert.equal(result.interp_rows, 1);
    assert.equal(result.whole_program_deopt, true);
    assert.equal(result.rows.map((row) => row.function).join(","), "scheduled,main");
    assert.equal(result.invocations.length, 2);
    assert.equal(result.invocations[0].interp_rows, 1);
    assert.equal(result.invocations[1].native_rows, 1);
  });
});

const execFileAsync = promisify(execFile);
const entriesDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../entries");
const traceEntries = ["sieve", "wordfreq", "datasummary", "csvtransform", "concurrency-service"];
const traceTimeoutMs = 15_000;

async function stageTraceEntry(root, name) {
  const entryDir = path.join(entriesDir, name);
  const entry = JSON.parse(await fs.readFile(path.join(entryDir, "entry.json"), "utf8"));
  const stageDir = path.join(root, name);
  await fs.cp(path.join(entryDir, "jet"), stageDir, { recursive: true });

  const expectedPath = entry.mode === "service"
    ? null
    : path.join(entryDir, entry.spec?.expected ?? "expected.out");
  const expected = expectedPath ? await fs.readFile(expectedPath) : null;
  const fixture = entry.spec?.fixtureGen;
  if (fixture) {
    await fs.cp(
      path.join(entryDir, path.dirname(fixture.script)),
      path.join(stageDir, path.dirname(fixture.script)),
      { recursive: true },
    );
    await fs.mkdir(path.dirname(path.join(stageDir, fixture.out)), { recursive: true });
    try {
      await execFileAsync("python3", [fixture.script, fixture.out], {
        cwd: stageDir,
        timeout: traceTimeoutMs,
      });
    } catch (error) {
      throw new Error(`${name}: fixture generation failed: ${error.message}`);
    }
  }
  return { entry, stageDir, expected };
}

async function collectMeasuredTrace(entry, stageDir, expected) {
  assert.ok(Array.isArray(entry.spec?.args), `${entry.name}: spec args must be an array`);
  const command = ["jet", "run", "run.jet", "--", ...entry.spec.args];
  const started = performance.now();
  const trace = entry.mode === "service"
    ? await collectServiceTierTrace(stageDir, entry, undefined, { timeoutMs: traceTimeoutMs })
    : await collectTierTrace(stageDir, [command], expected, null, { timeoutMs: traceTimeoutMs });
  return {
    command,
    trace,
    runtime_wall_seconds: (performance.now() - started) / 1000,
  };
}

function assertNativeTrace(name, entry, measured) {
  const label = `${name}: default jet run`;
  const { trace } = measured;
  if (trace.status !== "passed") {
    assert.fail(`${label} must exit cleanly and match output exactly: ${JSON.stringify(trace)}`);
  }
  assert.equal(trace.channel, "compiler_owned_sidecar", `${label} must use compiler-owned trace facts`);
  assert.equal(trace.whole_program_deopt, false, `${label} unexpectedly deopted the whole program`);
  assert.ok(Number.isInteger(trace.native_rows) && trace.native_rows > 0, `${label} has no native rows`);
  assert.equal(trace.interp_rows, 0, `${label} contains interpreter rows`);
  assert.ok(
    Number.isFinite(measured.runtime_wall_seconds) && measured.runtime_wall_seconds > 0,
    `${label} has no measured runtime_wall_seconds`,
  );
  assert.ok(Array.isArray(trace.invocations) && trace.invocations.length === 1, `${label} must have one invocation`);
  const [invocation] = trace.invocations;
  assert.equal(invocation.exit_code, 0, `${label} exit status must be zero`);
  assert.equal(invocation.whole_program_deopt, false, `${label} invocation unexpectedly deopted`);
  assert.ok(Number.isInteger(invocation.native_rows) && invocation.native_rows > 0, `${label} invocation has no native rows`);
  assert.equal(invocation.interp_rows, 0, `${label} invocation contains interpreter rows`);
  if (entry.mode === "service") {
    assert.deepEqual(invocation.command.slice(0, 4), ["jet", "run", "run.jet", "--"], `${label} command must be jet run`);
    assert.equal(invocation.command.length, 5, `${label} command must carry one service port`);
    assert.equal(invocation.command[4], "<port>", `${label} must use the harness-selected service port`);
  } else {
    assert.deepEqual(invocation.command, measured.command, `${label} must use the entry's spec args`);
  }
}

test(
  "default Jet run keeps every deopt corpus entry native and byte exact",
  { timeout: traceEntries.length * (traceTimeoutMs + 15_000) },
  async () => {
    await withTempDir(async (root) => {
      const seen = [];
      for (const name of traceEntries) {
        const staged = await stageTraceEntry(root, name);
        const measured = await collectMeasuredTrace(staged.entry, staged.stageDir, staged.expected);
        assertNativeTrace(name, staged.entry, measured);
        seen.push(name);
      }
      assert.deepEqual(seen, traceEntries);
    });
  },
);
