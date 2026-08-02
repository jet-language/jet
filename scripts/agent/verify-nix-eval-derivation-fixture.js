"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const root = path.resolve(__dirname, "../..");
const fixture = JSON.parse(
  fs.readFileSync(
    path.join(root, "tests/fixtures/nix-compat/stage-a-derivation.json"),
    "utf8",
  ),
);
const nix = process.env.JET_NIX_BIN || "nix";
const storeRoot = fs.mkdtempSync(path.join(os.tmpdir(), "jet-nix-eval-"));
const store = "local?root=" + storeRoot;

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

function equal(actual, expected, label) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    fail(
      label +
        ": expected " +
        JSON.stringify(expected) +
        ", got " +
        JSON.stringify(actual),
    );
  }
}

function evaluate(expression) {
  return JSON.parse(
    run([
      "eval",
      "--store",
      store,
      "--extra-experimental-features",
      "nix-command flakes",
      "--json",
      "--expr",
      expression,
    ]),
  );
}

try {
  const version = run(["--version"]);
  if (!version.includes("nix (Nix) " + fixture.oracle.nix_version)) {
    fail("reference evaluator must be Nix " + fixture.oracle.nix_version + ", got " + version);
  }

  for (const test of fixture.values) {
    equal(evaluate(test.nix_expression), test.nix_value, test.name);
  }

  for (const test of fixture.errors) {
    equal(evaluate(test.nix_expression), test.nix_value, test.name + " reference value");
  }
  console.log(
    "verified Nix " +
      fixture.oracle.nix_version +
      " derivation primitive fixture across " +
      fixture.values.length +
      " values",
  );
} finally {
  fs.rmSync(storeRoot, { recursive: true, force: true });
}
