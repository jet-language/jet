#!/usr/bin/env node
import { createHash } from "node:crypto";
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { spawn } from "node:child_process";
import { homedir } from "node:os";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const SELFHOST_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SELFHOST_DIR, "../../..");
const MANIFEST_PATH = join(SELFHOST_DIR, "manifest.json");
const MANIFEST = JSON.parse(readFileSync(MANIFEST_PATH, "utf8"));
const JET_ENV = join(REPO_ROOT, "scripts/agent/jet-env");
const TIME_BIN = "/usr/bin/time";
const SCRATCH_ROOT = resolve(
  process.env.JET_SELFHOST_SCRATCH ?? join(homedir(), ".cache/jet-luna/selfhost"),
);
const ALLOWED_PHASES = new Set(["identity", "complexity", "check", "build", "run", "repair", "reasoning", "all"]);
const ALLOWED_MODES = new Set(["check", "run", "run-interpret", "build", "run-release"]);
const REASONING_TASKS = ["explain", "predict", "modify", "derive"];
function usage() {
  return [
    "usage: node tools/agent-eval/selfhost/run.mjs [options]",
    "",
    "  --list                    list the eight isolated workload contracts",
    "  --all                     execute every requested phase",
    "  --workload ID             select one workload (repeatable)",
    "  --phase NAME              identity|complexity|check|build|run|repair|reasoning|all",
    "  --mode NAME               check|run|run-interpret|build|run-release (for build/run)",
    "  --reasoning-file FILE     consume a captured reasoning protocol JSON",
    "  --output FILE             write the JSON receipt as well as stdout",
    "  --help                    show this help",
    "",
    "No command is run unless --all or an execution phase is selected. Unrun and",
    "unavailable results stay visible and never become readiness evidence.",
  ].join("\n");
}

function parseArgs(argv) {
  const options = {
    list: false,
    all: false,
    workloads: [],
    phase: null,
    mode: null,
    reasoningFile: null,
    output: null,
    help: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--list") {
      options.list = true;
    } else if (arg === "--all") {
      options.all = true;
    } else if (arg === "--help" || arg === "-h") {
      options.help = true;
    } else if (arg === "--workload" || arg === "--phase" || arg === "--mode" || arg === "--reasoning-file" || arg === "--output") {
      const value = argv[index + 1];
      if (!value || value.startsWith("--")) {
        throw new Error(`${arg} requires a value`);
      }
      index += 1;
      if (arg === "--workload") options.workloads.push(value);
      if (arg === "--phase") options.phase = value;
      if (arg === "--mode") options.mode = value;
      if (arg === "--reasoning-file") options.reasoningFile = value;
      if (arg === "--output") options.output = value;
    } else {
      throw new Error(`unknown option: ${arg}`);
    }
  }
  if (options.phase && !ALLOWED_PHASES.has(options.phase)) {
    throw new Error(`unknown phase: ${options.phase}`);
  }
  if (options.mode && !ALLOWED_MODES.has(options.mode)) {
    throw new Error(`unknown mode: ${options.mode}`);
  }
  if (options.workloads.length > 0) {
    const known = new Set(MANIFEST.workloads.map((workload) => workload.id));
    for (const id of options.workloads) {
      if (!known.has(id)) throw new Error(`unknown workload: ${id}`);
    }
  }
  return options;
}

function listWorkloads() {
  for (const workload of MANIFEST.workloads) {
    const modes = workload.supportedModes.join(",");
    console.log(`${workload.id}\t${workload.title}\tmodes=${modes}`);
  }
}

