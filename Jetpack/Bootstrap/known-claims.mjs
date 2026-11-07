#!/usr/bin/env node
// Classify one tier's Jetpack claim results against the testkit's
// known-failing ledger (tests/known-failing.tsv). Jetpack claim rows
// use the ID `jetpack-claim:<Area>/<claim name>`, then the card and the reason,
// tab-separated; the testkit matches IDs against case paths, so it never
// selects these rows.
//
//   node known-claims.mjs run <map.json> <run results> [ledger]
//   node known-claims.mjs aot <map.json> <aot log> [ledger]
//
// `run` results are check.sh's default-tier lines `pass <name>` / `fail <name>`;
// `aot` reads `jet test`'s `<name>: pass` / `<name>: FAIL` lines and requires
// every claim of the unit to report. A listed claim that fails is
// KNOWN-FAILING; a listed claim that passes is UNEXPECTED-PASS. Exit 0 only
// when every failure is listed and no listed claim passed.
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const PREFIX = "jetpack-claim:";

function fail(message) {
  console.error(`jetpack-bootstrap: ${message}`);
  process.exit(2);
}

// ID -> "card reason" for every Jetpack claim row of the ledger.
export function knownClaims(ledgerText) {
  const known = new Map();
  for (const raw of ledgerText.split("\n")) {
    const line = raw.trim();
    if (line === "" || line.startsWith("#") || !line.startsWith(PREFIX)) continue;
    const fields = line.split("\t");
    if (fields.length !== 3 || !/^#\d+$/.test(fields[1]) || fields[2].trim() === "") {
      throw new Error(`known-failing row needs \`${PREFIX}<Area>/<claim>\\t#card\\treason\`: ${line}`);
    }
    if (known.has(fields[0])) throw new Error(`duplicate known-failing row ${fields[0]}`);
    known.set(fields[0], `${fields[1]} ${fields[2]}`);
  }
  return known;
}

// name -> "pass" | "fail" from one tier's output.
export function tierResults(mode, map, text) {
  const names = new Set(map.tests);
  const results = new Map();
  for (const line of text.split("\n")) {
    let name = null;
    let status = null;
    if (mode === "run") {
      const match = /^(pass|fail) (.+)$/.exec(line);
      if (match) [name, status] = [match[2], match[1]];
    } else if (line.endsWith(": pass") || line.endsWith(": FAIL")) {
      name = line.slice(0, line.lastIndexOf(": "));
      status = line.endsWith(": pass") ? "pass" : "fail";
    }
    if (name === null || !names.has(name)) continue;
    results.set(name, status);
  }
  return results;
}

// The verdict lines and whether the tier is green.
export function classify(mode, map, text, ledgerText) {
  if (!Array.isArray(map.test_areas) || map.test_areas.length !== map.tests.length) {
    throw new Error("the source map has no test_areas for its tests; reassemble with this assemble.mjs");
  }
  const known = knownClaims(ledgerText);
  const results = tierResults(mode, map, text);
  const lines = [];
  const counts = { pass: 0, fail: 0, known: 0, unexpected: 0, missing: 0 };
  map.tests.forEach((name, index) => {
    const id = `${PREFIX}${map.test_areas[index]}/${name}`;
    const owner = known.get(id);
    const status = results.get(name);
    if (status === undefined) {
      if (mode === "aot") {
        counts.missing += 1;
        lines.push(`MISSING ${name}: the AOT run reported no result`);
      }
      return;
    }
    if (status === "pass" && owner === undefined) counts.pass += 1;
    else if (status === "pass") {
      counts.unexpected += 1;
      lines.push(`UNEXPECTED-PASS ${name} (${owner}): remove the stale known-failing row ${id}`);
    } else if (owner === undefined) {
      counts.fail += 1;
      lines.push(`FAIL ${name}`);
    } else {
      counts.known += 1;
      lines.push(`KNOWN-FAILING ${name} (${owner})`);
    }
  });
  lines.push(`jetpack-bootstrap: ${mode} claims: ${counts.pass} passed, ${counts.fail} failed, ${counts.known} known-failing, ${counts.unexpected} unexpected-pass${mode === "aot" ? `, ${counts.missing} missing` : ""}`);
  return { lines, ok: counts.fail === 0 && counts.unexpected === 0 && counts.missing === 0 };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [mode, mapPath, resultsPath, ledgerArg] = process.argv.slice(2);
  if (!["run", "aot"].includes(mode) || !mapPath || !resultsPath) fail("usage: known-claims.mjs run|aot <map.json> <results> [ledger]");
  const ledgerPath = ledgerArg ?? resolve(dirname(fileURLToPath(import.meta.url)), "../../tests/known-failing.tsv");
  let verdict;
  try {
    verdict = classify(mode, JSON.parse(readFileSync(mapPath, "utf8")), readFileSync(resultsPath, "utf8"), readFileSync(ledgerPath, "utf8"));
  } catch (error) {
    fail(error.message);
  }
  for (const line of verdict.lines) console.log(line);
  process.exit(verdict.ok ? 0 : 1);
}
