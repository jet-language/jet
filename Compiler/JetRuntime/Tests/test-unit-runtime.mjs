#!/usr/bin/env node
// Run against the C fixture linked to the exact composed native runtime pack.
// Collect every discrepancy before failing; this is not a Rust-tier proof.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
const [binary] = process.argv.slice(2);
if (!binary || process.argv.length !== 3) throw new Error("usage: test-unit-runtime.mjs FIXTURE_EXE");
const rows = [
  { name: "pass", ok: true, expectedFailure: false, skipped: false, stdout: "pass-out\n", stderr: "" },
  { name: "fail", ok: false, expectedFailure: false, skipped: false, stdout: "fail-out\n", stderr: "fail diagnostic" },
  { name: "expected", ok: true, expectedFailure: true, skipped: false, stdout: "expected-out\n", stderr: "expected diagnostic" },
  { name: "unexpected", ok: false, expectedFailure: true, skipped: false, stdout: "unexpected-out\n", stderr: "" },
  { name: "skip", ok: true, expectedFailure: false, skipped: true, stdout: "skip-out\n", stderr: "" },
  { name: 'quoted "\x01\x7f\u0080中', ok: true, expectedFailure: false, skipped: false, stdout: "UTF8 é中\n", stderr: "" },
];
function quote(s) {
  return JSON.stringify(s).replace(/[\u007f-\u009f]/gu, c => `\\u${c.charCodeAt(0).toString(16).padStart(4, "0")}`);
}
function captured(row, mode) { return mode === "all" || mode !== "none" && !row.ok; }
function expectedReport(mode, json, seed) {
  if (json) {
    const tests = rows.map(row => Object.entries(row).map(([key, value]) => {
      if ((key === "stdout" || key === "stderr") && !captured(row, mode)) value = "";
      return `${quote(key)}:${typeof value === "string" ? quote(value) : value}`;
    }).join(",")).map(row => `{${row}}`).join(",");
    return { status: 1, stdout: `{"schema":"jet.test.v1","ok":false,"passed":2,"failed":1,"skipped":1,"expectedFailures":1,"unexpectedPasses":1,"tests":[${tests}]}\n`, stderr: "" };
  }
  let stdout = seed === undefined ? "" : `shuffle: seed=${seed}\n`;
  let stderr = "";
  for (const row of rows) {
    if (captured(row, mode)) {
      stdout += row.stdout;
      if (row.stderr) stderr += row.stderr + "\n";
    }
    const status = row.skipped ? "skip" : row.expectedFailure ? (row.ok ? "expected-fail" : "UNEXPECTED-PASS (remove expected_fail: true)") : row.ok ? "pass" : "FAIL";
    stdout += `${row.name}: ${status}\n`;
  }
  stdout += "2 passed, 1 failed, 1 skipped, 1 expected-fail, 1 unexpected-pass\n";
  return { status: 1, stdout, stderr };
}
const failures = [];
let count = 0;
function check(name, args, config, expected) {
  count++;
  const env = { ...process.env };
  for (const key of ["JET_TEST_JSON", "JET_TEST_CAPTURE", "JET_TEST_SHUFFLE_SEED"]) delete env[key];
  Object.assign(env, config);
  try {
    const result = spawnSync(resolve(binary), args, { env, encoding: "utf8", timeout: 30000 });
    if (result.error) throw result.error;
    assert.equal(result.signal, null);
    for (const key of ["status", "stdout", "stderr"]) assert.equal(result[key], expected[key], `${name}: ${key}`);
  } catch (error) { failures.push(`${name}: ${error.message}`); }
}
check("wire ownership", ["wire"], {}, { status: 0, stdout: "unit wire OK\n", stderr: "" });
check("thread capture nesting and cleanup", ["capture"], {}, { status: 0, stdout: "unit capture OK\n", stderr: "" });
check("test scope cells and TLS cleanup", ["scopes"], {}, { status: 0, stdout: "unit scope cells OK\n", stderr: "" });
check("empty text", ["empty"], {}, { status: 0, stdout: "0 passed, 0 failed, 0 skipped\n", stderr: "" });
check("empty JSON", ["empty"], { JET_TEST_JSON: "" }, { status: 0, stdout: '{"schema":"jet.test.v1","ok":true,"passed":0,"failed":0,"skipped":0,"expectedFailures":0,"unexpectedPasses":0,"tests":[]}\n', stderr: "" });
for (const mode of ["failed", "all", "none", "unknown"]) {
  check(`text ${mode}`, [], { JET_TEST_CAPTURE: mode }, expectedReport(mode, false));
  check(`JSON ${mode}`, [], { JET_TEST_CAPTURE: mode, JET_TEST_JSON: "" }, expectedReport(mode, true));
}
check("nonunicode JSON presence", ["nonunicode-json"], {}, expectedReport("failed", true));
for (const [seed, normalized] of [["17", "17"], ["+00017", "17"], ["0", "0"], ["18446744073709551615", "18446744073709551615"], ["18446744073709551616", undefined], ["invalid", undefined], ["", undefined]]) {
  check(`shuffle ${JSON.stringify(seed)}`, [], { JET_TEST_SHUFFLE_SEED: seed }, expectedReport("failed", false, normalized));
}
check("property engine named gap", ["property"], {}, { status: 70, stdout: "", stderr: "native property tests require a Jet property engine\n" });
check("truncated wire", ["malformed"], {}, { status: 101, stdout: "", stderr: "internal error: native test record is invalid\n" });
if (failures.length) {
  for (const failure of failures) console.error(failure);
  throw new Error(`${failures.length}/${count} native unit runtime fixtures failed`);
}
console.log(`unit runtime: ${count} cases passed`);