function sha256Bytes(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function sha256Text(text) {
  return sha256Bytes(Buffer.from(text, "utf8"));
}

function normalizePath(filePath) {
  return filePath.split(sep).join("/");
}
function commandIdentity(command) {
  if (!command.includes(sep) || !existsSync(command)) return { path: command, sha256: null, bytes: null };
  const bytes = readFileSync(command);
  return { path: normalizePath(command), sha256: sha256Bytes(bytes), bytes: bytes.length };
}

function copyTree(source, destination) {
  const stat = lstatSync(source);
  if (stat.isSymbolicLink()) {
    throw new Error(`symlink is not an isolated fixture input: ${source}`);
  }
  if (stat.isDirectory()) {
    mkdirSync(destination, { recursive: true });
    for (const name of readdirSync(source)) {
      copyTree(join(source, name), join(destination, name));
    }
    return;
  }
  mkdirSync(dirname(destination), { recursive: true });
  writeFileSync(destination, readFileSync(source));
}

function collectFiles(root, current = root) {
  const files = [];
  for (const name of readdirSync(current)) {
    const filePath = join(current, name);
    const stat = lstatSync(filePath);
    if (stat.isSymbolicLink()) {
      throw new Error(`symlink is not an isolated fixture input: ${filePath}`);
    }
    if (stat.isDirectory()) {
      files.push(...collectFiles(root, filePath));
    } else {
      files.push(filePath);
    }
  }
  return files.sort();
}

function identityForDirectory(directory) {
  const files = collectFiles(directory).map((filePath) => {
    const relativePath = normalizePath(relative(directory, filePath));
    const bytes = readFileSync(filePath);
    return {
      path: relativePath,
      bytes: bytes.length,
      sha256: sha256Bytes(bytes),
    };
  });
  const aggregate = sha256Text(files.map((file) => `${file.path}\0${file.sha256}\0${file.bytes}`).join("\n"));
  return { algorithm: "sha256", aggregate, files };
}

function complexityForDirectory(directory) {
  const files = collectFiles(directory)
    .filter((filePath) => filePath.endsWith(".jet"))
    .map((filePath) => {
      const relativePath = normalizePath(relative(directory, filePath));
      const source = readFileSync(filePath, "utf8");
      const lines = source.length === 0 ? 0 : source.split("\n").length;
      const lineList = source.split("\n");
      const comments = lineList.filter((line) => /^\s*\/\//.test(line)).length;
      const declarations = (source.match(/^\s*(?:pub\s+)?(?:fn|struct|enum|module|impl|trait)\b/gm) ?? []).length;
      const controlForms = (source.match(/\b(?:if|loop|match|task|return|break|continue)\b/g) ?? []).length;
      return {
        path: relativePath,
        bytes: Buffer.byteLength(source, "utf8"),
        lines,
        comments,
        declarations,
        controlForms,
      };
    });
  return {
    files,
    totals: {
      bytes: files.reduce((total, file) => total + file.bytes, 0),
      lines: files.reduce((total, file) => total + file.lines, 0),
      comments: files.reduce((total, file) => total + file.comments, 0),
      declarations: files.reduce((total, file) => total + file.declarations, 0),
      controlForms: files.reduce((total, file) => total + file.controlForms, 0),
    },
    interpretation: "machine source-shape counts only; not a readability or repair score",
  };
}

function modeArgs(workload, mode) {
  const entry = workload.entry;
  if (mode === "check") return ["check", entry];
  if (mode === "run") return ["run", entry, "--watch=off"];
  if (mode === "run-interpret") return ["run", "--interpret", entry, "--watch=off"];
  if (mode === "build") return ["build", entry, "--release"];
  if (mode === "run-release") return ["run", "--release", entry, "--watch=off"];
  throw new Error(`unsupported mode: ${mode}`);
}

function timeoutFor(workload, phase) {
  const limits = workload.limits ?? MANIFEST.defaults.limits;
  if (phase === "check") return limits.checkTimeoutMs;
  if (phase === "build") return limits.buildTimeoutMs;
  return limits.runTimeoutMs;
}

function commandEnvironment(runRoot) {
  const environment = { ...process.env };
  environment.JET_STORE_DIR = join(runRoot, "store");
  environment.JET_PACKAGE_STORE_DIR = join(runRoot, "package-store");
  environment.JET_RUN_CACHE_DIR = join(runRoot, "run-cache");
  environment.JET_FFI_CACHE_DIR = join(runRoot, "ffi");
  environment.JET_DEV_ORACLE_CACHE_DIR = join(runRoot, "oracle-cache");
  environment.JET_TEST_SCRATCH_DIR = join(runRoot, "test-scratch");
  environment.CARGO_INCREMENTAL = "0";
  environment.NO_COLOR = "1";
  environment.TMPDIR = join(runRoot, "tmp");
  environment.TMP = environment.TMPDIR;
  environment.TEMP = environment.TMPDIR;
  for (const directory of [environment.JET_STORE_DIR, environment.JET_PACKAGE_STORE_DIR, environment.JET_RUN_CACHE_DIR, environment.JET_FFI_CACHE_DIR, environment.JET_DEV_ORACLE_CACHE_DIR, environment.JET_TEST_SCRATCH_DIR, environment.TMPDIR]) {
    mkdirSync(directory, { recursive: true });
  }
  return environment;
}

function runCommand(command, args, cwd, environment, timeoutMs) {
  const started = process.hrtime.bigint();
  const commandProvenance = commandIdentity(command);
  const useTime = existsSync(TIME_BIN);
  const actualCommand = useTime ? TIME_BIN : command;
  const actualArgs = useTime
    ? ["-f", "__JET_SELFHOST_RSS_KIB__=%M", "--", command, ...args]
    : args;
  return new Promise((resolveResult) => {
    let stdout = "";
    let stderr = "";
    let timedOut = false;
    let outputLimit = false;
    let settled = false;
    let child;
    try {
      child = spawn(actualCommand, actualArgs, {
        cwd,
        env: environment,
        stdio: ["ignore", "pipe", "pipe"],
      });
    } catch (error) {
      resolveResult({
        status: "unavailable",
        command,
        commandIdentity: commandProvenance,
        args,
        exitCode: null,
        signal: null,
        stdout: "",
        stderr: String(error),
        latencyMs: Number(process.hrtime.bigint() - started) / 1e6,
        peakRssBytes: null,
      });
      return;
    }

    const finish = (result) => {
      if (settled) return;
      settled = true;
      resolveResult(result);
    };
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGTERM");
      setTimeout(() => child.kill("SIGKILL"), 500);
    }, timeoutMs);
    const append = (chunk, target) => {
      const text = chunk.toString("utf8");
      if (target === "stdout") stdout += text;
      else stderr += text;
      if (stdout.length + stderr.length > 8 * 1024 * 1024 && !outputLimit) {
        outputLimit = true;
        child.kill("SIGTERM");
      }
    };
    child.stdout.on("data", (chunk) => append(chunk, "stdout"));
    child.stderr.on("data", (chunk) => append(chunk, "stderr"));
    child.on("error", (error) => {
      clearTimeout(timer);
      finish({
        status: "unavailable",
        command,
        commandIdentity: commandProvenance,
        args,
        exitCode: null,
        signal: null,
        stdout,
        stderr: `${stderr}${String(error)}`,
        latencyMs: Number(process.hrtime.bigint() - started) / 1e6,
        peakRssBytes: null,
      });
    });
    child.on("close", (exitCode, signal) => {
      clearTimeout(timer);
      const rssMatch = stderr.match(/(?:^|\n)__JET_SELFHOST_RSS_KIB__=(\d+)\s*$/m);
      const peakRssBytes = rssMatch ? Number(rssMatch[1]) * 1024 : null;
      if (rssMatch) stderr = stderr.replace(rssMatch[0], "").replace(/\n$/, "");
      const status = timedOut ? "timeout" : outputLimit ? "failed-output-limit" : exitCode === 0 ? "completed" : "failed";
      finish({
        status,
        command,
        commandIdentity: commandProvenance,
        args,
        exitCode,
        signal,
        stdout,
        stderr,
        latencyMs: Number(process.hrtime.bigint() - started) / 1e6,
        peakRssBytes,
      });
    });
  });
}

