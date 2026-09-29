#!/usr/bin/env node
// Core battery activation cost (#3245, CORE-F018).
//
// For each selected battery, three programs are built with one frozen compiler:
//   control: no import;  import: `use` only;  use: one call.
// Each arm reports, separately:
//   compile: `jet check` wall and `jet build --release` wall (unavoidable compile cost);
//   linked:  binary bytes, `size -A` sections, defined-symbol count (`nm --defined-only`);
//   reach:   Core modules present in the generated Rust (reachable-dependency proxy);
//   startup: process wall median over --samples runs of the AOT binary;
//   alloc:   allocation count and peak live bytes of one AOT run (LD_PRELOAD counter);
//   jit:     `jet run` wall (one sample; compile included).
// Deltas are import-control and use-control. A zero claim needs a measured zero in
// sections, startup and allocations; dead-code elimination in source is not evidence.
//
//   node Tools/perf/core-activation.mjs --jet <jet or wrapper> --out <file.json> [--samples 20]

import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

const BATTERIES = {
  json: { use: "use core.encoding.json as m", call: "m.to_string(DataTree.Int(42))" },
  regex: { use: "use core.regex as m", call: "m.escape(\"4.2\")" },
  time: { use: "use core.time as m", call: "m.datetime(2024, 3, 1, 12, 34, 56)" },
  data: { use: "use core.data as m", call: "m.sum([40.0, 2.0])" },
  http: { use: "use core.http as m", call: "m.status_ok()" },
};

function parseArgs(argv) {
  const args = {
    jet: join(homedir(), ".cache/jet-luna/safe-jet.sh"),
    out: join(homedir(), ".cache/jet-test-scratch/core-activation.json"),
    work: join(homedir(), ".cache/jet-luna/core-activation"),
    samples: 20,
    batteries: Object.keys(BATTERIES),
  };
  for (let i = 2; i < argv.length; i += 2) {
    const key = argv[i].replace(/^--/, "");
    const value = argv[i + 1];
    if (!(key in args) || value === undefined) throw new Error(`unknown or incomplete option ${argv[i]}`);
    args[key] = key === "samples" ? Number(value) : key === "batteries" ? value.split(",") : value;
  }
  if (!Number.isInteger(args.samples) || args.samples < 1) throw new Error("--samples must be a positive integer");
  for (const b of args.batteries) if (!(b in BATTERIES)) throw new Error(`unknown battery ${b}`);
  return args;
}

function timed(cmd, argv, opts = {}) {
  const started = process.hrtime.bigint();
  const r = spawnSync(cmd, argv, { encoding: "utf8", maxBuffer: 1 << 28, ...opts });
  const wall_ns = Number(process.hrtime.bigint() - started);
  return { status: r.status, stdout: r.stdout ?? "", stderr: r.stderr ?? "", wall_ns };
}

function median(values) {
  const s = [...values].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m] : Math.round((s[m - 1] + s[m]) / 2);
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function program(battery, arm) {
  const b = BATTERIES[battery];
  if (arm === "control") return `fn run() {\n    print("42")\n}\n`;
  if (arm === "import") return `${b.use}\n\nfn run() {\n    print("42")\n}\n`;
  return `${b.use}\n\nfn run() {\n    print(${b.call})\n}\n`;
}

function sections(binary) {
  const r = spawnSync("size", ["-A", binary], { encoding: "utf8" });
  if (r.status !== 0) return "unavailable";
  const out = {};
  for (const line of r.stdout.split("\n")) {
    const m = line.match(/^(\.\S+)\s+(\d+)\s+\d+/);
    if (m) out[m[1]] = Number(m[2]);
  }
  return out;
}

function symbolCount(binary) {
  const r = spawnSync("nm", ["--defined-only", binary], { encoding: "utf8", maxBuffer: 1 << 28 });
  return r.status === 0 ? r.stdout.split("\n").filter(Boolean).length : "unavailable";
}

function reachableCoreModules(rustPath) {
  if (!existsSync(rustPath)) return "unavailable";
  const text = readFileSync(rustPath, "utf8");
  const found = new Set();
  for (const m of text.matchAll(/corelib_x3e_sCore_s([a-z0-9_]+?)_s/g)) found.add(m[1]);
  return { generated_rust_bytes: statSync(rustPath).size, modules: [...found].sort() };
}

function buildAllocCounter(work) {
  const lib = join(work, "liballoccount.so");
  const r = spawnSync("cc", ["-O2", "-shared", "-fPIC", "-o", lib, join(ROOT, "Tools/perf/alloccount.c")], { encoding: "utf8" });
  return r.status === 0 ? lib : "";
}

function allocations(binary, lib, dir) {
  if (!lib || !existsSync(lib)) return { status: "unavailable", reason: "allocation counter library could not be built" };
  const outFile = join(dir, "alloc.count");
  rmSync(outFile, { force: true });
  const r = spawnSync(binary, [], { encoding: "utf8", env: { ...process.env, LD_PRELOAD: lib, ALLOCCOUNT_OUT: outFile } });
  if (r.status !== 0 || !existsSync(outFile)) return { status: "unavailable", reason: `exit ${r.status}` };
  const fields = Object.fromEntries(readFileSync(outFile, "utf8").trim().split(" ").map((kv) => kv.split("=")).map(([k, v]) => [k, Number(v)]));
  if (fields.allocations === 0) return { status: "unavailable", reason: "binary does not use the preloadable libc allocator" };
  return { status: "measured", ...fields };
}

