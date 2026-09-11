#!/usr/bin/env node

/**
 * Exercise the beginner Jet path without making the path itself another
 * compiler test harness.  The default is a side-effect-free dry run.  `--run`
 * is explicit because it creates a project and invokes the compiler.
 */

import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
  accessSync,
  closeSync,
  constants as fsConstants,
  existsSync,
  fsyncSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  statSync,
  statfsSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import {
  basename,
  dirname,
  isAbsolute,
  join,
  relative,
  resolve,
} from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const SCRIPT = "scripts/agent/first-hour-workflow.mjs";
const WORKFLOW_SCHEMA = "jet.first-hour.workflow.v1";
const WORKFLOW_SCHEMA_VERSION = 1;
const RECEIPT_SCHEMA = "jet.first-hour.workflow.receipt.v1";
const REPORT_SCHEMA = "jet.first-hour.workflow.report.v1";
const SESSION_SCHEMA = "jet.first-hour.workflow.session.v1";
const DEFAULT_TIMEOUT_MS = 180_000;
const MAX_CAPTURE_BYTES = 128 * 1024;
const PROJECT_NAME = "first-hour-workflow";
const DEFAULT_SCRATCH = resolve(
  process.env.JET_FIRST_HOUR_SCRATCH
    || process.env.JET_TEST_SCRATCH
    || join(homedir(), ".cache", "jet-test-scratch"),
);
const FAILURE_CLASSES = Object.freeze([
  "environment",
  "command",
  "diagnostic",
  "state",
  "timeout",
]);
const EXPECTED_FILES = Object.freeze([
  "package.jet",
  "run.jet",
  "@run.jet",
  "@build.jet",
  "@dev.jet",
  "@test.jet",
  ".gitignore",
]);
const EDITED_SOURCE = [
  "#CLI",
  "struct GreetingArgs {",
  "    #Doc(\"name to greet\") name: String{\"Jet\"}",
  "}",
  "",
  "fn greeting(name: String) String -> \"hello from {name}\"",
  "",
  "fn run(args: GreetingArgs) { print(greeting(args.name)) }",
  "",
  "#Test(\"the greeting stays stable\") {",
  "    assert_eq(greeting(\"Jet\"), \"hello from Jet\")",
  "}",
  "",
].join("\n");
const BROKEN_SOURCE = [
  'print("before")',
  EDITED_SOURCE.trimEnd(),
  'print("after")',
  "",
].join("\n");

class WorkflowFailure extends Error {
  constructor(failureClass, message, details = {}) {
    super(message);
    this.name = "WorkflowFailure";
    this.failureClass = failureClass;
    this.details = details;
  }
}

function fail(failureClass, message, details = {}) {
  throw new WorkflowFailure(failureClass, message, details);
}


function deepFreeze(value) {
  if (!value || typeof value !== "object" || Object.isFrozen(value)) return value;
  Object.freeze(value);
  for (const child of Object.values(value)) deepFreeze(child);
  return value;
}

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, canonical(value[key])]),
    );
  }
  return value;
}

function canonicalJson(value) {
  return JSON.stringify(canonical(value));
}

function digest(value) {
  return `sha256:${createHash("sha256").update(canonicalJson(value), "utf8").digest("hex")}`;
}

function command(argv) {
  return Object.freeze([...argv]);
}

