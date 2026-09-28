#!/usr/bin/env node

// Compare an off-device Nix oracle with native Jet evaluator rows.
// This tool consumes evidence. It never starts Nix, reads a store, or grants
// process/network authority to the evaluator.

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";

const SCHEMA = 1;
const CLASSES = [
  "matched",
  "version_mismatch",
  "drv_mismatch",
  "output_set_mismatch",
  "output_path_mismatch",
  "graph_mismatch",
  "closure_mismatch",
  "missing_identity",
  "unsupported",
  "jet_error",
  "nix_error",
  "missing_source",
];

function fail(message) {
  throw new Error(`differential report: ${message}`);
}

function object(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    fail(`${label} must be an object`);
  }
  return value;
}

function string(value, label) {
  if (typeof value !== "string" || value.length === 0) {
    fail(`${label} must be a non-empty string`);
  }
  return value;
}

function revisionString(value, label) {
  const revision = string(value, label);
  if (!/^[0-9a-f]{40}$/.test(revision)) {
    fail(`${label} must be exactly 40 lowercase hexadecimal characters`);
  }
  return revision;
}

function attrpath(value, label) {
  if (!Array.isArray(value) || value.length === 0 || value.some((part) => typeof part !== "string" || part.length === 0)) {
    fail(`${label} must be a non-empty string array`);
  }
  return [...value];
}

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, canonical(value[key])]),
    );
  }
  return value;
}

function canonicalJson(value) {
  return JSON.stringify(canonical(value));
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function normalizeOutputs(value, label) {
  const outputs = {};
  if (Array.isArray(value)) {
    for (const output of value) {
      const fields = object(output, `${label} output`);
      const name = string(fields.name, `${label} output.name`);
      const storePath = fields.storePath ?? fields.store_path;
      outputs[name] = string(storePath, `${label} output ${name}.storePath`);
    }
  } else {
    const fields = object(value, label);
    for (const name of Object.keys(fields)) {
      outputs[name] = string(fields[name], `${label}.${name}`);
    }
  }
  if (Object.keys(outputs).length === 0) fail(`${label} must contain one output`);
  return Object.fromEntries(Object.keys(outputs).sort().map((name) => [name, outputs[name]]));
}

function normalizeReferences(value, label) {
  if (value === undefined) return null;
  if (!Array.isArray(value) || value.some((item) => typeof item !== "string")) {
    fail(`${label} must be a string array`);
  }
  return [...new Set(value)].sort();
}

function normalizeRecord(value, label) {
  const fields = object(value, label);
  const status = fields.status ?? "ok";
  string(status, `${label}.status`);
  const record = {
    attrpath: attrpath(fields.attrpath, `${label}.attrpath`),
    status,
  };
  if (status !== "ok") {
    record.errorClass = fields.errorClass ?? fields.error_class ?? null;
    record.error = fields.error ?? null;
    return record;
  }
  record.version = string(fields.version ?? "", `${label}.version`);
  record.drvPath = string(fields.drvPath ?? fields.drv_path, `${label}.drvPath`);
  record.outputs = normalizeOutputs(fields.outputs, `${label}.outputs`);
  record.directReferences = normalizeReferences(
    fields.directReferences ?? fields.direct_references ?? fields.references,
    `${label}.directReferences`,
  );
  record.closureDigest = fields.closureDigest ?? fields.closure_digest ?? null;
  if (record.closureDigest !== null) string(record.closureDigest, `${label}.closureDigest`);
  return record;
}

function normalizeInput(value, label) {
  const input = object(value, label);
  if (input.schema !== undefined && input.schema !== SCHEMA) {
    fail(`${label}.schema must be ${SCHEMA}`);
  }
  const revision = revisionString(input.revision ?? input.nixpkgs_revision, `${label}.revision`);
  const system = string(input.system, `${label}.system`);
  if (input.nixpkgs !== undefined && input.nixpkgs !== null) {
    const nixpkgs = object(input.nixpkgs, `${label}.nixpkgs`);
    const pinnedRevision = nixpkgs.revision ?? nixpkgs.rev;
    if (pinnedRevision !== undefined && revisionString(pinnedRevision, `${label}.nixpkgs.revision`) !== revision) {
      fail(`${label}.nixpkgs.revision must match ${label}.revision`);
    }
  }
  if (!Array.isArray(input.records)) fail(`${label}.records must be an array`);
  const records = input.records.map((record, index) => normalizeRecord(record, `${label}.records[${index}]`));
  const byKey = new Map();
  for (const record of records) {
    const key = canonicalJson(record.attrpath);
    if (byKey.has(key)) fail(`${label} repeats attrpath ${record.attrpath.join(".")}`);
    byKey.set(key, record);
  }
  return {
    schema: SCHEMA,
    revision,
    system,
    nix: input.nix ?? null,
    nixpkgs: input.nixpkgs ?? null,
    records,
    byKey,
  };
}

function compareIdentity(nix, jet) {
  if (nix.revision !== jet.revision || nix.system !== jet.system) {
    fail(`Nix and Jet inputs disagree on revision/system (${nix.revision}/${nix.system} vs ${jet.revision}/${jet.system})`);
  }
}

function compareRecord(nix, jet) {
  if (!jet) return { class: "missing_source", nix };
  if (nix.status !== "ok") return { class: "nix_error", nix, jet };
  if (jet.status === "unsupported") return { class: "unsupported", nix, jet };
  if (jet.status !== "ok") return { class: "jet_error", nix, jet };
  if (nix.version !== jet.version) return { class: "version_mismatch", nix, jet };
  if (nix.drvPath !== jet.drvPath) return { class: "drv_mismatch", nix, jet };

  const nixNames = Object.keys(nix.outputs).sort();
  const jetNames = Object.keys(jet.outputs).sort();
  if (canonicalJson(nixNames) !== canonicalJson(jetNames)) {
    return { class: "output_set_mismatch", nix, jet };
  }
  for (const name of nixNames) {
    if (nix.outputs[name] !== jet.outputs[name]) {
      return { class: "output_path_mismatch", nix, jet };
    }
  }
  if (nix.directReferences === null || jet.directReferences === null || nix.closureDigest === null || jet.closureDigest === null) {
    return { class: "missing_identity", nix, jet };
  }
  if (canonicalJson(nix.directReferences) !== canonicalJson(jet.directReferences)) {
    return { class: "graph_mismatch", nix, jet };
  }
  if (nix.closureDigest !== jet.closureDigest) {
    return { class: "closure_mismatch", nix, jet };
  }
  return { class: "matched", nix, jet };
}

export function compare(nixInput, jetInput) {
  const nix = normalizeInput(nixInput, "nix");
  const jet = normalizeInput(jetInput, "jet");
  compareIdentity(nix, jet);

  const rows = [];
  for (const record of nix.records) {
    const key = canonicalJson(record.attrpath);
    const result = compareRecord(record, jet.byKey.get(key));
    rows.push({ attrpath: record.attrpath, ...result });
  }
  rows.sort((left, right) => canonicalJson(left.attrpath).localeCompare(canonicalJson(right.attrpath)));

  const counts = Object.fromEntries(CLASSES.map((name) => [name, 0]));
  for (const row of rows) counts[row.class] += 1;
  const status = rows.length === 0
    ? "not-measured"
    : counts.matched === rows.length
      ? "pass"
      : "fail";
  const inventory = rows.map((row) => row.attrpath);
  return {
    schema: SCHEMA,
    kind: "jet-nix-evaluator-differential",
    status,
    nix_version: nix.nix?.version ?? null,
    nix_source_commit: nix.nix?.source_commit ?? null,
    nixpkgs_revision: nix.nixpkgs?.revision ?? nix.nixpkgs?.rev ?? nix.revision,
    nixpkgs_nar_hash: nix.nixpkgs?.nar_hash ?? nix.nixpkgs?.narHash ?? null,
    system: nix.system,
    inventory_digest: `sha256:${sha256(canonicalJson(inventory))}`,
    records_total: rows.length,
    counts,
    rows,
  };
}

function parseArgs(args) {
  const result = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--self-test") {
      result.selfTest = true;
    } else if (["--nix", "--jet", "--output"].includes(arg)) {
      const value = args[++index];
      if (!value) fail(`${arg} needs a value`);
      result[arg.slice(2)] = value;
    } else {
      fail(`unknown option ${arg}`);
    }
  }
  return result;
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    fail(`cannot read ${file}: ${error.message}`);
  }
}

