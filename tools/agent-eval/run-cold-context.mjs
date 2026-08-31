#!/usr/bin/env node

import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import { homedir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const capsuleDefault = path.join(repoDir, "tools/agent-eval/jet-context-capsule.md");
const llmsDefault = path.join(repoDir, "llms.text");
const tasksDefault = path.join(repoDir, "tools/agent-eval/tasks/tasks.json");
const adaptersDefault = path.join(repoDir, "tools/agent-eval/adapters.json");
const baselineDefault = path.join(repoDir, "docs/audits/cold-agent-jet-baseline.json");
const scoreboardDefault = path.join(repoDir, "docs/audits/cold-agent-jet-scoreboard.json");
const jetEnvDefault = path.join(repoDir, "scripts/agent/jet-env");
const capsuleLimitBytes = 32 * 1024;
const modelOutputLimitBytes = 2 * 1024 * 1024;
const processOutputLimitBytes = 2 * 1024 * 1024;
const defaultTimeoutMs = 120_000;
const serviceReadyTimeoutMs = 60_000;
const serviceProbeTimeoutMs = 3_000;
const serviceShutdownTimeoutMs = 5_000;
const unavailableExit = 78;
const usageExit = 64;
const requiredAdapterFamilies = ["openai", "anthropic"];
const requiredVerbs = [
  "term.print", "term.eprint", "term.input", "term.readline",
  "term.read_all_input", "files.read", "files.write", "files.read_bytes",
  "files.write_bytes", "files.exists", "files.list_dir", "files.walk",
  "process.argv", "process.run", "process.cmd", "sys.get", "sys.set",
  "json.parse", "json.decode", "json.to_string", "csv.parse", "csv.to_string",
  "text.trim", "text.lower", "text.upper", "text.splitn", "regex.compile",
  "regex.replace", "math.abs", "math.min", "math.max", "math.round",
  "math.sqrt", "time.now", "time.sleep", "net.tcp_listen", "net.tcp_connect",
  "http.get", "http.post", "http.serve",
];
const requiredPrograms = [
  "Hello", "Arithmetic", "Function call", "Branching", "Loop and mutation",
  "List ordering", "Optional fallback", "Command-line argument", "File input",
  "HTTP health endpoint",
];

export class HarnessUsageError extends Error {}
export class AdapterBlockedError extends Error {}
export class ModelOutputError extends Error {}
export class RegressionError extends Error {}

function usage() {
  return `Usage: node tools/agent-eval/run-cold-context.mjs [options]

Required external adapter configuration is read from tools/agent-eval/adapters.json.
The two stock families are represented by the openai and anthropic entries. Select
one transport per entry with its transport_env: api requires its declared
environment variables; command uses command_env or the adapter's default_argv.
Command adapters declare whether the request is JSON on stdin or a prompt argv
item. No credentials are stored in this repository.

Options:
  --config PATH          Adapter configuration JSON
  --capsule PATH         Capsule artifact (default: tools/agent-eval/jet-context-capsule.md)
  --llms PATH            llms.text control source
  --tasks PATH           Four-task fixture JSON
  --baseline PATH        Recorded capsule baseline JSON
  --output PATH          Deterministic scoreboard JSON
  --jet-bin PATH         Direct Jet binary; default uses scripts/agent/jet-env
  --mode capsule|control|both
  --record-baseline      Write a capsule-only baseline after a complete run
  --check-baseline       Fail if the capsule result regresses below baseline
  --help                 Show this help
`;
}

function parseArgs(argv) {
  const options = {
    config: adaptersDefault,
    capsule: capsuleDefault,
    llms: llmsDefault,
    tasks: tasksDefault,
    baseline: baselineDefault,
    output: scoreboardDefault,
    jetBin: null,
    mode: "both",
    recordBaseline: false,
    checkBaseline: false,
    help: false,
  };
  const pathOptions = new Map([
    ["--config", "config"],
    ["--capsule", "capsule"],
    ["--llms", "llms"],
    ["--tasks", "tasks"],
    ["--baseline", "baseline"],
    ["--output", "output"],
    ["--jet-bin", "jetBin"],
  ]);
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token === "--help" || token === "-h") {
      options.help = true;
      continue;
    }
    if (token === "--record-baseline") {
      options.recordBaseline = true;
      continue;
    }
    if (token === "--check-baseline") {
      options.checkBaseline = true;
      continue;
    }
    if (token === "--mode") {
      const value = argv[++i];
      if (!value) throw new HarnessUsageError("--mode needs capsule, control, or both");
      options.mode = value;
      continue;
    }
    const key = pathOptions.get(token);
    if (key) {
      const value = argv[++i];
      if (!value) throw new HarnessUsageError(`${token} needs a path`);
      options[key] = path.resolve(value);
      continue;
    }
    throw new HarnessUsageError(`unknown option: ${token}`);
  }
  if (!["capsule", "control", "both"].includes(options.mode)) {
    throw new HarnessUsageError(`invalid mode: ${options.mode}`);
  }
  if (options.recordBaseline && options.mode === "control") {
    throw new HarnessUsageError("--record-baseline needs capsule or both mode");
  }
  if (options.recordBaseline && options.checkBaseline) {
    throw new HarnessUsageError("--record-baseline and --check-baseline cannot be used together");
  }
  return options;
}

async function readJson(file) {
  let text;
  try {
    text = await fs.readFile(file, "utf8");
  } catch (error) {
    throw new HarnessUsageError(`cannot read ${file}: ${error.message}`);
  }
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new HarnessUsageError(`invalid JSON in ${file}: ${error.message}`);
  }
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function utf8Compare(left, right) {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function canonicalize(value) {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .sort(utf8Compare)
        .map((key) => [key, canonicalize(value[key])]),
    );
  }
  return value;
}

function hashJson(value) {
  return sha256(Buffer.from(JSON.stringify(canonicalize(value)), "utf8"));
}

function relPath(file) {
  const relative = path.relative(repoDir, file).split(path.sep).join("/");
  return relative.startsWith("../") ? relative : relative || ".";
}

async function fileDescriptor(file, content = null) {
  const bytes = content === null ? await fs.readFile(file) : Buffer.from(content);
  return { path: relPath(file), bytes: bytes.length, sha256: sha256(bytes) };
}

