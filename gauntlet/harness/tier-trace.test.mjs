import test from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";
import { collectTierTrace } from "./run.mjs";

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