function expectedModeForCase(testCase, mode) {
  if (testCase.kind === "check") return "check";
  return mode;
}

function caseProgram(workload, testCase) {
  return testCase.program ?? workload.entry;
}

function parseDiagnostic(text) {
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed.startsWith("{")) continue;
    try {
      const candidate = JSON.parse(trimmed);
      if (candidate && (candidate.schema === "jet.report/v1" || candidate.code)) return candidate;
    } catch {
      continue;
    }
  }
  return null;
}

function diagnosticMatches(actual, expected) {
  if (!actual) return false;
  if (actual.code !== expected.code) return false;
  const file = actual.file ? normalizePath(actual.file).split("/").at(-1) : null;
  if (file !== expected.file) return false;
  if (actual.line !== expected.line || actual.col !== expected.col) return false;
  if (!actual.span || actual.span.start !== expected.span.start || actual.span.end !== expected.span.end) return false;
  const edits = Array.isArray(actual.fix_edits) ? actual.fix_edits : [];
  return edits.some((edit) => edit.new_text === expected.fixText && edit.span?.start === expected.span.start && edit.span?.end === expected.span.end);
}

function compareCommand(result, expected, mode = "run") {
  const checkOutput = mode !== "check";
  const comparison = {
    expectedExit: expected.exit,
    actualExit: result.exitCode,
    stdoutExact: expected.stdout === undefined || !checkOutput ? null : result.stdout === expected.stdout,
    stderrExact: expected.stderr === undefined || !checkOutput ? null : result.stderr === expected.stderr,
    diagnosticExact: expected.diagnostic ? diagnosticMatches(parseDiagnostic(`${result.stdout}\n${result.stderr}`), expected.diagnostic) : null,
  };
  const exitMatches = result.exitCode === expected.exit;
  const outputMatches = (comparison.stdoutExact ?? true) && (comparison.stderrExact ?? true);
  const diagnosticOk = comparison.diagnosticExact ?? true;
  return {
    matches: (result.status === "completed" || (expected.exit !== 0 && result.status === "failed")) && exitMatches && outputMatches && diagnosticOk,
    comparison,
  };
}