function utf8Prefix(text, budget) {
  const bytes = Buffer.from(text, "utf8");
  if (bytes.length <= budget) return text;
  let end = budget;
  while (end > 0) {
    const candidate = bytes.subarray(0, end);
    const decoded = candidate.toString("utf8");
    if (Buffer.byteLength(decoded, "utf8") === end) return decoded;
    end -= 1;
  }
  return "";
}

export function buildControlContext(llmsText, budgetBytes) {
  if (budgetBytes < 0) throw new HarnessUsageError("context budget cannot be negative");
  const context = utf8Prefix(llmsText, budgetBytes);
  if (Buffer.byteLength(context, "utf8") !== Math.min(Buffer.byteLength(llmsText, "utf8"), budgetBytes)) {
    throw new HarnessUsageError("llms.text control could not preserve its byte budget");
  }
  return context;
}

export function validateCapsule(capsule) {
  if (typeof capsule !== "string") throw new HarnessUsageError("capsule must be text");
  const bytes = Buffer.byteLength(capsule, "utf8");
  if (bytes > capsuleLimitBytes) {
    throw new HarnessUsageError(`capsule is ${bytes} bytes; limit is ${capsuleLimitBytes}`);
  }
  const required = [
    "## 1. Program shape",
    "## 2. Dot rule and common calls",
    "## 3. Memory and ownership verbs",
    "## 4. Effects and outcomes",
    "## 5. Forty common library verbs",
    "## 6. Ten runnable canonical programs",
    "## 7. Cold-agent rules",
    "&T",
    "^T",
    "~value",
    "-[",
    "Ok(value)",
    "Err(error)",
    "None",
    "??",
  ];
  for (const needle of required) {
    if (!capsule.includes(needle)) throw new HarnessUsageError(`capsule is missing required subject: ${needle}`);
  }
  const verbs = [...capsule.matchAll(/(?:^|\s)(\d+)\.\s+`([^`]+)`/gm)];
  if (verbs.length !== requiredVerbs.length || verbs.some((match, index) => (
    Number(match[1]) !== index + 1 || match[2] !== requiredVerbs[index]
  ))) {
    throw new HarnessUsageError("capsule must contain the canonical numbered verb list");
  }
  const programs = [...capsule.matchAll(/^### (\d+)\. ([^\r\n]+)$/gm)];
  if (programs.length !== requiredPrograms.length || programs.some((match, index) => (
    Number(match[1]) !== index + 1 || match[2] !== requiredPrograms[index]
  ))) {
    throw new HarnessUsageError("capsule must contain the canonical numbered program list");
  }
  for (const [index, heading] of programs.entries()) {
    const sectionEnd = programs[index + 1]?.index ?? capsule.length;
    const section = capsule.slice(heading.index, sectionEnd);
    const sources = [...section.matchAll(/```jet\s*\r?\n([\s\S]*?)\r?\n```/g)];
    if (sources.length !== 1 || !/\bfn\s+run\b/u.test(sources[0][1])) {
      throw new HarnessUsageError(`capsule program ${index + 1} is missing a complete fn run source block`);
    }
  }
}

function validateTask(task) {
  if (!task || typeof task !== "object" || typeof task.id !== "string" || typeof task.prompt !== "string") {
    throw new HarnessUsageError("each task needs string id and prompt fields");
  }
  if (!["batch", "http"].includes(task.mode)) throw new HarnessUsageError(`${task.id}: invalid task mode`);
  if (!Array.isArray(task.args) || task.args.some((arg) => typeof arg !== "string")) {
    throw new HarnessUsageError(`${task.id}: args must be a string array`);
  }
  if (task.mode === "batch" && typeof task.expected_stdout !== "string") {
    throw new HarnessUsageError(`${task.id}: batch task needs expected_stdout`);
  }
  if (task.files !== undefined) {
    if (!task.files || typeof task.files !== "object" || Array.isArray(task.files)) {
      throw new HarnessUsageError(`${task.id}: files must be an object`);
    }
    for (const [name, value] of Object.entries(task.files)) {
      if (!name || path.posix.isAbsolute(name) || name.split(/[\\/]/u).includes("..") || typeof value !== "string") {
        throw new HarnessUsageError(`${task.id}: unsafe or non-text fixture ${name}`);
      }
    }
  }
  if (task.mode === "http") {
    const service = task.service;
    if (!service || service.port_arg !== true || typeof service.ready_path !== "string" || !Array.isArray(service.probes) || service.probes.length === 0) {
      throw new HarnessUsageError(`${task.id}: HTTP task needs port_arg, ready_path, and probes`);
    }
    for (const [index, probe] of service.probes.entries()) {
      if (!probe || typeof probe.path !== "string" || typeof probe.method !== "string" || !Number.isInteger(probe.status) || typeof probe.body !== "string") {
        throw new HarnessUsageError(`${task.id}: invalid HTTP probe ${index}`);
      }
    }
  }
}

function loadTasks(config) {
  if (!config || config.schema !== "jet.cold-agent.tasks.v1" || !Array.isArray(config.tasks)) {
    throw new HarnessUsageError("task fixture has the wrong schema");
  }
  const ids = new Set();
  for (const task of config.tasks) {
    validateTask(task);
    if (ids.has(task.id)) throw new HarnessUsageError(`duplicate task ${task.id}`);
    ids.add(task.id);
  }
  const required = ["hello", "cli", "data-transform", "http"];
  if (config.tasks.length !== required.length || required.some((id) => !ids.has(id))) {
    throw new HarnessUsageError("task fixture must contain hello, cli, data-transform, and http");
  }
  return [...config.tasks].sort((left, right) => utf8Compare(left.id, right.id));
}

function loadAdapters(config) {
  if (!config || config.schema !== "jet.cold-agent.adapters.v1" || !Array.isArray(config.adapters)) {
    throw new HarnessUsageError("adapter config has the wrong schema");
  }
  if (!Array.isArray(config.required_families) || config.required_families.some((family) => typeof family !== "string")) {
    throw new HarnessUsageError("adapter config needs a string required_families array");
  }
  const required = new Set(config.required_families);
  if (required.size < requiredAdapterFamilies.length || requiredAdapterFamilies.some((family) => !required.has(family))) {
    throw new HarnessUsageError("adapter config must require the openai and anthropic families");
  }
  const ids = new Set();
  const families = new Set();
  for (const adapter of config.adapters) {
    if (!adapter || typeof adapter.id !== "string" || typeof adapter.family !== "string") {
      throw new HarnessUsageError("each adapter needs id and family");
    }
    if (ids.has(adapter.id) || families.has(adapter.family)) throw new HarnessUsageError("adapter ids and families must be unique");
    ids.add(adapter.id);
    families.add(adapter.family);
    if (typeof adapter.transport_env !== "string" || (!adapter.api && !adapter.command)) {
      throw new HarnessUsageError(`${adapter.id}: api or command and transport_env are required`);
    }
    if (adapter.command) {
      const input = adapter.command.input ?? "json-stdin";
      if (!["json-stdin", "prompt-argument"].includes(input)) {
        throw new HarnessUsageError(`${adapter.id}: command input must be json-stdin or prompt-argument`);
      }
      if (adapter.command.command_env !== undefined && typeof adapter.command.command_env !== "string") {
        throw new HarnessUsageError(`${adapter.id}: command_env must be a string`);
      }
      const defaultArgv = adapter.command.default_argv;
      if (defaultArgv !== undefined && (!Array.isArray(defaultArgv)
        || defaultArgv.length === 0
        || defaultArgv.some((part) => typeof part !== "string" || part.length === 0))) {
        throw new HarnessUsageError(`${adapter.id}: default_argv must be a non-empty JSON argv array`);
      }
    }
  }
  for (const family of required) if (!families.has(family)) throw new HarnessUsageError(`missing required family ${family}`);
  return [...config.adapters].sort((left, right) => utf8Compare(left.id, right.id));
}

function selectedTransport(adapter, env) {
  const value = String(env[adapter.transport_env] ?? adapter.default_transport ?? "").trim().toLowerCase();
  if (value !== "api" && value !== "command") throw new HarnessUsageError(`${adapter.id}: transport must be api or command`);
  return value;
}

function commandFromEnvironment(adapter, env) {
  const commandEnv = adapter.command?.command_env;
  const value = commandEnv ? String(env[commandEnv] ?? "").trim() : "";
  if (!value) {
    const defaultArgv = adapter.command?.default_argv;
    if (defaultArgv) return [...defaultArgv];
    throw new AdapterBlockedError(`${adapter.id}: set ${commandEnv} for command transport`);
  }
  let command;
  try {
    command = JSON.parse(value);
  } catch {
    throw new AdapterBlockedError(`${adapter.id}: ${commandEnv} must contain a JSON argv array`);
  }
  if (!Array.isArray(command) || command.length === 0 || command.some((part) => typeof part !== "string" || part.length === 0)) {
    throw new AdapterBlockedError(`${adapter.id}: ${commandEnv} must contain a non-empty JSON argv array`);
  }
  return command;
}

export function preflightAdapters(adapters, env) {
  const resolved = [];
  const blocked = [];
  for (const adapter of adapters) {
    let transport;
    try {
      transport = selectedTransport(adapter, env);
      if (transport === "command") {
        if (!adapter.command) throw new HarnessUsageError(`${adapter.id}: command transport is not configured`);
        commandFromEnvironment(adapter, env);
      } else {
        const api = adapter.api;
        if (!api) throw new HarnessUsageError(`${adapter.id}: API transport is not configured`);
        const requiredEnv = [api.endpoint_env, api.api_key_env, api.model_env];
        if (api.version_env) requiredEnv.push(api.version_env);
        const missing = requiredEnv.filter((name) => !String(env[name] ?? "").trim());
        if (missing.length > 0) throw new AdapterBlockedError(`${adapter.id}: missing ${missing.join(", ")}`);
        if (api.protocol !== "openai-chat") {
          throw new HarnessUsageError(`${adapter.id}: unsupported API protocol ${api.protocol}`);
        }
      }
      resolved.push({ adapter, transport });
    } catch (error) {
      if (error instanceof AdapterBlockedError) blocked.push(error.message);
      else throw error;
    }
  }
  return { resolved, blocked };
}

function killTree(child, signal = "SIGTERM") {
  if (!child || child.exitCode !== null) return;
  try {
    if (process.platform === "win32") child.kill(signal);
    else process.kill(-child.pid, signal);
  } catch {
    try { child.kill(signal); } catch { /* already exited */ }
  }
}

function spawnTracked(command, cwd, { env = process.env, input = null, timeoutMs = defaultTimeoutMs } = {}) {
  let child;
  let resolveResult;
  const result = new Promise((resolve) => { resolveResult = resolve; });
  const stdout = [];
  const stderr = [];
  let stdoutBytes = 0;
  let stderrBytes = 0;
  let outputLimit = false;
  let timedOut = false;
  let settled = false;
  let timer = null;
  try {
    child = spawn(command[0], command.slice(1), {
      cwd,
      env,
      detached: process.platform !== "win32",
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
  } catch (error) {
    settled = true;
    resolveResult({ code: null, signal: null, stdout: Buffer.alloc(0), stderr: Buffer.alloc(0), spawnError: error.message, outputLimit: false, timedOut: false });
    return { child: null, result, terminate: () => {} };
  }
  const collect = (chunks, chunk, currentBytes) => {
    const remaining = processOutputLimitBytes - currentBytes;
    if (remaining > 0) chunks.push(chunk.subarray(0, remaining));
    return currentBytes + chunk.length;
  };
  child.stdout.on("data", (chunk) => {
    stdoutBytes = collect(stdout, chunk, stdoutBytes);
    if (stdoutBytes > processOutputLimitBytes && !outputLimit) {
      outputLimit = true;
      killTree(child);
    }
  });
  child.stderr.on("data", (chunk) => {
    stderrBytes = collect(stderr, chunk, stderrBytes);
    if (stderrBytes > processOutputLimitBytes && !outputLimit) {
      outputLimit = true;
      killTree(child);
    }
  });
  const finish = (code, signal, spawnError = null) => {
    if (settled) return;
    settled = true;
    clearTimeout(timer);
    resolveResult({
      code,
      signal,
      stdout: Buffer.concat(stdout),
      stderr: Buffer.concat(stderr),
      spawnError,
      outputLimit,
      timedOut,
    });
  };
  child.once("error", (error) => finish(null, null, error.message));
  child.once("close", (code, signal) => finish(code, signal));
  if (timeoutMs > 0) {
    timer = setTimeout(() => {
      timedOut = true;
      killTree(child);
    }, timeoutMs);
    timer.unref?.();
  }
  if (input !== null) {
    child.stdin.end(input);
  } else {
    child.stdin.end();
  }
  return { child, result, terminate: (signal = "SIGTERM") => killTree(child, signal) };
}

function jetEnvironment(env = process.env) {
  const next = { ...env };
  const config = String(next.NIX_CONFIG ?? "").trim();
  next.NIX_CONFIG = config ? `${config}\nwarn-dirty = false` : "warn-dirty = false";
  next.JET_NIX_TMP_CLEANED = "1";
  return next;
}

function spawnJet(command, cwd, options = {}) {
  return spawnTracked(command, cwd, { ...options, env: jetEnvironment(options.env) });
}

async function waitForTracked(tracked, timeoutMs) {
  let timer;
  const result = await Promise.race([
    tracked.result,
    new Promise((resolve) => {
      timer = setTimeout(() => resolve(null), timeoutMs);
      timer.unref?.();
    }),
  ]);
  clearTimeout(timer);
  return result;
}

function jetPrefix(jetBin) {
  return jetBin ? [jetBin] : [jetEnvDefault, "jet"];
}

function jetCheckCommand(jetBin) {
  return [...jetPrefix(jetBin), "check", "candidate.jet"];
}

function jetRunCommand(jetBin, args) {
  return [...jetPrefix(jetBin), "run", "--profile=debug", "candidate.jet", "--", ...args];
}

function fixedFailure(phase, result) {
  if (result.spawnError) return `${phase}:spawn`;
  if (result.timedOut) return `${phase}:timeout`;
  if (result.outputLimit) return `${phase}:output-limit`;
  return `${phase}:exit-${result.code ?? "signal"}`;
}

function baseRow(adapter, context, task, source = null) {
  return {
    adapter: adapter.id,
    family: adapter.family,
    context,
    task: task.id,
    source_sha256: source === null ? null : sha256(Buffer.from(source, "utf8")),
    compile_score: 0,
    run_score: 0,
    score: 0,
    status: "failed",
  };
}

async function writeFixtures(task, caseDir) {
  await fs.writeFile(path.join(caseDir, "package.jet"), `name: "cold-agent-case"
version: "0.1.0"
edition: "2026"
authority: .{ holds: { allow: [Env, Exec, FS, Net, IO, Mem.Alloc, Mem.Rc, Panic, Time] } }
`, "utf8");
  for (const [name, value] of Object.entries(task.files ?? {})) {
    const target = path.join(caseDir, name);
    await fs.mkdir(path.dirname(target), { recursive: true });
    await fs.writeFile(target, value, "utf8");
  }
}

function hashExpected(task) {
  return sha256(Buffer.from(task.mode === "batch" ? task.expected_stdout : "", "utf8"));
}

async function runBatchCase(task, adapter, context, source, caseDir, jetBin) {
  const row = baseRow(adapter, context, task, source);
  await writeFixtures(task, caseDir);
  await fs.writeFile(path.join(caseDir, "candidate.jet"), source, "utf8");
  const check = spawnJet(jetCheckCommand(jetBin), caseDir);
  const checkResult = await check.result;
  row.stderr_sha256 = sha256(checkResult.stderr);
  if (checkResult.code !== 0) {
    row.error = fixedFailure("compile", checkResult);
    return row;
  }
  row.compile_score = 1;
  const run = spawnJet(jetRunCommand(jetBin, task.args), caseDir, { input: task.stdin });
  const runResult = await run.result;
  row.stdout_sha256 = sha256(runResult.stdout);
  row.stderr_sha256 = sha256(runResult.stderr);
  row.expected_stdout_sha256 = hashExpected(task);
  const expected = Buffer.from(task.expected_stdout, "utf8");
  if (runResult.code !== 0) {
    row.error = fixedFailure("run", runResult);
    return row;
  }
  if (!runResult.stdout.equals(expected)) {
    row.error = "run:stdout-mismatch";
    return row;
  }
  if (runResult.stderr.length !== 0) {
    row.error = "run:stderr-mismatch";
    return row;
  }
  row.run_score = 1;
  row.score = 1;
  row.status = "passed";
  return row;
}

function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : null;
      server.close((error) => error ? reject(error) : resolve(port));
    });
  });
}

function requestHttp(port, probe, timeoutMs = serviceProbeTimeoutMs) {
  return new Promise((resolve) => {
    let settled = false;
    const finish = (value) => {
      if (settled) return;
      settled = true;
      resolve(value);
    };
    const request = http.request({
      host: "127.0.0.1",
      port,
      path: probe.path,
      method: probe.method,
      headers: probe.body === undefined ? {} : { "content-type": "text/plain", "content-length": Buffer.byteLength(probe.body, "utf8") },
    }, (response) => {
      const chunks = [];
      let bytes = 0;
      response.on("data", (chunk) => {
        bytes += chunk.length;
        if (bytes <= 1_048_576) chunks.push(chunk);
      });
      response.on("end", () => finish({
        status: response.statusCode ?? 0,
        body: Buffer.concat(chunks).toString("utf8"),
        headers: response.headers,
        error: bytes > 1_048_576 ? "response-too-large" : null,
      }));
      response.on("error", (error) => finish({ status: 0, body: "", headers: {}, error: error.message }));
    });
    request.setTimeout(timeoutMs, () => {
      request.destroy();
      finish({ status: 0, body: "", headers: {}, error: "timeout" });
    });
    request.once("error", (error) => finish({ status: 0, body: "", headers: {}, error: error.message }));
    if (probe.body !== undefined) request.write(probe.body);
    request.end();
  });
}

async function waitForReady(tracked, port, service) {
  const started = Date.now();
  while (Date.now() - started < serviceReadyTimeoutMs) {
    if (!tracked.child || tracked.child.exitCode !== null) return false;
    const response = await requestHttp(port, { method: "GET", path: service.ready_path }, 500);
    if (response.status >= 200 && response.status < 500) return true;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return false;
}

function probeMatches(probe, response) {
  const contentType = probe.content_type;
  if (response.status !== probe.status || response.body !== probe.body) return false;
  if (contentType && !String(response.headers["content-type"] ?? "").toLowerCase().startsWith(contentType.toLowerCase())) return false;
  return true;
}

async function runHttpCase(task, adapter, context, source, caseDir, jetBin) {
  const row = baseRow(adapter, context, task, source);
  await writeFixtures(task, caseDir);
  await fs.writeFile(path.join(caseDir, "candidate.jet"), source, "utf8");
  const check = spawnJet(jetCheckCommand(jetBin), caseDir);
  const checkResult = await check.result;
  row.stderr_sha256 = sha256(checkResult.stderr);
  if (checkResult.code !== 0) {
    row.error = fixedFailure("compile", checkResult);
    return row;
  }
  row.compile_score = 1;
  const port = await freePort();
  const service = spawnJet(jetRunCommand(jetBin, [String(port)]), caseDir);
  try {
    if (!(await waitForReady(service, port, task.service))) {
      row.error = "run:readiness";
      return row;
    }
    for (const [index, probe] of task.service.probes.entries()) {
      const response = await requestHttp(port, probe);
      if (!probeMatches(probe, response)) {
        row.error = `run:probe-${index}`;
        return row;
      }
    }
    const exit = await waitForTracked(service, serviceShutdownTimeoutMs);
    if (!exit || exit.code !== 0) {
      if (exit) {
        row.stdout_sha256 = sha256(exit.stdout);
        row.stderr_sha256 = sha256(exit.stderr);
      }
      row.error = !exit ? "run:shutdown-timeout" : fixedFailure("run", exit);
      return row;
    }
    row.stdout_sha256 = sha256(exit.stdout);
    row.stderr_sha256 = sha256(exit.stderr);
    if (exit.stdout.length !== 0 || exit.stderr.length !== 0) {
      row.error = exit.stdout.length !== 0 ? "run:stdout-mismatch" : "run:stderr-mismatch";
      return row;
    }
    row.run_score = 1;
    row.score = 1;
    row.status = "passed";
    return row;
  } finally {
    if (service.child?.exitCode === null) {
      service.terminate();
      const exit = await waitForTracked(service, 1_000);
      if (!exit) {
        service.terminate("SIGKILL");
        await service.result;
      }
    }
  }
}

async function runCase(task, adapter, context, source, rootDir, jetBin) {
  const caseDir = await fs.mkdtemp(path.join(rootDir, "case-"));
  try {
    if (task.mode === "http") return await runHttpCase(task, adapter, context, source, caseDir, jetBin);
    return await runBatchCase(task, adapter, context, source, caseDir, jetBin);
  } finally {
    await fs.rm(caseDir, { recursive: true, force: true });
  }
}

function extractSource(text) {
  let value = text.trim();
  if (value.startsWith("{")) {
    try {
      const parsed = JSON.parse(value);
      if (typeof parsed.source === "string") value = parsed.source.trim();
    } catch { /* raw Jet may begin with a brace; source validation below decides */ }
  }
  const fenced = value.match(/^```(?:jet)?\s*\r?\n([\s\S]*?)\r?\n```$/iu);
  if (fenced) value = fenced[1].trim();
  if (!value.includes("fn run")) throw new ModelOutputError("model output has no fn run entry point");
  if (value.startsWith("```") || value.includes("\n```")) throw new ModelOutputError("model output contains an unclosed Markdown fence");
  return `${value}\n`;
}
async function callCommandAdapter(adapter, prompt, metadata, env, cwd) {
  const command = commandFromEnvironment(adapter, env);
  const input = adapter.command?.input ?? "json-stdin";
  const request = JSON.stringify({
    schema: "jet.cold-agent.request.v1",
    adapter: adapter.id,
    family: adapter.family,
    task: metadata.task,
    context: metadata.context,
    prompt,
  }) + "\n";
  const argv = input === "prompt-argument" ? [...command, prompt] : command;
  const processResult = await spawnTracked(argv, cwd, {
    env,
    input: input === "prompt-argument" ? null : request,
    timeoutMs: defaultTimeoutMs,
  }).result;
  if (processResult.spawnError || processResult.code !== 0 || processResult.timedOut || processResult.outputLimit) {
    throw new AdapterBlockedError(`${adapter.id}: command ${fixedFailure("adapter", processResult)}`);
  }
  if (processResult.stdout.length > modelOutputLimitBytes) throw new AdapterBlockedError(`${adapter.id}: model output exceeded limit`);
  return processResult.stdout.toString("utf8");
}

async function readApiResponse(response, adapter) {
  const declaredLength = response.headers.get("content-length");
  if (declaredLength !== null && Number(declaredLength) > modelOutputLimitBytes) {
    throw new AdapterBlockedError(`${adapter.id}: API response exceeded limit`);
  }
  if (!response.body) return "";
  const reader = response.body.getReader();
  const chunks = [];
  let bytes = 0;
  try {
    while (true) {
      const part = await reader.read();
      if (part.done) break;
      bytes += part.value.byteLength;
      if (bytes > modelOutputLimitBytes) {
        await reader.cancel();
        throw new AdapterBlockedError(`${adapter.id}: API response exceeded limit`);
      }
      chunks.push(Buffer.from(part.value));
    }
  } catch (error) {
    if (error instanceof AdapterBlockedError) throw error;
    throw new AdapterBlockedError(`${adapter.id}: API response could not be read`);
  }
  return Buffer.concat(chunks).toString("utf8");
}

async function callApiAdapter(adapter, prompt, env) {
  const api = adapter.api;
  const endpoint = String(env[api.endpoint_env] ?? "").trim();
  const key = String(env[api.api_key_env] ?? "").trim();
  const model = String(env[api.model_env] ?? "").trim();
  const headers = { "content-type": "application/json" };
  let body;
  if (api.protocol === "openai-chat") {
    headers.authorization = `Bearer ${key}`;
    body = JSON.stringify({ model, messages: [{ role: "user", content: prompt }], temperature: 0, seed: 0, max_tokens: 4096 });
  } else {
    throw new HarnessUsageError(`${adapter.id}: unsupported API protocol ${api.protocol}`);
  }
  if (typeof fetch !== "function") throw new AdapterBlockedError(`${adapter.id}: this Node runtime has no fetch`);
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), defaultTimeoutMs);
  timer.unref?.();
  let response;
  try {
    response = await fetch(endpoint, { method: "POST", headers, body, signal: controller.signal });
  } catch (error) {
    throw new AdapterBlockedError(`${adapter.id}: API request failed: ${error.name === "AbortError" ? "timeout" : "network"}`);
  } finally {
    clearTimeout(timer);
  }
  const responseText = await readApiResponse(response, adapter);
  if (!response.ok) throw new AdapterBlockedError(`${adapter.id}: API returned HTTP ${response.status}`);
  let parsed;
  try {
    parsed = JSON.parse(responseText);
  } catch {
    throw new AdapterBlockedError(`${adapter.id}: API response was not JSON`);
  }
  const value = parsed?.choices?.[0]?.message?.content;
  if (typeof value !== "string") throw new AdapterBlockedError(`${adapter.id}: API response has no message content`);
  return value;
}
async function callAdapter(resolved, prompt, metadata, env, cwd) {
  if (resolved.transport === "command") return callCommandAdapter(resolved.adapter, prompt, metadata, env, cwd);
  return callApiAdapter(resolved.adapter, prompt, env);
}

function promptFor(task, context, mode) {
  const label = mode === "capsule" ? "Jet context capsule" : "truncated llms.text control";
  return [
    "You are solving one isolated Jet programming task.",
    "You have no repository context. Return only one complete Jet source file.",
    `Task ID: ${task.id}`,
    `Domain: ${task.domain}`,
    "",
    task.prompt,
    "",
    `## ${label}`,
    context,
    "",
    "The evaluator will write your response to candidate.jet, run `jet check`, then execute it.",
    "Do not return Markdown, explanations, diagnostics, or a second file.",
  ].join("\n");
}