const WORKFLOW_MANIFEST = deepFreeze({
  schema: WORKFLOW_SCHEMA,
  schema_version: WORKFLOW_SCHEMA_VERSION,
  id: "first-hour",
  title: "Jet first-hour beginner workflow",
  project_name: PROJECT_NAME,
  scratch: {
    default_root: "$HOME/.cache/jet-test-scratch",
    project_directory: "project",
    receipt_directory: "receipts",
    forbidden_prefixes: ["/tmp", "/tmp/"],
    policy: "disk-backed scratch only; never place a target or scratch tree under /tmp",
  },
  output: {
    receipt_schema: RECEIPT_SCHEMA,
    report_schema: REPORT_SCHEMA,
    capture_limit_bytes: MAX_CAPTURE_BYTES,
    evidence_policy: "receipts and reports are create-only; a rerun gets a new attempt or report path",
  },
  steps: [
    {
      ordinal: 1,
      id: "new",
      title: "Create a project",
      cwd: "scratch",
      commands: [command(["jet", "new", PROJECT_NAME])],
      expected: {
        exit: 0,
        stdout_contains: [`created ${PROJECT_NAME}/`],
        files: EXPECTED_FILES.map((file) => `project/${file}`),
        retained: ["project/package.jet", "project/run.jet"],
      },
    },
    {
      ordinal: 2,
      id: "run",
      title: "Run the generated program",
      cwd: "project",
      commands: [command(["jet", "run"])],
      expected: {
        exit: 0,
        stdout_exact: "hello, world\n",
        retained: ["project/package.jet", "project/run.jet"],
      },
    },
    {
      ordinal: 3,
      id: "edit-dev",
      title: "Edit the entry and use dev",
      cwd: "project",
      edit: {
        path: "project/run.jet",
        operation: "replace",
        source_sha256: digest(EDITED_SOURCE),
        observable: "run.jet contains the edited greeting and a #Test block",
      },
      commands: [command(["jet", "dev", "run.jet", "--watch=off"])],
      expected: {
        exit: 0,
        stdout_exact: "hello from Jet\n",
        source_sha256: digest(EDITED_SOURCE),
        retained: ["project/run.jet"],
      },
    },
    {
      ordinal: 4,
      id: "diagnostic-fix",
      title: "Read a diagnostic and apply its safe fix",
      cwd: "project",
      edit: {
        path: "project/run.jet",
        operation: "replace",
        source_sha256: digest(BROKEN_SOURCE),
        observable: "loose statements deliberately conflict with the explicit run entry",
      },
      commands: [
        command(["jet", "check", "run.jet"]),
        command(["jet", "fix", "run.jet"]),
        command(["jet", "check", "run.jet"]),
      ],
      expected: {
        exit: 0,
        diagnostic_code: "E0621",
        diagnostic_contains: ["E0621", "Fix:"],
        post_fix_source: "valid",
        retained: ["project/run.jet"],
      },
    },
    {
      ordinal: 5,
      id: "test",
      title: "Run the source test",
      cwd: "project",
      commands: [command(["jet", "test", "run.jet"])],
      expected: {
        exit: 0,
        stdout_contains: ["the greeting stays stable", "pass"],
        retained: ["project/run.jet"],
      },
    },
    {
      ordinal: 6,
      id: "package",
      title: "Build and package the executable",
      cwd: "project",
      commands: [
        command(["jet", "build"]),
        command([
          "jet",
          "package",
          "--target",
          "linux-appimage",
          "--executable",
          "build/run",
          "--package",
          PROJECT_NAME,
          "--name",
          PROJECT_NAME,
          "--version",
          "0.1.0",
        ]),
      ],
      expected: {
        exit: 0,
        artifact: "project/build/run",
        stdout_contains: ["target: linux-appimage", "artifact:", "receipt:"],
        retained: ["project/build/run"],
      },
    },
  ],
});
const MANIFEST_DIGEST = digest(WORKFLOW_MANIFEST);

function usage() {
  return [
    `usage: node ${SCRIPT} [--help|--check|--dry-run|--run] [options]`,
    "",
    "Run the deterministic Jet beginner path. Dry-run is the default; --run is explicit.",
    "",
    "steps: jet new -> run -> edit/dev -> diagnostic/fix -> test -> package",
    "",
    "options:",
    "  --help                 show this help without invoking Jet",
    "  --check                validate the frozen manifest/schema without invoking Jet",
    "  --dry-run              print the ordered commands without creating scratch",
    "  --run                  execute the workflow in disk-backed scratch",
    "  --scratch PATH         disk-backed scratch root (default: $HOME/.cache/jet-test-scratch)",
    "  --resume PATH          resume an existing run directory; implies --run",
    "  --keep                 retain the project directory after a successful run",
    "  --timeout MS           per-command timeout (default: 180000)",
    "",
    "Receipts are immutable JSON files under <run>/receipts; reports use a new path on every run.",
  ].join("\n");
}

function parseArgs(argv) {
  const options = {
    mode: "dry-run",
    scratch: DEFAULT_SCRATCH,
    resume: null,
    keep: false,
    timeoutMs: DEFAULT_TIMEOUT_MS,
    help: false,
  };
  let modeSeen = false;
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help" || arg === "-h") {
      options.help = true;
      continue;
    }
    if (["--check", "--dry-run", "--run"].includes(arg)) {
      const mode = arg.slice(2);
      if (modeSeen && options.mode !== mode) {
        throw new WorkflowFailure("state", "choose exactly one of --check, --dry-run, or --run");
      }
      options.mode = mode;
      modeSeen = true;
      continue;
    }
    if (arg === "--keep") {
      options.keep = true;
      continue;
    }
    if (["--scratch", "--resume", "--timeout"].includes(arg)) {
      const value = argv[++index];
      if (!value || value.startsWith("--")) {
        throw new WorkflowFailure("state", `${arg} needs a value`);
      }
      if (arg === "--scratch") options.scratch = resolve(value);
      else if (arg === "--resume") {
        options.resume = resolve(value);
        if (!modeSeen) {
          options.mode = "run";
          modeSeen = true;
        }
      } else options.timeoutMs = parseTimeout(value);
      continue;
    }
    if (arg.startsWith("--scratch=")) {
      options.scratch = resolve(arg.slice("--scratch=".length));
      continue;
    }
    if (arg.startsWith("--resume=")) {
      options.resume = resolve(arg.slice("--resume=".length));
      if (!modeSeen) {
        options.mode = "run";
        modeSeen = true;
      }
      continue;
    }
    if (arg.startsWith("--timeout=")) {
      options.timeoutMs = parseTimeout(arg.slice("--timeout=".length));
      continue;
    }
    throw new WorkflowFailure("state", `unknown option ${arg}`);
  }
  if (options.help) return options;
  if (options.resume && options.mode !== "run") {
    throw new WorkflowFailure("state", "--resume requires --run");
  }
  if (options.keep && options.mode !== "run") {
    throw new WorkflowFailure("state", "--keep requires --run");
  }
  return options;
}