async function prepareWorkload(workload, runRoot) {
  const sourceDirectory = resolve(SELFHOST_DIR, workload.directory);
  const workspace = join(runRoot, workload.id);
  copyTree(sourceDirectory, workspace);
  mkdirSync(join(workspace, "build"), { recursive: true });
  const identity = identityForDirectory(sourceDirectory);
  return { sourceDirectory, workspace, identity };
}
function freshWorkspace(sourceDirectory, workspace) {
  copyTree(sourceDirectory, workspace);
  mkdirSync(join(workspace, "build"), { recursive: true });
  return workspace;
}

async function buildNativeProvider(workload, workspace, environment, runRoot) {
  if (!workload.native?.build) return null;
  const buildScript = resolve(workspace, workload.native.build);
  if (!existsSync(buildScript)) {
    return { status: "unavailable", reason: `missing native provider build script: ${workload.native.build}` };
  }
  const result = await runCommand("/bin/sh", [buildScript], workspace, environment, timeoutFor(workload, "build"));
  const artifactPath = resolve(workspace, workload.native.artifact);
  const nativeRoot = dirname(artifactPath);
  const identity = existsSync(nativeRoot) ? identityForDirectory(nativeRoot) : null;
  const artifact = existsSync(artifactPath)
    ? {
        path: normalizePath(relative(workspace, artifactPath)),
        sha256: sha256Bytes(readFileSync(artifactPath)),
        bytes: readFileSync(artifactPath).length,
      }
    : null;
  return {
    ...result,
    status: result.status === "completed" && !artifact ? "failed" : result.status,
    toolchain: { command: "/bin/sh", script: workload.native.build, target: workload.targets },
    artifact,
    identity,
  };
}

async function runJetCases(workload, mode, workspace, environment) {
  const cases = [];
  for (const testCase of workload.cases) {
    if (mode !== "check" && testCase.kind === "check") {
      cases.push({ id: testCase.id, status: "not-applicable", reason: "diagnostic check case is not a runtime case" });
      continue;
    }
    const selectedMode = expectedModeForCase(testCase, mode);
    const entry = caseProgram(workload, testCase);
    const args = selectedMode === "check" ? ["check", entry, "--json"] : modeArgs({ ...workload, entry }, selectedMode);
    const result = await runCommand(JET_ENV, ["jet", ...args], workspace, environment, timeoutFor(workload, selectedMode === "build" ? "build" : selectedMode));
    const expected = testCase.expected;
    const comparison = compareCommand(result, expected, selectedMode);
    cases.push({
      id: testCase.id,
      mode: selectedMode,
      command: { program: JET_ENV, args: ["jet", ...args] },
      result,
      ...comparison,
    });
  }
  return cases;
}