function summarize(rows) {
  const result = {};
  for (const context of ["capsule", "control"]) {
    const selected = rows.filter((row) => row.context === context);
    const passes = selected.filter((row) => row.score === 1).length;
    const compilePasses = selected.filter((row) => row.compile_score === 1).length;
    const runPasses = selected.filter((row) => row.run_score === 1).length;
    result[context] = {
      cases: selected.length,
      passes,
      compile_passes: compilePasses,
      run_passes: runPasses,
      pass_rate: selected.length === 0 ? null : passes / selected.length,
    };
  }
  return result;
}

function taskDescriptor(task) {
  return {
    id: task.id,
    domain: task.domain,
    mode: task.mode,
    spec_sha256: hashJson(task),
    expected_sha256: hashExpected(task),
  };
}

function adapterDescriptor(resolved, env) {
  const descriptor = {
    id: resolved.adapter.id,
    family: resolved.adapter.family,
    transport: resolved.transport,
  };
  if (resolved.transport === "api") {
    const api = resolved.adapter.api;
    descriptor.protocol = api.protocol;
    descriptor.model = String(env[api.model_env] ?? "").trim();
    descriptor.endpoint_sha256 = sha256(String(env[api.endpoint_env] ?? "").trim());
    if (api.version_env) descriptor.version = String(env[api.version_env] ?? "").trim();
  } else {
    descriptor.command_sha256 = hashJson(commandFromEnvironment(resolved.adapter, env));
  }
  return descriptor;
}

