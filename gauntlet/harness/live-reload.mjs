import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { promises as fs } from "node:fs";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { performance } from "node:perf_hooks";

const harnessDir = path.dirname(fileURLToPath(import.meta.url));
const defaultRepoDir = path.resolve(harnessDir, "../..");
const defaultEnvRunner = path.join(defaultRepoDir, "scripts/agent/jet-env");
const AXIS_OUTPUT_LIMIT = 4_000;
const AXIS_HTTP_BODY_LIMIT = 64 * 1024;
const PROCESS_TERM_TIMEOUT_MS = 5_000;
const PROCESS_KILL_TIMEOUT_MS = 1_000;

function monotonicNow() {
  return performance.now();
}

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

async function fileSha256(file) {
  return sha256(await fs.readFile(file));
}

function shellQuote(value) {
  return `'${String(value).replaceAll("'", "'\\''")}'`;
}

function axisRepoPath(repoDir, relative) {
  if (typeof relative !== "string" || relative.length === 0 || path.isAbsolute(relative)) {
    throw new Error(`axis source must be a relative repository path: ${relative}`);
  }
  const resolved = path.resolve(repoDir, relative);
  const remainder = path.relative(repoDir, resolved);
  if (!remainder || remainder.startsWith("..") || path.isAbsolute(remainder)) {
    throw new Error(`axis source escapes repository: ${relative}`);
  }
  return resolved;
}

function axisStagePath(stageDir, relative) {
  if (typeof relative !== "string" || relative.length === 0 || path.isAbsolute(relative)) {
    throw new Error(`axis target must be a relative path: ${relative}`);
  }
  const resolved = path.resolve(stageDir, relative);
  const remainder = path.relative(stageDir, resolved);
  if (!remainder || remainder.startsWith("..") || path.isAbsolute(remainder)) {
    throw new Error(`axis target escapes runner stage: ${relative}`);
  }
  return resolved;
}

function axisCommand(command, replacements) {
  if (!Array.isArray(command) || command.length === 0 || command.some((part) => typeof part !== "string")) {
    throw new Error("axis command must be a non-empty string array");
  }
  return command.map((part) => {
    const expanded = Object.entries(replacements).reduce(
      (value, [needle, replacement]) => value.replaceAll(`{${needle}}`, String(replacement)),
      part,
    );
    return expanded === "jet" && replacements.jet_bin ? replacements.jet_bin : expanded;
  });
}

async function stageAxisFiles(repoDir, stageDir, files) {
  if (!Array.isArray(files) || files.length === 0) throw new Error("axis runner declares no files");
  const targets = new Set();
  const staged = [];
  for (const file of files) {
    if (!file || typeof file.source !== "string" || typeof file.target !== "string") {
      throw new Error("axis runner file must declare source and target");
    }
    const source = axisRepoPath(repoDir, file.source);
    const target = axisStagePath(stageDir, file.target);
    const targetKey = path.relative(stageDir, target).replaceAll(path.sep, "/");
    if (targets.has(targetKey)) throw new Error(`axis runner declares duplicate target: ${targetKey}`);
    targets.add(targetKey);
    const stat = await fs.stat(source);
    if (!stat.isFile()) throw new Error(`axis source is not a file: ${file.source}`);
    await fs.mkdir(path.dirname(target), { recursive: true });
    await fs.copyFile(source, target);
    staged.push({
      source: path.relative(repoDir, source).replaceAll(path.sep, "/"),
      target: targetKey,
      bytes: stat.size,
      sha256: await fileSha256(source),
    });
  }
  return staged;
}

async function runProcess(envRunner, cwd, args, { timeoutMs = 10_000 } = {}) {
  return new Promise((resolve) => {
    const child = spawn(envRunner, ["sh", "-c", args.map(shellQuote).join(" ")], {
      cwd,
      env: process.env,
      stdio: ["ignore", "pipe", "pipe"],
      detached: true,
    });
    const stdout = [];
    const stderr = [];
    let timedOut = false;
    let settled = false;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(deadline);
      resolve(result);
    };
    const deadline = setTimeout(() => {
      timedOut = true;
      try { process.kill(-child.pid, "SIGKILL"); } catch {}
      try { child.kill("SIGKILL"); } catch {}
    }, timeoutMs);
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    child.once("error", (error) => finish({
      code: 127,
      signal: null,
      stdout: Buffer.concat(stdout),
      stderr: Buffer.from(String(error)),
      timedOut,
    }));
    child.once("close", (code, signal) => finish({
      code: timedOut ? 124 : (code ?? 128),
      signal,
      stdout: Buffer.concat(stdout),
      stderr: Buffer.concat(stderr),
      timedOut,
    }));
  });
}