function parseTimeout(value) {
  if (!/^\d+$/.test(value)) throw new WorkflowFailure("state", "--timeout must be a positive integer in milliseconds");
  const timeout = Number(value);
  if (!Number.isSafeInteger(timeout) || timeout < 1 || timeout > 3_600_000) {
    throw new WorkflowFailure("state", "--timeout must be between 1 and 3600000 milliseconds");
  }
  return timeout;
}

function validateManifest(manifest = WORKFLOW_MANIFEST) {
  if (!manifest || manifest.schema !== WORKFLOW_SCHEMA || manifest.schema_version !== WORKFLOW_SCHEMA_VERSION) {
    throw new Error("workflow manifest schema is unsupported");
  }
  if (manifest.id !== "first-hour" || manifest.project_name !== PROJECT_NAME) {
    throw new Error("workflow manifest identity drifted");
  }
  if (!Array.isArray(manifest.steps) || manifest.steps.length !== 6) {
    throw new Error("workflow manifest must contain six ordered steps");
  }
  const ids = ["new", "run", "edit-dev", "diagnostic-fix", "test", "package"];
  const seen = new Set();
  for (const [index, step] of manifest.steps.entries()) {
    if (step.ordinal !== index + 1 || step.id !== ids[index] || seen.has(step.id)) {
      throw new Error("workflow step order or identity drifted");
    }
    seen.add(step.id);
    if (!step.title || !["scratch", "project"].includes(step.cwd)) {
      throw new Error(`workflow step ${step.id} has invalid identity`);
    }
    if (!Array.isArray(step.commands) || step.commands.length === 0) {
      throw new Error(`workflow step ${step.id} has no commands`);
    }
    for (const argv of step.commands) {
      if (!Array.isArray(argv) || argv.length < 2 || argv[0] !== "jet" || argv.some((part) => typeof part !== "string" || !part)) {
        throw new Error(`workflow step ${step.id} has an invalid command`);
      }
    }
    if (!step.expected || step.expected.exit !== 0) {
      throw new Error(`workflow step ${step.id} has no success expectation`);
    }
    if (!Array.isArray(step.expected.retained)) {
      throw new Error(`workflow step ${step.id} has no retained-state expectation`);
    }
  }
  const diagnostic = manifest.steps.find((step) => step.id === "diagnostic-fix");
  if (diagnostic.expected.diagnostic_code !== "E0621"
    || !diagnostic.expected.diagnostic_contains.includes("Fix:")) {
    throw new Error("diagnostic/fix expectation drifted");
  }
  const packageStep = manifest.steps.find((step) => step.id === "package");
  if (packageStep.expected.artifact !== "project/build/run") {
    throw new Error("package artifact expectation drifted");
  }
  return manifest;
}

function checkManifest() {
  const manifest = validateManifest();
  if (digest(manifest) !== MANIFEST_DIGEST) throw new Error("workflow manifest digest drifted");
  return manifest;
}

function isTmpPath(path) {
  const normalized = resolve(path);
  return normalized === "/tmp" || normalized.startsWith("/tmp/");
}

function nearestExisting(path) {
  let current = resolve(path);
  while (!existsSync(current)) {
    const parent = dirname(current);
    if (parent === current) return current;
    current = parent;
  }
  return current;
}

function diskBacked(path) {
  const requested = resolve(path);
  if (isTmpPath(requested)) return { ok: false, reason: "path is under RAM-backed /tmp" };
  const existing = nearestExisting(requested);
  let actual;
  try {
    actual = realpathSync(existing);
  } catch (error) {
    return { ok: false, reason: `cannot resolve scratch parent: ${error.message}` };
  }
  if (isTmpPath(actual)) return { ok: false, reason: "resolved scratch parent is under RAM-backed /tmp" };
  try {
    const stats = statfsSync(actual);
    const type = Number(stats.type);
    if (type === 0x01021994) return { ok: false, reason: "filesystem is tmpfs" };
  } catch (error) {
    return { ok: false, reason: `cannot inspect scratch filesystem: ${error.message}` };
  }
  if (existsSync(requested) && lstatSync(requested).isSymbolicLink()) {
    return { ok: false, reason: "scratch root must not be a symlink" };
  }
  return { ok: true, path: requested, filesystem: actual };
}

function validateScratch(path) {
  const result = diskBacked(path);
  if (!result.ok) fail("environment", `scratch path is not disk-backed: ${path} (${result.reason})`, { path });
  return result.path;
}

function executableOnPath(name) {
  if (name.includes("/") || isAbsolute(name)) {
    try {
      accessSync(name, fsConstants.X_OK);
      return resolve(name);
    } catch {
      return null;
    }
  }
  for (const directory of (process.env.PATH || "").split(":").filter(Boolean)) {
    const candidate = join(directory, name);
    try {
      accessSync(candidate, fsConstants.X_OK);
      return candidate;
    } catch {
      // Continue through PATH in deterministic order.
    }
  }
  return null;
}

