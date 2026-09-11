#!/usr/bin/env node
// Focused proof driver for D-INSPECT-DECISIONS1=A.
//
// The driver exercises the product surface only. It does not infer a compiler
// decision from diagnostic prose: every assertion below reads the shared
// decision rows and their derivation references.

import { spawnSync } from "node:child_process";

const args = process.argv.slice(2);
const json = args.includes("--json");
const valueFor = (name) => {
  const index = args.indexOf(name);
  return index === -1 ? undefined : args[index + 1];
};
const accepted = valueFor("--accepted");
const rejected = valueFor("--rejected");
const lint = valueFor("--lint");
const jet = process.env.JET_BIN ?? "jet";

const result = { ok: true, checks: [], failures: [] };
const fail = (message) => {
  result.ok = false;
  result.failures.push(message);
};
const pass = (message) => result.checks.push(message);

function run(command, commandArgs) {
  const child = spawnSync(command, commandArgs, {
    encoding: "utf8",
    env: process.env,
    maxBuffer: 32 * 1024 * 1024,
  });
  if (child.error) throw child.error;
  return {
    status: child.status ?? 1,
    stdout: child.stdout ?? "",
    stderr: child.stderr ?? "",
  };
}

function fieldsOf(value) {
  return value?.fields ?? value?.value?.fields ?? value;
}

function inspect(file) {
  const invocation = run(jet, ["inspect", "decisions", file, "--json"]);
  if (invocation.status !== 0) {
    throw new Error(`inspect decisions failed for ${file}: ${invocation.stderr.trim()}`);
  }
  let parsed;
  try {
    parsed = JSON.parse(invocation.stdout);
  } catch (error) {
    throw new Error(`inspect decisions returned non-JSON for ${file}: ${error}`);
  }
  const fields = fieldsOf(parsed);
  const rows = fields?.rows;
  if (!Array.isArray(rows)) throw new Error(`decision envelope for ${file} has no rows array`);
  return { parsed, fields, rows, text: invocation.stdout };
}

function checkIdentity(label, rows) {
  for (const row of rows) {
    if (!row.derivation_ref) fail(`${label}: row ${row.id} has no derivation_ref`);
    const identity = row.identity;
    if (!identity) {
      fail(`${label}: row ${row.id} has no full identity`);
      continue;
    }
    for (const key of [
      "source",
      "configuration",
      "profile",
      "target",
      "implementation",
      "artifact",
      "run",
    ]) {
      if (typeof identity[key] !== "string" || identity[key].length === 0) {
        fail(`${label}: row ${row.id} identity.${key} is empty`);
      }
    }
  }
  pass(`${label}: ${rows.length} rows carry shared derivation identity`);
}

function checkAccepted(file) {
  const projection = inspect(file);
  checkIdentity("accepted", projection.rows);
  const row = projection.rows.find(
    (candidate) =>
      candidate.kind === "vectorize" &&
      ["accepted", "selected"].includes(candidate.disposition),
  );
  if (!row) fail(`accepted: no vectorization acceptance row in ${file}`);
  else pass(`accepted: vectorization rule ${row.rule}`);
}

function checkRejected(file) {
  const projection = inspect(file);
  checkIdentity("rejected", projection.rows);
  const row = projection.rows.find((candidate) => candidate.kind === "vectorize");
  if (!row) fail(`rejected: no vectorization row in ${file}`);
  else if (
    row.disposition !== "rejected" ||
    !/strict[- ]order|strict order/i.test(`${row.reason} ${row.rule}`)
  ) {
    fail(`rejected: Float reduction row does not name strict order (${row.reason})`);
  } else pass("rejected: strict-order reduction rejection is present");
}

function checkEdits(file) {
  const projection = inspect(file);
  const lintRows = projection.rows.filter((row) => row.edit);
  for (const row of lintRows) {
    if (!row.derivation_ref) fail(`lint: row ${row.id} edit has no derivation_ref`);
  }
  if (lintRows.length === 0) fail(`lint: no ledger row with a structured edit in ${file}`);
  else pass(`lint: ${lintRows.length} structured edit row(s) share the ledger`);
}

function checkDeopt(file) {
  const invocation = run(jet, ["run", file]);
  const output = `${invocation.stdout}\n${invocation.stderr}`;
  if (invocation.status !== 0) {
    fail(`deopt: run failed for ${file}: ${invocation.stderr.trim()}`);
    return;
  }
  if (/trace[-_]tiers/i.test(output)) fail("deopt: ordinary run unexpectedly requires --trace-tiers");
  const lines = output.split(/\r?\n/).filter((line) => /^deopt:\s+function=/i.test(line.trim()));
  if (lines.length === 0) fail(`deopt: no ordinary deopt notice for ${file}`);
  else pass(`deopt: ${lines.length} concise notice(s) without --trace-tiers`);
}

try {
  if (accepted) checkAccepted(accepted);
  if (rejected) checkRejected(rejected);
  if (lint) checkEdits(lint);
  if (deopt) checkDeopt(deopt);
  if (!accepted && !rejected && !lint && !deopt) {
    throw new Error(
      "provide at least one of --accepted, --rejected, --lint, or --deopt",
    );
  }
} catch (error) {
  fail(String(error));
}

if (json) {
  process.stdout.write(`${JSON.stringify(result)}\n`);
} else {
  for (const check of result.checks) console.log(`ok: ${check}`);
  for (const failure of result.failures) console.error(`not ok: ${failure}`);
}
process.exitCode = result.ok ? 0 : 1;
