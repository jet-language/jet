"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const root = path.resolve(__dirname, "../..");
const fixture = JSON.parse(
  fs.readFileSync(path.join(root, "tests/fixtures/nix-compat/stage-a.json"), "utf8"),
);
const oracle = JSON.parse(
  fs.readFileSync(path.join(root, "tests/fixtures/nix-compat/oracle.json"), "utf8"),
);
const nix = process.env.JET_NIX_BIN || "nix";

function run(args) {
  return execFileSync(nix, args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

function fail(message) {
  throw new Error(message);
}

function equal(actual, expected, label) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    fail(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
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

const version = run(["--version"]);
if (!version.includes(`nix (Nix) ${fixture.oracle.nix_version}`)) {
  fail(`reference evaluator must be Nix ${fixture.oracle.nix_version}, got ${version}`);
}
equal(oracle.nix.version, fixture.oracle.nix_version, "oracle Nix version");
equal(oracle.nix.source_commit, fixture.oracle.source_commit, "oracle source commit");
equal(oracle.nixpkgs.rev, fixture.oracle.nixpkgs_rev, "oracle nixpkgs revision");
equal(oracle.nixpkgs.nar_hash, fixture.oracle.nixpkgs_nar_hash, "oracle nixpkgs NAR hash");

for (const group of [fixture.values, fixture.errors, fixture.locks]) {
  for (const test of group) {
    equal(evaluate(test.nix_expression), test.nix_value, `${test.name} reference value`);
  }
}

const flake = `github:NixOS/nix/${oracle.nix.source_commit}`;
for (const [system, expected] of Object.entries(fixture.output_identities)) {
  const outputs = run([
    "build",
    "--extra-experimental-features",
    "nix-command flakes",
    "--no-link",
    "--print-out-paths",
    `${flake}#packages.${system}.nix`,
  ]).split("\n").filter(Boolean);
  if (outputs.length === 0) {
    fail(`${system}: expected at least one package output`);
  }

  let install;
  let executable;
  for (const output of outputs) {
    const binary = path.join(output, "bin/nix");
    if (!fs.existsSync(binary)) {
      continue;
    }
    if (fs.lstatSync(binary).isSymbolicLink()) {
      install = output;
      executable = fs.realpathSync(binary).replace(/\/bin\/nix$/, "");
    }
  }
  if (!install || !executable) {
    fail(`${system}: could not identify the complete install and its evaluator executable`);
  }

  const info = (storePath) => JSON.parse(
    run(["path-info", "--json", "--json-format", "1", "--offline", storePath]),
  )[storePath];
  const actual = {
    build_nar_hash: info(install).narHash,
    executable_nar_hash: info(executable).narHash,
  };
  equal(actual, expected, `${system} fixture output identities`);
  equal(
    actual,
    {
      build_nar_hash: oracle.nix.builds[system].build_nar_hash,
      executable_nar_hash: oracle.nix.builds[system].executable_nar_hash,
    },
    `${system} oracle output identities`,
  );
}

console.log(`verified Nix ${fixture.oracle.nix_version} Stage A fixture across ${Object.keys(fixture.output_identities).length} systems`);
