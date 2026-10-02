#!/usr/bin/env node
// Reference-versus-candidate behavioral comparator (#670).
//
// Runs the repository's existing public corpora (the golden feature examples,
// the UI diagnostic fixtures and the sema differential corpus) through two
// `jet` binaries under deterministic paths and a scrubbed environment, and
// reports the first observable divergence: panics, exit status, diagnostics,
// stdout, stderr and side-effect files. It compares behavior only; stage
// artifact identity is owned by #815/#218.
//
// Exit status: 0 no divergence, 1 divergence, 2 unavailable or usage error
// (a missing binary or tool is never a match), 3 no divergence but some case
// was inconclusive (both sides timed out).
import { createHash } from "node:crypto";
import {
  accessSync,
  constants as fsConstants,
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  openSync,
  closeSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { copyFeatureProject, featureProjectRoot } from "./run-feature-examples.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const FEATURES = join(ROOT, "Examples/features");
const UI_DIR = join(ROOT, "tests/ui");
const DIFFERENTIAL_DIR = join(ROOT, "tests/fuzz/sema/differential");
const CANARY_DIR = join(ROOT, "tests/compiler-diff/canary");
const CANARY_CANDIDATE = join(ROOT, "tests/compiler-diff/canary-candidate.mjs");
const EXAMPLE_STDIN_SOURCE = join(ROOT, "tests/common/mod.rs");
const EXT = "jet";
const WORKSPACE_FILE = "workspace.jet";

const CORPORA = ["golden", "ui", "differential"];
const PHASES = ["check", "run", "interpret", "build"];
const DEFAULT_PHASES = {
  golden: ["check", "run"],
  differential: ["check", "run", "interpret"],
  canary: ["check", "run"],
};
// The order a divergence is reported in: the most specific fact first.
const FIELDS = ["panic", "status", "diagnostics", "stdout", "stderr", "side-effects"];
const STREAM_CAP = 8 * 1024 * 1024;
const SHOW_LINES = 40;
const SHOW_LINE_CHARS = 400;

// Environment a compiler run inherits. Everything else is dropped so ambient
// JET_* settings or a developer's shell cannot steer one side.
const PASS_ENV = /^(PATH|HOME|USER|LOGNAME|SHELL|XDG_RUNTIME_DIR|DBUS_SESSION_BUS_ADDRESS|CARGO_HOME|RUSTUP_HOME|RUSTUP_TOOLCHAIN|RUST_MIN_STACK|CC|CXX|AR|LD|NM|RANLIB|PKG_CONFIG|PKG_CONFIG_PATH|LIBRARY_PATH|LD_LIBRARY_PATH|CPATH|C_INCLUDE_PATH|CPLUS_INCLUDE_PATH|SSL_CERT_FILE|NIX_[A-Z0-9_]+)$/u;

export function usage() {
  return [
    "Usage: compiler-diff.mjs --reference <jet> --candidate <jet> [options]",
    "       compiler-diff.mjs --canary [--reference <jet>]",
    "  --reference PATH     pinned reference compiler (default $JET_REFERENCE_COMPILER, else target/debug/jet)",
    "  --candidate PATH     candidate compiler (default $JET_CANDIDATE_COMPILER; required)",
    "  --corpus NAME        golden | ui | differential | all (default all; repeatable)",
    "  --phases LIST        override phases: check,run,interpret,build",
    "  --filter TEXT        keep cases whose path contains TEXT (repeatable)",
    "  --limit N            compare at most N cases (after filtering)",
    "  --keep-going         compare every case; report every divergence",
    "  --jobs N             concurrent cases (default 1)",
    "  --timeout SEC        per-invocation timeout (default 300)",
    "  --mem-limit SIZE     per-invocation memory cap via systemd-run, or none (default 6G)",
    "  --root DIR           deterministic scratch root (default <scratch>/compiler-diff)",
    "  --list               print the selected cases and exit",
    "  --canary             plant a divergence through a checked candidate wrapper and prove it is reported",
  ].join("\n");
}

class Unavailable extends Error {}

export function parseArgs(argv) {
  const options = {
    reference: process.env.JET_REFERENCE_COMPILER || join(ROOT, "target/debug/jet"),
    candidate: process.env.JET_CANDIDATE_COMPILER || null,
    corpora: [],
    phases: null,
    filters: [],
    limit: null,
    keepGoing: false,
    jobs: 1,
    timeoutSec: 300,
    memLimit: "6G",
    root: null,
    list: false,
    canary: false,
    help: false,
  };
  const value = (i, flag) => {
    if (i >= argv.length) throw new Unavailable(`${flag} needs a value`);
    return argv[i];
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--reference") options.reference = value(++i, arg);
    else if (arg === "--candidate") options.candidate = value(++i, arg);
    else if (arg === "--corpus") {
      const corpus = value(++i, arg);
      if (corpus === "all") options.corpora.push(...CORPORA);
      else if (CORPORA.includes(corpus)) options.corpora.push(corpus);
      else throw new Unavailable(`unknown corpus ${corpus}; expected ${CORPORA.join(", ")} or all`);
    } else if (arg === "--phases") {
      options.phases = value(++i, arg).split(",").map((phase) => phase.trim()).filter(Boolean);
      for (const phase of options.phases) {
        if (!PHASES.includes(phase)) throw new Unavailable(`unknown phase ${phase}; expected ${PHASES.join(", ")}`);
      }
    } else if (arg === "--filter") options.filters.push(value(++i, arg));
    else if (arg === "--limit") options.limit = positive(value(++i, arg), arg);
    else if (arg === "--keep-going") options.keepGoing = true;
    else if (arg === "--jobs") options.jobs = positive(value(++i, arg), arg);
    else if (arg === "--timeout") options.timeoutSec = positive(value(++i, arg), arg);
    else if (arg === "--mem-limit") options.memLimit = value(++i, arg);
    else if (arg === "--root") options.root = resolve(value(++i, arg));
    else if (arg === "--list") options.list = true;
    else if (arg === "--canary") options.canary = true;
    else if (arg === "--help" || arg === "-h") options.help = true;
    else throw new Unavailable(`unknown argument ${arg}`);
  }
  if (options.corpora.length === 0) options.corpora = [...CORPORA];
  options.corpora = [...new Set(options.corpora)];
  return options;
}

function positive(value, flag) {
  const n = Number(value);
  if (!Number.isInteger(n) || n <= 0) throw new Unavailable(`${flag} expects a positive integer`);
  return n;
}

// ---------------------------------------------------------------------------
// Corpus discovery: the same enumeration the Rust harnesses use.
// ---------------------------------------------------------------------------

function sortedDir(dir) {
  return readdirSync(dir, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}

// Work directories must not look like Jet sources: the compiler treats an
// ancestor directory named `*.jet` as a source path, so dots are folded too.
function slugOf(text) {
  return text.replace(/[^A-Za-z0-9_-]+/gu, "_");
}

function copyTree(source, destination, skip = () => false) {
  mkdirSync(destination, { recursive: true });
  for (const item of sortedDir(source)) {
    if (skip(item.name)) continue;
    const from = join(source, item.name);
    const to = join(destination, item.name);
    if (item.isDirectory()) copyTree(from, to);
    else if (item.isFile()) copyFileSync(from, to);
  }
}

function copyTopLevelFiles(source, destination) {
  mkdirSync(destination, { recursive: true });
  for (const item of sortedDir(source)) {
    if (item.isFile()) copyFileSync(join(source, item.name), join(destination, item.name));
  }
}

// Port of `collect_feature_golden_entries` (tests/golden.rs).
export function collectGoldenEntries(exDir = FEATURES) {
  const entries = [];
  const push = (path, stem, shown) => entries.push({ path, stem, shown });
  const collectProject = (dir) => {
    const name = basename(dir);
    if (name.startsWith(".") || name === "expected") return;
    const run = join(dir, `run.${EXT}`);
    if (existsSync(run)) {
      const stem = relative(exDir, dir).replaceAll("\\", "/");
      push(run, stem, `Examples/features/${stem}/run.${EXT}`);
      return;
    }
    if (existsSync(join(dir, `package.${EXT}`)) || existsSync(join(dir, WORKSPACE_FILE))) return;
    for (const child of sortedDir(dir)) {
      if (child.isDirectory()) collectProject(join(dir, child.name));
    }
  };
  if (!existsSync(exDir)) return entries;
  for (const topic of sortedDir(exDir)) {
    if (!topic.isDirectory() || topic.name.startsWith(".") || topic.name === "expected") continue;
    const topicPath = join(exDir, topic.name);
    for (const child of sortedDir(topicPath)) {
      const path = join(topicPath, child.name);
      if (
        child.isFile()
        && child.name.endsWith(`.${EXT}`)
        && child.name !== `package.${EXT}`
        && child.name !== WORKSPACE_FILE
        && !child.name.startsWith(".")
      ) {
        const stem = relative(exDir, path).replaceAll("\\", "/").slice(0, -(EXT.length + 1));
        push(path, stem, `Examples/features/${stem}.${EXT}`);
      } else if (child.isDirectory()) {
        collectProject(path);
      }
    }
  }
  return entries.sort((a, b) => (a.stem < b.stem ? -1 : a.stem > b.stem ? 1 : 0));
}

// The interactive-example answers live in one place: `example_stdin` in
// tests/common/mod.rs. Read that table instead of restating it.
export function loadExampleStdin(source = readFileSync(EXAMPLE_STDIN_SOURCE, "utf8")) {
  const consts = new Map();
  for (const match of source.matchAll(/const (\w+): ExampleStdin = ExampleStdin \{\s*piped: ("(?:[^"\\]|\\.)*")/gu)) {
    consts.set(match[1], JSON.parse(match[2]));
  }
  const body = source.split("pub fn example_stdin(")[1]?.split("\n}\n")[0] ?? "";
  const table = new Map();
  for (const match of body.matchAll(/"([^"]+)" => Some\(&(\w+)\)/gu)) {
    if (!consts.has(match[2])) throw new Error(`example_stdin names ${match[2]} but its piped answers were not found`);
    table.set(match[1], consts.get(match[2]));
  }
  if (table.size === 0) throw new Error(`no example_stdin rows found in ${relative(ROOT, EXAMPLE_STDIN_SOURCE)}`);
  return table;
}

function phaseArgs(phase, file) {
  switch (phase) {
    case "check": return ["check", file];
    case "run": return ["run", file];
    case "interpret": return ["run", "--interpret", file];
    case "build": return ["build", file];
    default: throw new Error(`unknown phase ${phase}`);
  }
}

function commandsFor(phases, file, stdin) {
  return phases.map((phase) => ({
    phase,
    args: phaseArgs(phase, file),
    stdin: phase === "run" || phase === "interpret" ? stdin : null,
  }));
}

function goldenCases(phases) {
  const stdinTable = loadExampleStdin();
  return collectGoldenEntries().map((entry) => ({
    corpus: "golden",
    id: entry.shown,
    slug: slugOf(`golden_${entry.stem}`),
    stage(dir) {
      const projectRoot = featureProjectRoot(entry);
      if (projectRoot) {
        copyFeatureProject(projectRoot, join(dir, relative(ROOT, projectRoot)));
      } else if (basename(entry.path) === `run.${EXT}`) {
        copyFeatureProject(dirname(entry.path), join(dir, relative(ROOT, dirname(entry.path))));
      } else {
        copyTopLevelFiles(dirname(entry.path), join(dir, relative(ROOT, dirname(entry.path))));
      }
      return { cwd: dir, env: {} };
    },
    commands: commandsFor(phases ?? DEFAULT_PHASES.golden, entry.shown, stdinTable.get(entry.stem) ?? null),
  }));
}

function flatCases(corpus, sourceDir, phases) {
  if (!existsSync(sourceDir)) return [];
  return sortedDir(sourceDir)
    .filter((item) => item.isFile() && item.name.endsWith(`.${EXT}`))
    .map((item) => {
      const shown = relative(ROOT, join(sourceDir, item.name)).replaceAll("\\", "/");
      return {
        corpus,
        id: shown,
        slug: slugOf(`${corpus}_${item.name.slice(0, -(EXT.length + 1))}`),
        stage(dir) {
          mkdirSync(join(dir, dirname(shown)), { recursive: true });
          copyFileSync(join(sourceDir, item.name), join(dir, shown));
          return { cwd: dir, env: {} };
        },
        commands: commandsFor(phases, shown, null),
      };
    });
}

// The CLI surface a UI fixture directive selects. Fixtures without one are
// ordinary `jet check` fixtures. In-process-only directives (manifest parse,
// env model, jetlib stamps) have no CLI surface and fall back to `check`.
function uiCommands(source, file) {
  const lines = source.split("\n").map((line) => line.trim());
  const has = (directive) => lines.includes(`// @${directive}`);
  const arg = (directive) => lines.find((line) => line.startsWith(`// @${directive} `))?.slice(directive.length + 4).trim();
  const complexity = arg("complexity_cli");
  if (has("typed_settings_cli")) return [{ phase: "check", args: ["check", file, "--set", "cli_only=true"] }];
  if (has("cli_e0043")) return [{ phase: "check", args: ["install"] }];
  if (has("cli_e2101")) return [{ phase: "check", args: ["bench"] }];
  if (has("cli_e1219")) return [{ phase: "build", args: ["build", file, "--profile=turbo"] }];
  if (has("cli_build")) return [{ phase: "build", args: ["build", file] }];
  if (has("sandbox_effect") || has("plugin_target")) return [{ phase: "build", args: ["build", file, "--target=sandbox"] }];
  if (complexity) return [{ phase: "check", args: ["lint", "--complexity", file, `--max=${complexity.split(/\s+/u)[1]}`] }];
  if (has("cost_cli")) return [{ phase: "check", args: ["explain", "--cost", file] }];
  if (has("dev_interpreter") || has("runtime_ffi") || has("parent_control_cancel")) {
    return [{ phase: "interpret", args: ["run", "--interpret", file] }];
  }
  if (has("run_cli") || has("web_run")) return [{ phase: "run", args: ["run", file] }];
  return [{ phase: "check", args: ["check", file] }];
}

// Port of the `ui_snapshots` discovery and `UiFixtureScratch` staging
// (tests/diagnostic_snapshots.rs).
function uiCases(phases) {
  const cases = [];
  for (const item of sortedDir(UI_DIR)) {
    const path = join(UI_DIR, item.name);
    if (item.isFile() && item.name.endsWith(`.${EXT}`) && !item.name.includes(".fixed.")) {
      cases.push({ entry: path, fixtureDir: null, shown: `tests/ui/${item.name}` });
    } else if (item.isDirectory()) {
      const entry = ["run", "main"].map((name) => join(path, `${name}.${EXT}`)).find((candidate) => existsSync(candidate))
        ?? (existsSync(join(path, WORKSPACE_FILE)) ? join(path, WORKSPACE_FILE) : null);
      if (entry) cases.push({ entry, fixtureDir: path, shown: `tests/ui/${item.name}/${basename(entry)}` });
    }
  }
  cases.sort((a, b) => (a.shown < b.shown ? -1 : a.shown > b.shown ? 1 : 0));
  return cases.map(({ entry, fixtureDir, shown }) => {
    const source = readFileSync(entry, "utf8");
    const file = basename(entry);
    const commands = phases ? commandsFor(phases, file, null) : uiCommands(source, file);
    return {
      corpus: "ui",
      id: shown,
      slug: slugOf(`ui_${shown.slice("tests/ui/".length)}`),
      stage(dir) {
        const env = {};
        let cwd;
        if (fixtureDir) {
          cwd = join(dir, "tests/ui", basename(fixtureDir));
          const state = join(fixtureDir, "state");
          if (existsSync(state) && statSync(state).isDirectory()) {
            copyTree(fixtureDir, cwd, (name) => name === "state" || name === "stderr");
            const hasInlinePackage = source.split("\n").some((line) => line.trimStart().startsWith("package {"));
            if (!existsSync(join(cwd, `package.${EXT}`)) && !hasInlinePackage && file !== WORKSPACE_FILE) {
              writeFileSync(join(cwd, `package.${EXT}`), `name: "${file.slice(0, -(EXT.length + 1))}"\nversion: "0.1.0"\n`);
            }
            copyTree(state, join(cwd, ".jet"));
          } else {
            copyTree(fixtureDir, cwd);
          }
        } else {
          cwd = join(dir, "tests/ui");
          mkdirSync(cwd, { recursive: true });
          const stem = file.slice(0, -(EXT.length + 1));
          for (const sibling of sortedDir(UI_DIR)) {
            if (sibling.isFile() && (sibling.name === file || sibling.name.startsWith(`${stem}.`))) {
              copyFileSync(join(UI_DIR, sibling.name), join(cwd, sibling.name));
            }
          }
          const snapshot = join(UI_DIR, `${stem}.published.snapshot`);
          if (existsSync(snapshot)) {
            const text = readFileSync(snapshot, "utf8");
            const typeName = text.split("\n").find((line) => line.startsWith("type = "))?.slice("type = ".length).trim() || "Unknown";
            const cache = join(dir, "schema-cache");
            mkdirSync(cache, { recursive: true });
            writeFileSync(join(cache, `${typeName}.snapshot`), text);
            env.JET_SCHEMA_CACHE_DIR = cache;
          }
        }
        const extension = source.split("\n").map((line) => line.trim()).find((line) => line.startsWith("// @compiler_extension "));
        if (extension) env.JET_COMPILER_EXTENSION = join(ROOT, extension.slice("// @compiler_extension ".length).trim());
        return { cwd, env };
      },
      commands,
    };
  });
}

function canaryCases() {
  return flatCases("canary", CANARY_DIR, DEFAULT_PHASES.canary);
}

export function selectCases(options) {
  let cases = [];
  if (options.canary) cases = canaryCases();
  else {
    for (const corpus of options.corpora) {
      if (corpus === "golden") cases.push(...goldenCases(options.phases));
      else if (corpus === "ui") cases.push(...uiCases(options.phases));
      else if (corpus === "differential") {
        cases.push(...flatCases("differential", DIFFERENTIAL_DIR, options.phases ?? DEFAULT_PHASES.differential));
      }
    }
  }
  if (options.filters.length) cases = cases.filter((c) => options.filters.some((needle) => c.id.includes(needle)));
  if (options.limit) cases = cases.slice(0, options.limit);
  return cases;
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

function isWithin(parent, child) {
  const path = relative(parent, child);
  return path === "" || (path !== ".." && !path.startsWith(`..${sep}`) && !isAbsolute(path));
}

function scratchRoot(options) {
  const base = process.env.JET_TEST_SCRATCH_DIR
    || (process.env.HOME ? join(process.env.HOME, ".cache/jet-test-scratch") : null);
  if (!options.root && !base) throw new Unavailable("no scratch root: pass --root or set JET_TEST_SCRATCH_DIR or HOME");
  const root = options.root ?? join(base, options.canary ? "compiler-diff-canary" : "compiler-diff");
  if (isWithin("/tmp", root)) throw new Unavailable(`scratch root ${root} is on RAM-backed /tmp; use a disk path`);
  if (isWithin(ROOT, root)) throw new Unavailable(`scratch root ${root} is inside the repository`);
  return root;
}

function requireExecutable(path, role) {
  if (!path) throw new Unavailable(`no ${role} compiler: pass --${role} or set JET_${role.toUpperCase()}_COMPILER`);
  const absolute = resolve(path);
  try {
    if (!statSync(absolute).isFile()) throw new Error("not a file");
    accessSync(absolute, fsConstants.X_OK);
  } catch (error) {
    throw new Unavailable(`${role} compiler ${absolute} is unavailable (${error.code ?? error.message})`);
  }
  return absolute;
}

// A compiler is pinned to the identity of every file it executes, taken once at
// startup and re-verified around each invocation. Snapshot rotation can delete
// or replace a binary mid-run; both sides then fail the same way, and that must
// be unavailable, never compared as a match.
function fileIdentity(path) {
  try {
    const { dev, ino, size, mtimeMs } = statSync(path);
    return `dev ${dev} ino ${ino} size ${size} mtime ${mtimeMs}`;
  } catch (error) {
    return error.code ?? error.message;
  }
}

function pinFiles(paths) {
  return paths.map((path) => ({ path, identity: fileIdentity(path) }));
}

function verifyPinned(compiler, when) {
  for (const { path, identity } of compiler.pinned) {
    const current = fileIdentity(path);
    if (current !== identity) {
      throw new Unavailable(`${compiler.role} compiler ${path} changed or disappeared ${when} (pinned ${identity}; now ${current})`);
    }
  }
}

// systemd-run reports its own failure to exec the compiler as exit 1, an empty
// stdout and exactly one stderr line; that is no observation of the compiler.
export function launcherFailure(run, executable, memLimit) {
  if (memLimit === "none" || run.code !== 1 || run.stdout !== "") return null;
  const lines = run.stderr.split("\n");
  if (lines.length !== 2 || lines[1] !== "") return null;
  const [line] = lines;
  const launcher = line.startsWith(`Failed to find executable ${executable}:`)
    || line.startsWith("Failed to execute: ")
    || line.startsWith("Failed to start transient scope unit: ");
  return launcher ? line : null;
}

function toolAvailable(tool, args, env) {
  const result = spawnSync(tool, args, { env, stdio: "ignore" });
  return !result.error && result.status === 0;
}

function baseEnv() {
  const env = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (PASS_ENV.test(key)) env[key] = value;
  }
  return env;
}

function sideEnv(sideRoot) {
  return {
    ...baseEnv(),
    LC_ALL: "C",
    LANG: "C",
    TZ: "UTC",
    NO_COLOR: "1",
    CLICOLOR: "0",
    TERM: "dumb",
    RUST_BACKTRACE: "0",
    JET_REPL_HISTORY: "off",
    JET_NIX_TMP_CLEANED: "1",
    JET_STORE_DIR: join(sideRoot, "store"),
    JET_FFI_CACHE_DIR: join(sideRoot, "ffi-cache"),
    JET_TEST_SCRATCH_DIR: join(sideRoot, "scratch"),
    XDG_CACHE_HOME: join(sideRoot, "xdg/cache"),
    XDG_CONFIG_HOME: join(sideRoot, "xdg/config"),
    XDG_DATA_HOME: join(sideRoot, "xdg/data"),
    XDG_STATE_HOME: join(sideRoot, "xdg/state"),
    TMPDIR: join(sideRoot, "tmp"),
    TMP: join(sideRoot, "tmp"),
    TEMP: join(sideRoot, "tmp"),
  };
}

function prepareSideRoot(sideRoot) {
  rmSync(sideRoot, { recursive: true, force: true });
  for (const sub of ["store", "ffi-cache", "scratch", "xdg/cache", "xdg/config", "xdg/data", "xdg/state", "tmp"]) {
    mkdirSync(join(sideRoot, sub), { recursive: true });
  }
}

function launch(compiler, args, memLimit) {
  const [command, ...prefix] = compiler.argv;
  const argv = [...prefix, ...args];
  if (memLimit === "none") return { command, argv };
  return {
    command: "systemd-run",
    argv: ["--user", "--scope", "-q", "-p", `MemoryMax=${memLimit}`, "-p", "MemorySwapMax=0", "--", command, ...argv],
  };
}

function invoke(compiler, command, cwd, env, options) {
  return new Promise((resolveRun) => {
    const { command: exe, argv } = launch(compiler, command.args, options.memLimit);
    const started = Date.now();
    const child = spawn(exe, argv, {
      cwd,
      env: { ...env, ...compiler.env },
      stdio: [command.stdin === null ? "ignore" : "pipe", "pipe", "pipe"],
      detached: true,
    });
    const out = { chunks: [], size: 0, overflow: false };
    const err = { chunks: [], size: 0, overflow: false };
    const collect = (sink) => (chunk) => {
      if (sink.size >= STREAM_CAP) { sink.overflow = true; return; }
      sink.chunks.push(chunk);
      sink.size += chunk.length;
    };
    child.stdout.on("data", collect(out));
    child.stderr.on("data", collect(err));
    if (command.stdin !== null) {
      child.stdin.on("error", () => {});
      child.stdin.end(command.stdin);
    }
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      try { process.kill(-child.pid, "SIGKILL"); } catch { child.kill("SIGKILL"); }
    }, options.timeoutSec * 1000);
    let spawnError = null;
    child.on("error", (error) => { spawnError = error; });
    child.on("close", (code, signal) => {
      clearTimeout(timer);
      resolveRun({
        code,
        signal,
        timedOut,
        spawnError: spawnError ? `${spawnError.code ?? ""} ${spawnError.message}`.trim() : null,
        stdout: Buffer.concat(out.chunks).toString("utf8"),
        stderr: Buffer.concat(err.chunks).toString("utf8"),
        overflow: out.overflow || err.overflow,
        ms: Date.now() - started,
      });
    });
  });
}