async function runJetBuild(workload, mode, workspace, environment) {
  const native = await buildNativeProvider(workload, workspace, environment, workspace);
  if (native && native.status !== "completed") {
    return { native, compiler: null, artifact: null, run: null, matches: false };
  }
  const args = modeArgs(workload, mode);
  const compiler = await runCommand(JET_ENV, ["jet", ...args], workspace, environment, timeoutFor(workload, "build"));
  const artifactPath = join(workspace, "build", "run");
  const artifact = existsSync(artifactPath)
    ? { path: normalizePath(relative(workspace, artifactPath)), sha256: sha256Bytes(readFileSync(artifactPath)), bytes: readFileSync(artifactPath).length }
    : null;
  const run = compiler.status === "completed" && compiler.exitCode === 0 && artifact
    ? await runCommand(artifactPath, [], workspace, environment, timeoutFor(workload, "run"))
    : null;
  const expectedCase = workload.cases.find((testCase) => testCase.kind !== "check") ?? workload.cases[0];
  const comparison = run ? compareCommand(run, expectedCase.expected, "run") : null;
  return {
    native,
    compiler,
    artifact,
    run,
    comparison: comparison?.comparison ?? null,
    matches: comparison?.matches ?? false,
  };
}

async function runRust(workload, workspace, environment) {
  const source = resolve(workspace, workload.oracle);
  const binary = join(workspace, "oracle-bin");
  const build = await runCommand("rustc", ["--edition", "2021", source, "-O", "-o", binary], workspace, environment, timeoutFor(workload, "build"));
  const result = build.status === "completed" && build.exitCode === 0
    ? await runCommand(binary, [], workspace, environment, timeoutFor(workload, "run"))
    : null;
  const expectedCase = workload.cases.find((testCase) => testCase.kind !== "check") ?? workload.cases[0];
  const comparison = result ? compareCommand(result, expectedCase.expected, "run") : { matches: false, comparison: null };
  return {
    build,
    run: result,
    matches: comparison.matches,
    comparison: comparison.comparison,
    artifact: existsSync(binary)
      ? { path: "oracle-bin", sha256: sha256Bytes(readFileSync(binary)), bytes: readFileSync(binary).length }
      : null,
  };
}

function initialEvidence(workload, identity, complexity) {
  return {
    identity,
    sourceComplexity: complexity,
    machine: { status: "unavailable", records: [] },
    model: {
      status: "unrun",
      contracts: workload.contract,
      expected: workload.cases.map((testCase) => ({ id: testCase.id, expected: testCase.expected })),
    },
    human: { status: "unavailable", records: [], note: "No participant observation is authored by this driver." },
    trust: {
      rust: "independent same-job counterpart; agreement does not prove Jet semantics",
      foreign: workload.native?.trust ?? "no foreign artifact in this workload",
      bootstrap: "retained Rust-host boundary is not a semantic correctness certificate",
    },
    supportedModes: workload.supportedModes.map((mode) => ({ mode, status: "unrun" })),
  };
}

function markMode(evidence, mode, status, record) {
  const row = evidence.supportedModes.find((candidate) => candidate.mode === mode);
  if (row) {
    row.status = status;
    row.record = record;
  }
  if (status === "observed") {
    evidence.machine.status = "observed";
    evidence.machine.records.push({ kind: mode, record });
  }
}

function reasoningTemplate(workload) {
  return REASONING_TASKS.map((taskId) => {
    const task = workload.reasoning.find((candidate) => candidate.id === taskId);
    return {
      id: taskId,
      status: "unrun",
      evidenceClass: "model",
      prompt: task?.prompt ?? "",
      expectedObservations: task?.expectedObservations ?? [],
    };
  });
}

