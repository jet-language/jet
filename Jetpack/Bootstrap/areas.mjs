#!/usr/bin/env node
// Area graph for the Jetpack Jet port. `areas.list` declares each area's direct dependencies; this
// script validates the graph against the `# == Area ==` sections of sources.list and answers:
//   areas.mjs closure <Area>   comma-separated area plus its transitive dependencies (bottom-up order)
//   areas.mjs populated        Jetpack/<Area> folders that list at least one source, bottom-up, one per line
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const bootstrapDir = dirname(fileURLToPath(import.meta.url));
const sourceRoot = resolve(process.env.JETPACK_BOOTSTRAP_SOURCE_ROOT ?? resolve(bootstrapDir, "../.."));
const bootstrap = resolve(sourceRoot, "Jetpack/Bootstrap");

function fail(message) {
  console.error(`jetpack-areas: ${message}`);
  process.exit(1);
}

const sectionHeader = /^#\s*==\s*([^=(]+?)\s*(?:\(.*\))?\s*==\s*$/;
function sections(file) {
  const found = new Map();
  let current = null;
  for (const line of readFileSync(resolve(bootstrap, file), "utf8").split(/\r?\n/)) {
    const header = sectionHeader.exec(line.trim());
    if (header) {
      current = header[1];
      found.set(current, 0);
    } else if (current !== null && line.trim() !== "" && !line.trim().startsWith("#")) {
      found.set(current, found.get(current) + 1);
    }
  }
  return found;
}

const order = [];
const deps = new Map();
for (const [index, raw] of readFileSync(resolve(bootstrap, "areas.list"), "utf8").split(/\r?\n/).entries()) {
  const line = raw.trim();
  if (line === "" || line.startsWith("#")) continue;
  const match = /^([^:]+):(.*)$/.exec(line);
  if (!match) fail(`areas.list:${index + 1}: expected \`Area: Dep, Dep\``);
  const area = match[1].trim();
  if (deps.has(area)) fail(`areas.list:${index + 1}: duplicate area ${area}`);
  const direct = match[2].split(",").map((dep) => dep.trim()).filter(Boolean);
  for (const dep of direct) {
    if (!deps.has(dep)) fail(`areas.list:${index + 1}: ${area} depends on ${dep}, which must be declared earlier (bottom-up, no cycles)`);
  }
  deps.set(area, direct);
  order.push(area);
}

const sourceSections = sections("sources.list");
const testSections = sections("tests.list");
for (const area of sourceSections.keys()) if (!deps.has(area)) fail(`sources.list section ${area} is missing from areas.list`);
for (const area of testSections.keys()) if (!deps.has(area)) fail(`tests.list section ${area} is missing from areas.list`);
for (const area of order) if (!sourceSections.has(area)) fail(`areas.list names ${area}, which has no sources.list section`);

function closure(area) {
  if (!deps.has(area)) fail(`unknown area ${area}`);
  const seen = new Set();
  const visit = (name) => {
    if (seen.has(name)) return;
    seen.add(name);
    for (const dep of deps.get(name)) visit(dep);
  };
  visit(area);
  return order.filter((name) => seen.has(name));
}

const [command, argument] = process.argv.slice(2);
if (command === "closure") {
  console.log(closure(argument ?? fail("closure needs an area")).join(","));
} else if (command === "populated") {
  // Only real Jetpack/<Area> folders are checkable units; reused Compiler sections are dependencies.
  const populated = order.filter((area) => (sourceSections.get(area) ?? 0) > 0 && existsSync(resolve(sourceRoot, "Jetpack", area)));
  if (populated.length > 0) console.log(populated.join("\n"));
} else {
  fail("usage: areas.mjs closure <Area> | populated");
}