function runSelfTest() {
  const base = {
    schema: SCHEMA,
    revision: "a".repeat(40),
    system: "x86_64-linux",
    nix: { version: "2.34.8", source_commit: "nix-source" },
    nixpkgs: { revision: "a".repeat(40), nar_hash: "sha256-nixpkgs" },
  };
  const record = {
    attrpath: ["hello"],
    version: "1.0",
    drvPath: "/nix/store/hello.drv",
    outputs: [{ name: "out", storePath: "/nix/store/hello" }],
    directReferences: ["/nix/store/glibc"],
    closureDigest: "sha256:closure",
  };
  const passed = compare({ ...base, records: [record] }, { ...base, records: [record] });
  assert.equal(passed.status, "pass");
  assert.equal(passed.counts.matched, 1);

  const unsupported = compare(
    { ...base, records: [record] },
    { ...base, records: [{ ...record, status: "unsupported", error: "callPackage" }] },
  );
  assert.equal(unsupported.counts.unsupported, 1);
  assert.equal(unsupported.status, "fail");

  const incomplete = compare(
    { ...base, records: [record] },
    { ...base, records: [{ ...record, directReferences: undefined, closureDigest: undefined }] },
  );
  assert.equal(incomplete.counts.missing_identity, 1);

  assert.throws(
    () => compare({ ...base, revision: "not-a-revision", records: [] }, { ...base, records: [] }),
    /exactly 40 lowercase hexadecimal characters/,
  );
  assert.throws(
    () => compare({ ...base, nixpkgs: { revision: "b".repeat(40) }, records: [] }, { ...base, records: [] }),
    /nixpkgs\.revision must match nix\.revision/,
  );

  const empty = compare({ ...base, records: [] }, { ...base, records: [] });
  assert.equal(empty.status, "not-measured");
  console.log("differential report self-test: passed");
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.selfTest) return runSelfTest();
  if (!args.nix || !args.jet) fail("usage: differential-report.mjs --nix NIX.json --jet JET.json [--output REPORT.json]");
  const report = compare(readJson(args.nix), readJson(args.jet));
  const text = `${JSON.stringify(canonical(report), null, 2)}\n`;
  if (args.output) fs.writeFileSync(args.output, text, { encoding: "utf8", flag: "wx" });
  else process.stdout.write(text);
  if (report.status !== "pass") process.exitCode = 1;
}

if (import.meta.url === `file://${process.argv[1]}`) main();
