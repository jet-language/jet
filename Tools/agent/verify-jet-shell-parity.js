"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const root = path.resolve(__dirname, "../..");
const source = fs.readFileSync(path.join(root, "env.jet"), "utf8");
const flake = fs.readFileSync(path.join(root, "flake.nix"));
const oracle = JSON.parse(
  fs.readFileSync(path.join(root, "tests/fixtures/nix-compat/env-shell-oracle.json"), "utf8"),
);

if (oracle.schema !== "jet-shell-oracle/v1" || oracle.flake !== "flake.nix") {
  throw new Error("shell oracle must identify schema jet-shell-oracle/v1 and flake.nix");
}
const flakeSha256 = crypto.createHash("sha256").update(flake).digest("hex");
if (oracle.flake_sha256 !== flakeSha256) {
  throw new Error(
    `shell oracle is stale for flake.nix: expected ${flakeSha256}, got ${oracle.flake_sha256 || "missing"}`,
  );
}

function declaredPackages(environment) {
  const moduleName = environment === "default" ? "dev" : environment;
  const moduleStart = source.indexOf(`module env.${moduleName}`);
  if (moduleStart < 0) throw new Error(`missing module env.${environment}`);
  const listStart = source.indexOf("default.[", moduleStart);
  if (listStart < 0) throw new Error(`module env.${environment} has no default package list`);
  const bodyStart = listStart + "default.[".length;
  const bodyEnd = source.indexOf("]", bodyStart);
  if (bodyEnd < 0) throw new Error(`module env.${environment} has an unterminated package list`);
  const packages = source
    .slice(bodyStart, bodyEnd)
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
  const moduleEnd = source.indexOf("\n}", bodyEnd);
  const tail = source.slice(bodyEnd, moduleEnd < 0 ? source.length : moduleEnd);
  for (const match of tail.matchAll(/default\.([A-Za-z][A-Za-z0-9_.-]*)/g)) {
    packages.push(match[1]);
  }
  return packages;
}

function expectedPackages(selection) {
  const derived = selection.derived || {};
  return selection.nix.flatMap((name) => derived[name] || [name]).concat(selection.runtime || []);
}

function same(actual, expected, label) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

function verifyNativeProjectionManifest() {
  const manifest = oracle.native_projection;
  if (!manifest || !Array.isArray(manifest.tools) || !Array.isArray(manifest.facts)) {
    throw new Error("shell oracle must declare the native projection manifest");
  }
  const expectedHookReplacements = {
    JET_ROOT: "project-root",
    TZDIR: "output(tzdata)/share/zoneinfo",
    JET_ENV_DISABLE: "constant:1",
    LD_LIBRARY_PATH: "output(vulkan-loader)/lib then output(raylib)/lib on Linux",
    JET_NIX_TMP_CLEANED: "native no-op marker",
    "shellHook.banner": "Shell.enter presentation",
  };
  same(
    Object.keys(oracle.hook_replacements || {}).sort(),
    Object.keys(expectedHookReplacements).sort(),
    "Nix hook replacement coverage",
  );
  for (const [name, expected] of Object.entries(expectedHookReplacements)) {
    same(oracle.hook_replacements[name], expected, `Nix hook replacement ${name}`);
  }
  same(
    manifest.tools,
    [
      {
        definition: "jetDev",
        command: "jet",
        relative_binary: "target/debug/jet",
        mode: "direct",
      },
      {
        definition: "jetpackDev",
        command: "jetpack",
        relative_binary: "target/debug/jetpack",
        mode: "direct",
      },
    ],
    "native tool manifest",
  );
  same(
    manifest.facts,
    [
      { variable: "JET_ROOT", kind: "project-root" },
      {
        variable: "TZDIR",
        kind: "output-path",
        package: "tzdata",
        relative: "share/zoneinfo",
        platform: "any",
      },
      {
        variable: "JET_ENV_DISABLE",
        kind: "constant",
        value: "1",
        decision: "D-ENVHOOK1",
      },
      {
        variable: "LD_LIBRARY_PATH",
        kind: "ordered-output-paths",
        packages: ["vulkan-loader", "raylib"],
        relative: "lib",
        platform: "linux",
        append: "inherited",
      },
      {
        variable: "JET_NIX_TMP_CLEANED",
        kind: "not-created-marker",
        value: "1",
        executes: false,
      },
    ],
    "native environment fact manifest",
  );
  for (const tool of manifest.tools) {
    const declaration = `${tool.definition} -> ${tool.relative_binary}`;
    if (!source.includes(declaration)) {
      throw new Error(`env.jet is missing native tool declaration: ${declaration}`);
    }
  }
  for (const fact of manifest.facts) {
    if (!source.includes(fact.variable)) {
      throw new Error(`env.jet is missing native environment fact: ${fact.variable}`);
    }
  }
  if (!source.includes("does not execute the flake shellHook")) {
    throw new Error("env.jet must state that shellHook execution is not part of the native projection");
  }
}

for (const environment of ["default", "full"]) {
  const selection = oracle.selections[environment];
  same(
    declaredPackages(environment).sort(),
    expectedPackages(selection).sort(),
    `${environment} package manifest`,
  );
  if (selection.native.join(",") !== "jet,jetpack") {
    throw new Error(`${environment} native tool projection must expose jet and jetpack`);
  }
}

verifyNativeProjectionManifest();

if (source.includes("/nix/store")) {
  throw new Error("env.jet must not hand-pin a Nix store path");
}
if (oracle.unsupported_hook_facts.length !== 0) {
  throw new Error(`unsupported hook facts: ${oracle.unsupported_hook_facts.join(", ")}`);
}

const defaultCount = oracle.selections.default.nix.length;
const fullCount = oracle.selections.full.nix.length;
console.log(`default parity: ${defaultCount} Nix selections + native runtime projection`);
console.log(`full parity: ${fullCount} Nix selections + native runtime projection`);
console.log("unsupported hook facts: 0");
