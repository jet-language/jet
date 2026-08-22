"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const root = path.resolve(__dirname, "../..");
const fixture = JSON.parse(
  fs.readFileSync(path.join(root, "tests/fixtures/nix-compat/breadth.json"), "utf8"),
);
const oracle = JSON.parse(
  fs.readFileSync(path.join(root, "tests/fixtures/nix-compat/oracle.json"), "utf8"),
);
const nix = process.env.JET_NIX_BIN || "nix";

function run(args) {
  return execFileSync(nix, args, {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

function fail(message) {
  throw new Error(message);
}

function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, canonicalize(value[key])]),
    );
  }
  return value;
}

function equal(actual, expected, label) {
  const actualJson = JSON.stringify(canonicalize(actual));
  const expectedJson = JSON.stringify(canonicalize(expected));
  if (actualJson !== expectedJson) {
    fail(`${label}: expected ${expectedJson}, got ${actualJson}`);
  }
}

function evaluate(expression) {
  return JSON.parse(
    run([
      "eval",
      "--extra-experimental-features",
      "nix-command flakes",
      "--json",
      "--expr",
      expression,
    ]),
  );
}

function mutate(expression, seed) {
  switch (seed % 5) {
    case 0:
      return `# breadth seed ${seed}\n${expression}`;
    case 1:
      return `(\n${expression}\n)`;
    case 2:
      return `${expression}\n# breadth seed ${seed}\n`;
    case 3:
      return `(\n# breadth seed ${seed}\n${expression}\n)`;
    case 4:
      return `let result = (${expression}); in result`;
    default:
      return `let result = ${expression}; in result`;
  }
}

const version = run(["--version"]);
if (!version.includes(`nix (Nix) ${fixture.oracle.nix_version}`)) {
  fail(`reference evaluator must be Nix ${fixture.oracle.nix_version}, got ${version}`);
}
equal(oracle.nix.version, fixture.oracle.nix_version, "oracle Nix version");
equal(oracle.nix.source_commit, fixture.oracle.source_commit, "oracle source commit");
equal(oracle.nixpkgs.rev, fixture.oracle.nixpkgs_rev, "oracle nixpkgs revision");
equal(oracle.nixpkgs.nar_hash, fixture.oracle.nixpkgs_nar_hash, "oracle nixpkgs NAR hash");
for (const [name, value] of Object.entries(fixture.budgets)) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    fail(`budget ${name} must be a positive integer`);
  }
}

for (const group of [fixture.values, fixture.errors, fixture.locks || []]) {
  for (const test of group) {
    equal(evaluate(test.nix_expression), test.nix_value, `${test.name} reference value`);
    for (const seed of fixture.fuzz_seeds) {
      equal(
        evaluate(mutate(test.nix_expression, seed)),
        test.nix_value,
        `${test.name} seed ${seed} reference value`,
      );
    }
  }
}

for (const test of fixture.authority_values || []) {
  equal(evaluate(test.nix_expression), test.nix_value, `${test.name} authority value`);
  for (const seed of fixture.fuzz_seeds) {
    equal(
      evaluate(mutate(test.nix_expression, seed)),
      test.nix_value,
      `${test.name} seed ${seed} authority value`,
    );
  }
}

for (const test of fixture.authority_derivations || []) {
  equal(evaluate(test.nix_expression), test.nix_value, `${test.name} authority derivation value`);
  for (const seed of fixture.fuzz_seeds) {
    equal(
      evaluate(mutate(test.nix_expression, seed)),
      test.nix_value,
      `${test.name} seed ${seed} authority derivation value`,
    );
  }
}

for (const test of fixture.derivations || []) {
  equal(evaluate(test.nix_expression), test.nix_value, `${test.name} derivation value`);
  for (const seed of fixture.fuzz_seeds) {
    equal(
      evaluate(mutate(test.nix_expression, seed)),
      test.nix_value,
      `${test.name} seed ${seed} derivation value`,
    );
  }
}

execFileSync(
  process.execPath,
  [path.join(root, "scripts/agent/verify-nix-eval-fixture.js"), "breadth.json"],
  { cwd: root, env: process.env, stdio: "inherit" },
);

console.log(
  `verified Nix ${fixture.oracle.nix_version} breadth fixture: ${fixture.values.length} values, ${fixture.errors.length} errors, ${fixture.locks?.length || 0} locks, ${fixture.authority_values?.length || 0} authority values, ${fixture.authority_derivations?.length || 0} authority derivations, ${fixture.derivations?.length || 0} derivations, ${fixture.fuzz_seeds.length} seeds`,
);