async function probeAxisTool(envRunner, cwd, tool, jetBin) {
  if (tool === "jet") {
    const resolved = path.resolve(jetBin);
    try {
      const stat = await fs.stat(resolved);
      if (!stat.isFile()) return { tool, status: "probe_failed", reason: `Jet binary is not a file: ${resolved}` };
      const versionResult = await runProcess(envRunner, cwd, [resolved, "--version"]);
      const versionOutput = versionResult.stdout.toString("utf8").trim() || versionResult.stderr.toString("utf8").trim();
      return {
        tool,
        status: "available",
        resolved,
        sha256: await fileSha256(resolved),
        version: versionOutput.split(/\r?\n/, 1)[0].slice(0, 300),
        version_exit_code: versionResult.code,
      };
    } catch (error) {
      if (error.code === "ENOENT") return { tool, status: "unavailable", reason: `Jet binary is absent: ${resolved}` };
      return { tool, status: "probe_failed", reason: `could not inspect Jet binary ${resolved}: ${error.message}` };
    }
  }
  const result = await runProcess(envRunner, cwd, ["sh", "-c", `command -v ${shellQuote(tool)}`]);
  const output = result.stdout.toString("utf8").trim();
  if (result.code === 0 && output) {
    const resolved = output.split(/\r?\n/, 1)[0];
    const versionFlag = tool === "entr" ? "-V" : "--version";
    const versionResult = await runProcess(envRunner, cwd, [tool, versionFlag]);
    const versionOutput = versionResult.stdout.toString("utf8").trim() || versionResult.stderr.toString("utf8").trim();
    return {
      tool,
      status: "available",
      resolved,
      version: versionOutput.split(/\r?\n/, 1)[0].slice(0, 300),
      version_exit_code: versionResult.code,
    };
  }
  if (result.code === 1 && !output && !result.stderr.toString("utf8").trim()) {
    return { tool, status: "unavailable", reason: `${tool} is absent from the declared tool environment` };
  }
  return {
    tool,
    status: "probe_failed",
    reason: `could not determine whether ${tool} is available (exit ${result.code})`,
    exit_code: result.code,
    stderr: result.stderr.toString("utf8").trim().slice(0, AXIS_OUTPUT_LIMIT),
  };
}

async function probeAxisTools(envRunner, cwd, tools, jetBin) {
  if (!Array.isArray(tools) || tools.length === 0) throw new Error("axis runner declares no tools");
  const probes = [];
  for (const tool of tools) probes.push(await probeAxisTool(envRunner, cwd, tool, jetBin));
  return probes;
}

async function freeTcpPort() {
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  const port = typeof address === "object" && address ? address.port : null;
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  if (!Number.isInteger(port) || port < 1) throw new Error("could not allocate a local TCP port");
  return port;
}

