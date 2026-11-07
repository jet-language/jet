#!/usr/bin/env node
// chain-lib.mjs : the hashing, change classification and store helpers of chain.sh and publish-base.sh.
//   diff TREE BASE REPO        -> JSON {base, changed:[{path,status}], digest}: TREE against git commit BASE of REPO
//   plan TREE DIFF.json        -> JSON {level, reasons:[...], members:[...]}: the --auto level and the check members
//   inputs TREE                -> "BUILD_HASH PACK_HASH": the content hashes of what a build stage / pack stage reads
//   dirhash DIR                -> content hash of a directory (runtime packs): every file's relative path and bytes
//   evict STORE CAP_GB         -> remove least recently used store entries until the store is under CAP_GB
// Every hash is SHA-256 over a domain string, then per file in byte order: u64 path length, path, u64 length, bytes.
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, lstatSync, readFileSync, readdirSync, readlinkSync, rmSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { toolJet } from "../roots.mjs";

const [cmd, ...args] = process.argv.slice(2);
const SKIPPED = new Set(["target", "build", "node_modules", "bin"]);
const byBytes = (a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b));
const u64 = (n) => { const b = Buffer.alloc(8); b.writeBigUInt64BE(BigInt(n)); return b; };

// Every regular file under `dir` (repo-relative to `root`), skipping dot names and build-output names; symlinks
// count by their target text.
function walk(root, dir, out) {
  const abs = join(root, dir);
  if (!existsSync(abs)) return out;
  const st = lstatSync(abs);
  if (!st.isDirectory()) { out.add(dir); return out; }
  for (const name of readdirSync(abs)) {
    if (name.startsWith(".") || SKIPPED.has(name)) continue;
    const rel = `${dir}/${name}`;
    const s = lstatSync(join(root, rel));
    if (s.isDirectory()) walk(root, rel, out); else out.add(rel);
  }
  return out;
}

function contentHash(root, files, domain) {
  const h = createHash("sha256").update(domain + "\0");
  for (const path of [...files].sort(byBytes)) {
    const abs = join(root, path);
    const s = lstatSync(abs);
    const bytes = s.isSymbolicLink() ? Buffer.from("symlink:" + readlinkSync(abs)) : readFileSync(abs);
    h.update(u64(Buffer.byteLength(path))).update(path).update(u64(bytes.length)).update(bytes);
  }
  return h.digest("hex");
}

// Tools/agent/CompilerPackages.jet of `tree`, run by roots.mjs toolJet: its output rows.
function compilerPackages(tree, ...args) {
  return execFileSync(toolJet(), [join(tree, "Tools/agent/CompilerPackages.jet"), "--root", tree, ...args], { maxBuffer: 1 << 28 }).toString("utf8").split("\n").filter(Boolean);
}

// The build stage reads the compiler's source identity inputs (workspace files, every package `jet build Compiler`
// compiles, Core) plus the Prelude and runtime trees the toolchain root serves and the dev-shell definition;
// the pack stage reads those plus the packaging recipe (the Tools/packaging modules prebuild-runtime.mjs loads:
// every non-test .mjs there except prebuild-runtime-reuse.mjs, which the lander's scratch census still writes
// into the r14 snapshot for its linger workaround and the chain never runs) and every runtime source sources.list names.
function stageInputs(tree) {
  const build = new Set(compilerPackages(tree, "inputs"));
  for (const extra of ["Compiler/Prelude", "Compiler/JetRuntime", "env.jet", "flake.nix", "flake.lock"]) walk(tree, extra, build);
  const pack = new Set(build);
  for (const f of walk(tree, "Tools/packaging", new Set())) if (f.endsWith(".mjs") && !f.endsWith(".test.mjs") && !f.endsWith("/prebuild-runtime-reuse.mjs")) pack.add(f);
  const list = join(tree, "Compiler/JetRuntime/sources.list");
  if (existsSync(list)) for (const row of readFileSync(list, "utf8").split("\n").map((r) => r.trim())) if (row && !row.startsWith("#") && existsSync(join(tree, row))) pack.add(row);
  return { build, pack };
}

function gitBlob(bytes) { return createHash("sha1").update(`blob ${bytes.length}\0`).update(bytes).digest("hex"); }

