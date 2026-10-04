#!/usr/bin/env node
// The self-compile ladder (ladder.sh): real compiler subsets, each one
// aggregate unit assembled exactly the way the self-compile is, and the
// per-phase report over their JET_TRACE_FILE traces.
//
//   ladder.mjs rungs SOURCE_TREE RUNGS_DIR
//     For L1..L4, a private tree holding only the rung's packages (hard links
//     to SOURCE_TREE's files), SOURCE_TREE's own assemble.mjs, the
//     sources.list filtered to those packages in their canonical order, and
//     an anchor file (Compiler/Bootstrap/Host/LadderAnchor.jet, outside the
//     inventory like Host/Entry.jet); that tree's assemble.mjs writes
//     RUNGS_DIR/<rung>/project (package.jet + src/compiler.jet) and
//     compiler.map.json. L5 is SOURCE_TREE assembled unmodified: the whole
//     compiler, byte for byte selfcheck.sh's unit. RUNGS_DIR/rungs.tsv lists
//     each rung's packages, files and unit size.
//
//   ladder.mjs report OUT
//     Reads OUT/rungs.tsv and OUT/<rung>/{result.env,trace.json} and prints
//     the per-rung phase seconds, the RSS at each phase's end, and the
//     growth table: rung-to-rung time and RSS ratios against the unit-size
//     ratio, the fitted exponent k (time ~ bytes^k), a verdict per phase and
//     the time extrapolated to the full compiler (L5's unit).
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  copyFileSync, existsSync, linkSync, mkdirSync, readFileSync, realpathSync, renameSync, rmSync, writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Each rung adds packages in dependency order (Compiler/*/package.jet deps);
// every rung is closed under its packages' dependencies.
const RUNGS = [
  { name: "L1", packages: ["JetFoundation"], anchors: ["foundation"] },
  { name: "L2", packages: ["JetFoundation", "JetLexer", "JetParser"], anchors: ["foundation", "lexer", "parser"] },
  { name: "L3", packages: ["JetFoundation", "JetLexer", "JetParser", "JetSema"], anchors: ["foundation", "lexer", "parser", "sema"] },
  {
    name: "L4",
    packages: ["JetFoundation", "JetLexer", "JetParser", "JetSema", "JetOptimizer", "JetCodegen"],
    anchors: ["foundation", "lexer", "parser", "sema", "optimizer", "codegen"],
  },
  { name: "L5", packages: null, anchors: [] },
];

// The rung's entry anchor, the counterpart of Host/Entry.jet's
// `jet_bootstrap_compile`: library functions calling each package's public
// entry points. A rung, like the self-compile, is a library unit (no `main`):
// the driver lowers every checked user function of a library and prunes only
// unreached Core bodies (jet_codegen_reachable_core_functions), so no rung
// code is dead, and the artifact plan is the self-compile's NativeLibrary.
// The `use` lines name the package APIs; the assembler blanks them, as in
// every compiler file.
const ANCHORS = {
  foundation: `use jet_foundation.[MIRProgram, compiler_threads_single, mir_program_digest]

fn jet_ladder_foundation_digest(program: MIRProgram) -> String {
    mir_program_digest(program)
}
`,
  lexer: `use jet_lexer.[Lexed, lex]

fn jet_ladder_lexer_lex(source: [U8]) -> Lexed {
    lex(source)
}
`,
  parser: `use jet_parser.[parse_source]

fn jet_ladder_parser_items(source: [U8]) -> Int {
    parse_source(source).program.items.len()
}
`,
  sema: `use jet_sema.[SemaSourceSegment, check_program]

fn jet_ladder_sema_functions(source: [U8]) -> Int {
    parsed :: parse_source(source)
    checked :: check_program(parsed.program, "ladder", "ladder.jet", [SemaSourceSegment]{})
    checked.functions.len()
}
`,
  optimizer: `use jet_optimizer.[optimize_mir_program]

fn jet_ladder_optimizer_complete(program: &MIRProgram) -> Bool {
    optimize_mir_program(&program, false, compiler_threads_single()).complete
}
`,
  codegen: `use jet_codegen.[JetRustEmitConfig, JetRustEmitResult, jet_rust_emit_program, lower_functions]

fn jet_ladder_codegen_lower(functions: [TFunc], modules: [SemaGraphModuleResult], source_files: [MIRSourceFile]) -> Int {
    lower_functions(functions, modules, source_files, None).functions.len()
}

fn jet_ladder_codegen_emit(program: MIRProgram, config: JetRustEmitConfig) -> Bool {
    emitted :: jet_rust_emit_program(program, config, None)
    complete :: if emitted == {
        .Complete(_, _) -> true
        .Incomplete(_) -> false
    }
    complete
}
`,
};
const ANCHOR_PATH = "Compiler/Bootstrap/Host/LadderAnchor.jet";

function fail(message) {
  console.error(`ladder: ${message}`);
  process.exit(2);
}

// Run TREE/Compiler/Bootstrap/assemble.mjs with a private HOME (it writes
// under $HOME/.cache/jet-luna/compiler-bootstrap, shared with the stage-zero
// test), then move its project and source map into DIR (lib.sh
// assemble_compiler).
function assemble(tree, dir) {
  const home = join(dir, "assemble-home");
  rmSync(home, { recursive: true, force: true });
  rmSync(join(dir, "project"), { recursive: true, force: true });
  rmSync(join(dir, "compiler.map.json"), { force: true });
  mkdirSync(home, { recursive: true });
  const env = { ...process.env, HOME: home };
  delete env.JET_BOOTSTRAP_SOURCE_ROOT;
  const run = spawnSync(process.execPath, [join(tree, "Compiler/Bootstrap/assemble.mjs")], { cwd: tree, env, encoding: "utf8" });
  writeFileSync(join(dir, "assemble.log"), `${run.stdout ?? ""}${run.stderr ?? ""}`);
  if (run.status !== 0) fail(`assemble failed for ${dir}: ${(run.stderr ?? "").trim().split("\n").slice(-3).join(" | ")}`);
  const scratch = join(home, ".cache/jet-luna/compiler-bootstrap");
  renameSync(join(scratch, "project"), join(dir, "project"));
  renameSync(join(scratch, "compiler.map.json"), join(dir, "compiler.map.json"));
  rmSync(home, { recursive: true, force: true });
}

function place(source, target) {
  mkdirSync(dirname(target), { recursive: true });
  try {
    linkSync(source, target);
  } catch {
    copyFileSync(source, target);
  }
}

function rungs(sourceTree, rungsDir) {
  const src = resolve(sourceTree);
  const assembler = join(src, "Compiler/Bootstrap/assemble.mjs");
  const manifest = join(src, "Compiler/Bootstrap/sources.list");
  if (!existsSync(assembler) || !existsSync(manifest)) fail(`${src} has no Compiler/Bootstrap/assemble.mjs and sources.list`);
  const rootsBlock = readFileSync(assembler, "utf8").match(/const sourceRoots = \[([\s\S]*?)\];/);
  if (!rootsBlock) fail(`cannot find sourceRoots in ${assembler}`);
  const sourceRoots = [...rootsBlock[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
  const listed = readFileSync(manifest, "utf8").split(/\r?\n/).map((line) => line.trim()).filter((line) => line !== "" && !line.startsWith("#"));
  mkdirSync(rungsDir, { recursive: true });
  const rows = ["rung\tpackages\tfiles\tunit_bytes\tunit_lines\tunit_sha256"];
  for (const rung of RUNGS) {
    const dir = join(rungsDir, rung.name);
    mkdirSync(dir, { recursive: true });
    let files;
    if (rung.packages === null) {
      files = listed.length;
      rmSync(join(dir, "tree"), { recursive: true, force: true });
      assemble(src, dir);
    } else {
      const tree = join(dir, "tree");
      rmSync(tree, { recursive: true, force: true });
      const selected = listed.filter((path) => rung.packages.some((pkg) => path.startsWith(`Compiler/${pkg}/`)));
      if (selected.length === 0) fail(`${rung.name}: no sources for ${rung.packages.join(",")}`);
      for (const root of sourceRoots) mkdirSync(join(tree, root), { recursive: true });
      for (const path of selected) place(join(src, path), join(tree, path));
      copyFileSync(assembler, join(tree, "Compiler/Bootstrap/assemble.mjs"));
      const anchor = [
        `// Ladder rung ${rung.name} anchor (stage1/ladder.mjs): the rung's counterpart of Host/Entry.jet.`,
        `// Packages: ${rung.packages.join(", ")}.`,
        ...rung.anchors.map((name) => ANCHORS[name]),
      ].join("\n");
      mkdirSync(dirname(join(tree, ANCHOR_PATH)), { recursive: true });
      writeFileSync(join(tree, ANCHOR_PATH), anchor);
      writeFileSync(join(tree, "Compiler/Bootstrap/sources.list"), `# Ladder rung ${rung.name}: ${rung.packages.join(", ")}\n${[...selected, ANCHOR_PATH].join("\n")}\n`);
      files = selected.length + 1;
      assemble(tree, dir);
    }
    const unit = readFileSync(join(dir, "project/src/compiler.jet"));
    const lines = unit.toString("utf8").split("\n").length - 1;
    const sha = createHash("sha256").update(unit).digest("hex");
    rows.push([rung.name, rung.packages ? rung.packages.join(",") : "all", files, unit.length, lines, sha].join("\t"));
    console.log(`${rung.name}: ${files} files, ${unit.length} bytes -> ${join(dir, "project")}`);
  }
  writeFileSync(join(rungsDir, "rungs.tsv"), `${rows.join("\n")}\n`);
}

// ---------------------------------------------------------------- report

function readEnv(path) {
  const values = {};
  if (!existsSync(path)) return null;
  for (const line of readFileSync(path, "utf8").split("\n")) {
    const at = line.indexOf("=");
    if (at > 0) values[line.slice(0, at)] = line.slice(at + 1);
  }
  return values;
}

// The streaming trace (crates/jet-driver/src/Trace.rs): one event per line,
// the array possibly unterminated (a killed run). Spans nest per thread;
// `cat` is the span kind. A span still open at the end ran until the last
// event and is marked open.
function readTrace(path) {
  if (!existsSync(path)) return { spans: [], lastTs: 0, unitItems: null };
  const stacks = new Map();
  const spans = [];
  let lastTs = 0;
  let unitItems = null;
  for (const raw of readFileSync(path, "utf8").split("\n")) {
    let line = raw.trim();
    if (line === "" || line === "[" || line === "]") continue;
    if (line.endsWith(",")) line = line.slice(0, -1);
    let event;
    try {
      event = JSON.parse(line);
    } catch {
      continue;
    }
    if (typeof event.ts === "number") lastTs = Math.max(lastTs, event.ts);
    if (event.ph !== "B" && event.ph !== "E") continue;
    const stack = stacks.get(event.tid) ?? [];
    stacks.set(event.tid, stack);
    if (event.ph === "B") {
      stack.push({ kind: event.cat, label: event.name, start: event.ts, parents: stack.map((span) => span.kind) });
      continue;
    }
    const at = stack.map((span) => span.kind).lastIndexOf(event.cat);
    if (at < 0) continue;
    const [span] = stack.splice(at, 1);
    span.end = event.ts;
    span.rss = event.args?.rss_kb ?? null;
    span.peak = event.args?.peak_rss_kb ?? null;
    if (span.kind === "parse" && span.label === "src/compiler.jet") unitItems = event.args?.items ?? null;
    spans.push(span);
  }
  for (const stack of stacks.values()) {
    for (const span of stack) spans.push({ ...span, end: lastTs, rss: null, peak: null, open: true });
  }
  return { spans, lastTs, unitItems };
}

// Phase rows: the span path below the `compile` root, at most two kinds deep
// (load/parse, sema.check/sema.module, sema.finalize/sema.bodies, ...), plus
// `<parent>/(rest)` for a parent's time outside its children, `compile`
// itself, and `startup+exit` (process wall outside every top-level span:
// process start, compiler-image restore, receipt).
function phases(trace, wallSeconds) {
  const rows = new Map();
  const row = (name, order) => {
    if (!rows.has(name)) rows.set(name, { name, order, parent: null, secs: 0, rss: null, peak: null, open: false });
    return rows.get(name);
  };
  let topLevel = 0;
  for (const span of trace.spans) {
    const path = [...span.parents.filter((kind) => kind !== "compile"), span.kind];
    if (span.parents.length === 0) topLevel += (span.end - span.start) / 1e6;
    if (span.kind !== "compile" && path.length > 2) continue;
    const name = span.kind === "compile" ? "compile" : path.join("/");
    const entry = row(name, span.start);
    entry.parent = path.length > 1 ? path.slice(0, -1).join("/") : span.parents.includes("compile") ? "compile" : null;
    entry.order = Math.min(entry.order, span.start);
    entry.secs += (span.end - span.start) / 1e6;
    if (span.rss !== null) entry.rss = Math.max(entry.rss ?? 0, span.rss);
    if (span.peak !== null) entry.peak = Math.max(entry.peak ?? 0, span.peak);
    if (span.open) entry.open = true;
  }
  for (const parent of [...rows.values()]) {
    const children = [...rows.values()].filter((child) => child.parent === parent.name);
    if (children.length === 0) continue;
    const rest = row(`${parent.name}/(rest)`, parent.order + 0.5);
    rest.parent = parent.name;
    rest.secs = Math.max(0, parent.secs - children.reduce((sum, child) => sum + child.secs, 0));
    rest.open = parent.open;
  }
  if (wallSeconds !== null) {
    const outside = row("startup+exit", Number.MAX_SAFE_INTEGER - 1);
    outside.secs = Math.max(0, wallSeconds - topLevel);
    const wall = row("wall", Number.MAX_SAFE_INTEGER);
    wall.secs = wallSeconds;
    wall.peak = Math.max(0, ...trace.spans.map((span) => span.peak ?? 0)) || null;
  }
  return rows;
}

const fmtSecs = (secs) => (secs === null || secs === undefined ? "-" : secs >= 100 ? secs.toFixed(0) : secs >= 10 ? secs.toFixed(1) : secs.toFixed(2));
const fmtGb = (kb) => (kb === null || kb === undefined ? "-" : (kb / 1048576).toFixed(2));
const fmtRatio = (value) => (value === null || !Number.isFinite(value) ? "-" : `${value.toFixed(2)}x`);
const pad = (text, width) => String(text).padEnd(width);
const padStart = (text, width) => String(text).padStart(width);

// Least-squares slope of ln(time) on ln(bytes): time ~ bytes^k.
function fitExponent(points) {
  if (points.length < 2) return null;
  const xs = points.map(([bytes]) => Math.log(bytes));
  const ys = points.map(([, secs]) => Math.log(secs));
  const mx = xs.reduce((a, b) => a + b, 0) / xs.length;
  const my = ys.reduce((a, b) => a + b, 0) / ys.length;
  const sxx = xs.reduce((sum, x) => sum + (x - mx) ** 2, 0);
  if (sxx === 0) return null;
  return xs.reduce((sum, x, index) => sum + (x - mx) * (ys[index] - my), 0) / sxx;
}

// Thresholds: below NOISE_SECS in every rung a phase's ratios are noise; an
// exponent above SUPERLINEAR_K is superlinear (a constant per-run cost pulls a
// linear phase's k below 1, so 1.3 leaves room for timing noise on the small
// size ratios of adjacent rungs).
const NOISE_SECS = 0.5;
const FIT_MIN_SECS = 0.05;
const SUPERLINEAR_K = 1.3;

function report(out) {
  const rungsPath = join(out, "rungs.tsv");
  if (!existsSync(rungsPath)) fail(`${rungsPath} is missing`);
  const [, ...rungRows] = readFileSync(rungsPath, "utf8").trim().split("\n");
  const all = rungRows.map((line) => {
    const [name, packages, files, bytes, lines] = line.split("\t");
    return { name, packages, files: Number(files), bytes: Number(bytes), lines: Number(lines) };
  });
  const full = all.find((rung) => rung.packages === "all") ?? all[all.length - 1];
  const ran = [];
  for (const rung of all) {
    const result = readEnv(join(out, rung.name, "result.env"));
    if (!result) continue;
    const wall = result.wall_ms ? Number(result.wall_ms) / 1000 : null;
    const trace = readTrace(join(out, rung.name, "trace.json"));
    const rows = phases(trace, wall);
    const failedPhases = new Set();
    if (result.status !== "ok") {
      const open = [...rows.values()].filter((entry) => entry.open && entry.name !== "compile" && !entry.name.endsWith("/(rest)"));
      if (open.length > 0) {
        for (const entry of open) failedPhases.add(entry.name);
      } else {
        // A compile that returned incomplete stopped in the last compiler
        // phase it began; host spans after it (decode, package) only carried
        // the result out.
        const begun = (entry) => !entry.name.endsWith("/(rest)") && !["wall", "startup+exit", "compile"].includes(entry.name);
        const latest = (entries) => entries.sort((a, b) => b.order - a.order)[0];
        const last = latest([...rows.values()].filter((entry) => begun(entry) && entry.parent === "compile")) ?? latest([...rows.values()].filter((entry) => begun(entry) && entry.parent === null));
        failedPhases.add(last ? last.name : "startup+exit");
      }
    }
    ran.push({ ...rung, result, rows, failedPhases, unitItems: trace.unitItems });
  }
  if (ran.length === 0) fail(`no rung results under ${out}`);

  console.log(`ladder: ${out}`);
  console.log(`full compiler (${full.name}): ${full.files} files, ${(full.bytes / 1e6).toFixed(2)} MB unit`);
  console.log("");
  console.log("rung  status  unit_MB  size_x  items  wall_s  peak_GB  verdict / packages");
  let previous = null;
  for (const rung of ran) {
    const peak = rung.rows.get("wall")?.peak ?? null;
    const status = rung.result.status;
    let detail = status === "ok" ? rung.packages : `${rung.result.verdict ?? ""}; complete=${rung.result.complete ?? "?"} errors=${rung.result.errors ?? "?"}; failed in ${[...rung.failedPhases].join(", ")}`;
    const reportsPath = join(out, rung.name, "reports.tsv");
    if (status !== "ok" && existsSync(reportsPath)) {
      const errors = readFileSync(reportsPath, "utf8").split("\n").filter((line) => line.startsWith("E"));
      const counts = new Map();
      for (const line of errors) counts.set(line.split("\t")[0], (counts.get(line.split("\t")[0]) ?? 0) + 1);
      if (errors.length > 0) detail += `; ${[...counts].sort((a, b) => b[1] - a[1]).slice(0, 3).map(([code, count]) => `${code} x${count}`).join(", ")}, first: ${errors[0].replaceAll("\t", " ").slice(0, 160)}`;
    }
    console.log([
      pad(rung.name, 4), pad(status, 6), padStart((rung.bytes / 1e6).toFixed(2), 7), padStart(previous ? fmtRatio(rung.bytes / previous.bytes) : "-", 6),
      padStart(rung.unitItems ?? "-", 5), padStart(fmtSecs(rung.result.wall_ms ? Number(rung.result.wall_ms) / 1000 : null), 6), padStart(fmtGb(peak), 7), ` ${detail}`,
    ].join("  "));
    previous = rung;
  }

  const names = [];
  const order = new Map();
  for (const rung of ran) {
    for (const entry of rung.rows.values()) {
      if (!order.has(entry.name)) {
        order.set(entry.name, entry.order);
        names.push(entry.name);
      }
    }
  }
  // Order by the first rung's timeline; a nested row follows its parent.
  const top = (name) => name.split("/")[0];
  const topOrder = new Map();
  for (const name of names) {
    const key = top(name);
    topOrder.set(key, Math.min(topOrder.get(key) ?? Infinity, order.get(name)));
  }
  names.sort((a, b) => {
    const byTop = topOrder.get(top(a)) - topOrder.get(top(b));
    if (byTop !== 0) return byTop;
    if (top(a) === a) return -1;
    if (top(b) === b) return 1;
    return order.get(a) - order.get(b);
  });
  const label = (name) => (name.includes("/") ? `  ${name.split("/").slice(1).join("/")}` : name);
  const width = Math.max(14, ...names.map((name) => label(name).length)) + 1;
  const cell = (rung, name, pick) => {
    const entry = rung.rows.get(name);
    if (!entry) return "-";
    const text = pick(entry);
    return rung.failedPhases.has(name) ? `${text}!` : entry.open ? `${text}+` : text;
  };

  // Receipt reports carry no compile stage, so a rung's diagnostics are
  // listed against the phase its compile returned from: the failed phase, or
  // the whole run when the rung passed. E codes first, then by count.
  const codeCounts = ran.map((rung) => {
    const counts = new Map();
    const path = join(out, rung.name, "reports.tsv");
    if (existsSync(path)) {
      for (const line of readFileSync(path, "utf8").split("\n")) {
        if (line === "") continue;
        const code = line.split("\t")[0];
        counts.set(code, (counts.get(code) ?? 0) + 1);
      }
    }
    return counts;
  });
  const codes = [...new Set(codeCounts.flatMap((counts) => [...counts.keys()]))].sort((a, b) => {
    const errorOrder = Number(!b.startsWith("E")) - Number(!a.startsWith("E"));
    if (errorOrder !== 0) return -errorOrder;
    const total = (code) => codeCounts.reduce((sum, counts) => sum + (counts.get(code) ?? 0), 0);
    return total(b) - total(a) || a.localeCompare(b);
  });
  console.log("");
  console.log("diagnostics by code per rung, at the phase each compile returned from");
  console.log(pad("code", 8) + ran.map((rung) => padStart(`${rung.name}@${rung.result.status === "ok" ? "all" : [...rung.failedPhases].join(",")}`, 20)).join(""));
  if (codes.length === 0) console.log("(none)");
  for (const code of codes) console.log(pad(code, 8) + codeCounts.map((counts) => padStart(counts.get(code) ?? "-", 20)).join(""));

  console.log("");
  console.log("seconds per phase (+ = still open when the run ended, ! = the phase the rung failed in)");
  console.log(pad("phase", width) + ran.map((rung) => padStart(rung.name, 9)).join(""));
  for (const name of names) console.log(pad(label(name), width) + ran.map((rung) => padStart(cell(rung, name, (entry) => fmtSecs(entry.secs)), 9)).join(""));

  console.log("");
  console.log("RSS GB at phase end / peak GB so far (VmRSS / VmHWM; wall row: process peak)");
  console.log(pad("phase", width) + ran.map((rung) => padStart(rung.name, 13)).join(""));
  for (const name of names) {
    if (name.endsWith("/(rest)") || name === "startup+exit") continue;
    console.log(pad(label(name), width) + ran.map((rung) => padStart(cell(rung, name, (entry) => (name === "wall" ? fmtGb(entry.peak) : `${fmtGb(entry.rss)}/${fmtGb(entry.peak)}`)), 13)).join(""));
  }

  console.log("");
  const pairs = ran.slice(1).map((rung, index) => [ran[index], rung]);
  console.log(`growth: time x / RSS x per rung step against the unit-size step; k = fitted exponent (time ~ bytes^k over rungs with >= ${FIT_MIN_SECS}s);`);
  console.log(`verdict LINEAR (k <= ${SUPERLINEAR_K}), SUPERLINEAR (k > ${SUPERLINEAR_K}), FAILED; LINEAR* = under ${NOISE_SECS}s in every rung (noise, not scaling).`);
  console.log(`full est = last rung's time x (${(full.bytes / 1e6).toFixed(2)} MB / its size)^k (k unknown or LINEAR*: 1); >= marks a lower bound; total = top-level phases.`);
  const head = pairs.map(([a, b]) => padStart(`${a.name}>${b.name} size ${fmtRatio(b.bytes / a.bytes)}`, 24)).join("");
  console.log(pad("phase", width) + head + padStart("k", 7) + padStart("verdict", 13) + padStart("full est s", 12));
  let estimate = 0;
  let estimateBound = false;
  for (const name of names) {
    if (name === "wall") continue;
    const ratios = pairs.map(([a, b]) => {
      const left = a.rows.get(name);
      const right = b.rows.get(name);
      if (!left || !right || left.secs <= 0) return padStart("-", 24);
      const time = right.secs / left.secs;
      const rss = left.peak && right.peak ? right.peak / left.peak : null;
      return padStart(`t ${fmtRatio(time)} rss ${fmtRatio(rss)}`, 24);
    }).join("");
    const points = ran.filter((rung) => rung.rows.get(name) && !rung.rows.get(name).open && !rung.failedPhases.has(name) && rung.rows.get(name).secs >= FIT_MIN_SECS).map((rung) => [rung.bytes, rung.rows.get(name).secs]);
    const k = fitExponent(points);
    const failed = ran.some((rung) => rung.failedPhases.has(name));
    const noisy = ran.every((rung) => !rung.rows.get(name) || rung.rows.get(name).secs < NOISE_SECS);
    let verdict;
    if (failed) verdict = "FAILED";
    else if (noisy) verdict = "LINEAR*";
    else if (k === null) verdict = "-";
    else verdict = k > SUPERLINEAR_K ? "SUPERLINEAR" : "LINEAR";
    const last = [...ran].reverse().find((rung) => rung.rows.get(name));
    let est = "-";
    if (last) {
      const entry = last.rows.get(name);
      const exponent = noisy || k === null ? 1 : Math.max(0, k);
      const value = last.bytes === full.bytes ? entry.secs : entry.secs * (full.bytes / last.bytes) ** exponent;
      const bound = failed || entry.open;
      est = `${bound ? ">=" : ""}${fmtSecs(value)}`;
      if (name !== "compile" && (entry.parent === null || entry.parent === "compile")) {
        estimate += value;
        estimateBound ||= bound;
      }
    }
    console.log(pad(label(name), width) + ratios + padStart(k === null ? "-" : k.toFixed(2), 7) + padStart(verdict, 13) + padStart(est, 12));
  }
  console.log(pad("total (est)", width) + " ".repeat(pairs.length * 24 + 20) + padStart(`${estimateBound ? ">=" : ""}${fmtSecs(estimate)}`, 12));
}

// The HARD gate consumes the same unrounded phase rows as the human report,
// including hierarchical children, exclusive (rest), and startup+exit.
export { readTrace, phases, fitExponent };
if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, ...args] = process.argv.slice(2);
  if (command === "rungs" && args.length === 2) rungs(args[0], args[1]);
  else if (command === "report" && args.length === 1) report(args[0]);
  else fail("usage: ladder.mjs rungs SOURCE_TREE RUNGS_DIR | report OUT");
}