// ---------------------------------------------------------------------------
// Normalization, redaction and observation
// ---------------------------------------------------------------------------

function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
}

const SECRET_PATTERNS = [
  /\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{20,}\b/gu,
  /\bgithub_pat_[A-Za-z0-9_]{20,}\b/gu,
  /\bsk-[A-Za-z0-9_-]{20,}\b/gu,
  /\bxox[abprs]-[A-Za-z0-9-]{10,}\b/gu,
  /\bAKIA[0-9A-Z]{16}\b/gu,
  /-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----/gu,
];

export function redact(text) {
  let result = text;
  for (const pattern of SECRET_PATTERNS) result = result.replace(pattern, "<REDACTED>");
  return result.replace(/\b((?:api[_-]?key|token|secret|password|passwd)\s*[=:]\s*)\S+/giu, "$1<REDACTED>");
}

function pathVariants(path) {
  const variants = new Set([path]);
  try { variants.add(realpathSync(path)); } catch { /* not created yet */ }
  return [...variants].sort((a, b) => b.length - a.length);
}

// Replace machine-specific roots with stable tokens. Longest paths first so a
// case directory wins over the scratch root that contains it.
function makeNormalizer(replacements) {
  const pairs = [];
  for (const [path, token] of replacements) {
    if (!path) continue;
    for (const variant of pathVariants(path)) pairs.push([variant, token]);
  }
  pairs.sort((a, b) => b[0].length - a[0].length);
  const patterns = pairs.map(([path, token]) => [new RegExp(escapeRegExp(path), "gu"), token]);
  return (text) => {
    let result = text.replaceAll("\r\n", "\n");
    for (const [pattern, token] of patterns) result = result.replace(pattern, token);
    result = result.replace(/panicked at [^\n]*?:\d+:\d+:?/gu, "panicked at <LOC>:");
    return redact(result);
  };
}