function rowKey(row) {
  return `${row.adapter}\u0000${row.context}\u0000${row.task}`;
}

function makeReport({ capsule, llms, budgetBytes, tasks, resolved, rows, modes, status, blocked, fixtures, harness, env = process.env }) {
  const expectedRows = tasks.length * resolved.length * modes.length;
  const expectedKeys = new Set(
    resolved.flatMap((resolvedAdapter) => modes.flatMap((context) => tasks.map((task) => (
      `${resolvedAdapter.adapter.id}\u0000${context}\u0000${task.id}`
    )))),
  );
  const rowKeys = rows.map(rowKey);
  const complete = rows.length === expectedRows
    && new Set(rowKeys).size === rows.length
    && rowKeys.every((key) => expectedKeys.has(key));
  const reportStatus = status ?? (blocked.length > 0 || !complete ? "blocked" : "recorded");
  return {
    schema: "jet.cold-agent.scoreboard.v1",
    status: reportStatus,
    contexts: [...modes],
    required_families: [...requiredAdapterFamilies],
    capsule,
    control: {
      path: llms.path,
      source_bytes: llms.bytes,
      source_sha256: llms.sha256,
      context_budget_bytes: budgetBytes,
      context_sha256: llms.context_sha256,
    },
    fixtures,
    harness,
    adapters: resolved.map((resolvedAdapter) => adapterDescriptor(resolvedAdapter, env)),
    tasks: tasks.map(taskDescriptor),
    rows: [...rows].sort((left, right) => utf8Compare(rowKey(left), rowKey(right))),
    summary: reportStatus === "recorded" && complete ? summarize(rows) : null,
    blocked_reasons: [...new Set(blocked)].sort(),
  };
}