function validateReasoningCapture(capture, workload) {
  if (!capture || capture.schema !== "jet.selfhost.reasoning.capture.v1") {
    return { status: "rejected", reason: "reasoning capture schema is not jet.selfhost.reasoning.capture.v1" };
  }
  if (capture.study !== MANIFEST.study || capture.workload !== workload.id) {
    return { status: "rejected", reason: "reasoning capture is attached to a different study or workload" };
  }
  if (capture.readiness === true || capture.portApproval === "approved") {
    return { status: "rejected", reason: "reasoning capture cannot grant readiness or port approval" };
  }
  const entries = Array.isArray(capture.tasks) ? capture.tasks : [];
  const byId = new Map(entries.map((entry) => [entry.id, entry]));
  if (entries.length !== REASONING_TASKS.length || byId.size !== entries.length || entries.some((entry) => !REASONING_TASKS.includes(entry.id))) {
    return { status: "rejected", reason: "reasoning capture must contain exactly one explain, predict, modify, and derive task" };
  }
  const tasks = [];
  for (const taskId of REASONING_TASKS) {
    const entry = byId.get(taskId);
    if (!entry) return { status: "rejected", reason: `missing reasoning task: ${taskId}` };
    if (!entry.observation || typeof entry.observation !== "string") {
      return { status: "rejected", reason: `reasoning task has no observation: ${taskId}` };
    }
    if (!["machine", "model", "human"].includes(entry.evidenceClass)) {
      return { status: "rejected", reason: `invalid evidence class for ${taskId}` };
    }
    if (entry.source !== undefined && entry.source !== null && typeof entry.source !== "string") {
      return { status: "rejected", reason: `invalid source provenance for ${taskId}` };
    }
    tasks.push({
      id: taskId,
      status: "observed",
      evidenceClass: entry.evidenceClass,
      observation: entry.observation,
      source: entry.source ?? null,
    });
  }
  return { status: "observed", tasks };
}

function loadReasoningCapture(pathname, workload) {
  if (!pathname) return { status: "unrun", tasks: reasoningTemplate(workload) };
  if (!existsSync(pathname)) return { status: "unavailable", reason: `missing reasoning capture: ${pathname}`, tasks: reasoningTemplate(workload) };
  try {
    return validateReasoningCapture(JSON.parse(readFileSync(pathname, "utf8")), workload);
  } catch (error) {
    return { status: "rejected", reason: `invalid reasoning JSON: ${String(error)}` };
  }
}

function repairPatch(source, replacement) {
  if (replacement.start < 0 || replacement.end < replacement.start || replacement.end > source.length) {
    throw new Error("repair span is outside the source");
  }
  return `${source.slice(0, replacement.start)}${replacement.newText}${source.slice(replacement.end)}`;
}

async function runRepair(workload, workspace, environment) {
  if (!workload.repair) return { status: "not-applicable", reason: "no authored repair protocol" };
  const program = workload.repair.program;
  const sourcePath = join(workspace, program);
  if (!existsSync(sourcePath)) return { status: "unavailable", reason: `missing repair program: ${program}` };
  const source = readFileSync(sourcePath, "utf8");
  const edited = repairPatch(source, workload.repair.replacement);
  const editedPath = join(workspace, "repair-candidate.jet");
  writeFileSync(editedPath, edited);
  const result = await runCommand(JET_ENV, ["jet", "check", "repair-candidate.jet", "--json"], workspace, environment, timeoutFor(workload, "check"));
  const expectedPath = join(workspace, workload.repair.expectedProgram);
  const sourceMatchesExpected = existsSync(expectedPath) && readFileSync(expectedPath, "utf8") === edited;
  const diagnostic = parseDiagnostic(`${result.stdout}\n${result.stderr}`);
  const matches = result.status === "completed" && result.exitCode === workload.repair.expectedExit && sourceMatchesExpected && !diagnostic;
  return {
    status: matches ? "observed" : result.status === "completed" ? "failed" : result.status,
    program,
    replacement: workload.repair.replacement,
    editedIdentity: { sha256: sha256Bytes(Buffer.from(edited)), bytes: Buffer.byteLength(edited, "utf8") },
    result,
    expectedExit: workload.repair.expectedExit,
    expectedProgram: workload.repair.expectedProgram,
    sourceMatchesExpected,
    diagnostic,
  };
}