function sleep(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function childFailure(child) {
  if (child?.spawnError) return `process spawn failed: ${child.spawnError.message}`;
  if (child && child.exitCode !== null) return `process exited before readiness (exit ${child.exitCode ?? child.signalCode ?? "unknown"})`;
  return null;
}

function httpProbe(port, probe, timeoutMs = 5_000) {
  return new Promise((resolve) => {
    const started = monotonicNow();
    const chunks = [];
    let bytes = 0;
    let finished = false;
    const finish = (result) => {
      if (finished) return;
      finished = true;
      resolve({ ...result, latency_ms: monotonicNow() - started });
    };
    const body = probe.body === undefined ? undefined : String(probe.body);
    const request = http.request({
      host: "127.0.0.1",
      port,
      path: probe.path,
      method: probe.method ?? "GET",
      headers: body === undefined ? undefined : { "content-length": Buffer.byteLength(body) },
      timeout: timeoutMs,
    }, (response) => {
      response.on("data", (chunk) => {
        bytes += chunk.length;
        if (bytes > AXIS_HTTP_BODY_LIMIT) {
          request.destroy(new Error(`HTTP probe body exceeded ${AXIS_HTTP_BODY_LIMIT} bytes`));
          return;
        }
        chunks.push(chunk);
      });
      response.once("error", (error) => finish({ ok: false, error: error.message }));
      response.once("end", () => finish({
        ok: true,
        status: response.statusCode,
        body: Buffer.concat(chunks).toString("utf8"),
      }));
    });
    request.setTimeout(timeoutMs, () => request.destroy(new Error("HTTP probe timeout")));
    request.on("error", (error) => finish({ ok: false, error: error.message }));
    if (body !== undefined) request.write(body);
    request.end();
  });
}

async function waitForAxisReady(child, port, readiness, timeoutMs, pollIntervalMs, previousValue = null) {
  if (!readiness || typeof readiness.path !== "string" || readiness.path.length === 0) {
    throw new Error("axis runner readiness must declare a path");
  }
  const started = monotonicNow();
  const expectedStatus = readiness.status ?? 200;
  let lastReason = "readiness endpoint did not return a numeric counter";
  while (monotonicNow() - started < timeoutMs) {
    const failure = childFailure(child);
    if (failure) throw new Error(failure);
    const remaining = Math.max(1, timeoutMs - (monotonicNow() - started));
    const result = await httpProbe(port, { path: readiness.path }, Math.min(1_000, remaining));
    if (result.ok && result.status === expectedStatus) {
      const body = result.body.trim();
      const value = Number(body);
      if (body.length > 0 && Number.isSafeInteger(value) && value >= 0 && (previousValue === null || value > previousValue)) {
        const observedAt = monotonicNow();
        return { value, body: result.body, status: result.status, waited_ms: observedAt - started, observed_at_ms: observedAt };
      }
      lastReason = previousValue === null
        ? `readiness body was not a non-negative integer: ${JSON.stringify(result.body)}`
        : `readiness counter ${JSON.stringify(result.body)} did not exceed ${previousValue}`;
    } else if (!result.ok) {
      lastReason = result.error;
    } else {
      lastReason = `readiness status ${result.status}, expected ${expectedStatus}`;
    }
    await sleep(Math.min(pollIntervalMs, Math.max(1, timeoutMs - (monotonicNow() - started))));
  }
  throw new Error(`${readiness.path} was not ready within ${timeoutMs}ms: ${lastReason}`);
}

function validateAxisBudget(axis) {
  const budget = axis?.budget;
  const from = axis?.edit?.from;
  const to = axis?.edit?.to;
  if (!budget || !Number.isInteger(budget.sample_count) || budget.sample_count < 1 || budget.sample_count > 100 ||
    !Number.isFinite(budget.startup_timeout_ms) || budget.startup_timeout_ms <= 0 || budget.startup_timeout_ms > 600_000 ||
    !Number.isFinite(budget.reload_timeout_ms) || budget.reload_timeout_ms <= 0 || budget.reload_timeout_ms > 600_000 ||
    !Number.isFinite(budget.poll_interval_ms) || budget.poll_interval_ms <= 0 || budget.poll_interval_ms > 10_000 ||
    typeof from !== "string" || from.length === 0 || typeof to !== "string" || to.length === 0 || from === to) {
    throw new Error("invalid live-reload budget or edit markers");
  }
  return { ...budget, edit_from: from, edit_to: to };
}

function outputSpec(runner) {
  const spec = runner.output ?? runner.acknowledgement ?? runner.output_acknowledgement;
  if (!spec || typeof spec.path !== "string" || spec.path.length === 0) {
    throw new Error("axis runner must declare output acknowledgement path");
  }
  if (spec.status !== undefined && (!Number.isInteger(spec.status) || spec.status < 100 || spec.status > 599)) {
    throw new Error("axis output acknowledgement status must be an HTTP status");
  }
  return spec;
}

function markerMatches(body, expectedMarker, staleMarker) {
  if (typeof body !== "string" || typeof expectedMarker !== "string" || expectedMarker.length === 0 ||
    !body.includes(expectedMarker)) return false;
  return typeof staleMarker !== "string" || staleMarker.length === 0 || !body.includes(staleMarker);
}
async function waitForAxisOutput(child, port, output, expectedMarker, staleMarker, timeoutMs, pollIntervalMs) {
  const started = monotonicNow();
  const expectedStatus = output.status ?? 200;
  let lastReason = `output did not acknowledge ${JSON.stringify(expectedMarker)}`;
  while (monotonicNow() - started < timeoutMs) {
    const failure = childFailure(child);
    if (failure) throw new Error(failure);
    const remaining = Math.max(1, timeoutMs - (monotonicNow() - started));
    const result = await httpProbe(port, { path: output.path }, Math.min(1_000, remaining));
    if (result.ok && result.status === expectedStatus && markerMatches(result.body, expectedMarker, staleMarker)) {
      const observedAt = monotonicNow();
      return {
        path: output.path,
        status: result.status,
        body: result.body,
        body_sha256: sha256(result.body),
        marker: expectedMarker,
        waited_ms: observedAt - started,
        observed_at_ms: observedAt,
      };
    }
    if (!result.ok) lastReason = result.error;
    else if (result.status !== expectedStatus) lastReason = `output status ${result.status}, expected ${expectedStatus}`;
    else lastReason = `output body did not contain only the current marker ${JSON.stringify(expectedMarker)}`;
    await sleep(Math.min(pollIntervalMs, Math.max(1, timeoutMs - (monotonicNow() - started))));
  }
  throw new Error(`${output.path} did not acknowledge ${JSON.stringify(expectedMarker)} within ${timeoutMs}ms: ${lastReason}`);
}

async function waitForAxisState({ child, port, readiness, output, expectedMarker, staleMarker, timeoutMs, pollIntervalMs, previousValue = null }) {
  const started = monotonicNow();
  const ready = await waitForAxisReady(child, port, readiness, timeoutMs, pollIntervalMs, previousValue);
  const remaining = Math.max(1, timeoutMs - (monotonicNow() - started));
  const acknowledged = await waitForAxisOutput(child, port, output, expectedMarker, staleMarker, remaining, pollIntervalMs);
  return { ready, output: acknowledged, observed_at_ms: acknowledged.observed_at_ms };
}

async function normalizeAxisMarker(file, from, to) {
  const text = await fs.readFile(file, "utf8");
  const fromCount = text.split(from).length - 1;
  const toCount = text.split(to).length - 1;
  if (fromCount === 1 && toCount === 0) return text;
  if (fromCount === 0 && toCount === 1) {
    const baseline = text.replace(to, from);
    await fs.writeFile(file, baseline);
    return baseline;
  }
  throw new Error(`axis edit markers are not unique in ${path.basename(file)} (from=${fromCount}, to=${toCount})`);
}

async function applyAxisEdit(file, from, to) {
  const before = await fs.readFile(file, "utf8");
  const count = before.split(from).length - 1;
  if (count !== 1 || before.split(to).length - 1 !== 0) {
    throw new Error(`axis edit did not find exactly one baseline marker in ${path.basename(file)}`);
  }
  const after = before.replace(from, to);
  await fs.writeFile(file, after);
  return {
    file: path.basename(file),
    from,
    to,
    before_sha256: sha256(before),
    after_sha256: sha256(after),
  };
}

async function restoreAxisEdit(file, from, to) {
  const text = await fs.readFile(file, "utf8").catch(() => null);
  if (text === null || text === undefined) return;
  if (text.split(from).length - 1 === 1 && text.split(to).length - 1 === 0) return;
  if (text.split(to).length - 1 === 1 && text.split(from).length - 1 === 0) {
    await fs.writeFile(file, text.replace(to, from));
    return;
  }
  throw new Error(`axis edit cannot restore ${path.basename(file)} to its baseline marker`);
}

function startProcess(envRunner, cwd, args) {
  const child = spawn(envRunner, ["sh", "-c", args.map(shellQuote).join(" ")], {
    cwd,
    env: process.env,
    stdio: ["ignore", "ignore", "pipe"],
    detached: true,
  });
  const stderr = [];
  child.spawnError = null;
  child.once("error", (error) => { child.spawnError = error; });
  child.stderr.on("data", (chunk) => stderr.push(chunk));
  child.stderrText = () => Buffer.concat(stderr).toString("utf8").trim().slice(0, AXIS_OUTPUT_LIMIT);
  return child;
}

function signalProcessGroup(child, signal) {
  if (!child?.pid) return false;
  try {
    process.kill(-child.pid, signal);
    return true;
  } catch (error) {
    if (error.code === "ESRCH") return false;
    try { child.kill(signal); return true; } catch { return false; }
  }
}

function processGroupAlive(pid) {
  if (!Number.isInteger(pid) || pid < 1) return false;
  try {
    process.kill(-pid, 0);
    return true;
  } catch (error) {
    return error.code === "EPERM";
  }
}

function waitForExit(child, timeoutMs = PROCESS_TERM_TIMEOUT_MS) {
  if (child.exitCode !== null) return Promise.resolve({ code: child.exitCode, signal: child.signalCode });
  return new Promise((resolve) => {
    let finished = false;
    const done = (result) => {
      if (finished) return;
      finished = true;
      clearTimeout(timeout);
      resolve(result);
    };
    const timeout = setTimeout(() => done({ code: null, signal: "TIMEOUT" }), timeoutMs);
    child.once("close", (code, signal) => done({ code, signal }));
  });
}

async function waitForProcessGroupGone(pid, timeoutMs = PROCESS_KILL_TIMEOUT_MS) {
  const started = monotonicNow();
  while (processGroupAlive(pid) && monotonicNow() - started < timeoutMs) await sleep(20);
  return !processGroupAlive(pid);
}

async function cleanupProcessGroup(child) {
  if (!child) return { attempted: false, group_gone: true };
  const pid = child.pid ?? null;
  const term_sent = signalProcessGroup(child, "SIGTERM");
  let exit = await waitForExit(child, PROCESS_TERM_TIMEOUT_MS);
  let forced = false;
  let kill_sent = false;
  if (pid !== null && processGroupAlive(pid)) {
    forced = true;
    kill_sent = signalProcessGroup(child, "SIGKILL");
    exit = await waitForExit(child, PROCESS_KILL_TIMEOUT_MS);
  }
  const group_gone = pid === null ? child.exitCode !== null : await waitForProcessGroupGone(pid);
  return { attempted: true, pid, term_sent, kill_sent, forced, exit, group_gone };
}

function sampleFailure(sample, error) {
  sample.status = "failed";
  sample.reason = error instanceof Error ? error.message : String(error);
}

function validateSampleTimestamps(sample) {
  const timestamps = sample.timestamps;
  if (!timestamps || timestamps.monotonic_clock !== "performance.now" ||
    !Number.isFinite(timestamps.edit_started_at_ms) ||
    !Number.isFinite(timestamps.edit_written_at_ms) ||
    !Number.isFinite(timestamps.observed_at_ms) ||
    timestamps.edit_started_at_ms > timestamps.edit_written_at_ms ||
    timestamps.edit_written_at_ms > timestamps.observed_at_ms) {
    throw new Error("reload sample has invalid non-monotonic timestamps");
  }
  const latency = timestamps.observed_at_ms - timestamps.edit_written_at_ms;
  if (!Number.isFinite(latency) || latency < 0) throw new Error("reload sample has invalid latency");
  sample.reload_latency_ms = latency;
}

export async function runLiveReloadSample({ runner, stageDir, editPath, phase, index, jetBin, budget, envRunner = defaultEnvRunner }) {
  const port = await freeTcpPort();
  const command = axisCommand(runner.command, { port, jet_bin: jetBin });
  const sample = {
    phase,
    index,
    status: "failed",
    fresh_process: true,
    command,
    port,
    timestamps: { monotonic_clock: "performance.now" },
  };
  let output;
  try {
    output = outputSpec(runner);
  } catch (error) {
    sampleFailure(sample, error);
    return sample;
  }
  let child = null;
  try {
    await normalizeAxisMarker(editPath, budget.edit_from, budget.edit_to);
    child = startProcess(envRunner, stageDir, command);
    sample.pid = child.pid ?? null;
    const initial = await waitForAxisState({
      child,
      port,
      readiness: runner.readiness,
      output,
      expectedMarker: budget.edit_from,
      staleMarker: budget.edit_to,
      timeoutMs: budget.startup_timeout_ms,
      pollIntervalMs: budget.poll_interval_ms,
    });
    sample.readiness = {
      path: runner.readiness.path,
      initial: initial.ready.value,
      status: initial.ready.status,
      startup_wait_ms: initial.ready.waited_ms,
    };
    sample.output = { path: output.path, initial: initial.output };
    let previous = initial.ready.value;
    if (phase === "warm") {
      const warmup = [];
      const warmedAfterEdit = await applyAxisEdit(editPath, budget.edit_from, budget.edit_to);
      const warmedAfter = await waitForAxisState({
        child,
        port,
        readiness: runner.readiness,
        output,
        expectedMarker: budget.edit_to,
        staleMarker: budget.edit_from,
        timeoutMs: budget.reload_timeout_ms,
        pollIntervalMs: budget.poll_interval_ms,
        previousValue: previous,
      });
      warmup.push({ edit: warmedAfterEdit, readiness: warmedAfter.ready.value, output: warmedAfter.output.marker });
      previous = warmedAfter.ready.value;
      const warmedBeforeEdit = await applyAxisEdit(editPath, budget.edit_to, budget.edit_from);
      const warmedBefore = await waitForAxisState({
        child,
        port,
        readiness: runner.readiness,
        output,
        expectedMarker: budget.edit_from,
        staleMarker: budget.edit_to,
        timeoutMs: budget.reload_timeout_ms,
        pollIntervalMs: budget.poll_interval_ms,
        previousValue: previous,
      });
      warmup.push({ edit: warmedBeforeEdit, readiness: warmedBefore.ready.value, output: warmedBefore.output.marker });
      previous = warmedBefore.ready.value;
      sample.warmup = { edits: warmup, measured: false };
    }
    await normalizeAxisMarker(editPath, budget.edit_from, budget.edit_to);
    const editStartedAt = monotonicNow();
    const edit = await applyAxisEdit(editPath, budget.edit_from, budget.edit_to);
    const editWrittenAt = monotonicNow();
    const observed = await waitForAxisState({
      child,
      port,
      readiness: runner.readiness,
      output,
      expectedMarker: budget.edit_to,
      staleMarker: budget.edit_from,
      timeoutMs: budget.reload_timeout_ms,
      pollIntervalMs: budget.poll_interval_ms,
      previousValue: previous,
    });
    sample.edit = { ...edit, started_at_ms: editStartedAt, written_at_ms: editWrittenAt };
    sample.readiness.after = observed.ready.value;
    sample.readiness.counter_delta = observed.ready.value - previous;
    sample.output.after = observed.output;
    sample.timestamps = {
      monotonic_clock: "performance.now",
      edit_started_at_ms: editStartedAt,
      edit_written_at_ms: editWrittenAt,
      observed_at_ms: observed.observed_at_ms,
    };
    validateSampleTimestamps(sample);
    sample.status = "complete";
  } catch (error) {
    sampleFailure(sample, error);
    if (child?.spawnError) sample.spawn_error = child.spawnError.message;
    if (child?.stderrText) sample.stderr = child.stderrText();
  } finally {
    if (child) {
      sample.cleanup = await cleanupProcessGroup(child);
      sample.process_exit = sample.cleanup.exit;
      if (sample.cleanup.forced) sample.cleanup_forced = true;
      if (!sample.cleanup.group_gone) {
        sample.cleanup_error = "process group remained alive after SIGTERM and SIGKILL";
        if (sample.status === "complete") sampleFailure(sample, new Error(sample.cleanup_error));
      }
      if (!sample.stderr && child.stderrText) sample.stderr = child.stderrText();
    }
    try {
      await restoreAxisEdit(editPath, budget.edit_from, budget.edit_to);
    } catch (error) {
      sample.restore_error = error.message;
      if (sample.status === "complete") sampleFailure(sample, error);
    }
  }
  return sample;
}

function liveReloadSummary(samples) {
  const phases = {};
  for (const phase of ["cold", "warm"]) {
    const phaseSamples = samples.filter((sample) => sample.phase === phase);
    const successful = phaseSamples.filter((sample) => sample.status === "complete");
    phases[phase] = {
      sample_count: phaseSamples.length,
      successful_samples: successful.length,
      median_reload_latency_ms: median(successful.map((sample) => sample.reload_latency_ms)),
      samples: phaseSamples,
    };
  }
  return phases;
}

function median(values) {
  const numbers = values.filter((value) => Number.isFinite(value)).sort((a, b) => a - b);
  if (!numbers.length) return null;
  const middle = Math.floor(numbers.length / 2);
  return numbers.length % 2 === 1 ? numbers[middle] : (numbers[middle - 1] + numbers[middle]) / 2;
}

export async function runLiveReloadRunner(axis, runner, axisDir, jetBin, { repoDir = defaultRepoDir, envRunner = defaultEnvRunner } = {}) {
  const runnerDir = path.join(axisDir, runner.id.replaceAll(/[^A-Za-z0-9_.-]/g, "_"));
  const budget = validateAxisBudget(axis);
  await fs.mkdir(runnerDir, { recursive: true });
  const files = await stageAxisFiles(repoDir, runnerDir, runner.files);
  const probes = await probeAxisTools(envRunner, runnerDir, runner.tools, jetBin);
  const result = {
    id: runner.id,
    output_acknowledgement: runner.output ?? runner.acknowledgement ?? runner.output_acknowledgement,
    workload: axis.workload,
    tools: probes,
    tool_provenance: probes,
    source_files: files,
    edit_file: runner.edit_file,
    readiness: runner.readiness,
    output: runner.output ?? runner.acknowledgement ?? runner.output_acknowledgement,
    declared_command: runner.command,
    status: "unmeasured",
    measurements: [],
  };
  try {
    outputSpec(runner);
  } catch (error) {
    result.status = "failed";
    result.reason = error.message;
    return result;
  }
  const unavailable = probes.find((probe) => probe.status === "unavailable");
  const probeFailure = probes.find((probe) => probe.status === "probe_failed");
  if (unavailable) {
    result.status = "unavailable";
    result.reason = unavailable.reason;
    return result;
  }
  if (probeFailure) {
    result.status = "failed";
    result.reason = probeFailure.reason;
    return result;
  }
  const editPath = axisStagePath(runnerDir, runner.edit_file);
  await normalizeAxisMarker(editPath, budget.edit_from, budget.edit_to);
  for (const phase of ["cold", "warm"]) {
    for (let index = 1; index <= budget.sample_count; index += 1) {
      result.measurements.push(await runLiveReloadSample({ runner, stageDir: runnerDir, editPath, phase, index, jetBin, budget, envRunner }));
    }
  }
  result.summary = liveReloadSummary(result.measurements);
  const expected = budget.sample_count * 2;
  const successful = result.measurements.filter((sample) => sample.status === "complete").length;
  result.status = successful === expected ? "complete" : "failed";
  if (result.status !== "complete") result.reason = `${expected - successful}/${expected} reload samples failed`;
  return result;
}
export async function runLiveReloadAxis(axis, runDir, jetBin, options = {}) {
  const axisDir = path.join(runDir, "axes", "live-reload");
  await fs.mkdir(axisDir, { recursive: true });
  const declaredRunners = Array.isArray(axis?.runners) ? axis.runners : [];
  const runners = [];
  for (const runner of declaredRunners) {
    try {
      runners.push(await runLiveReloadRunner(axis, runner, axisDir, jetBin, options));
    } catch (error) {
      runners.push({ id: runner.id, status: "failed", reason: `runner setup failed: ${error.message}`, declared_command: runner.command });
    }
  }
  const complete = declaredRunners.length > 0 && runners.length === declaredRunners.length &&
    runners.every((runner) => runner.status === "complete");
  const blockers = runners.filter((runner) => runner.status !== "complete").map((runner) => `${runner.id}: ${runner.reason ?? `status ${runner.status}`}`);
  const metrics = Object.fromEntries(runners.map((runner) => [runner.id, {
    cold_reload_latency_ms: runner.summary?.cold?.median_reload_latency_ms ?? null,
    warm_reload_latency_ms: runner.summary?.warm?.median_reload_latency_ms ?? null,
  }]));
  return {
    id: "live_reload",
    required: axis.status === "required",
    status: complete ? "complete" : "incomplete",
    contract: axis,
    schema: axis.schema,
    metric: axis.metric,
    workload: axis.workload,
    signal: axis.signal,
    budget: axis.budget,
    edit: axis.edit,
    phases: axis.phases,
    fairness: axis.fairness,
    metrics,
    runners,
    publication: {
      status: blockers.length === 0 ? "ready" : "blocked",
      blockers,
    },
  };
}

export const liveReloadInternals = {
  applyAxisEdit,
  axisCommand,
  cleanupProcessGroup,
  markerMatches,
  normalizeAxisMarker,
  outputSpec,
  probeAxisTool,
  validateAxisBudget,
  validateSampleTimestamps,
  waitForAxisOutput,
  waitForAxisReady,
};