function diff(tree, base, repo) {
  const raw = execFileSync("git", ["-C", repo, "ls-tree", "-r", "-z", "--full-tree", base], { maxBuffer: 1 << 28 }).toString("utf8");
  const baseFiles = new Map();
  const prefixes = new Set();
  for (const row of raw.split("\0")) {
    if (!row) continue;
    const tab = row.indexOf("\t");
    const [mode, type, sha] = row.slice(0, tab).split(" ");
    if (type !== "blob") continue;
    const path = row.slice(tab + 1);
    baseFiles.set(path, { mode, sha });
    const parts = path.split("/");
    for (let i = 1; i < parts.length; i++) prefixes.add(parts.slice(0, i).join("/"));
  }
  // Walk the tree; directories the base does not have are skipped when they are dot or build-output names
  // (chain outputs, .jet caches, keep-going reports), so only source changes count.
  const seen = new Set();
  const changed = [];
  const visit = (rel) => {
    const abs = rel ? join(tree, rel) : tree;
    for (const name of readdirSync(abs)) {
      const child = rel ? `${rel}/${name}` : name;
      const s = lstatSync(join(tree, child));
      if (s.isDirectory()) {
        if (!prefixes.has(child) && (name.startsWith(".") || SKIPPED.has(name))) continue;
        visit(child);
        continue;
      }
      const b = baseFiles.get(child);
      // An untracked dot file (.chain-base, a lock) is chain bookkeeping, not a source change.
      if (!b && name.startsWith(".")) continue;
      seen.add(child);
      const bytes = s.isSymbolicLink() ? Buffer.from(readlinkSync(join(tree, child))) : readFileSync(join(tree, child));
      const sha = gitBlob(bytes);
      if (!b) changed.push({ path: child, status: "A", sha });
      else if (b.sha !== sha) changed.push({ path: child, status: "M", sha });
    }
  };
  visit("");
  for (const path of baseFiles.keys()) if (!seen.has(path)) changed.push({ path, status: "D", sha: "" });
  changed.sort((a, b) => byBytes(a.path, b.path));
  const digest = createHash("sha256").update(`chain-diff-v1\0${base}\0`);
  for (const c of changed) digest.update(`${c.status} ${c.path} ${c.sha}\n`);
  return { base, changed, digest: digest.digest("hex") };
}

// --auto classes, judged per changed path; the chain level is the highest class any path needs.
//   full : the code generator and what it links or evaluates: JetIR, JetBackend, JetEval (compile-time evaluation),
//          JetRuntime, Prelude, Bootstrap, the Compiler workspace files, the runtime pack recipe (Tools/packaging),
//          the dev shell (flake.nix/flake.lock), and any compiler source whose file name names comptime, lowering,
//          linking, native objects, emission, the scheduler or threads.
//   s1   : the rest of the compiler front end `jet build Compiler` compiles (JetFoundation, JetLexer, JetParser,
//          JetSema, JetBuildSession, JetDriver, JetCli and the Jetpack packages it reaches) and root env.jet.
//   check: Tests/tests, Examples, Tools, Docs, Core library sources, packages the compiler does not build
//          (JetLSP, JetRepl, ...), and anything no chain stage reads.
const FULL_DIRS = ["Compiler/JetIR/", "Compiler/JetBackend/", "Compiler/JetEval/", "Compiler/JetRuntime/", "Compiler/Prelude/", "Compiler/Bootstrap/", "Tools/packaging/"];
const FULL_FILES = new Set(["Compiler/package.jet", "Compiler/build.jet", "flake.nix", "flake.lock"]);
const FULL_NAME = /comptime|lower|link|native|emit|codegen|object|sched|thread|parallel/i;
const RANK = { check: 0, s1: 1, full: 2 };

function classify(path, inputs) {
  const parts = path.split("/");
  if (path === "Compiler/JetRuntime/Tests/check-runtime.mjs") return ["full", "runtime pack assembler (prebuild-runtime.mjs runs it)"];
  if (parts.some((p) => p === "Tests" || p === "tests")) return ["check", "tests"];
  if (FULL_FILES.has(path)) return ["full", path.startsWith("flake") ? "dev shell definition" : "Compiler workspace file"];
  const fd = FULL_DIRS.find((d) => path.startsWith(d));
  if (fd) return ["full", `${fd.slice(0, -1)} (code generation, runtime or runtime pack)`];
  if (parts[0] === "Core") return ["check", "Core library source"];
  if (["Examples", "Docs", "Tools"].includes(parts[0])) return ["check", parts[0]];
  if (inputs.build.has(path) || inputs.isInput(path)) {
    if (FULL_NAME.test(parts[parts.length - 1])) return ["full", "compiler source named for comptime/lowering/linking/emission/scheduler/threads"];
    return ["s1", `compiler front end (${parts.slice(0, 2).join("/")})`];
  }
  if (path === "env.jet") return ["s1", "toolchain env.jet"];
  return ["check", "not read by any chain stage"];
}