function workloadSelected(workload, options) {
  return options.workloads.length === 0 || options.workloads.includes(workload.id);
}

function shouldRun(options, phase) {
  if (options.all) return true;
  if (options.phase === "all") return true;
  if (!options.phase) return false;
  return options.phase === phase;
}

async function evaluateWorkload(workload, options, runRoot) {
  const prepared = await prepareWorkload(workload, runRoot);
  const evidence = initialEvidence(workload, prepared.identity, complexityForDirectory(prepared.sourceDirectory));
  const item = {
    id: workload.id,
    title: workload.title,
    status: "unrun",
    sourceDirectory: normalizePath(relative(REPO_ROOT, prepared.sourceDirectory)),
    workspace: normalizePath(relative(runRoot, prepared.workspace)),
    evidence,
    oracle: null,
    jet: [],
    repair: null,
    reasoning: loadReasoningCapture(options.reasoningFile ? resolve(options.reasoningFile) : null, workload),
  };

  if (shouldRun(options, "complexity") || shouldRun(options, "identity")) {
    item.status = "specified";
  }

  const modesToRun = options.all || options.phase === "all"
    ? workload.supportedModes
    : shouldRun(options, "build")
      ? [options.mode ?? "build"]
      : shouldRun(options, "check")
        ? [options.mode ?? "check"]
        : shouldRun(options, "run")
          ? [options.mode ?? "run"]
          : [];
  let allMachineModesPassed = modesToRun.length > 0;
  for (const mode of modesToRun) {
    if (!workload.supportedModes.includes(mode)) {
      item.jet.push({ mode, status: "unsupported" });
      allMachineModesPassed = false;
      continue;
    }
    const modeRoot = join(runRoot, workload.id, `mode-${mode}`);
    const modeWorkspace = freshWorkspace(prepared.sourceDirectory, join(modeRoot, "workspace"));
    const environment = commandEnvironment(join(modeRoot, "environment"));
    if (mode === "build") {
      const build = await runJetBuild(workload, mode, modeWorkspace, environment);
      item.jet.push({ mode, build });
      const passed = build.compiler?.status === "completed"
        && build.compiler.exitCode === 0
        && Boolean(build.artifact)
        && build.matches === true
        && (!build.native || build.native.status === "completed");
      const buildStatus = passed ? "observed" : build.compiler?.status === "completed" ? "failed" : build.compiler?.status ?? build.native?.status ?? "unavailable";
      markMode(evidence, mode, buildStatus, build);
      if (!passed) allMachineModesPassed = false;
      continue;
    }
    const native = mode === "check" ? null : await buildNativeProvider(workload, modeWorkspace, environment, modeRoot);
    if (native && native.status !== "completed") {
      item.jet.push({ mode, native, cases: [] });
      markMode(evidence, mode, native.status, native);
      allMachineModesPassed = false;
      continue;
    }
    const cases = await runJetCases(workload, mode, modeWorkspace, environment);
    item.jet.push({ mode, native, cases });
    const applicable = cases.filter((candidate) => candidate.status !== "not-applicable");
    const passed = applicable.length > 0 && applicable.every((candidate) => candidate.matches);
    const modeStatus = passed
      ? "observed"
      : applicable.some((candidate) => candidate.result?.status === "unavailable") ? "unavailable" : "failed";
    markMode(evidence, mode, modeStatus, cases);
    if (!passed) allMachineModesPassed = false;
  }
  if (allMachineModesPassed) item.status = "observed";

  if (shouldRun(options, "all") || options.all) {
    const oracleRoot = join(runRoot, workload.id, "oracle");
    const oracleWorkspace = freshWorkspace(prepared.sourceDirectory, join(oracleRoot, "workspace"));
    const environment = commandEnvironment(join(oracleRoot, "environment"));
    item.oracle = await runRust(workload, oracleWorkspace, environment);
    if (item.oracle.matches) item.status = item.status === "observed" ? "observed" : "oracle-observed";
  }

  if (shouldRun(options, "repair") || options.all) {
    const repairRoot = join(runRoot, workload.id, "repair");
    const repairWorkspace = freshWorkspace(prepared.sourceDirectory, join(repairRoot, "workspace"));
    const environment = commandEnvironment(join(repairRoot, "environment"));
    item.repair = await runRepair(workload, repairWorkspace, environment);
  }

  if (item.status === "unrun" && item.reasoning.status === "observed") item.status = "reasoning-observed";
  return item;
}

