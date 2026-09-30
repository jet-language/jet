#!/usr/bin/env node
// D-MOD-CYCLE1=A cutover for the example and tooling trees.
//
// A package is one namespace with at most one `fn run`; a library has none.
// Directories that hold several independent programs under one package.jet
// (a "collection") therefore stop being packages: the manifest goes away and
// every file becomes a loose file. A loose file holds only the beginner
// authority floor, so a file that needs more gets a leading `package { }`
// header with the exact row `jet fix --all` computes for it. Only that header
// is taken from the fix; every other fix is discarded. A real multi-file
// package (one program) keeps its manifest and loses its file imports, since
// its files share one namespace (drop-file-imports.mjs).
//
//   node Tools/compiler-modules/migrate-examples.mjs [--apply] [--repo <root>] [<dir>...]
//
// Without --apply it prints the plan. Roots default to Examples and Tools;
// tests/ fixtures are listed for review, never rewritten, because each one
// pins a specific harness contract. A manifest that carries more than identity
// and authority (settings, build profiles, outputs, dependencies) or denies a
// right is reported as MANUAL: its facts must move into the one program that
// uses them. JET_CMD selects the jet launcher. Computed headers are cached by
// file content under ~/.cache/jet-luna/compiler-modules/headers.json, so a
// second run (or the same cutover on another checkout) costs no jet calls.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const apply = args.includes("--apply");
const repoAt = args.indexOf("--repo");
const repo = resolve(repoAt >= 0 ? args[repoAt + 1] : join(here, "../.."));
const scopes = args.filter((arg, index) => arg !== "--apply" && arg !== "--repo" && index !== repoAt + 1);
const roots = scopes.length > 0 ? scopes : ["Examples", "Tools", "tests"];
const jetCmd = (process.env.JET_CMD ?? `${repo}/Tools/agent/jet-env jet`).split(/\s+/);
const SIMPLE_KEYS = new Set(["name", "version", "edition", "authority", "jet", "description", "license", "repository"]);
const SKIP = new Set(["target", "node_modules", "build", "bin"]);
const cachePath = join(homedir(), ".cache/jet-luna/compiler-modules/headers.json");
const cache = existsSync(cachePath) ? JSON.parse(readFileSync(cachePath, "utf8")) : {};

function walk(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith(".") || SKIP.has(entry.name) || entry.name.startsWith("target-")) continue;
    const path = join(dir, entry.name);
    if (entry.isDirectory()) walk(path, out);
    else out.push(path);
  }
  return out;
}

function members(root) {
  const out = [];
  const visit = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.name.startsWith(".")) continue;
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        if (!existsSync(join(path, "package.jet"))) visit(path);
      } else if (entry.name.endsWith(".jet") && !["package.jet", "workspace.jet", "pkg.jet"].includes(entry.name)) {
        out.push(path);
      }
    }
  };
  visit(root);
  return out.sort();
}