function launcher() {
  if (process.env.JET_BIN) {
    const path = executableOnPath(process.env.JET_BIN);
    if (!path) fail("environment", `JET_BIN is not executable: ${process.env.JET_BIN}`);
    return { program: path, prefix: [] };
  }
  const jetEnv = join(ROOT, "scripts", "agent", "jet-env");
  if (!executableOnPath(jetEnv)) fail("environment", `missing Jet launcher: ${jetEnv}`);
  return { program: jetEnv, prefix: ["jet"] };
}

function commandText(argv) {
  return argv.map((part) => (/^[A-Za-z0-9_./:@=-]+$/.test(part) ? part : JSON.stringify(part))).join(" ");
}

function pathLabel(path, runDir) {
  const value = relative(runDir, path).split("\\").join("/");
  return value || ".";
}

function now() {
  return new Date().toISOString();
}

function roundMs(value) {
  return Math.round(value * 10) / 10;
}

function resourceSnapshot(path) {
  const usage = process.resourceUsage();
  let disk = null;
  try {
    const stats = statfsSync(path);
    disk = {
      free_bytes: Number(stats.bavail) * Number(stats.bsize),
      total_bytes: Number(stats.blocks) * Number(stats.bsize),
    };
  } catch {
    disk = null;
  }
  return {
    captured_at: now(),
    process: {
      user_cpu_us: usage.userCPUTime,
      system_cpu_us: usage.systemCPUTime,
      max_rss_bytes: usage.maxRSS * 1024,
    },
    disk,
  };
}

function appendCapture(state, chunk) {
  const bytes = Buffer.byteLength(chunk, "utf8");
  state.bytes += bytes;
  if (state.text.length < MAX_CAPTURE_BYTES) {
    const remaining = MAX_CAPTURE_BYTES - Buffer.byteLength(state.text, "utf8");
    state.text += Buffer.from(chunk, "utf8").subarray(0, remaining).toString("utf8");
  }
  if (state.bytes > MAX_CAPTURE_BYTES) state.truncated = true;
}

function killChild(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  try {
    if (process.platform !== "win32" && child.pid) process.kill(-child.pid, "SIGKILL");
  } catch {
    // The direct child may be the only process left or may have exited.
  }
  try { child.kill("SIGKILL"); } catch { /* already exited */ }
}

function runCommand(argv, cwd, timeoutMs, env, resourcePath, launch) {
  const startedAt = now();
  const started = process.hrtime.bigint();
  const stdout = { text: "", bytes: 0, truncated: false };
  const stderr = { text: "", bytes: 0, truncated: false };
  const before = resourceSnapshot(resourcePath);
  const actualArgs = [...launch.prefix, ...argv.slice(1)];
  return new Promise((resolveResult) => {
    let timedOut = false;
    let spawnError = null;
    let timer = null;
    let child;
    try {
      child = spawn(launch.program, actualArgs, {
        cwd,
        env,
        detached: process.platform !== "win32",
        stdio: ["ignore", "pipe", "pipe"],
      });
    } catch (error) {
      resolveResult({
        logical_command: argv,
        actual_command: [launch.program, ...actualArgs],
        cwd,
        started_at: startedAt,
        finished_at: now(),
        duration_ms: roundMs(Number(process.hrtime.bigint() - started) / 1_000_000),
        exit: null,
        signal: null,
        timeout: false,
        spawn_error: error.message,
        stdout,
        stderr,
        resources: { before, after: resourceSnapshot(resourcePath) },
      });
      return;
    }
    child.stdout.on("data", (chunk) => appendCapture(stdout, chunk));
    child.stderr.on("data", (chunk) => appendCapture(stderr, chunk));
    child.on("error", (error) => { spawnError = error; });
    timer = setTimeout(() => {
      timedOut = true;
      killChild(child);
    }, timeoutMs);
    child.on("close", (exit, signal) => {
      clearTimeout(timer);
      resolveResult({
        logical_command: argv,
        actual_command: [launch.program, ...actualArgs],
        cwd,
        started_at: startedAt,
        finished_at: now(),
        duration_ms: roundMs(Number(process.hrtime.bigint() - started) / 1_000_000),
        exit,
        signal,
        timeout: timedOut,
        spawn_error: spawnError?.message || null,
        stdout,
        stderr,
        resources: { before, after: resourceSnapshot(resourcePath) },
      });
    });
  });
}

function textContains(text, needles) {
  return needles.every((needle) => text.includes(needle));
}

function normalizeSource(path) {
  try { return readFileSync(path, "utf8"); } catch { return null; }
}

function checkFiles(project, files) {
  return files.map((relativePath) => ({
    path: relativePath,
    present: existsSync(join(project, relativePath)),
  }));
}