export function baselineFromReport(report) {
  if (report.status !== "recorded" || !report.summary) throw new HarnessUsageError("cannot record a baseline from an incomplete or blocked run");
  if (!report.contexts?.includes("capsule")) throw new HarnessUsageError("cannot record a baseline without capsule results");
  if (!report.control || !report.fixtures || !report.harness || !Array.isArray(report.adapters)
    || !Array.isArray(report.tasks) || !Array.isArray(report.required_families) || !Array.isArray(report.rows)) {
    throw new HarnessUsageError("cannot record a baseline without reproducibility metadata");
  }
  const rows = report.rows.filter((row) => row.context === "capsule");
  if (rows.length === 0 || report.summary.capsule?.cases !== rows.length) {
    throw new HarnessUsageError("cannot record a baseline without a complete capsule matrix");
  }
  return {
    schema: "jet.cold-agent.baseline.v1",
    status: "recorded",
    contexts: ["capsule"],
    required_families: report.required_families,
    capsule: report.capsule,
    control: report.control,
    context_budget_bytes: report.control.context_budget_bytes,
    fixtures: report.fixtures,
    harness: report.harness,
    adapters: report.adapters,
    tasks: report.tasks,
    rows,
    summary: { capsule: report.summary.capsule },
  };
}

function descriptorSha(value, label) {
  if (!value || typeof value !== "object" || typeof value.sha256 !== "string" || !Number.isInteger(value.bytes) || typeof value.path !== "string") {
    throw new AdapterBlockedError(`baseline ${label} descriptor is incomplete`);
  }
  return value.sha256;
}