// Volatile-by-construction facts in compiler chatter: wall-clock durations and
// content/identity digests (the two compilers have different identities).
function stabilizeStderr(text) {
  return text
    .replace(/\b\d+(?:\.\d+)?\s?(?:ns|µs|us|ms|s|sec|secs|seconds)\b/gu, "<DUR>")
    .replace(/\b[0-9a-f]{16,}\b/gu, "<HEX>");
}

const DIAGNOSTIC_HEADER = /^(Error|Warning|Note|Help|Info|Hint|Stop|Lint)\b[^[\n]*\[([A-Z][A-Za-z0-9-]*)\](?: \(([^)]*)\))?: ?(.*)$/u;
const DIAGNOSTIC_SPAN = /^\s*-->\s*(\S+)$/u;

export function parseDiagnostics(text) {
  const diagnostics = [];
  for (const line of text.split("\n")) {
    const header = DIAGNOSTIC_HEADER.exec(line);
    if (header) {
      diagnostics.push({ severity: header[1], code: header[2], lint: header[3] ?? null, message: header[4], span: null });
      continue;
    }
    const span = DIAGNOSTIC_SPAN.exec(line);
    if (span && diagnostics.length && diagnostics.at(-1).span === null) diagnostics.at(-1).span = span[1];
  }
  return diagnostics;
}

