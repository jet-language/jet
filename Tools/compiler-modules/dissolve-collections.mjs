#!/usr/bin/env node
// D-MOD-CYCLE1=A cutover for directories that hold several independent
// programs under one package.jet. A package is one namespace with at most one
// `fn run`, so such a directory is a collection of loose files, not a package.
// This removes the directory's package.jet; each file then checks and runs as
// a loose file. A manifest that carries more than identity and authority
// (settings, build profiles, outputs, dependencies) is reported instead of
// removed, because its facts must move by hand into the one program that uses
// them.
//
//   node Tools/compiler-modules/dissolve-collections.mjs [--apply] <root>...
//
// Without --apply it lists what it would do. Roots are package directories
// relative to the repository.
import { existsSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const args = process.argv.slice(2);
const apply = args.includes("--apply");
const roots = args.filter((arg) => arg !== "--apply");
const SIMPLE_KEYS = new Set(["name", "version", "edition", "authority", "jet", "description", "license", "repository"]);

function members(root) {
  const out = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.name.startsWith(".")) continue;
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        if (!existsSync(join(path, "package.jet"))) walk(path);
      } else if (entry.name.endsWith(".jet") && entry.name !== "package.jet") out.push(path);
    }
  };
  walk(root);
  return out;
}

let blocked = 0;
for (const rootArg of roots) {
  const root = resolve(repo, rootArg);
  const manifestPath = join(root, "package.jet");
  if (!existsSync(manifestPath)) {
    console.log(`skip ${rootArg}: no package.jet`);
    continue;
  }
  const manifest = readFileSync(manifestPath, "utf8");
  const keys = [...manifest.matchAll(/^([a-z_]+):/gm)].map((match) => match[1]);
  const extra = keys.filter((key) => !SIMPLE_KEYS.has(key));
  const runs = members(root).filter((file) => /^(pub )?fn run\(/m.test(readFileSync(file, "utf8")));
  if (extra.length > 0) {
    blocked += 1;
    console.log(`MANUAL ${rootArg}: manifest keys ${extra.join(", ")} must move into the program that uses them`);
    continue;
  }
  console.log(`${apply ? "removed" : "would remove"} ${relative(repo, manifestPath)} (${runs.length} programs)`);
  if (apply) rmSync(manifestPath);
}
if (blocked > 0) process.exitCode = 1;