function validateControl(control) {
  if (!control || typeof control.path !== "string" || !Number.isInteger(control.source_bytes)
    || typeof control.source_sha256 !== "string" || !Number.isInteger(control.context_budget_bytes)
    || typeof control.context_sha256 !== "string") {
    throw new AdapterBlockedError("baseline control descriptor is incomplete");
  }
}

function validateBaselineRows(baseline) {
  if (!Array.isArray(baseline.adapters) || baseline.adapters.length < requiredAdapterFamilies.length) {
    throw new AdapterBlockedError("baseline has no complete adapter identity");
  }
  if (!Array.isArray(baseline.tasks) || baseline.tasks.length === 0) {
    throw new AdapterBlockedError("baseline has no task identity");
  }
  const families = new Set(baseline.adapters.map((adapter) => adapter?.family));
  if (requiredAdapterFamilies.some((family) => !families.has(family))) {
    throw new AdapterBlockedError("baseline does not cover both required model families");
  }
  if (families.size !== baseline.adapters.length) {
    throw new AdapterBlockedError("baseline adapter families are duplicated");
  }
  const adapterIds = new Set();
  for (const adapter of baseline.adapters) {
    if (!adapter || typeof adapter.id !== "string" || adapterIds.has(adapter.id)) {
      throw new AdapterBlockedError("baseline adapter identities are invalid");
    }
    adapterIds.add(adapter.id);
  }
  const taskIds = new Set();
  for (const task of baseline.tasks) {
    if (!task || typeof task.id !== "string" || taskIds.has(task.id)) {
      throw new AdapterBlockedError("baseline task identities are invalid");
    }
    taskIds.add(task.id);
  }
  const rows = baseline.rows;
  if (!Array.isArray(rows) || rows.length !== baseline.adapters.length * baseline.tasks.length) {
    throw new AdapterBlockedError("baseline capsule matrix is incomplete");
  }
  const seen = new Set();
  for (const row of rows) {
    const key = `${row?.adapter}\u0000${row?.task}`;
    if (!row || row.context !== "capsule" || typeof row.adapter !== "string" || !adapterIds.has(row.adapter)
      || typeof row.task !== "string" || !taskIds.has(row.task) || seen.has(key)
      || ![row.compile_score, row.run_score, row.score].every((score) => score === 0 || score === 1)) {
      throw new AdapterBlockedError("baseline capsule rows are invalid");
    }
    seen.add(key);
  }
}