function measureArm(args, battery, arm) {
  const dir = join(args.work, battery, arm);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "package.jet"), 'name: "core_activation"\nversion: "0.1.0"\nauthority: {\n    holds: {\n        allow: [IO, Mem.Alloc, Time, Net, FS, Env]\n    }\n}\n');
  const stem = `${battery}_${arm}`;
  const source = join(dir, `${stem}.jet`);
  writeFileSync(source, program(battery, arm));
  const row = { battery, arm, source: program(battery, arm) };
  const check = timed(args.jet, ["check", `${stem}.jet`], { cwd: dir });
  row.check = { exit: check.status, wall_ns: check.wall_ns };
  const binary = join(dir, ".jet/build", stem);
  rmSync(binary, { force: true });
  const build = timed(args.jet, ["build", "--release", `${stem}.jet`], { cwd: dir });
  row.build = { exit: build.status, wall_ns: build.wall_ns };
  if (build.status !== 0 || !existsSync(binary)) {
    row.build.stderr_tail = (build.stderr + build.stdout).slice(-1200);
    row.status = "unavailable";
    row.reason = "AOT build failed";
    return row;
  }
  row.binary = { bytes: statSync(binary).size, sha256: sha256(binary), sections: sections(binary), defined_symbols: symbolCount(binary) };
  row.reach = reachableCoreModules(join(dir, ".jet/build", `${stem}.rs`));
  const walls = [];
  let stdout = null;
  for (let i = 0; i < args.samples; i += 1) {
    const r = timed(binary, []);
    if (r.status !== 0) {
      row.startup = { status: "unavailable", reason: `exit ${r.status}`, stderr: r.stderr.slice(-400) };
      break;
    }
    stdout ??= r.stdout;
    walls.push(r.wall_ns);
  }
  if (walls.length === args.samples) row.startup = { status: "measured", samples: walls, median_ns: median(walls), stdout };
  row.alloc = allocations(binary, args.alloccount, dir);
  const jit = timed(args.jet, ["run", `${stem}.jet`], { cwd: dir });
  row.jit = { exit: jit.status, wall_ns: jit.wall_ns, stdout: jit.stdout.slice(0, 200) };
  row.status = "measured";
  return row;
}

function delta(a, b, pick) {
  const x = pick(a);
  const y = pick(b);
  return typeof x === "number" && typeof y === "number" ? x - y : "unavailable";
}

function main() {
  const args = parseArgs(process.argv);
  mkdirSync(args.work, { recursive: true });
  args.alloccount = buildAllocCounter(args.work);
  const jetVersion = timed(args.jet, ["--version"]);
  const report = {
    card: 3245,
    finding: "CORE-F018",
    commit: execFileSync("git", ["rev-parse", "HEAD"], { cwd: ROOT, encoding: "utf8" }).trim(),
    dirty_tree: execFileSync("git", ["status", "--porcelain"], { cwd: ROOT, encoding: "utf8" }).trim().length > 0,
    jet: args.jet,
    jet_version: jetVersion.stdout.trim() || jetVersion.stderr.trim(),
    host: `${process.platform}-${process.arch}`,
    samples: args.samples,
    targets: { host_aot: "measured", jet_run: "one wall sample incl. compile", other_targets: "unavailable: no cross toolchain run" },
    rows: [],
    deltas: [],
  };
  for (const battery of args.batteries) {
    const arms = {};
    for (const arm of ["control", "import", "use"]) {
      const row = measureArm(args, battery, arm);
      arms[arm] = row;
      report.rows.push(row);
      process.stderr.write(`${battery}/${arm}: ${row.status}\n`);
    }
    for (const arm of ["import", "use"]) {
      const a = arms[arm];
      const c = arms.control;
      report.deltas.push({
        battery,
        arm: `${arm}-control`,
        check_wall_ns: delta(a, c, (r) => r.check?.wall_ns),
        build_wall_ns: delta(a, c, (r) => r.build?.wall_ns),
        binary_bytes: delta(a, c, (r) => r.binary?.bytes),
        text_bytes: delta(a, c, (r) => r.binary?.sections?.[".text"]),
        rodata_bytes: delta(a, c, (r) => r.binary?.sections?.[".rodata"]),
        defined_symbols: delta(a, c, (r) => r.binary?.defined_symbols),
        generated_rust_bytes: delta(a, c, (r) => r.reach?.generated_rust_bytes),
        startup_median_ns: delta(a, c, (r) => r.startup?.median_ns),
        allocations: delta(a, c, (r) => r.alloc?.allocations),
        peak_live_bytes: delta(a, c, (r) => r.alloc?.peak_live_bytes),
        jit_wall_ns: delta(a, c, (r) => r.jit?.wall_ns),
      });
    }
  }
  writeFileSync(args.out, JSON.stringify(report, null, 2));
  process.stdout.write(`${args.out}\n`);
}

main();