const DECL = /^(?:pub(?:\(package\))? )?(?:fn|struct|enum|trait|tag|distinct|type) ([A-Za-z_]\w*)|^(?:pub )?([A-Z_][A-Z0-9_]*) ::/gm;
function classify(root) {
  const files = members(root);
  const texts = files.map((file) => readFileSync(file, "utf8"));
  const owner = new Map();
  let duplicates = 0;
  texts.forEach((text, index) => {
    for (const match of text.matchAll(DECL)) {
      const name = match[1] ?? match[2];
      if (owner.has(name) && owner.get(name) !== index) duplicates += 1;
      else owner.set(name, index);
    }
  });
  const manifest = readFileSync(join(root, "package.jet"), "utf8");
  return {
    files,
    runs: texts.filter((text) => /^(?:pub )?fn run\(/m.test(text)).length,
    fileImports: files.filter((_, index) => /^\s*(?:pub )?use "(?![^"]*\.h")/m.test(texts[index])),
    duplicates,
    keys: [...manifest.matchAll(/^([a-z_]+):/gm)].map((match) => match[1]),
    denies: /\bdeny\s*:/.test(manifest),
  };
}

const HEADER = /^package \{\n[\s\S]*?\n\}\n\n/m;
function headerFor(file) {
  const original = readFileSync(file, "utf8");
  const key = `${relative(repo, file)}@${createHash("sha256").update(original).digest("hex")}`;
  if (key in cache) return cache[key];
  let header = "";
  try {
    execFileSync(jetCmd[0], [...jetCmd.slice(1), "fix", "--all", file], { cwd: dirname(file), stdio: "ignore", timeout: 600_000 });
    const fixed = readFileSync(file, "utf8");
    const match = fixed.match(HEADER);
    if (match) {
      const prefix = fixed.slice(0, match.index);
      header = original.startsWith(prefix) ? `${match.index}\n${match[0]}` : `?\n${match[0]}`;
    }
  } catch (error) {
    header = `!\n${String(error.message).split("\n")[0]}\n`;
  } finally {
    writeFileSync(file, original);
  }
  cache[key] = header;
  mkdirSync(dirname(cachePath), { recursive: true });
  writeFileSync(cachePath, JSON.stringify(cache, null, 1));
  return header;
}

function insertHeader(file, entry) {
  const [at, ...rest] = entry.split("\n");
  const block = rest.join("\n").replace(/name: "[^"]*"/, `name: "${basename(file, ".jet")}"`);
  const source = readFileSync(file, "utf8");
  const offset = Number(at);
  writeFileSync(file, source.slice(0, offset) + block + source.slice(offset));
}

const all = roots.flatMap((root) => walk(resolve(repo, root)));
const packageRoots = all.filter((file) => basename(file) === "package.jet").map(dirname).sort();
const report = { dissolved: [], headers: [], manual: [], review: [], unresolved: [], importDrops: [] };
for (const root of packageRoots) {
  const shown = relative(repo, root);
  const facts = classify(root);
  const collection = facts.runs > 1;
  if (shown.startsWith("tests/")) {
    if (collection || facts.duplicates > 0 || facts.fileImports.length > 0) report.review.push(shown);
    continue;
  }
  if (!collection) {
    if (facts.duplicates > 0) report.manual.push(`${shown} [one program or library declares a name twice]`);
    else if (facts.fileImports.length > 0 && facts.keys.includes("outputs")) {
      report.manual.push(`${shown} [an \`outputs\` entry follows a file import]`);
    } else if (facts.fileImports.length > 0) {
      report.importDrops.push(...facts.fileImports.map((file) => relative(repo, file)));
    }
    continue;
  }
  const extra = facts.keys.filter((key) => !SIMPLE_KEYS.has(key));
  if (extra.length > 0 || facts.denies) {
    report.manual.push(`${shown} [${[...extra, ...(facts.denies ? ["deny"] : [])].join(", ")}]`);
    continue;
  }
  report.dissolved.push(`${shown} (${facts.files.length} files, ${facts.runs} programs)`);
  if (!apply) continue;
  const grants = facts.keys.includes("authority");
  rmSync(join(root, "package.jet"));
  for (const file of facts.files) {
    if (!grants) continue;
    const entry = headerFor(file);
    if (entry === "") continue;
    if (/^[0-9]+\n/.test(entry)) {
      insertHeader(file, entry);
      report.headers.push(relative(repo, file));
    } else {
      report.unresolved.push(`${relative(repo, file)}: ${entry.split("\n")[1] ?? entry}`);
    }
  }
}
if (apply && report.importDrops.length > 0) {
  execFileSync(process.execPath, [join(here, "drop-file-imports.mjs"), "--apply", ...report.importDrops.map((file) => join(repo, file))], {
    stdio: "inherit",
    env: { ...process.env, JET_CMD: jetCmd.join(" ") },
  });
}
for (const [section, rows] of Object.entries(report)) {
  console.log(`== ${section} (${rows.length})`);
  for (const row of rows) console.log(`  ${row}`);
}