function validateRecordedBaseline(baseline) {
  if (baseline?.schema !== "jet.cold-agent.baseline.v1" || baseline.status !== "recorded") {
    throw new AdapterBlockedError("baseline is not recorded; run --record-baseline with real adapter results first");
  }
  if (!Array.isArray(baseline.contexts) || !baseline.contexts.includes("capsule")) {
    throw new AdapterBlockedError("baseline has no capsule context");
  }
  if (!Array.isArray(baseline.required_families)
    || requiredAdapterFamilies.some((family) => !baseline.required_families.includes(family))) {
    throw new AdapterBlockedError("baseline does not declare both required model families");
  }
  descriptorSha(baseline.capsule, "capsule");
  validateControl(baseline.control);
  descriptorSha(baseline.fixtures?.task_file, "task fixture");
  descriptorSha(baseline.fixtures?.adapter_file, "adapter fixture");
  descriptorSha(baseline.harness, "harness");
  if (baseline.context_budget_bytes !== baseline.control.context_budget_bytes) {
    throw new AdapterBlockedError("baseline context budget is inconsistent");
  }
  if (!baseline.summary?.capsule || !Number.isInteger(baseline.summary.capsule.cases)) {
    throw new AdapterBlockedError("baseline summary is incomplete");
  }
  validateBaselineRows(baseline);
  const summary = baseline.summary.capsule;
  if (summary.cases !== baseline.rows.length || !Number.isInteger(summary.passes)
    || !Number.isInteger(summary.compile_passes) || !Number.isInteger(summary.run_passes)
    || summary.passes < 0 || summary.passes > summary.cases
    || summary.compile_passes < 0 || summary.compile_passes > summary.cases
    || summary.run_passes < 0 || summary.run_passes > summary.cases
    || summary.pass_rate !== summary.passes / summary.cases) {
    throw new AdapterBlockedError("baseline summary does not cover its capsule matrix");
  }
}

function requireIdentityMatch(report, baseline, field, message) {
  if (JSON.stringify(canonicalize(report[field])) !== JSON.stringify(canonicalize(baseline[field]))) {
    throw new RegressionError(message);
  }
}

export function compareBaseline(report, baseline) {
  validateRecordedBaseline(baseline);
  if (report.status !== "recorded") throw new RegressionError("current scoreboard is incomplete");
  if (!Array.isArray(report.contexts) || !report.contexts.includes("capsule") || !report.summary?.capsule) {
    throw new RegressionError("current scoreboard has no complete capsule context");
  }
  if (!Array.isArray(report.rows)) throw new RegressionError("current scoreboard has no result rows");
  if (!report.control || typeof report.control.source_sha256 !== "string" || !Number.isInteger(report.control.source_bytes)
    || typeof report.control.context_sha256 !== "string") {
    throw new RegressionError("current scoreboard lacks control provenance");
  }
  if (report.capsule.sha256 !== baseline.capsule?.sha256) throw new RegressionError("capsule artifact changed; record a new baseline");
  if (report.control.context_budget_bytes !== baseline.context_budget_bytes) throw new RegressionError("context budget changed; record a new baseline");
  if (report.control.source_sha256 !== baseline.control.source_sha256
    || report.control.source_bytes !== baseline.control.source_bytes
    || report.control.context_sha256 !== baseline.control.context_sha256) {
    throw new RegressionError("control context changed; record a new baseline");
  }
  if (report.harness?.sha256 !== baseline.harness?.sha256) throw new RegressionError("harness changed; record a new baseline");
  requireIdentityMatch(report, baseline, "required_families", "required model families changed; record a new baseline");
  requireIdentityMatch(report, baseline, "fixtures", "evaluation fixtures changed; record a new baseline");
  requireIdentityMatch(report, baseline, "adapters", "model adapter identity changed; record a new baseline");
  requireIdentityMatch(report, baseline, "tasks", "evaluation task identity changed; record a new baseline");
  const currentCapsuleRows = report.rows.filter((row) => row.context === "capsule");
  if (report.summary.capsule.cases !== currentCapsuleRows.length
    || !currentCapsuleRows.every((row) => [row.compile_score, row.run_score, row.score].every((score) => score === 0 || score === 1))) {
    throw new RegressionError("current capsule summary or rows are invalid");
  }
  const currentRows = new Map(currentCapsuleRows.map((row) => [`${row.adapter}\u0000${row.task}`, row]));
  if (currentRows.size !== baseline.rows.length) throw new RegressionError("current capsule matrix is incomplete");
  const failures = [];
  for (const expected of baseline.rows ?? []) {
    const current = currentRows.get(`${expected.adapter}\u0000${expected.task}`);
    if (!current) {
      failures.push(`${expected.adapter}/${expected.task}:missing`);
      continue;
    }
    if (current.compile_score < Number(expected.compile_score ?? 0)) failures.push(`${expected.adapter}/${expected.task}:compile`);
    if (current.run_score < Number(expected.run_score ?? 0)) failures.push(`${expected.adapter}/${expected.task}:run`);
    if (current.score < Number(expected.score ?? 0)) failures.push(`${expected.adapter}/${expected.task}:score`);
  }
  const baselineRate = Number(baseline.summary?.capsule?.pass_rate);
  const currentRate = Number(report.summary?.capsule?.pass_rate);
  if (Number.isFinite(baselineRate) && Number.isFinite(currentRate) && currentRate < baselineRate) failures.push("capsule:pass-rate");
  if (failures.length > 0) throw new RegressionError(`capsule baseline regression: ${failures.sort().join(", ")}`);
}