function checkRetainedState(step, project, runDir) {
  const retained = step.expected.retained.map((value) => ({
    path: value,
    present: existsSync(join(runDir, value)),
  }));
  const missing = retained.filter((item) => !item.present).map((item) => item.path);
  if (missing.length) fail("state", `step ${step.id} lost retained state: ${missing.join(", ")}`, { missing });
  if (step.id === "new") {
    const missingFiles = checkFiles(project, EXPECTED_FILES).filter((item) => !item.present).map((item) => item.path);
    if (missingFiles.length) fail("state", `scaffold is missing: ${missingFiles.join(", ")}`, { missing: missingFiles });
  }
  if (step.id === "edit-dev") {
    const source = normalizeSource(join(project, "run.jet"));
    if (source === null || digest(source) !== step.edit.source_sha256) {
      fail("state", "edited run.jet no longer matches the frozen source receipt");
    }
  }
  if (step.id === "diagnostic-fix") {
    const source = normalizeSource(join(project, "run.jet"));
    if (source === null || !source.includes("#Test") || source.includes("pirnt")) {
      fail("state", "fixed run.jet is not retained");
    }
  }
  return retained;
}

function expectedCommandFailure(step, index, observation) {
  if (observation.timeout) fail("timeout", `step ${step.id} command ${index + 1} timed out`, { observation });
  if (observation.spawn_error) fail("environment", `step ${step.id} could not start: ${observation.spawn_error}`, { observation });
  const text = `${observation.stdout.text}\n${observation.stderr.text}`;
  if (step.id === "diagnostic-fix" && index === 0) {
    if (observation.exit === 0 || !textContains(text, step.expected.diagnostic_contains)) {
      fail("diagnostic", `step ${step.id} did not report ${step.expected.diagnostic_code} with a Fix`, { observation });
    }
    return;
  }
  if (observation.exit !== step.expected.exit || observation.signal) {
    const failureClass = step.id === "diagnostic-fix" ? "diagnostic" : "command";
    fail(failureClass, `step ${step.id} command ${index + 1} exited unexpectedly`, { observation });
  }
}

function verifyCommand(step, index, observation, project) {
  expectedCommandFailure(step, index, observation);
  const output = observation.stdout.text;
  if (step.id === "run" && output !== step.expected.stdout_exact) {
    fail("state", `step run output differed from expected ${JSON.stringify(step.expected.stdout_exact)}`, { observation });
  }
  if (step.id === "edit-dev" && output !== step.expected.stdout_exact) {
    fail("state", `step edit-dev output differed from expected ${JSON.stringify(step.expected.stdout_exact)}`, { observation });
  }
  if (step.id === "test" && !textContains(output, step.expected.stdout_contains)) {
    fail("state", "test output did not name the passing test", { observation });
  }
  if (step.id === "package" && index === 1 && !textContains(output, step.expected.stdout_contains)) {
    fail("state", "package output did not expose target, artifact, and receipt", { observation });
  }
  if (step.id === "package" && index === 0 && !existsSync(join(project, "build", "run"))) {
    fail("state", "jet build succeeded without retaining project/build/run", { observation });
  }
}

