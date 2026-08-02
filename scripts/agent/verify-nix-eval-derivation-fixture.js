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
const oracle = JSON.parse(
  fs.readFileSync(path.join(root, "tests/fixtures/nix-compat/oracle.json"), "utf8"),
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

function resolvedExecutable(file) {
  if (file.includes(path.sep)) {
    return fs.realpathSync(file);
  }
  for (const directory of (process.env.PATH || "").split(path.delimiter)) {
    if (!directory) continue;
    const candidate = path.join(directory, file);
    try {
      if (fs.statSync(candidate).isFile()) return fs.realpathSync(candidate);
    } catch (_) {
      // Try the next PATH entry.
    }
  }
  fail("could not resolve the reference evaluator executable");
}

function hostSystem() {
  const architecture = process.arch === "arm64" ? "aarch64" : "x86_64";
  const operatingSystem = process.platform === "darwin" ? "darwin" : "linux";
  return `${architecture}-${operatingSystem}`;
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
  equal(oracle.nix.source_commit, fixture.oracle.source_commit, "oracle source commit");

  const source = `github:NixOS/nix/${fixture.oracle.source_commit}`;
  const metadata = JSON.parse(
    run([
      "flake",
      "metadata",
      "--offline",
      "--json",
      "--extra-experimental-features",
      "nix-command flakes",
      source,
    ]),
  );
  const resolvedCommit = metadata.revision || (metadata.locked && metadata.locked.rev);
  if (resolvedCommit !== fixture.oracle.source_commit) {
    fail(
      "reference evaluator source commit: expected " +
        fixture.oracle.source_commit +
        ", got " +
        resolvedCommit,
    );
  }

  const executable = resolvedExecutable(nix);
  const install = path.resolve(path.dirname(executable), "..");
  if (!install.startsWith("/nix/store/")) {
    fail("reference evaluator executable is not a pinned Nix store output: " + install);
  }
  const info = JSON.parse(
    run(["path-info", "--json", "--json-format", "1", "--offline", install]),
  )[install];
  const expectedNix = oracle.nix.builds[hostSystem()];
  if (!expectedNix || !info || info.narHash !== expectedNix.executable_nar_hash) {
    fail(
      "reference evaluator executable identity mismatch: expected " +
        (expectedNix && expectedNix.executable_nar_hash) +
        ", got " +
        (info && info.narHash),
    );
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