function diagnosticText(d) {
  return `${d.severity} [${d.code}]${d.lint ? ` (${d.lint})` : ""}: ${d.message}${d.span ? ` @ ${d.span}` : ""}`;
}

export function classifyPanic(stderr) {
  const lines = stderr.split("\n");
  const find = (pattern) => lines.findIndex((line) => pattern.test(line));
  const overflow = find(/has overflowed its stack/u);
  if (overflow >= 0) return { kind: "stack-overflow", message: lines[overflow].replace(/thread '[^']*'/u, "thread <T>") };
  const ice = find(/internal compiler error/iu);
  if (ice >= 0) return { kind: "ice", message: lines[ice] };
  const panic = find(/^thread '.*' panicked at/u);
  if (panic >= 0) return { kind: "panic", message: (lines[panic + 1] ?? "").trim() };
  return { kind: "none", message: "" };
}

function statusText(run) {
  if (run.timedOut) return "timeout";
  if (run.spawnError) return `spawn-error: ${run.spawnError}`;
  if (run.overflow) return `output-overflow (> ${STREAM_CAP} bytes)`;
  if (run.signal) return `signal ${run.signal}`;
  return `exit ${run.code}`;
}

function hashFile(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

// Snapshot every file under the case directory: rel path -> fingerprint.
function snapshotTree(dir) {
  const files = new Map();
  const walk = (current) => {
    let items;
    try { items = sortedDir(current); } catch { return; }
    for (const item of items) {
      const path = join(current, item.name);
      const rel = relative(dir, path).replaceAll("\\", "/");
      if (item.isDirectory()) walk(path);
      else {
        const stat = lstatSync(path);
        const small = stat.isFile() && stat.size <= 16 * 1024 * 1024;
        files.set(rel, {
          size: stat.size,
          executable: (stat.mode & 0o111) !== 0,
          symlink: stat.isSymbolicLink(),
          hash: small ? hashFile(path) : `size:${stat.size}`,
        });
      }
    }
  };
  walk(dir);
  return files;
}

function isText(buffer) {
  if (buffer.includes(0)) return false;
  return Buffer.from(buffer.toString("utf8"), "utf8").equals(buffer);
}

// A side effect as an observation: the path (with digest-named components
// folded, since content-addressed names carry compiler identity), the change,
// and, for ordinary text files the program or compiler wrote for the user,
// the normalized content. Compiler state under `.jet/` and binary or
// executable artifacts compare by presence only: behavior, not bytes.
function sideEffects(caseDir, before, after, normalize) {
  const effects = [];
  const describe = (rel, change, fingerprint) => {
    const shown = rel.split("/").map((part) => (/^[0-9a-f]{32,}$/u.test(part) ? "<HASH>" : part)).join("/");
    if (change === "deleted") return `deleted ${shown}`;
    const internal = rel.split("/").includes(".jet");
    if (internal || fingerprint.executable || fingerprint.symlink || fingerprint.size > 1024 * 1024) {
      return `${change} ${shown}${fingerprint.executable ? " (executable)" : ""}`;
    }
    const bytes = readFileSync(join(caseDir, rel));
    if (!isText(bytes)) return `${change} ${shown} (binary)`;
    const digest = createHash("sha256").update(normalize(bytes.toString("utf8"))).digest("hex").slice(0, 16);
    return `${change} ${shown} (text ${digest})`;
  };
  for (const [rel, fingerprint] of after) {
    const prior = before.get(rel);
    if (!prior) effects.push(describe(rel, "created", fingerprint));
    else if (prior.hash !== fingerprint.hash || prior.executable !== fingerprint.executable) {
      effects.push(describe(rel, "modified", fingerprint));
    }
  }
  for (const rel of before.keys()) if (!after.has(rel)) effects.push(describe(rel, "deleted", null));
  return effects.sort();
}

function observe(run, normalize, effects) {
  const stdout = normalize(run.stdout);
  const stderr = stabilizeStderr(normalize(run.stderr));
  return {
    status: statusText(run),
    panic: classifyPanic(stderr),
    diagnostics: parseDiagnostics(stderr).concat(parseDiagnostics(stdout)),
    stdout,
    stderr,
    sideEffects: effects,
    ms: run.ms,
  };
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

function clipLine(line) {
  return line.length > SHOW_LINE_CHARS ? `${line.slice(0, SHOW_LINE_CHARS)}…` : line;
}

function clip(text) {
  const lines = text.split("\n");
  const shown = lines.slice(0, SHOW_LINES).map(clipLine);
  if (lines.length > SHOW_LINES) shown.push(`… ${lines.length - SHOW_LINES} more line(s)`);
  return shown.join("\n");
}

function firstLineDifference(expected, candidate) {
  const e = expected.split("\n");
  const c = candidate.split("\n");
  for (let i = 0; i < Math.max(e.length, c.length); i += 1) {
    if (e[i] !== c[i]) return { line: i + 1, expected: e[i] ?? "<end of output>", candidate: c[i] ?? "<end of output>" };
  }
  return null;
}

function compareField(field, ref, cand) {
  switch (field) {
    case "panic": {
      const a = `${ref.panic.kind}${ref.panic.message ? `: ${ref.panic.message}` : ""}`;
      const b = `${cand.panic.kind}${cand.panic.message ? `: ${cand.panic.message}` : ""}`;
      return a === b ? null : { expected: a, candidate: b };
    }
    case "status":
      return ref.status === cand.status ? null : { expected: ref.status, candidate: cand.status };
    case "diagnostics": {
      const a = ref.diagnostics.map(diagnosticText);
      const b = cand.diagnostics.map(diagnosticText);
      for (let i = 0; i < Math.max(a.length, b.length); i += 1) {
        if (a[i] !== b[i]) {
          return {
            index: i,
            span: ref.diagnostics[i]?.span ?? cand.diagnostics[i]?.span ?? null,
            expected: a[i] ?? `<no diagnostic #${i + 1}; reference reports ${a.length}>`,
            candidate: b[i] ?? `<no diagnostic #${i + 1}; candidate reports ${b.length}>`,
          };
        }
      }
      return null;
    }
    case "stdout":
    case "stderr": {
      const difference = firstLineDifference(ref[field], cand[field]);
      return difference && {
        line: difference.line,
        expected: difference.expected,
        candidate: difference.candidate,
        expectedText: clip(ref[field]),
        candidateText: clip(cand[field]),
      };
    }
    case "side-effects": {
      const a = new Set(ref.sideEffects);
      const b = new Set(cand.sideEffects);
      const missing = ref.sideEffects.find((effect) => !b.has(effect));
      const extra = cand.sideEffects.find((effect) => !a.has(effect));
      if (!missing && !extra) return null;
      return {
        expected: missing ?? "<nothing further>",
        candidate: extra ?? "<nothing further>",
        expectedText: clip(ref.sideEffects.join("\n")),
        candidateText: clip(cand.sideEffects.join("\n")),
      };
    }
    default:
      throw new Error(`unknown field ${field}`);
  }
}

export function comparePhase(ref, cand) {
  if (ref.status === "timeout" && cand.status === "timeout") return { inconclusive: "both sides timed out" };
  const differing = [];
  for (const field of FIELDS) {
    const difference = compareField(field, ref, cand);
    if (difference) {
      differing.push({ field, ...difference, expected: clipLine(difference.expected), candidate: clipLine(difference.candidate) });
    }
  }
  return { differing };
}

// ---------------------------------------------------------------------------
// Case runner
// ---------------------------------------------------------------------------

async function runSide(testCase, compiler, workDir, context, options) {
  rmSync(workDir, { recursive: true, force: true });
  mkdirSync(workDir, { recursive: true });
  const staged = testCase.stage(workDir);
  const env = { ...sideEnv(compiler.sideRoot), ...staged.env };
  const normalize = makeNormalizer([
    [workDir, "<CASE>"],
    [compiler.sideRoot, "<SIDE>"],
    [context.root, "<ROOT>"],
    [compiler.path, "<JET>"],
    [ROOT, "<REPO>"],
    [process.env.HOME, "~"],
  ]);
  const observations = [];
  for (const command of testCase.commands) {
    const before = snapshotTree(workDir);
    const where = `${testCase.id} (${command.phase})`;
    verifyPinned(compiler, `before ${where}`);
    const run = await invoke(compiler, command, staged.cwd, env, options);
    if (run.spawnError) {
      throw new Unavailable(`${compiler.role} could not start for ${where}: ${run.spawnError}`);
    }
    const launcher = launcherFailure(run, compiler.argv[0], options.memLimit);
    if (launcher) throw new Unavailable(`${compiler.role} could not be launched for ${where}: ${launcher}`);
    verifyPinned(compiler, `during ${where}`);
    const after = snapshotTree(workDir);
    observations.push({
      command,
      raw: run,
      cwd: relative(workDir, staged.cwd).replaceAll("\\", "/") || ".",
      env,
      observed: observe(run, normalize, sideEffects(workDir, before, after, normalize)),
    });
  }
  return observations;
}

async function runCase(testCase, context, options) {
  const workDir = join(context.root, "work", testCase.slug);
  const reference = await runSide(testCase, context.reference, workDir, context, options);
  const candidate = await runSide(testCase, context.candidate, workDir, context, options);
  const phases = [];
  let inconclusive = null;
  for (let i = 0; i < testCase.commands.length; i += 1) {
    const verdict = comparePhase(reference[i].observed, candidate[i].observed);
    if (verdict.inconclusive) {
      inconclusive ??= { phase: testCase.commands[i].phase, reason: verdict.inconclusive };
      continue;
    }
    if (verdict.differing.length) {
      return {
        outcome: "diverged",
        divergence: { index: i, phase: testCase.commands[i].phase, command: testCase.commands[i], differing: verdict.differing },
        reference,
        candidate,
      };
    }
    phases.push(testCase.commands[i].phase);
  }
  rmSync(workDir, { recursive: true, force: true });
  if (inconclusive) return { outcome: "inconclusive", inconclusive };
  const observed = reference.map(({ command, observed: o }) => ({
    phase: command.phase,
    status: o.status,
    stdout: clip(o.stdout),
    codes: o.diagnostics.map((d) => d.code),
  }));
  return { outcome: "matched", observed };
}

function shellQuote(text) {
  return /^[A-Za-z0-9_./:=@%+-]+$/u.test(text) ? text : `'${text.replaceAll("'", "'\\''")}'`;
}

// A bounded, self-contained repro: the staged inputs, both sides' raw outputs
// up to and including the divergent phase, and a script that replays one side.
function writeRepro(testCase, result, context) {
  const dir = join(context.root, "repro", testCase.slug);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const input = join(dir, "input");
  mkdirSync(input, { recursive: true });
  testCase.stage(input);
  const last = result.divergence.index;
  for (const [role, observations] of [["reference", result.reference], ["candidate", result.candidate]]) {
    const out = join(dir, role);
    mkdirSync(out, { recursive: true });
    for (let i = 0; i <= last; i += 1) {
      const { command, raw, observed } = observations[i];
      const name = `${i + 1}-${command.phase}`;
      writeFileSync(join(out, `${name}.stdout`), raw.stdout);
      writeFileSync(join(out, `${name}.stderr`), raw.stderr);
      writeFileSync(join(out, `${name}.status`), `${observed.status}\n`);
      writeFileSync(join(out, `${name}.side-effects`), `${observed.sideEffects.join("\n")}\n`);
    }
  }
  const lines = [
    "#!/usr/bin/env bash",
    `# compiler-diff repro: ${testCase.corpus} ${testCase.id}, divergent phase ${result.divergence.phase}.`,
    "# Usage: ./repro.sh reference|candidate  (replays that side in ./work).",
    "set -u",
    'here="$(cd "$(dirname "$0")" && pwd)"',
    'side="${1:?usage: repro.sh reference|candidate}"',
    'case "$side" in',
  ];
  for (const role of ["reference", "candidate"]) {
    const compiler = context[role];
    lines.push(`  ${role}) compiler=(${compiler.argv.map(shellQuote).join(" ")}); side_root=${shellQuote(compiler.sideRoot)}; extra_env=(${Object.entries(compiler.env).map(([k, v]) => shellQuote(`${k}=${v}`)).join(" ")}) ;;`);
  }
  lines.push('  *) echo "usage: repro.sh reference|candidate" >&2; exit 64 ;;', "esac");
  lines.push('rm -rf "$here/work" && cp -a "$here/input" "$here/work"');
  const env = result.reference[0].env;
  const workDir = join(context.root, "work", testCase.slug);
  for (let i = 0; i <= last; i += 1) {
    const { command, cwd } = result.reference[i];
    const assignments = Object.entries(env)
      .filter(([key]) => !["JET_STORE_DIR", "JET_FFI_CACHE_DIR", "JET_TEST_SCRATCH_DIR", "TMPDIR", "TMP", "TEMP"].includes(key) && !key.startsWith("XDG_"))
      .map(([key, value]) => (value.startsWith(workDir)
        ? `${key}="$here/work"${shellQuote(value.slice(workDir.length) || "/")}`
        : shellQuote(`${key}=${value}`)));
    lines.push(
      `echo "== ${command.phase}: jet ${command.args.map(shellQuote).join(" ").replaceAll('"', '\\"')}" >&2`,
      `(cd "$here/work/${cwd}" && ${command.stdin === null ? "" : `printf %s ${shellQuote(command.stdin)} | `}env -i ${assignments.join(" ")} \\`,
      '  JET_STORE_DIR="$side_root/store" JET_FFI_CACHE_DIR="$side_root/ffi-cache" JET_TEST_SCRATCH_DIR="$side_root/scratch" \\',
      '  TMPDIR="$side_root/tmp" TMP="$side_root/tmp" TEMP="$side_root/tmp" XDG_CACHE_HOME="$side_root/xdg/cache" \\',
      '  XDG_CONFIG_HOME="$side_root/xdg/config" XDG_DATA_HOME="$side_root/xdg/data" XDG_STATE_HOME="$side_root/xdg/state" \\',
      `  "\${extra_env[@]}" "\${compiler[@]}" ${command.args.map(shellQuote).join(" ")}${command.stdin === null ? " </dev/null" : ""})`,
      'echo "== status $?" >&2',
    );
  }
  writeFileSync(join(dir, "repro.sh"), `${lines.join("\n")}\n`, { mode: 0o755 });
  return dir;
}

async function runAll(cases, context, options, onProgress) {
  const results = new Array(cases.length).fill(null);
  let next = 0;
  let stopAt = Infinity;
  let fatal = null;
  const worker = async () => {
    while (fatal === null) {
      const index = next++;
      if (index >= cases.length || index > stopAt) return;
      try {
        results[index] = await runCase(cases[index], context, options);
      } catch (error) {
        fatal ??= error;
        return;
      }
      onProgress(index, results[index]);
      if (results[index].outcome === "diverged" && !options.keepGoing) stopAt = Math.min(stopAt, index);
    }
  };
  await Promise.all(Array.from({ length: Math.min(options.jobs, Math.max(cases.length, 1)) }, worker));
  if (fatal) throw fatal;
  return results;
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

function tilde(path) {
  return redact(process.env.HOME && path.startsWith(process.env.HOME) ? `~${path.slice(process.env.HOME.length)}` : path);
}

function divergenceRecord(testCase, result, repro) {
  const [first, ...rest] = result.divergence.differing;
  return {
    case: testCase.id,
    corpus: testCase.corpus,
    phase: result.divergence.phase,
    command: `jet ${result.divergence.command.args.join(" ")}`,
    field: first.field,
    detail: first,
    alsoDiffers: rest.map((difference) => difference.field),
    also: rest.map(({ field, expected, candidate, span, index, line }) => ({ field, expected, candidate, span, index, line })),
    repro: repro ? tilde(repro) : null,
  };
}

function printDivergence(record, heading) {
  const d = record.detail;
  const lines = [
    heading,
    `  case:      ${record.corpus} ${record.case}`,
    `  phase:     ${record.phase} (${record.command})`,
    `  field:     ${record.field}${d.index !== undefined ? ` #${d.index + 1}` : ""}${d.line !== undefined ? ` line ${d.line}` : ""}`,
  ];
  if (d.span) lines.push(`  span:      ${d.span}`);
  lines.push(`  expected:  ${d.expected}`, `  candidate: ${d.candidate}`);
  for (const other of record.also) {
    const where = `${other.index !== undefined ? ` #${other.index + 1}` : ""}${other.line !== undefined ? ` line ${other.line}` : ""}`;
    lines.push(`  also ${other.field}${where}:`, `    expected:  ${other.expected}`, `    candidate: ${other.candidate}`);
  }
  if (d.expectedText !== undefined) {
    lines.push("  --- reference", ...d.expectedText.split("\n").map((line) => `  | ${line}`));
    lines.push("  +++ candidate", ...d.candidateText.split("\n").map((line) => `  | ${line}`));
  }
  if (record.repro) lines.push(`  repro:     ${record.repro}  (./repro.sh reference|candidate)`);
  console.log(lines.join("\n"));
}

function acquireLock(root) {
  mkdirSync(root, { recursive: true });
  const lock = join(root, "lock");
  try {
    const fd = openSync(lock, "wx");
    writeFileSync(fd, `${process.pid}\n`);
    closeSync(fd);
  } catch (error) {
    if (error.code !== "EEXIST") throw error;
    const pid = Number(readFileSync(lock, "utf8").trim());
    let alive = false;
    try { process.kill(pid, 0); alive = true; } catch { alive = false; }
    if (alive) throw new Unavailable(`scratch root ${tilde(root)} is in use by pid ${pid}`);
    writeFileSync(lock, `${process.pid}\n`);
  }
  return () => rmSync(lock, { force: true });
}

async function compare(options) {
  const root = scratchRoot(options);
  const referencePath = requireExecutable(options.reference, "reference");
  const candidatePath = options.canary ? referencePath : requireExecutable(options.candidate, "candidate");
  const cases = selectCases(options);
  if (options.list) {
    for (const testCase of cases) console.log(`${testCase.corpus}\t${testCase.id}\t${testCase.commands.map((c) => c.phase).join(",")}`);
    return { exit: 0 };
  }
  if (cases.length === 0) throw new Unavailable("no cases selected");

  const probeEnv = baseEnv();
  if (options.memLimit !== "none" && !toolAvailable("systemd-run", ["--version"], probeEnv)) {
    throw new Unavailable("systemd-run is unavailable for --mem-limit; pass --mem-limit none to run uncapped");
  }
  const needsRustc = cases.some((c) => c.commands.some((command) => command.phase === "run" || command.phase === "build"));
  if (needsRustc && !toolAvailable("rustc", ["--version"], probeEnv)) {
    throw new Unavailable("rustc is unavailable; run and build phases cannot be compared (run under Tools/agent/jet-env)");
  }

  const release = acquireLock(root);
  try {
    rmSync(join(root, "work"), { recursive: true, force: true });
    rmSync(join(root, "repro"), { recursive: true, force: true });
    const context = {
      root,
      reference: {
        role: "reference",
        path: referencePath,
        argv: [referencePath],
        env: {},
        sideRoot: join(root, "reference"),
        pinned: pinFiles([referencePath]),
      },
      candidate: options.canary
        ? {
          role: "candidate",
          path: CANARY_CANDIDATE,
          argv: [process.execPath, CANARY_CANDIDATE],
          env: { COMPILER_DIFF_CANARY_REAL: referencePath },
          sideRoot: join(root, "candidate"),
          pinned: pinFiles([CANARY_CANDIDATE, referencePath]),
        }
        : {
          role: "candidate",
          path: candidatePath,
          argv: [candidatePath],
          env: {},
          sideRoot: join(root, "candidate"),
          pinned: pinFiles([candidatePath]),
        },
    };
    prepareSideRoot(context.reference.sideRoot);
    prepareSideRoot(context.candidate.sideRoot);

    console.log(`compiler-diff: reference ${tilde(referencePath)}`);
    console.log(`compiler-diff: candidate ${options.canary ? `canary wrapper ${relative(ROOT, CANARY_CANDIDATE)} over the reference` : tilde(candidatePath)}`);
    if (!options.canary && realpathSync(referencePath) === realpathSync(candidatePath)) {
      console.log("compiler-diff: note: reference and candidate are the same binary (self-comparison)");
    }
    const corpora = options.canary ? ["canary"] : options.corpora;
    console.log(`compiler-diff: ${cases.length} case(s) from ${corpora.join(", ")}; root ${tilde(root)}`);

    const width = String(cases.length).length;
    const results = await runAll(cases, context, options, (index, result) => {
      const extra = result.outcome === "inconclusive" ? ` (${result.inconclusive.phase}: ${result.inconclusive.reason})`
        : result.outcome === "diverged" ? ` (${result.divergence.phase}: ${result.divergence.differing.map((d) => d.field).join(", ")})` : "";
      process.stderr.write(`[${String(index + 1).padStart(width)}/${cases.length}] ${cases[index].corpus} ${cases[index].id}: ${result.outcome}${extra}\n`);
    });

    const divergences = [];
    const inconclusive = [];
    let matched = 0;
    for (let i = 0; i < cases.length; i += 1) {
      const result = results[i];
      if (!result) continue;
      if (result.outcome === "matched") matched += 1;
      else if (result.outcome === "inconclusive") inconclusive.push({ case: cases[i].id, ...result.inconclusive });
      else if (divergences.length < 20) divergences.push(divergenceRecord(cases[i], result, writeRepro(cases[i], result, context)));
      else divergences.push(divergenceRecord(cases[i], result, null));
    }
    if (!options.keepGoing && divergences.length > 1) divergences.splice(1);
    const compared = results.filter(Boolean).length;
    if (divergences.length) {
      printDivergence(divergences[0], "FIRST DIVERGENCE");
      for (const record of divergences.slice(1)) printDivergence(record, "DIVERGENCE");
    }
    for (const row of inconclusive) console.log(`INCONCLUSIVE ${row.case} (${row.phase}): ${row.reason}`);
    const summary = {
      compared,
      matched,
      diverged: results.filter((r) => r?.outcome === "diverged").length,
      inconclusive: inconclusive.length,
      notRun: cases.length - compared,
    };
    console.log(`summary: compared ${summary.compared}, matched ${summary.matched}, diverged ${summary.diverged}, inconclusive ${summary.inconclusive}, not run ${summary.notRun}`);
    const report = {
      schema: "jet.compiler-diff.v1",
      reference: tilde(referencePath),
      candidate: options.canary ? `canary:${relative(ROOT, CANARY_CANDIDATE)}` : tilde(candidatePath),
      corpora,
      summary,
      firstDivergence: divergences[0] ?? null,
      divergences,
      inconclusive,
    };
    writeFileSync(join(root, "report.json"), `${JSON.stringify(report, null, 2)}\n`);
    console.log(`report: ${tilde(join(root, "report.json"))}`);
    return { exit: summary.diverged ? 1 : summary.inconclusive ? 3 : 0, report, cases, results };
  } finally {
    release();
  }
}

// The canary: a checked corpus whose candidate is a checked wrapper around the
// reference binary that plants the divergences `canary.json` declares. The
// comparator must report exactly the declared first divergence, detect every
// planted one, and find the unplanted cases in agreement.
async function canary(options) {
  const manifest = JSON.parse(readFileSync(join(CANARY_DIR, "canary.json"), "utf8"));
  const { exit, report, cases, results } = await compare({ ...options, keepGoing: true, filters: [], limit: null });
  if (exit === 2 || !report) return { exit: exit || 1 };
  const problems = [];
  // An agreement only counts if the reference really compiled and ran the
  // case: two identical harness failures would otherwise look like a match.
  for (const row of manifest.agree) {
    const index = cases.findIndex((c) => c.id === `tests/compiler-diff/canary/${row.case}`);
    const phase = results[index]?.observed?.find((o) => o.phase === row.phase);
    if (!phase) problems.push(`agreement case ${row.case} (${row.phase}) did not match`);
    else if (phase.status !== row.status) problems.push(`agreement case ${row.case} (${row.phase}) observed ${phase.status}, declared ${row.status}`);
    else if (row.stdout !== undefined && !phase.stdout.includes(row.stdout)) problems.push(`agreement case ${row.case} (${row.phase}) stdout lacks ${JSON.stringify(row.stdout)}`);
    else if (row.code !== undefined && !phase.codes.includes(row.code)) problems.push(`agreement case ${row.case} (${row.phase}) lacks diagnostic ${row.code}`);
  }
  const first = report.firstDivergence;
  const want = manifest.first;
  if (!first) problems.push("no divergence was reported");
  else {
    for (const key of ["case", "phase", "field"]) {
      if (first[key] !== want[key]) problems.push(`first divergence ${key} is ${first[key]}, planted ${want[key]}`);
    }
    if (!first.repro || !existsSync(join(first.repro.replace(/^~/u, process.env.HOME ?? "~"), "repro.sh"))) {
      problems.push("first divergence has no repro directory");
    }
  }
  const planted = new Set(manifest.plants.map((plant) => `tests/compiler-diff/canary/${plant.case}`));
  for (const plant of manifest.plants) {
    const id = `tests/compiler-diff/canary/${plant.case}`;
    const found = report.divergences.find((record) => record.case === id);
    if (!found) problems.push(`planted divergence in ${plant.case} (${plant.phase}) was not detected`);
    else if (found.phase !== plant.phase || ![found.field, ...found.alsoDiffers].includes(plant.field)) {
      problems.push(`planted ${plant.case} ${plant.phase}/${plant.field} reported as ${found.phase}/${found.field}`);
    }
  }
  for (const record of report.divergences) {
    if (!planted.has(record.case)) problems.push(`unplanted case ${record.case} diverged (${record.phase}/${record.field})`);
  }
  if (report.summary.inconclusive) problems.push(`${report.summary.inconclusive} canary case(s) were inconclusive`);
  if (problems.length) {
    console.log(`canary: FAILED\n${problems.map((problem) => `  - ${problem}`).join("\n")}`);
    return { exit: 1 };
  }
  console.log(`canary: ok (planted first divergence ${want.case} ${want.phase}/${want.field} reported; ${manifest.plants.length} plant(s) detected; ${report.summary.matched} unplanted case(s) agree)`);
  return { exit: 0 };
}

async function main() {
  let options;
  try {
    options = parseArgs(process.argv.slice(2));
    if (options.help) { console.log(usage()); return 0; }
    const { exit } = options.canary ? await canary(options) : await compare(options);
    return exit;
  } catch (error) {
    if (error instanceof Unavailable) {
      console.error(`compiler-diff: unavailable: ${error.message}`);
      return 2;
    }
    throw error;
  }
}

if (process.argv[1] && import.meta.url === `file://${process.argv[1]}`) {
  main().then((code) => { process.exitCode = code; }, (error) => {
    console.error(error.stack ?? error.message);
    process.exitCode = 2;
  });
}
