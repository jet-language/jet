#!/usr/bin/env node
// Focused native feature-example runner. Keep warnings separate from failures.
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
} from "node:fs";
import { dirname, extname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const FEATURES = join(ROOT, "Examples/features");
const EXPECTED = join(FEATURES, "expected");
const JET = join(ROOT, "target/debug/jet");
const SCRATCH_BASE = process.env.JET_TEST_SCRATCH_DIR
  ?? (process.env.HOME ? join(process.env.HOME, ".cache/jet-test-scratch") : null);
const EXPECTED_FAIL_EXITS = new Set([1, 70]);

export function usage() {
  return [
    "Usage: run-feature-examples.mjs [filter ...] [options]",
    "  filter             keep examples whose path contains this substring",
    "  --jobs N           concurrent jet run processes (default 4)",
    "  --timeout SEC      per-example timeout (default 90)",
    "  --list             print discovered examples and exit",
    "  --json             machine-readable summary",
    "  --no-env           do not wrap execution in jet-env",
  ].join("\n");
}

export function parseArgs(argv) {
  const options = { jobs: 4, timeoutSec: 90, list: false, json: false, noEnv: false, filters: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--jobs") options.jobs = positive(argv[++i], "--jobs");
    else if (arg === "--timeout") options.timeoutSec = positive(argv[++i], "--timeout");
    else if (arg === "--list") options.list = true;
    else if (arg === "--json") options.json = true;
    else if (arg === "--no-env") options.noEnv = true;
    else if (arg === "--help" || arg === "-h") options.help = true;
    else options.filters.push(arg);
  }
  return options;
}

function positive(value, flag) {
  const n = Number(value);
  if (!Number.isInteger(n) || n <= 0) throw new Error(`${flag} expects a positive integer`);
  return n;
}

function relativeFeature(path) {
  return relative(FEATURES, path).replaceAll("\\", "/");
}

function entryFor(path) {
  const rel = relativeFeature(path);
  const shown = `Examples/features/${rel}`;
  const stem = rel.endsWith("/run.jet")
    ? rel.slice(0, -"/run.jet".length)
    : rel.slice(0, -extname(rel).length);
  return { path, shown, stem };
}

function collectProject(dir, entries) {
  const run = join(dir, "run.jet");
  if (existsSync(run)) {
    entries.push(entryFor(run));
    return;
  }
  for (const child of readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    if (child.name === "expected" || child.name.startsWith(".")) continue;
    const childPath = join(dir, child.name);
    if (child.isDirectory()) collectProject(childPath, entries);
    else if (child.isFile() && child.name.endsWith(".jet")) entries.push(entryFor(childPath));
  }
}

export function collectFeatureExamples(featuresDir = FEATURES) {
  const entries = [];
  collectProject(featuresDir, entries);
  return entries.sort((a, b) => a.stem.localeCompare(b.stem));
}

function readExpected(stem) {
  const outPath = join(EXPECTED, `${stem}.out`);
  const errPath = join(EXPECTED, `${stem}.err.out`);
  const stderrPath = join(EXPECTED, `${stem}.stderr.out`);
  return {
    out: existsSync(outPath) ? readFileSync(outPath, "utf8") : null,
    err: existsSync(errPath) ? readFileSync(errPath, "utf8") : null,
    stderrOut: existsSync(stderrPath) ? readFileSync(stderrPath, "utf8") : null,
  };
}

export function nativeRunIsWebTarget(entry) {
  if (!existsSync(entry.path)) return false;
  const source = readFileSync(entry.path, "utf8");
  return /^\s*#(?:\[)?Target\((?:Web|JS|Wasm|Browser)\)/mu.test(source);
}

function sourceHasTarget(entry, target) {
  if (!existsSync(entry.path)) return false;
  return new RegExp(`^\\s*#(?:\\[)?Target\\(${target}\\)`, "mu").test(readFileSync(entry.path, "utf8"));
}

export function skipReason(entry, expected) {
  if (nativeRunIsWebTarget(entry)) return "web-targeted native run";
  if (expected.out === null && expected.err === null) return "no native golden";
  if (entry.stem.includes("polyglot_") && expected.out === null && expected.err === null) return "needs foreign binder";
  if (sourceHasTarget(entry, "Wasm") || sourceHasTarget(entry, "Browser")) return "web-targeted native run";
  return null;
}

export function withoutWarningDiagnostics(stderr) {
  const output = [];
  let skippingWarning = false;
  for (const line of stderr.replaceAll("\r\n", "\n").split("\n")) {
    if (line.startsWith("Warning [L")) {
      skippingWarning = true;
      continue;
    }
    if (skippingWarning) {
      if (line.startsWith("Error [") || line.startsWith("Stop") || line.startsWith("internal compiler error:")) {
        skippingWarning = false;
      } else {
        continue;
      }
    }
    output.push(line);
  }
  return output.join("\n");
}

export function warningOnlyStderr(stderr) {
  return stderr.split("\n").some((line) => line.startsWith("Warning [L"))
    && withoutWarningDiagnostics(stderr).trim() === "";
}

function normalize(text) {
  return text.replaceAll("\r\n", "\n").replace(/\n+$/u, "");
}

function gtkLoaderUnavailable(stderr) {
  return /gtk|libgtk|cannot open shared object|failed to load/u.test(stderr)
    && /not found|unavailable|could not load|cannot open/u.test(stderr);
}

function copyFixtureState(source, destination) {
  mkdirSync(destination, { recursive: true });
  for (const item of readdirSync(source, { withFileTypes: true })) {
    const from = join(source, item.name);
    const to = join(destination, item.name);
    if (item.isDirectory()) {
      copyFixtureState(from, to);
    } else if (item.isFile()) {
      copyFileSync(from, to);
    }
  }
}

export function copyFeatureProject(source, destination) {
  mkdirSync(destination, { recursive: true });
  for (const item of readdirSync(source, { withFileTypes: true })) {
    if (item.isDirectory() && item.name === ".jet") continue;
    const from = join(source, item.name);
    if (item.isDirectory() && item.name === "fixture-state") {
      copyFixtureState(from, join(destination, ".jet"));
    } else if (item.isDirectory()) {
      copyFeatureProject(from, join(destination, item.name));
    } else if (item.isFile()) {
      copyFileSync(from, join(destination, item.name));
    }
  }
}

export function featureProjectRoot(entry) {
  let directory = dirname(entry.path);
  while (true) {
    if (existsSync(join(directory, "package.jet")) || existsSync(join(directory, "fixture-state"))) {
      return directory;
    }
    if (directory === FEATURES) return null;
    const parent = dirname(directory);
    if (parent === directory) return null;
    directory = parent;
  }
}

function stageFeatureProject(entry, scratch, slot) {
  const sourceRoot = featureProjectRoot(entry);
  if (!sourceRoot) return null;
  const stageRoot = join(scratch, "projects", `${slot}-${entry.stem.replaceAll("/", "_")}`);
  const destination = join(stageRoot, relative(ROOT, sourceRoot));
  copyFeatureProject(sourceRoot, destination);
  const stagedEntry = join(stageRoot, relative(ROOT, entry.path));
  if (!existsSync(stagedEntry)) {
    throw new Error(`staged feature project is missing ${entry.shown}`);
  }
  if (relative(stageRoot, stagedEntry).replaceAll("\\", "/") !== entry.shown) {
    throw new Error(`staged feature path diverged from display path ${entry.shown}`);
  }
  return { cwd: stageRoot };
}

function runJet(args, options) {
  return new Promise((resolveRun) => {
    const child = spawn(JET, args, {
      cwd: options.cwd ?? ROOT,
      env: {
        ...process.env,
        NO_COLOR: "1",
        CLICOLOR: "0",
        JET_STORE_DIR: options.store,
        TMPDIR: options.scratch,
        TMP: options.scratch,
        TEMP: options.scratch,
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let timedOut = false;
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, options.timeoutMs);
    child.on("close", (code) => {
      clearTimeout(timer);
      resolveRun({ code, stdout, stderr, timedOut, timeoutSec: Math.round(options.timeoutMs / 1000) });
    });
    child.on("error", (error) => {
      clearTimeout(timer);
      resolveRun({ code: null, stdout, stderr: `${stderr}${error.message}\n`, timedOut, timeoutSec: Math.round(options.timeoutMs / 1000) });
    });
  });
}

export function classifyRun(entry, run, expected) {
  if (run.timedOut) return { status: "timeout", detail: `timeout ${run.timeoutSec}s` };
  if (entry.stem === "ui/ui_native_linux" && run.code !== 0 && gtkLoaderUnavailable(run.stderr)) {
    return { status: "skipped", detail: "gtk4 runtime loader unavailable" };
  }
  if (expected.err !== null) {
    if (!EXPECTED_FAIL_EXITS.has(run.code)) return { status: "failed", detail: `exit ${run.code} (expected 70 or 1)`, stderr: run.stderr };
    const actualStderr = normalize(withoutWarningDiagnostics(run.stderr));
    if (actualStderr !== normalize(expected.err)) {
      return { status: "no-match", detail: "stderr", expected: expected.err, actual: run.stderr };
    }
    return { status: "passed", detail: "expected failure" };
  }
  if (run.code !== 0 && !warningOnlyStderr(run.stderr)) {
    return { status: "failed", detail: `exit ${run.code}`, stderr: run.stderr, stdout: run.stdout };
  }
  const actual = normalize(run.stdout);
  if (expected.out === null) return { status: "failed", detail: "missing expected output" };
  if (actual !== normalize(expected.out)) {
    return { status: "no-match", detail: "stdout", expected: expected.out, actual };
  }
  if (expected.stderrOut !== null && normalize(run.stderr) !== normalize(expected.stderrOut)) {
    return { status: "no-match", detail: "stderr", expected: expected.stderrOut, actual: run.stderr };
  }
  return { status: "passed", detail: "" };
}

async function mapLimit(items, limit, fn) {
  const results = new Array(items.length);
  let next = 0;
  const workers = Array.from({ length: Math.max(1, Math.min(limit, items.length || 1)) }, async (_, slot) => {
    while (true) {
      const index = next++;
      if (index >= items.length) return;
      results[index] = await fn(items[index], slot);
    }
  });
  await Promise.all(workers);
  return results;
}

function summarize(results) {
  const counts = { passed: 0, failed: 0, "no-match": 0, timeout: 0, skipped: 0 };
  for (const result of results) counts[result.status] += 1;
  return counts;
}

function diff(expected, actual) {
  if (expected === actual) return "";
  const e = normalize(expected).split("\n");
  const a = normalize(actual).split("\n");
  const lines = [];
  for (let i = 0; i < Math.max(e.length, a.length) && lines.length < 24; i += 1) {
    if (e[i] === a[i]) continue;
    lines.push(`@@ line ${i + 1} @@`, `-${e[i] ?? ""}`, `+${a[i] ?? ""}`);
  }
  return lines.join("\n");
}

function isWithin(parent, child) {
  const path = relative(parent, child);
  return path === "" || (path !== ".." && !path.startsWith(`..${sep}`) && !isAbsolute(path));
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) { console.log(usage()); return; }
  const entries = collectFeatureExamples().filter((entry) => !options.filters.length || options.filters.some((needle) => entry.shown.includes(needle) || entry.stem.includes(needle)));
  if (!entries.length) throw new Error(`no examples matched: ${options.filters.join(" ")}`);
  if (options.list) { for (const entry of entries) console.log(entry.shown); console.log(`${entries.length} examples`); return; }
  if (!SCRATCH_BASE) throw new Error("HOME is required for feature-example scratch");
  const requestedBase = resolve(SCRATCH_BASE);
  mkdirSync(requestedBase, { recursive: true });
  const scratchBase = realpathSync(requestedBase);
  const repoRoot = realpathSync(ROOT);
  const ramTemp = realpathSync("/tmp");
  if (isWithin(repoRoot, scratchBase) || isWithin(ramTemp, scratchBase)) {
    throw new Error(`feature-example scratch must be external and disk-backed: ${scratchBase}`);
  }
  const scratch = join(scratchBase, `feature-examples-${process.pid}`);
  mkdirSync(scratch, { recursive: true });
  try {
    const results = await mapLimit(entries, options.jobs, async (entry, slot) => {
      const expected = readExpected(entry.stem);
      const skipped = skipReason(entry, expected);
      if (skipped) return { stem: entry.stem, shown: entry.shown, status: "skipped", detail: skipped };
      const staged = stageFeatureProject(entry, scratch, slot);
      const run = await runJet(["run", entry.shown], {
        cwd: staged?.cwd,
        store: join(scratch, "store", String(slot)),
        scratch,
        timeoutMs: options.timeoutSec * 1000,
      });
      return { stem: entry.stem, shown: entry.shown, ...classifyRun(entry, run, expected), stderr: run.stderr, stdout: run.stdout };
    });
    const counts = summarize(results);
    if (options.json) {
      console.log(JSON.stringify({ counts, results: results.map(({ stem, shown, status, detail }) => ({ stem, shown, status, detail })) }, null, 2));
    } else {
      for (const result of results) console.log(`${result.status.padEnd(8)} ${result.shown}${result.detail ? `  ${result.detail}` : ""}`);
      console.log(`summary: ${results.length} examples  passed ${counts.passed}  failed ${counts.failed}  no-match ${counts["no-match"]}  timeout ${counts.timeout}  skipped ${counts.skipped}`);
      for (const result of results.filter((r) => r.status === "failed" || r.status === "no-match")) {
        if (result.status === "failed" && result.stderr) console.log(result.stderr.trimEnd().split("\n").slice(0, 8).join("\n"));
        if (result.status === "no-match") console.log(diff(result.expected ?? "", result.actual ?? ""));
      }
    }
    process.exitCode = counts.failed || counts["no-match"] || counts.timeout ? 1 : 0;
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => { console.error(error.stack ?? error.message); process.exitCode = 2; });
}
