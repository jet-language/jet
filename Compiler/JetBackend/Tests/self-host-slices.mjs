#!/usr/bin/env node
// Plan all compiler package slices from a single compiler MIR conversion.
// Smallest root inventory first; actual reachable work may be larger.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join, resolve } from "node:path";
const input = process.argv[2];
if (!input) throw new Error("usage: self-host-slices.mjs <probe directory>");
const root = resolve(input);
const lines = (name) => readFileSync(join(root, name), "utf8").split("\n").filter(Boolean);
const segments = JSON.parse(readFileSync(join(root, "compiler.map.json"), "utf8")).source_segments.slice().sort((a, b) => a.generated.start - b.generated.start);
const sources = new Map(lines("self-all.sources.tsv").map((row) => row.split("\t")));
const packages = new Map();
for (const row of lines("self-all.functions.tsv")) {
  const [id, , , file, offset] = row.split("\t");
  if (!(sources.get(file) ?? "").endsWith("selfhost.jet")) continue;
  const start = Number(offset);
  let lo = 0, hi = segments.length - 1, found;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const segment = segments[mid];
    if (start < segment.generated.start) hi = mid - 1;
    else if (start >= segment.generated.end) lo = mid + 1;
    else { found = segment; break; }
  }
  // Every package of the assembled unit is a slice: Compiler/<Pkg> and Jetpack/<Pkg> (as Jetpack-<Pkg>).
  if (!found || !/^(Compiler|Jetpack)\//.test(found.source_file)) continue;
  const [top, name] = found.source_file.split("/");
  const pkg = top === "Compiler" ? name : `${top}-${name}`;
  if (!packages.has(pkg)) packages.set(pkg, []);
  packages.get(pkg).push(id);
}
if (packages.size === 0) throw new Error("no compiler package function roots; check assembled span/source map");
const order = [...packages].sort((a, b) => a[1].length - b[1].length || a[0].localeCompare(b[0]));
for (const [pkg, ids] of order) {
  const slice = join(root, "slices", pkg);
  mkdirSync(join(slice, "run"), { recursive: true });
  writeFileSync(join(slice, "roots.txt"), ids.join("\n") + "\n");
}
writeFileSync(join(root, "slice-order.txt"), order.map(([pkg]) => pkg).join("\n") + "\n");
writeFileSync(join(root, "SLICE-ORDER.tsv"), "package\troot_functions\n" + order.map(([pkg, ids]) => `${pkg}\t${ids.length}`).join("\n") + "\n");
console.log(`SELFHOST SLICE PLAN packages=${order.length}; smallest-first=${order.map(([pkg, ids]) => `${pkg}:${ids.length}`).join(",")}`);