async function writeReport(file, report) {
  await fs.mkdir(path.dirname(file), { recursive: true });
  await fs.writeFile(file, `${JSON.stringify(report, null, 2)}\n`, "utf8");
}

async function prepareInputs(options) {
  const capsuleText = await fs.readFile(options.capsule, "utf8");
  validateCapsule(capsuleText);
  const llmsText = await fs.readFile(options.llms, "utf8");
  const budgetBytes = Buffer.byteLength(capsuleText, "utf8");
  if (Buffer.byteLength(llmsText, "utf8") < budgetBytes) throw new HarnessUsageError("llms.text is shorter than the capsule budget");
  const controlText = buildControlContext(llmsText, budgetBytes);
  const tasks = loadTasks(await readJson(options.tasks));
  const adapters = loadAdapters(await readJson(options.config));
  const capsule = await fileDescriptor(options.capsule, capsuleText);
  const llms = await fileDescriptor(options.llms, llmsText);
  const fixtures = {
    task_file: await fileDescriptor(options.tasks),
    adapter_file: await fileDescriptor(options.config),
  };
  const harness = await fileDescriptor(fileURLToPath(import.meta.url));
  llms.context_sha256 = sha256(Buffer.from(controlText, "utf8"));
  return { capsuleText, controlText, budgetBytes, tasks, adapters, capsule, llms, fixtures, harness };
}

async function run(options) {
  const inputs = await prepareInputs(options);
  const modes = options.mode === "both" ? ["capsule", "control"] : [options.mode];
  const { resolved, blocked: preflightBlocked } = preflightAdapters(inputs.adapters, process.env);
  if (preflightBlocked.length > 0 || resolved.length === 0) {
    const report = makeReport({
      capsule: inputs.capsule,
      llms: inputs.llms,
      budgetBytes: inputs.budgetBytes,
      tasks: inputs.tasks,
      resolved: [],
      rows: [],
      modes,
      blocked: preflightBlocked,
      fixtures: inputs.fixtures,
      harness: inputs.harness,
    });
    await writeReport(options.output, report);
    for (const reason of preflightBlocked) console.error(`BLOCKED: ${reason}`);
    return unavailableExit;
  }
  const contexts = { capsule: inputs.capsuleText, control: inputs.controlText };
  const rows = [];
  const blocked = [...preflightBlocked];
  const sharedScratchRoot = path.resolve(process.env.JET_TEST_SCRATCH || process.env.JET_TEST_SCRATCH_DIR || path.join(homedir(), ".cache/jet-test-scratch"));
  const scratchRoot = path.resolve(process.env.JET_AGENT_EVAL_SCRATCH_DIR || path.join(sharedScratchRoot, "agent-eval"));
  const adapterScratchRoot = path.resolve(process.env.JET_AGENT_EVAL_ADAPTER_SCRATCH_DIR || path.join(sharedScratchRoot, "agent-eval-adapters"));
  await fs.mkdir(scratchRoot, { recursive: true });
  await fs.mkdir(adapterScratchRoot, { recursive: true });
  for (const resolvedAdapter of resolved) {
    for (const mode of modes) {
      for (const task of inputs.tasks) {
        const prompt = promptFor(task, contexts[mode], mode);
        const adapterCwd = await fs.mkdtemp(path.join(adapterScratchRoot, "request-"));
        let response;
        try {
          response = await callAdapter(resolvedAdapter, prompt, { task: task.id, context: mode }, process.env, adapterCwd);
        } catch (error) {
          if (error instanceof AdapterBlockedError) {
            blocked.push(error.message);
            continue;
          }
          throw error;
        } finally {
          await fs.rm(adapterCwd, { recursive: true, force: true });
        }
        let source;
        try {
          source = extractSource(response);
        } catch (error) {
          const row = baseRow(resolvedAdapter.adapter, mode, task);
          row.error = error instanceof ModelOutputError ? "model-output" : "model-output-parse";
          rows.push(row);
          continue;
        }
        rows.push(await runCase(task, resolvedAdapter.adapter, mode, source, scratchRoot, options.jetBin));
      }
    }
  }
  const report = makeReport({
    capsule: inputs.capsule,
    llms: inputs.llms,
    budgetBytes: inputs.budgetBytes,
    tasks: inputs.tasks,
    resolved,
    rows,
    modes,
    blocked,
    fixtures: inputs.fixtures,
    harness: inputs.harness,
  });
  await writeReport(options.output, report);
  if (blocked.length > 0 || report.status !== "recorded") {
    for (const reason of blocked) console.error(`BLOCKED: ${reason}`);
    return unavailableExit;
  }
  if (options.recordBaseline) {
    await writeReport(options.baseline, baselineFromReport(report));
  }
  if (options.checkBaseline) {
    let baseline;
    try {
      baseline = await readJson(options.baseline);
      compareBaseline(report, baseline);
    } catch (error) {
      if (error instanceof AdapterBlockedError) {
        console.error(`BLOCKED: ${error.message}`);
        return unavailableExit;
      }
      if (error instanceof RegressionError) {
        console.error(`REGRESSION: ${error.message}`);
        return 1;
      }
      throw error;
    }
  }
  return 0;
}

export async function main(argv = process.argv.slice(2)) {
  let options;
  try {
    options = parseArgs(argv);
  } catch (error) {
    console.error(`ERROR: ${error.message}`);
    console.error(usage());
    return usageExit;
  }
  if (options.help) {
    console.log(usage());
    return 0;
  }
  try {
    return await run(options);
  } catch (error) {
    const prefix = error instanceof AdapterBlockedError ? "BLOCKED" : "ERROR";
    console.error(`${prefix}: ${error.message}`);
    return error instanceof AdapterBlockedError ? unavailableExit : usageExit;
  }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const code = await main();
  process.exitCode = code;
}