function summarize(items) {
  const counts = {};
  for (const item of items) counts[item.status] = (counts[item.status] ?? 0) + 1;
  return counts;
}

function goNoGo(items) {
  const allObserved = items.length === MANIFEST.workloads.length && items.every((item) => {
    const repairObserved = item.repair && ["observed", "not-applicable"].includes(item.repair.status);
    return item.status === "observed"
      && item.evidence?.machine?.status === "observed"
      && item.oracle?.matches === true
      && repairObserved
      && item.reasoning?.status === "observed";
  });
  return {
    decision: allObserved ? "owner-decision-required" : "unresolved",
    portfolio: allObserved ? "supported-within-scope" : "unresolved",
    readiness: false,
    portApproval: "not-addressed",
    owner: "#217",
    portOwner: "#218",
    reason: allObserved
      ? "All supported machine modes, same-job oracles, repairs, and reasoning captures were observed, but this portfolio cannot approve or certify a compiler port."
      : "At least one workload, supported mode, oracle, repair, or reasoning obligation remains unrun, unavailable, or failed.",
    retainedBoundary: "Rust-hosted compiler and foreign artifacts remain explicit trust boundaries.",
  };
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    console.log(usage());
    return;
  }
  if (options.list) {
    listWorkloads();
    return;
  }
  if (!options.all && !options.phase) {
    console.log(usage());
    return;
  }
  mkdirSync(SCRATCH_ROOT, { recursive: true });
  const runRoot = mkdtempSync(join(SCRATCH_ROOT, "run-"));
  const selected = MANIFEST.workloads.filter((workload) => workloadSelected(workload, options));
  const receipt = {
    schema: "jet.selfhost.receipt.v1",
    study: MANIFEST.study,
    status: "unrun",
    generatedAt: new Date().toISOString(),
    manifest: { path: normalizePath(relative(REPO_ROOT, MANIFEST_PATH)), sha256: sha256Bytes(readFileSync(MANIFEST_PATH)) },
    runRoot: normalizePath(runRoot),
    requested: {
      phase: options.phase ?? "all",
      mode: options.mode ?? null,
      workloads: selected.map((workload) => workload.id),
    },
    sourceBoundary: MANIFEST.sourceBoundary,
    evidencePolicy: MANIFEST.evidencePolicy,
    workloads: [],
    summary: null,
    goNoGo: null,
  };
  for (const workload of selected) {
    try {
      receipt.workloads.push(await evaluateWorkload(workload, options, runRoot));
    } catch (error) {
      receipt.workloads.push({
        id: workload.id,
        status: "failed",
        error: String(error),
        evidence: {
          machine: { status: "unavailable", records: [] },
          model: { status: "unrun" },
          human: { status: "unavailable", records: [] },
        },
      });
    }
  }
  receipt.summary = summarize(receipt.workloads);
  receipt.goNoGo = goNoGo(receipt.workloads);
  receipt.status = receipt.goNoGo.decision === "owner-decision-required" ? "observed-within-scope" : "unresolved";
  const serialized = `${JSON.stringify(receipt, null, 2)}\n`;
  if (options.output) {
    const output = resolve(options.output);
    mkdirSync(dirname(output), { recursive: true });
    writeFileSync(output, serialized);
  }
  process.stdout.write(serialized);
}

main().catch((error) => {
  console.error(`selfhost driver error: ${String(error)}`);
  process.exitCode = 2;
});