function writeSource(step, project) {
  if (!step.edit) return null;
  const projectTarget = join(project, step.edit.path.replace(/^project\//u, ""));
  const source = step.id === "edit-dev" ? EDITED_SOURCE : BROKEN_SOURCE;
  try {
    writeFileSync(projectTarget, source, { mode: 0o600 });
  } catch (error) {
    fail("state", `could not write ${step.edit.path}: ${error.message}`);
  }
  const observed = normalizeSource(projectTarget);
  if (observed === null || digest(observed) !== step.edit.source_sha256) {
    fail("state", `edit receipt for ${step.id} does not match the source bytes`);
  }
  return {
    path: step.edit.path,
    operation: step.edit.operation,
    source_sha256: digest(observed),
  };
}

function safeJson(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

function writeExclusive(path, value) {
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  const fd = openSync(path, "wx", 0o600);
  try {
    writeFileSync(fd, typeof value === "string" ? value : safeJson(value));
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
}

function receiptCandidates(runDir, step) {
  const directory = join(runDir, "receipts");
  if (!existsSync(directory)) return [];
  const base = `${String(step.ordinal).padStart(2, "0")}-${step.id}`;
  const pattern = new RegExp(`^${base.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&")}(?:\\.attempt-(\\d+))?\\.json$`, "u");
  return readdirSync(directory)
    .filter((name) => pattern.test(name))
    .sort((left, right) => {
      const leftAttempt = Number(left.match(/\.attempt-(\d+)\.json$/u)?.[1] || 1);
      const rightAttempt = Number(right.match(/\.attempt-(\d+)\.json$/u)?.[1] || 1);
      return leftAttempt - rightAttempt || left.localeCompare(right);
    });
}

function readReceipt(path) {
  let value;
  try {
    value = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail("state", `cannot read receipt ${path}: ${error.message}`);
  }
  if (!value || value.schema !== RECEIPT_SCHEMA || value.schema_version !== 1
    || !value.workflow_schema || !value.manifest_digest || !value.run_id
    || !Number.isSafeInteger(value.ordinal) || value.ordinal < 1
    || !Number.isSafeInteger(value.attempt) || value.attempt < 1
    || !value.step_id || !["passed", "failed"].includes(value.status)) {
    fail("state", `receipt has invalid schema: ${path}`);
  }
  if (!Object.hasOwn(value, "failure_class")
    || (value.status === "passed" && value.failure_class !== null)
    || (value.status === "failed" && !FAILURE_CLASSES.includes(value.failure_class))) {
    fail("state", `receipt has invalid failure class: ${path}`);
  }
  return value;
}

function latestReceipt(runDir, step) {
  const candidates = receiptCandidates(runDir, step);
  if (candidates.length === 0) return null;
  const path = join(runDir, "receipts", candidates.at(-1));
  const value = readReceipt(path);
  if (value.workflow_schema !== WORKFLOW_SCHEMA
    || value.manifest_digest !== MANIFEST_DIGEST
    || value.run_id !== basename(runDir)
    || value.ordinal !== step.ordinal
    || value.step_id !== step.id) {
    fail("state", `receipt does not match step ${step.id}: ${path}`);
  }
  return { path, value, candidates };
}

function receiptPath(runDir, step, attempt) {
  const base = `${String(step.ordinal).padStart(2, "0")}-${step.id}`;
  return join(runDir, "receipts", attempt === 1 ? `${base}.json` : `${base}.attempt-${attempt}.json`);
}

function writeReceipt(runDir, step, attempt, receipt) {
  const path = receiptPath(runDir, step, attempt);
  try {
    writeExclusive(path, receipt);
  } catch (error) {
    if (error.code === "EEXIST") fail("state", `refusing to overwrite receipt ${path}`);
    fail("state", `could not write receipt ${path}: ${error.message}`);
  }
  return path;
}

function relativeArtifact(path, runDir) {
  return pathLabel(path, runDir);
}

function makeSession(runDir, scratchRoot) {
  const session = {
    schema: SESSION_SCHEMA,
    schema_version: 1,
    workflow_schema: WORKFLOW_SCHEMA,
    manifest_digest: MANIFEST_DIGEST,
    run_id: basename(runDir),
    created_at: now(),
    scratch_root: resolve(scratchRoot),
    run_directory: resolve(runDir),
    project_directory: "project",
    receipt_directory: "receipts",
  };
  try {
    writeExclusive(join(runDir, "session.json"), session);
  } catch (error) {
    fail("state", `could not create workflow session: ${error.message}`);
  }
  return session;
}

function loadSession(runDir) {
  const path = join(runDir, "session.json");
  if (!existsSync(path)) fail("state", `resume directory has no session.json: ${runDir}`);
  let session;
  try { session = JSON.parse(readFileSync(path, "utf8")); } catch (error) {
    fail("state", `cannot read workflow session: ${error.message}`);
  }
  if (!session || session.schema !== SESSION_SCHEMA || session.schema_version !== 1
    || session.workflow_schema !== WORKFLOW_SCHEMA || session.manifest_digest !== MANIFEST_DIGEST
    || session.run_directory !== runDir) {
    fail("state", "resume session does not match the frozen workflow manifest");
  }
  return session;
}

function environmentFor(runDir) {
  const scratch = resolve(runDir);
  return {
    ...process.env,
    NO_COLOR: "1",
    TERM: "dumb",
    JET_TEST_SCRATCH: scratch,
    JET_TEST_SCRATCH_DIR: scratch,
    JET_DEV_ORACLE_CACHE_DIR: join(scratch, "oracle-cache"),
    TMPDIR: scratch,
    TMP: scratch,
    TEMP: scratch,
  };
}

function summarizeReceipt(receipt, path, runDir) {
  return {
    ordinal: receipt.ordinal,
    id: receipt.step_id,
    title: receipt.title,
    status: receipt.status,
    resumed: false,
    attempt: receipt.attempt,
    duration_ms: receipt.duration_ms,
    failure_class: receipt.failure_class,
    receipt: relativeArtifact(path, runDir),
  };
}

async function runStep(step, context, attempt) {
  const startedAt = now();
  const started = process.hrtime.bigint();
  const resourcesBefore = resourceSnapshot(context.runDir);
  let mutation = null;
  const observations = [];
  let failure = null;
  try {
    if (step.edit) mutation = writeSource(step, context.project);
    for (const [index, argv] of step.commands.entries()) {
      const cwd = step.cwd === "scratch" ? context.runDir : context.project;
      const observation = await runCommand(
        argv,
        cwd,
        context.timeoutMs,
        context.environment,
        context.runDir,
        context.launch,
      );
      observations.push({
        index: index + 1,
        logical_command: argv,
        ...observation,
        cwd: pathLabel(cwd, context.runDir),
      });
      verifyCommand(step, index, observation, context.project);
      if (step.id === "diagnostic-fix" && index === 1) {
        checkRetainedState(step, context.project, context.runDir);
      }
    }
    checkRetainedState(step, context.project, context.runDir);
  } catch (error) {
    failure = error instanceof WorkflowFailure
      ? error
      : new WorkflowFailure("state", error instanceof Error ? error.message : String(error));
  }
  const finishedAt = now();
  const receipt = {
    schema: RECEIPT_SCHEMA,
    schema_version: 1,
    workflow_schema: WORKFLOW_SCHEMA,
    manifest_digest: MANIFEST_DIGEST,
    run_id: context.session.run_id,
    ordinal: step.ordinal,
    step_id: step.id,
    title: step.title,
    attempt,
    status: failure ? "failed" : "passed",
    resumed: false,
    started_at: startedAt,
    finished_at: finishedAt,
    duration_ms: roundMs(Number(process.hrtime.bigint() - started) / 1_000_000),
    failure_class: failure?.failureClass || null,
    failure: failure ? {
      message: failure.message,
      details: failure.details,
    } : null,
    mutation,
    commands: observations,
    retained_state: step.expected.retained.map((path) => ({
      path,
      present: existsSync(join(context.runDir, path)),
    })),
    resources: {
      before: resourcesBefore,
      after: resourceSnapshot(context.runDir),
    },
  };
  const path = writeReceipt(context.runDir, step, attempt, receipt);
  return { receipt, path, failure };
}

function resumedSummary(step, prior, runDir) {
  const retained = checkRetainedState(step, join(runDir, "project"), runDir);
  return {
    ordinal: step.ordinal,
    id: step.id,
    title: step.title,
    status: "resumed",
    resumed: true,
    attempt: prior.value.attempt,
    duration_ms: prior.value.duration_ms,
    failure_class: null,
    receipt: relativeArtifact(prior.path, runDir),
    retained_state: retained,
  };
}

function cleanupProject(project, keep, failed) {
  if (keep || failed) {
    return {
      requested: true,
      project_retained: true,
      project_removed: false,
      reason: failed ? "retained for --resume after failure" : "retained by --keep",
      complete: true,
    };
  }
  try {
    rmSync(project, { recursive: true, force: true });
  } catch (error) {
    return {
      requested: true,
      project_retained: existsSync(project),
      project_removed: false,
      reason: `cleanup failed: ${error.message}`,
      complete: false,
    };
  }
  return {
    requested: true,
    project_retained: existsSync(project),
    project_removed: !existsSync(project),
    reason: "successful workflow cleanup",
    complete: !existsSync(project),
  };
}

function uniqueReportPath(runDir) {
  for (let attempt = 1; attempt < 10_000; attempt += 1) {
    const name = attempt === 1 ? "report.json" : `report-${attempt}.json`;
    const path = join(runDir, name);
    if (!existsSync(path)) return path;
  }
  fail("state", `could not allocate a unique report path in ${runDir}`);
}

function buildReport(context, startedAt, summaries, failures, cleanup) {
  return {
    schema: REPORT_SCHEMA,
    schema_version: 1,
    workflow_schema: WORKFLOW_SCHEMA,
    manifest_digest: MANIFEST_DIGEST,
    run_id: context.session.run_id,
    status: failures.length === 0 ? "PASS" : "FAIL",
    started_at: startedAt,
    finished_at: now(),
    duration_ms: roundMs(Number(process.hrtime.bigint() - context.started) / 1_000_000),
    paths: {
      run_directory: context.runDir,
      project_directory: join(context.runDir, "project"),
      receipt_directory: join(context.runDir, "receipts"),
    },
    steps: summaries,
    failures,
    cleanup,
    resources: {
      before: context.resourcesBefore,
      after: resourceSnapshot(context.runDir),
    },
  };
}

function writeReport(runDir, report) {
  const path = uniqueReportPath(runDir);
  try { writeExclusive(path, report); } catch (error) {
    fail("state", `could not write report ${path}: ${error.message}`);
  }
  return path;
}

function renderDryRun() {
  const manifest = checkManifest();
  const lines = [
    "first-hour-workflow: DRY RUN",
    `schema: ${manifest.schema}`,
    `manifest: ${MANIFEST_DIGEST}`,
    "scratch: <disk-backed scratch root> (not created)",
    "",
  ];
  for (const step of manifest.steps) {
    lines.push(`${step.ordinal}. ${step.title} [${step.id}]`);
    lines.push(`   cwd: ${step.cwd}`);
    if (step.edit) lines.push(`   edit: ${step.edit.operation} ${step.edit.path}`);
    for (const argv of step.commands) lines.push(`   $ ${commandText(argv)}`);
    if (step.id === "diagnostic-fix") lines.push("   expect: check reports E0621 with Fix:, fix succeeds, check is clean");
    else if (step.id === "new") lines.push(`   expect: creates project/${EXPECTED_FILES.join(", project/")}`);
    else if (step.id === "package") lines.push("   expect: build/run exists; package reports linux-appimage and receipt");
    else lines.push("   expect: exit 0 and the recorded output/state fact");
  }
  lines.push("", "--run is required to invoke Jet.");
  return lines.join("\n");
}

function renderRun(report, reportPath) {
  const lines = [`first-hour-workflow: ${report.status}`, `run: ${report.run_id}`];
  for (const step of report.steps) {
    const suffix = step.failure_class ? ` [${step.failure_class}]` : "";
    lines.push(`  ${step.ordinal}. ${step.id}: ${step.status}${suffix}`);
  }
  for (const failure of report.failures) {
    lines.push(`failure: ${failure.step_id} [${failure.class}] ${failure.message}`);
  }
  lines.push(`report: ${reportPath}`);
  lines.push(`cleanup: ${report.cleanup.complete ? "complete" : "incomplete"}`);
  return lines.join("\n");
}

async function runWorkflow(options) {
  checkManifest();
  const scratchRoot = options.resume ? null : validateScratch(options.scratch);
  if (process.env.CARGO_TARGET_DIR && isTmpPath(process.env.CARGO_TARGET_DIR)) {
    fail("environment", "CARGO_TARGET_DIR is under RAM-backed /tmp; refusing compiler workload");
  }
  const launch = launcher();
  let runDir;
  let session;
  if (options.resume) {
    runDir = resolve(options.resume);
    if (!existsSync(runDir) || !statSync(runDir).isDirectory()) fail("state", `resume path is not a directory: ${runDir}`);
    session = loadSession(runDir);
    if (!diskBacked(runDir).ok) fail("environment", `resume path is not disk-backed: ${runDir}`);
  } else {
    mkdirSync(scratchRoot, { recursive: true, mode: 0o700 });
    runDir = mkdtempSync(join(scratchRoot, "first-hour-"));
    session = makeSession(runDir, scratchRoot);
  }
  const project = join(runDir, "project");
  const context = {
    runDir,
    project,
    session,
    launch,
    timeoutMs: options.timeoutMs,
    environment: environmentFor(runDir),
    started: process.hrtime.bigint(),
    resourcesBefore: resourceSnapshot(runDir),
  };
  const startedAt = now();
  const summaries = [];
  const failures = [];
  let failed = false;
  for (const step of WORKFLOW_MANIFEST.steps) {
    const prior = latestReceipt(runDir, step);
    if (prior?.value.status === "passed") {
      summaries.push(resumedSummary(step, prior, runDir));
      continue;
    }
    const attempt = (prior?.value.attempt || 0) + 1;
    const result = await runStep(step, context, attempt);
    summaries.push(summarizeReceipt(result.receipt, result.path, runDir));
    if (result.failure) {
      failed = true;
      failures.push({
        step_id: step.id,
        ordinal: step.ordinal,
        class: result.failure.failureClass,
        message: result.failure.message,
        details: result.failure.details,
      });
      break;
    }
  }
  const cleanup = cleanupProject(project, options.keep, failed);
  if (!cleanup.complete) {
    failures.push({
      step_id: null,
      ordinal: null,
      class: "state",
      message: cleanup.reason,
      details: {},
    });
  }
  const report = buildReport(context, startedAt, summaries, failures, cleanup);
  const reportPath = writeReport(runDir, report);
  process.stdout.write(`${renderRun(report, reportPath)}\n`);
  return report.status === "PASS" ? 0 : 1;
}

export {
  BROKEN_SOURCE,
  EDITED_SOURCE,
  FAILURE_CLASSES,
  MANIFEST_DIGEST,
  REPORT_SCHEMA,
  RECEIPT_SCHEMA,
  WORKFLOW_MANIFEST,
  WORKFLOW_SCHEMA,
  checkManifest,
  parseArgs,
  validateManifest,
};

export async function main(argv = process.argv.slice(2)) {
  let options;
  try {
    options = parseArgs(argv);
  } catch (error) {
    const failure = error instanceof WorkflowFailure
      ? error
      : new WorkflowFailure("state", error instanceof Error ? error.message : String(error));
    process.stderr.write(`first-hour-workflow: ${failure.failureClass} failure: ${failure.message}\n`);
    process.stderr.write(`${usage()}\n`);
    return 2;
  }
  if (options.help) {
    process.stdout.write(`${usage()}\n`);
    return 0;
  }
  try {
    if (options.mode === "check") {
      checkManifest();
      process.stdout.write(`first-hour-workflow: manifest valid (${WORKFLOW_MANIFEST.steps.length} steps)\n`);
      return 0;
    }
    if (options.mode === "dry-run") {
      if (options.scratch) {
        const result = diskBacked(options.scratch);
        if (!result.ok) fail("environment", `scratch path is not disk-backed: ${options.scratch} (${result.reason})`);
      }
      process.stdout.write(`${renderDryRun()}\n`);
      return 0;
    }
    return await runWorkflow(options);
  } catch (error) {
    const failure = error instanceof WorkflowFailure
      ? error
      : new WorkflowFailure("state", error instanceof Error ? error.message : String(error));
    process.stderr.write(`first-hour-workflow: ${failure.failureClass} failure: ${failure.message}\n`);
    return 1;
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().then((code) => { process.exitCode = code; });
}
