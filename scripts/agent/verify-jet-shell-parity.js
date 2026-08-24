"use strict";

const fs = require("node:fs");
const path = require("node:path");

const root = path.resolve(__dirname, "../..");
const source = fs.readFileSync(path.join(root, "env.jet"), "utf8");
const oracle = JSON.parse(
  fs.readFileSync(path.join(root, "tests/fixtures/nix-compat/env-shell-oracle.json"), "utf8"),
);

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