// The package a changed file belongs to: its nearest directory with a package.jet (tree first, then any ancestor).
function ownerPackage(tree, path) {
  const parts = path.split("/");
  for (let i = parts.length - 1; i >= 1; i--) {
    const dir = parts.slice(0, i).join("/");
    if (existsSync(join(tree, dir, "package.jet"))) return dir;
  }
  return null;
}

function plan(tree, diffPath) {
  const d = JSON.parse(readFileSync(diffPath, "utf8"));
  const inputs = stageInputs(tree);
  const selected = new Set(compilerPackages(tree, "select", ...d.changed.map((c) => c.path)));
  inputs.isInput = (p) => selected.has(p);
  const compilerDirs = new Set(compilerPackages(tree, "packages").map((row) => row.split("\t")[1]));
  let level = "check";
  const why = new Map();
  const members = new Set();
  for (const c of d.changed) {
    const [cls, reason] = classify(c.path, inputs);
    if (RANK[cls] > RANK[level]) level = cls;
    const k = `${cls}: ${reason}`;
    if (!why.has(k)) why.set(k, []);
    why.get(k).push(c.path);
    if (!c.path.endsWith(".jet")) continue;
    // Members for `check`: a compiler package (or Core, which every compiler package reads) is checked through
    // Compiler/JetCli, whose check covers every package it depends on; other packages are checked themselves.
    let owner = c.path.startsWith("Core/") ? "Compiler/JetCli" : ownerPackage(tree, c.path);
    if (owner && compilerDirs.has(owner)) owner = "Compiler/JetCli";
    if (owner) members.add(owner);
  }
  const reasons = [...why].sort((a, b) => RANK[b[0].split(":")[0]] - RANK[a[0].split(":")[0]])
    .map(([k, paths]) => `${k} (${paths.length} file${paths.length === 1 ? "" : "s"}: ${paths.slice(0, 4).join(" ")}${paths.length > 4 ? " ..." : ""})`);
  if (!d.changed.length) reasons.push("no changes against the base: check (nothing to check)");
  return { level, reasons, members: [...members].sort() };
}

function dirhash(dir) {
  const files = new Set();
  const visit = (rel) => {
    for (const name of readdirSync(rel ? join(dir, rel) : dir)) {
      const child = rel ? `${rel}/${name}` : name;
      const s = lstatSync(join(dir, child));
      if (s.isDirectory()) visit(child); else files.add(child);
    }
  };
  visit("");
  return contentHash(dir, files, "chain-dirhash-v1");
}

function evict(store, capGb) {
  const cap = Number(capGb) * 1024 ** 3;
  const entries = [];
  for (const name of readdirSync(store)) {
    if (!/^[0-9a-f]{64}$/.test(name)) continue;
    const dir = join(store, name);
    let bytes = 0;
    try { const meta = readFileSync(join(dir, "meta"), "utf8"); bytes = Number(/^bytes=(\d+)$/m.exec(meta)?.[1] ?? 0); } catch { continue; }
    let used = 0;
    try { used = statSync(join(dir, ".used")).mtimeMs; } catch { used = statSync(dir).mtimeMs; }
    entries.push({ name, dir, bytes, used });
  }
  let total = entries.reduce((s, e) => s + e.bytes, 0);
  entries.sort((a, b) => a.used - b.used);
  const gone = [];
  for (const e of entries) {
    if (total <= cap) break;
    rmSync(e.dir, { recursive: true, force: true });
    total -= e.bytes;
    gone.push(e.name.slice(0, 16));
  }
  console.log(`store ${(total / 1024 ** 3).toFixed(2)} GB of ${capGb} GB; evicted ${gone.length}${gone.length ? ": " + gone.join(" ") : ""}`);
}

if (cmd === "diff") process.stdout.write(JSON.stringify(diff(resolve(args[0]), args[1], args[2])) + "\n");
else if (cmd === "plan") process.stdout.write(JSON.stringify(plan(resolve(args[0]), args[1])) + "\n");
else if (cmd === "inputs") {
  const tree = resolve(args[0]);
  const { build, pack } = stageInputs(tree);
  console.log(`${contentHash(tree, build, "chain-build-inputs-v1")} ${contentHash(tree, pack, "chain-pack-inputs-v1")} ${build.size} ${pack.size}`);
} else if (cmd === "dirhash") console.log(dirhash(resolve(args[0])));
else if (cmd === "evict") evict(args[0], args[1]);
else { console.error("usage: chain-lib.mjs diff|plan|inputs|dirhash|evict ..."); process.exit(64); }
