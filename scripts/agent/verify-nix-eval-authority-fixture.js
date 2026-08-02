"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const root = path.resolve(__dirname, "../..");
const fixture = JSON.parse(
  fs.readFileSync(path.join(root, "tests/fixtures/nix-compat/stage-a-authority.json"), "utf8"),
);
const oracle = JSON.parse(
  fs.readFileSync(path.join(root, "tests/fixtures/nix-compat/oracle.json"), "utf8"),
);
const nix = process.env.JET_NIX_BIN || "nix";
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "jet-nix-authority-"));

function run(args, cwd) {
  return execFileSync(nix, args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

function equal(actual, expected, label) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

try {
  if (!run(["--version"], root).includes(`nix (Nix) ${fixture.oracle.nix_version}`)) {
    throw new Error(`reference evaluator must be Nix ${fixture.oracle.nix_version}`);
  }
  equal(fixture.oracle.nix_version, oracle.nix.version, "oracle Nix version");
  equal(fixture.oracle.source_commit, oracle.nix.source_commit, "oracle source commit");
  equal(fixture.oracle.nixpkgs_rev, oracle.nixpkgs.rev, "oracle nixpkgs revision");
  equal(fixture.oracle.nixpkgs_nar_hash, oracle.nixpkgs.nar_hash, "oracle nixpkgs NAR hash");
  for (const test of fixture.values) {
    const directory = path.join(scratch, test.name);
    fs.mkdirSync(directory, { recursive: true });
    for (const [relative, source] of Object.entries(test.files)) {
      const target = path.join(directory, relative);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(target, source);
    }
    const oracleExpression = `let
      pkgs = { fd = "fd"; ripgrep = "ripgrep"; };
      flake = ${test.source};
      shell = flake.outputs.devShells.${test.system}.default;
    in {
      packages = shell.packages;
      buildInputs = shell.buildInputs;
      shellHook = shell.shellHook;
    }`;
    equal(
      JSON.parse(
        run([
          "eval",
          "--extra-experimental-features",
          "nix-command flakes",
          "--impure",
          "--json",
          "--expr",
          oracleExpression,
        ], directory),
      ),
      test.nix_value,
      `${test.name} reference value`,
    );
  }
  console.log(`verified Nix ${fixture.oracle.nix_version} authority fixture`);
} finally {
  fs.rmSync(scratch, { recursive: true, force: true });
}
