#!/usr/bin/env node
import { spawn } from "node:child_process";
import { promises as fs } from "node:fs";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

const harnessDir = path.dirname(fileURLToPath(import.meta.url));
const repoDir = path.resolve(harnessDir, "../..");
const envRunner = path.join(repoDir, "scripts/agent/jet-env");
const timer = path.join(harnessDir, "timer.py");

const LANGUAGE_FILES = {
  jet: "main.jet",
  rust: "main.rs",
  python: "main.py",
  c: "main.c",
  zig: "main.zig",
  go: "main.go",
  js: "main.mjs",
};

function shellQuote(value) {
  return `'${String(value).replaceAll("'", "'\\''")}'`;
}

function parseArgs(argv) {
  const options = { runs: null, entry: null, jetBin: null, entriesDir: null };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--entry" || arg === "--jet-bin" || arg === "--runs" || arg === "--entries-dir") {
      if (i + 1 >= argv.length) throw new Error(`${arg} needs a value`);
      const value = argv[++i];
      if (arg === "--entry") options.entry = value;
      if (arg === "--jet-bin") options.jetBin = value;
      if (arg === "--entries-dir") options.entriesDir = value;
      if (arg === "--runs") options.runs = Number.parseInt(value, 10);
      continue;
    }
    if (arg === "--help" || arg === "-h") {
      console.log("usage: node gauntlet/harness/run.mjs [--entry name] [--jet-bin path] [--runs n] [--entries-dir path]");
      process.exit(0);
    }
    throw new Error(`unknown argument: ${arg}`);
  }
  if (options.runs !== null && (!Number.isInteger(options.runs) || options.runs < 1)) {
    throw new Error("--runs must be a positive integer");
  }
  return options;
}

async function exists(file) {
  try {
    await fs.access(file);
    return true;
  } catch {
    return false;
  }
}

async function runProcess(cwd, args, { input = undefined, full = false } = {}) {
  return new Promise((resolve) => {
    const child = spawn(envRunner, [...(full ? ["full"] : []), "sh", "-c", args.map(shellQuote).join(" ")], {
      cwd,
      env: process.env,
      stdio: [input === undefined ? "ignore" : "pipe", "pipe", "pipe"],
    });
    const stdout = [];
    const stderr = [];
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    child.on("error", (error) => resolve({ code: 127, stdout: Buffer.concat(stdout), stderr: Buffer.from(String(error)) }));
    child.on("close", (code, signal) => resolve({
      code: code ?? 128,
      signal,
      stdout: Buffer.concat(stdout),
      stderr: Buffer.concat(stderr),
    }));
    if (input !== undefined) child.stdin.end(input);
  });
}

async function timedProcess(cwd, args, { full = false } = {}) {
  const result = await runProcess(cwd, ["python3", timer, "--", ...args], { full });
  let sample;
  try {
    sample = JSON.parse(result.stdout.toString("utf8").trim());
  } catch {
    sample = { error: result.stdout.toString("utf8").slice(0, 300) || result.stderr.toString("utf8").slice(0, 300) };
  }
  if (result.code !== 0 && sample.exit_code === undefined) sample.exit_code = result.code;
  sample.stderr = result.stderr.toString("utf8").trim().slice(0, 500);
  return sample;
}

async function timedSequence(cwd, commands, { full = false } = {}) {
  const result = await runProcess(cwd, ["python3", timer, "--sequence-json", JSON.stringify(commands)], { full });
  let sample;
  try {
    sample = JSON.parse(result.stdout.toString("utf8").trim());
  } catch {
    sample = { error: result.stdout.toString("utf8").slice(0, 300) || result.stderr.toString("utf8").slice(0, 300) };
  }
  if (result.code !== 0 && sample.exit_code === undefined) sample.exit_code = result.code;
  sample.stderr = result.stderr.toString("utf8").trim().slice(0, 500);
  return sample;
}

function startProcess(cwd, args, { full = false } = {}) {
  const child = spawn(envRunner, [...(full ? ["full"] : []), "sh", "-c", args.map(shellQuote).join(" ")], {
    cwd,
    env: process.env,
    stdio: ["ignore", "ignore", "pipe"],
    detached: true,
  });
  const stderr = [];
  child.stderr.on("data", (chunk) => stderr.push(chunk));
  child.stderrText = () => Buffer.concat(stderr).toString("utf8").trim().slice(0, 500);
  return child;
}

function stopProcess(child) {
  if (!child || child.exitCode !== null) return;
  try { process.kill(-child.pid, "SIGTERM"); } catch { child.kill("SIGTERM"); }
}

function waitForExit(child, timeoutMs = 5000) {
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

function waitForTcp(port, timeoutMs = 5000) {
  const started = Date.now();
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const socket = net.createConnection({ host: "127.0.0.1", port });
      socket.once("connect", () => { socket.destroy(); resolve(); });
      socket.once("error", () => {
        socket.destroy();
        if (Date.now() - started >= timeoutMs) reject(new Error(`TCP port ${port} did not open`));
        else setTimeout(attempt, 25);
      });
    };
    attempt();
  });
}

function httpProbe(port, probe, timeoutMs = 5000) {
  return new Promise((resolve) => {
    const started = performance.now();
    const body = probe.body === undefined ? undefined : String(probe.body);
    const request = http.request({
      host: "127.0.0.1",
      port,
      path: probe.path,
      method: probe.method ?? "GET",
      headers: body === undefined ? undefined : { "content-length": Buffer.byteLength(body) },
      timeout: timeoutMs,
    }, (response) => {
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => resolve({
        ok: true,
        status: response.statusCode,
        body: Buffer.concat(chunks).toString("utf8"),
        latencyMs: performance.now() - started,
      }));
    });
    request.on("timeout", () => request.destroy(new Error("HTTP probe timeout")));
    request.on("error", (error) => resolve({ ok: false, error: error.message, latencyMs: performance.now() - started }));
    if (body !== undefined) request.write(body);
    request.end();
  });
}

async function probeSequence(port, probes) {
  for (let index = 0; index < probes.length; index += 1) {
    const probe = probes[index];
    const result = await httpProbe(port, probe);
    const statusMismatch = probe.expectStatus !== undefined && result.status !== probe.expectStatus;
    const bodyMismatch = probe.expectBody !== undefined && result.body !== probe.expectBody;
    if (!result.ok || statusMismatch || bodyMismatch) {
      return { ok: false, index, result, reason: !result.ok ? result.error : statusMismatch ? `status ${result.status}, expected ${probe.expectStatus}` : `body ${JSON.stringify(result.body)}, expected ${JSON.stringify(probe.expectBody)}` };
    }
  }
  return { ok: true };
}

async function readRssKb(pid) {
  try {
    const status = await fs.readFile(`/proc/${pid}/status`, "utf8");
    const match = status.match(/^VmRSS:\s+(\d+)\s+kB$/m);
    return match ? Number(match[1]) : 0;
  } catch {
    return 0;
  }
}

function median(values) {
  const numbers = values.filter((value) => Number.isFinite(value)).sort((a, b) => a - b);
  if (!numbers.length) return null;
  const middle = Math.floor(numbers.length / 2);
  return numbers.length % 2 ? numbers[middle] : (numbers[middle - 1] + numbers[middle]) / 2;
}

function percentile(values, fraction) {
  const numbers = values.filter((value) => Number.isFinite(value)).sort((a, b) => a - b);
  if (!numbers.length) return null;
  return numbers[Math.min(numbers.length - 1, Math.ceil(numbers.length * fraction) - 1)];
}

function summarizeSamples(samples) {
  const metrics = ["wall_seconds", "peak_rss_kb", "time_to_first_stdout_seconds"];
  const medians = Object.fromEntries(metrics.map((metric) => [metric, median(samples.map((sample) => sample[metric]))]));
  return { samples, median: medians };
}

function mismatch(expected, actual) {
  const limit = Math.min(expected.length, actual.length);
  let index = 0;
  while (index < limit && expected[index] === actual[index]) index += 1;
  if (index === expected.length && index === actual.length) return null;
  const show = (buffer) => JSON.stringify(buffer.subarray(index, index + 80).toString("utf8"));
  return `byte ${index}: expected ${show(expected)}, got ${show(actual)} (length ${expected.length}/${actual.length})`;
}

async function sourceMetrics(sourceDir, filename) {
  const source = path.join(sourceDir, filename);
  const text = await fs.readFile(source, "utf8");
  const lines = text.split(/\r?\n/);
  const loc = lines.filter((line) => {
    const trimmed = line.trim();
    return trimmed && !trimmed.startsWith("//") && !trimmed.startsWith("/*") && !trimmed.startsWith("*") && !trimmed.startsWith("# ") && !trimmed.startsWith("#!");
  }).length;
  const tokens = text.match(/[\p{L}\p{N}_]+|[^\s\p{L}\p{N}_]/gu)?.length ?? 0;
  return { loc, source_bytes: Buffer.byteLength(text), tokens };
}

async function discoverJetArtifact(dir) {
  const preferred = path.join(dir, "build", "main");
  if (await exists(preferred)) return preferred;
  const found = [];
  async function walk(current) {
    for (const item of await fs.readdir(current, { withFileTypes: true })) {
      if (item.name === ".jet" || item.name === "zig-cache" || item.name === "zig-global-cache") continue;
      const full = path.join(current, item.name);
      if (item.isDirectory()) await walk(full);
      else if (item.name === "main" || item.name === "main.exe") {
        try {
          const stat = await fs.stat(full);
          if ((stat.mode & 0o111) !== 0) found.push(full);
        } catch { /* file disappeared */ }
      }
    }
  }
  await walk(dir);
  return found[0] ?? null;
}

function buildCommand(language, jetBin, sourceDir) {
  if (language === "jet") return [jetBin, "build", "main.jet"];
  if (language === "rust") return ["rustc", "-O", "main.rs", "-o", "main-rust"];
  if (language === "c") return ["gcc", "-O2", "main.c", "-o", "main-c", "-lm"];
  if (language === "zig") return ["zig", "build-exe", "-O", "ReleaseFast", "--cache-dir", "zig-cache", "--global-cache-dir", "zig-global-cache", "main.zig"];
  if (language === "go") return ["env", "GO111MODULE=off", `GOCACHE=${path.join(sourceDir, "go-cache")}`, "go", "build", "-o", "main-go", "main.go"];
  return null;
}

function runCommand(language, sourceDir, artifact, args) {
  if (language === "jet") return [artifact, ...args];
  if (language === "python") return ["python3", "main.py", ...args];
  if (language === "js") return ["node", "main.mjs", ...args];
  return [artifact, ...args];
}

async function verify(cwd, command, expected, { full = false } = {}) {
  const result = await runProcess(cwd, command, { full });
  const error = mismatch(expected, result.stdout);
  if (result.code !== 0) return `exit ${result.code}${result.stderr ? `: ${result.stderr.toString("utf8").trim().slice(0, 300)}` : ""}`;
  return error;
}

async function verifySequence(cwd, commands, expected, reset = null, { full = false } = {}) {
  if (reset) await reset();
  const output = [];
  for (const command of commands) {
    const result = await runProcess(cwd, command, { full });
    output.push(result.stdout);
    if (result.code !== 0) return `exit ${result.code}${result.stderr ? `: ${result.stderr.toString("utf8").trim().slice(0, 300)}` : ""}`;
  }
  return mismatch(expected, Buffer.concat(output));
}

async function makeStateReset(sourceDir) {
  const baseline = `${sourceDir}.baseline`;
  await fs.rm(baseline, { recursive: true, force: true });
  await fs.cp(sourceDir, baseline, { recursive: true });
  return async () => {
    for (const item of await fs.readdir(sourceDir)) await fs.rm(path.join(sourceDir, item), { recursive: true, force: true });
    await fs.cp(baseline, sourceDir, { recursive: true });
  };
}

async function measureSequenceRuns(cwd, commands, count, reset, { full = false } = {}) {
  const samples = [];
  for (let i = 0; i < count; i += 1) {
    await reset();
    samples.push(await timedSequence(cwd, commands, { full }));
  }
  return summarizeSamples(samples);
}

async function buildAndMeasure(language, sourceDir, jetBin) {
  const command = buildCommand(language, jetBin, sourceDir);
  if (!command) return { supported: true, build: null, artifact: null };
  const cold = await timedProcess(sourceDir, command);
  const warm = cold.exit_code === 0 ? await timedProcess(sourceDir, command) : null;
  const artifact = language === "jet" ? await discoverJetArtifact(sourceDir) : path.join(sourceDir, {
    rust: "main-rust", c: "main-c", zig: "main", go: "main-go",
  }[language]);
  const failure = cold.exit_code !== 0 ? `cold build exit ${cold.exit_code}` : warm?.exit_code !== 0 ? `warm build exit ${warm?.exit_code}` : !await exists(artifact) ? "build produced no executable" : null;
  return { supported: true, build: { cold, warm }, artifact, failure };
}

function commandFromSpec(value, language, jetBin, fallback = null) {
  if (value === undefined || value === null) return fallback;
  const command = Array.isArray(value) ? [...value] : String(value).trim().split(/\s+/);
  if (command[0] === "jet") command[0] = jetBin;
  if (language === "jet" && command[0] === "build") command.unshift(jetBin);
  return command;
}

function languageSpecValue(value, language) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return value;
  return value[language] ?? value.default;
}

async function configuredBuildAndMeasure(language, sourceDir, jetBin, entry) {
  const configured = languageSpecValue(entry.spec?.build ?? entry.build, language);
  const command = commandFromSpec(configured, language, jetBin, null);
  if (!command) return { supported: true, build: null, artifact: null, failure: null };
  const full = entry.spec?.fullShell === true;
  const cold = await timedProcess(sourceDir, command, { full });
  const warm = cold.exit_code === 0 ? await timedProcess(sourceDir, command, { full }) : null;
  const failure = cold.exit_code !== 0 ? `cold build exit ${cold.exit_code}` : warm?.exit_code !== 0 ? `warm build exit ${warm?.exit_code}` : null;
  return { supported: true, build: { cold, warm }, artifact: null, failure };
}

async function copyRelativeFile(sourceDir, stageDir, relative) {
  const source = path.join(sourceDir, relative);
  if (!(await exists(source))) return false;
  const target = path.join(stageDir, relative);
  await fs.mkdir(path.dirname(target), { recursive: true });
  await fs.copyFile(source, target);
  return true;
}

async function artifactBytes(sourceDir) {
  const roots = [];
  for (const name of ["build", "dist", "out"]) if (await exists(path.join(sourceDir, name))) roots.push(path.join(sourceDir, name));
  if (!roots.length) roots.push(sourceDir);
  let total = 0;
  async function walk(dir, rootOnly) {
    for (const item of await fs.readdir(dir, { withFileTypes: true })) {
      const full = path.join(dir, item.name);
      if (item.isDirectory()) await walk(full, rootOnly);
      else if ((item.name.endsWith(".wasm") || item.name.endsWith(".js")) && (rootOnly || !["main.mjs", "runner.mjs"].includes(item.name))) total += (await fs.stat(full)).size;
    }
  }
  for (const root of roots) await walk(root, roots[0] !== sourceDir);
  return total;
}

async function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      server.close(() => resolve(port));
    });
  });
}

async function waitForReady(child, port, readyPath) {
  const started = performance.now();
  while (performance.now() - started < 10000) {
    if (child.exitCode !== null) throw new Error(`service exited ${child.exitCode}${child.stderrText() ? `: ${child.stderrText()}` : ""}`);
    const result = await httpProbe(port, { method: "GET", path: readyPath }, 500);
    if (result.ok) return { seconds: (performance.now() - started) / 1000, result };
  }
  throw new Error(`service did not answer ${readyPath}`);
}

async function runService(language, sourceDir, artifact, entry) {
  const service = entry.spec.service;
  const probes = service.probe ?? [];
  if (!service.readyPath || !service.portArg || probes.length === 0) return { failure: "service requires portArg, readyPath, and probe" };
  const commandFor = (port) => runCommand(language, sourceDir, artifact, [String(port)]);
  let child = null;
  let startupSeconds = null;
  let failure = null;
  try {
    const port = await freePort();
    child = startProcess(sourceDir, commandFor(port), { full: entry.spec?.fullShell === true });
    startupSeconds = (await waitForReady(child, port, service.readyPath)).seconds;
    const verification = await probeSequence(port, probes);
    if (!verification.ok) failure = `probe ${verification.index} failed: ${verification.reason}`;
    const firstExit = await waitForExit(child, 1000);
    if (firstExit.code !== 0 && !failure) failure = `verification service exit ${firstExit.code ?? firstExit.signal}`;
    if (firstExit.code === null) {
      if (!failure) failure = "verification shutdown did not produce a clean exit";
      stopProcess(child);
      await waitForExit(child);
    }
  } catch (error) {
    failure = error.message;
    if (child) { stopProcess(child); await waitForExit(child); }
  }
  if (failure) return { failure, startupSeconds };

  const port = await freePort();
  child = startProcess(sourceDir, commandFor(port), { full: entry.spec?.fullShell === true });
  let ready;
  try {
    ready = await waitForReady(child, port, service.readyPath);
  } catch (error) {
    stopProcess(child);
    await waitForExit(child);
    return { failure: error.message, startupSeconds };
  }
  let rssKb = await readRssKb(child.pid);
  const rssTimer = setInterval(() => { readRssKb(child.pid).then((value) => { rssKb = Math.max(rssKb, value); }); }, 20);
  const latencies = [];
  const repeatProbes = probes.slice(0, -1);
  let measurementFailure = null;
  for (let repeat = 0; repeat < 50 && !measurementFailure; repeat += 1) {
    for (const probe of repeatProbes) {
      const result = await httpProbe(port, probe);
      latencies.push(result.latencyMs);
      if (!result.ok || (probe.expectStatus !== undefined && result.status !== probe.expectStatus) || (probe.expectBody !== undefined && result.body !== probe.expectBody)) {
        measurementFailure = `probe failed during measurement: ${result.error ?? `status/body mismatch at ${probe.path}`}`;
        break;
      }
    }
  }
  if (!measurementFailure) {
    const shutdown = await httpProbe(port, probes[probes.length - 1]);
    latencies.push(shutdown.latencyMs);
    if (!shutdown.ok || (probes.at(-1).expectStatus !== undefined && shutdown.status !== probes.at(-1).expectStatus) || (probes.at(-1).expectBody !== undefined && shutdown.body !== probes.at(-1).expectBody)) measurementFailure = `shutdown probe failed: ${shutdown.error ?? "status/body mismatch"}`;
  }
  clearInterval(rssTimer);
  rssKb = Math.max(rssKb, await readRssKb(child.pid));
  const exit = await waitForExit(child, 5000);
  if (exit.code === null) { stopProcess(child); await waitForExit(child); }
  return {
    failure: measurementFailure ?? (exit.code !== 0 ? `service clean exit ${exit.code ?? exit.signal}` : null),
    startupSeconds,
    latencyMs: { median: median(latencies), p99: percentile(latencies, 0.99) },
    rssKb,
    cleanExit: exit.code === 0,
    exitCode: exit.code,
    readySeconds: ready.seconds,
  };
}

async function startPeer(sourceDir, peer) {
  const port = peer.port;
  const child = startProcess(sourceDir, ["python3", peer.script, String(port)]);
  try {
    await waitForTcp(port);
    return child;
  } catch (error) {
    stopProcess(child);
    await waitForExit(child);
    throw error;
  }
}

async function measureRuns(cwd, command, count, { full = false } = {}) {
  const samples = [];
  for (let i = 0; i < count; i += 1) samples.push(await timedProcess(cwd, command, { full }));
  return summarizeSamples(samples);
}

function ratioVerdict(ratio) {
  if (!Number.isFinite(ratio)) return null;
  if (ratio < 1) return "win";
  if (ratio <= 1.05) return "parity";
  return "loss";
}

function comparisons(languages, rows) {
  const jet = rows.jet;
  if (!jet || jet.status !== "ok") return {};
  const metrics = ["runtime_wall_seconds", "runtime_peak_rss_kb", "runtime_first_stdout_seconds", "cold_build_seconds", "warm_build_seconds", "binary_bytes", "loc", "source_bytes", "tokens"];
  const output = {};
  for (const language of languages.filter((item) => item !== "jet")) {
    const peer = rows[language];
    if (!peer || peer.status !== "ok") continue;
    output[language] = {};
    for (const metric of metrics) {
      const jetValue = jet.metrics[metric];
      const peerValue = peer.metrics[metric];
      const ratio = Number.isFinite(jetValue) && Number.isFinite(peerValue) && peerValue !== 0 ? jetValue / peerValue : null;
      output[language][metric] = { jet: jetValue ?? null, peer: peerValue ?? null, ratio, verdict: ratioVerdict(ratio) };
    }
  }
  return output;
}

async function stageEntry(entryDir, entry, runDir, jetBin, selectedRuns, dev) {
  const entryStage = path.join(runDir, entry.name);
  const languages = entry.languages ?? [];
  const failedRows = (reason) => Object.fromEntries(languages.map((language) => [language, {
    language,
    status: "broken",
    disqualified: true,
    reason,
    metrics: {},
    diagnostics: [],
  }]));
  await fs.mkdir(entryStage, { recursive: true });
  const expectedPath = path.join(entryDir, entry.spec?.expected ?? "expected.out");
  if (!(await exists(expectedPath))) return { entry, status: "broken", reason: "missing expected output", languages, rows: failedRows("missing expected output"), comparisons: {}, jet_tiers: {} };
  const expected = await fs.readFile(expectedPath);
  const fixture = entry.spec?.fixtureGen;
  const commonFixtures = path.join(entryStage, "fixtures");
  if (fixture) {
    const fixtureDir = path.join(entryDir, path.dirname(fixture.script));
    if (!(await exists(fixtureDir))) {
      const reason = `missing fixture directory ${fixture.script}`;
      return { entry, status: "broken", reason, languages, rows: failedRows(reason), comparisons: {}, jet_tiers: {} };
    }
    await fs.cp(fixtureDir, commonFixtures, { recursive: true });
    const output = fixture.out;
    await fs.mkdir(path.dirname(path.join(entryStage, output)), { recursive: true });
    const generated = await runProcess(entryStage, ["python3", fixture.script, output]);
    if (generated.code !== 0) {
      const reason = `fixture generator exit ${generated.code}: ${generated.stderr.toString("utf8").trim().slice(0, 300)}`;
      return { entry, status: "broken", reason, languages, rows: failedRows(reason), comparisons: {}, jet_tiers: {} };
    }
  }

  const rows = {};
  const resets = {};
  for (const language of languages) {
    const sourceDir = path.join(entryDir, language);
    const sourceFile = LANGUAGE_FILES[language];
    const row = { language, status: "broken", metrics: {}, diagnostics: [] };
    rows[language] = row;
    if (!sourceFile) {
      row.reason = "unsupported language";
      row.disqualified = true;
      continue;
    }
    if (!(await exists(path.join(sourceDir, sourceFile)))) {
      row.reason = `missing ${language}/${sourceFile}`;
      row.disqualified = true;
      continue;
    }
    const stagedSource = path.join(entryStage, language);
    await fs.cp(sourceDir, stagedSource, { recursive: true });
    if (fixture) await fs.cp(commonFixtures, path.join(stagedSource, "fixtures"), { recursive: true });
    if (entry.spec?.peer && !(await copyRelativeFile(entryDir, stagedSource, entry.spec.peer.script))) {
      row.reason = `missing peer script ${entry.spec.peer.script}`;
      row.disqualified = true;
      continue;
    }
    const stagedExpected = path.join(stagedSource, path.basename(expectedPath));
    await fs.copyFile(expectedPath, stagedExpected);
    const webMode = entry.mode === "web" || entry.mode === "web-app";
    const build = webMode ? await configuredBuildAndMeasure(language, stagedSource, jetBin, entry) : await buildAndMeasure(language, stagedSource, jetBin);
    row.build = build.build;
    row.metrics = await sourceMetrics(sourceDir, sourceFile);
    row.disqualified = false;
    if (build.failure) {
      row.reason = build.failure;
      row.disqualified = true;
      const missingToolchain = build.build?.cold?.exit_code === 127 && (build.build.cold.error || /command not found/i.test(build.build.cold.stderr ?? ""));
      if (missingToolchain) {
        row.missing_toolchain = true;
        console.warn(`WARN ${entry.name}/${language}: ${build.failure}; toolchain may be missing`);
      }
      continue;
    }
    const artifact = build.artifact;

    if (entry.mode === "batch-steps") {
      const steps = entry.spec?.steps ?? [];
      const commands = steps.map((args) => runCommand(language, stagedSource, artifact, args));
      const reset = await makeStateReset(stagedSource);
      resets[language] = reset;
      const verification = await verifySequence(stagedSource, commands, expected, reset);
      if (verification) {
        row.reason = verification;
        row.disqualified = true;
        continue;
      }
      row.status = "ok";
      row.command = commands;
      const runs = selectedRuns ?? (entry.perf ? 7 : 3);
      row.runtime = await measureSequenceRuns(stagedSource, commands, runs, reset);
      if (row.runtime.samples.some((sample) => sample.exit_code !== 0)) {
        row.status = "broken";
        row.reason = "measured run exited nonzero";
        row.disqualified = true;
        continue;
      }
      const binary = artifact && await exists(artifact) ? (await fs.stat(artifact)).size : null;
      row.metrics = {
        ...row.metrics,
        runtime_wall_seconds: row.runtime.median.wall_seconds,
        runtime_peak_rss_kb: row.runtime.median.peak_rss_kb,
        runtime_first_stdout_seconds: row.runtime.median.time_to_first_stdout_seconds,
        cold_build_seconds: row.build ? row.build.cold.wall_seconds : null,
        warm_build_seconds: row.build ? row.build.warm.wall_seconds : null,
        binary_bytes: binary,
      };
      continue;
    }

    if (entry.mode === "service") {
      const service = await runService(language, stagedSource, artifact, entry);
      row.command = runCommand(language, stagedSource, artifact, ["<port>"]);
      row.metrics = {
        ...row.metrics,
        cold_build_seconds: row.build ? row.build.cold.wall_seconds : null,
        warm_build_seconds: row.build ? row.build.warm.wall_seconds : null,
        binary_bytes: artifact && await exists(artifact) ? (await fs.stat(artifact)).size : null,
        startupSeconds: service.startupSeconds ?? null,
        latencyMs: service.latencyMs ?? { median: null, p99: null },
        rssKb: service.rssKb ?? null,
        cleanExit: service.cleanExit ?? false,
        exitCode: service.exitCode ?? null,
      };
      if (service.failure) {
        row.reason = service.failure;
        row.disqualified = true;
        continue;
      }
      row.status = "ok";
      continue;
    }

    if (webMode) {
      const configuredRun = languageSpecValue(entry.spec?.run ?? entry.run, language);
      const command = commandFromSpec(configuredRun, language, jetBin, ["node", "runner.mjs"]);
      const full = entry.spec?.fullShell === true;
      const verification = await verify(stagedSource, command, expected, { full });
      if (verification) {
        row.reason = verification;
        row.disqualified = true;
        continue;
      }
      row.status = "ok";
      row.command = command;
      const runs = selectedRuns ?? (entry.perf ? 7 : 3);
      row.runtime = await measureRuns(stagedSource, command, runs, { full });
      if (row.runtime.samples.some((sample) => sample.exit_code !== 0)) {
        row.status = "broken";
        row.reason = "measured run exited nonzero";
        row.disqualified = true;
        continue;
      }
      row.metrics = {
        ...row.metrics,
        firstResultSeconds: row.runtime.median.time_to_first_stdout_seconds,
        artifactBytes: await artifactBytes(stagedSource),
        cold_build_seconds: row.build ? row.build.cold.wall_seconds : null,
        warm_build_seconds: row.build ? row.build.warm.wall_seconds : null,
      };
      continue;
    }

    let peerChild = null;
    const stopPeer = async () => {
      if (peerChild) {
        stopProcess(peerChild);
        await waitForExit(peerChild);
        peerChild = null;
      }
    };
    if (entry.spec?.peer) {
      try {
        peerChild = await startPeer(stagedSource, entry.spec.peer);
      } catch (error) {
        row.reason = `peer unavailable: ${error.message}`;
        row.disqualified = true;
        continue;
      }
    }
    const command = runCommand(language, stagedSource, artifact, entry.spec?.args ?? []);
    const verification = await verify(stagedSource, command, expected);
    if (verification) {
      row.reason = verification;
      row.disqualified = true;
      await stopPeer();
      continue;
    }
    row.status = "ok";
    row.command = command;
    const runs = selectedRuns ?? (entry.perf ? 7 : 3);
    row.runtime = await measureRuns(stagedSource, command, runs);
    if (row.runtime.samples.some((sample) => sample.exit_code !== 0)) {
      row.status = "broken";
      row.reason = "measured run exited nonzero";
      row.disqualified = true;
      await stopPeer();
      continue;
    }
    const binary = artifact && await exists(artifact) ? (await fs.stat(artifact)).size : null;
    row.metrics = {
      ...row.metrics,
      runtime_wall_seconds: row.runtime.median.wall_seconds,
      runtime_peak_rss_kb: row.runtime.median.peak_rss_kb,
      runtime_first_stdout_seconds: row.runtime.median.time_to_first_stdout_seconds,
      cold_build_seconds: row.build ? row.build.cold.wall_seconds : null,
      warm_build_seconds: row.build ? row.build.warm.wall_seconds : null,
      binary_bytes: binary,
    };
    await stopPeer();
  }

  const jetRow = rows.jet;
  const tiers = {};
  if (jetRow && jetRow.status === "ok") {
    const jetDir = path.join(entryStage, "jet");
    for (const tier of ["run", "dev"]) {
      if (tier === "dev" && !dev) continue;
      const tierArgs = entry.mode === "batch-steps" ? (entry.spec?.steps ?? []).map((args) => tier === "run" ? [jetBin, "run", "main.jet", "--", ...args] : [jetBin, "dev", "--watch=off", "main.jet", "--", ...args]) : null;
      const command = tierArgs ?? (tier === "run" ? [jetBin, "run", "main.jet", "--", ...(entry.spec?.args ?? [])] : [jetBin, "dev", "--watch=off", "main.jet", "--", ...(entry.spec?.args ?? [])]);
      const tierRow = { command };
      const tierVerification = tierArgs ? await verifySequence(jetDir, tierArgs, expected, resets.jet) : await verify(jetDir, command, expected);
      if (tierVerification) {
        tierRow.status = "broken";
        tierRow.reason = tierVerification;
      } else {
        tierRow.status = "ok";
        tierRow.runtime = tierArgs ? await measureSequenceRuns(jetDir, tierArgs, selectedRuns ?? (entry.perf ? 7 : 3), resets.jet) : await measureRuns(jetDir, command, selectedRuns ?? (entry.perf ? 7 : 3));
      }
      tiers[tier] = tierRow;
    }
  }
  const status = languages.every((language) => rows[language]?.status === "ok") ? "ok" : "broken";
  return { entry, status, stage: entryStage, languages, rows, comparisons: comparisons(languages, rows), jet_tiers: tiers };
}

async function loadEntries(entriesDir, selected) {
  const loaded = [];
  const skipped = [];
  if (!(await exists(entriesDir))) return { loaded, skipped };
  for (const item of (await fs.readdir(entriesDir, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
    if (!item.isDirectory() || (selected && item.name !== selected)) continue;
    const dir = path.join(entriesDir, item.name);
    const file = path.join(dir, "entry.json");
    try {
      const entry = JSON.parse(await fs.readFile(file, "utf8"));
      entry.name ??= item.name;
      if (!["batch", "batch-steps", "service", "web", "web-app"].includes(entry.mode)) {
        console.warn(`WARN ${entry.name}: skipped mode ${entry.mode ?? "missing"}`);
        skipped.push({ name: entry.name, reason: `mode ${entry.mode ?? "missing"} is not batch` });
      } else loaded.push({ dir, entry });
    } catch (error) {
      console.warn(`WARN ${item.name}: skipped invalid entry.json: ${error.message}`);
      skipped.push({ name: item.name, reason: `invalid entry.json: ${error.message}` });
    }
  }
  return { loaded, skipped };
}

async function copyJetBinary(options, runDir) {
  if (options.jetBin) return path.resolve(process.cwd(), options.jetBin);
  const source = path.join(repoDir, "target/debug/jet");
  if (!(await exists(source))) throw new Error(`missing default Jet binary: ${source}`);
  const destination = path.join(runDir, "jet-bin", "jet");
  await fs.mkdir(path.dirname(destination), { recursive: true });
  await fs.copyFile(source, destination);
  await fs.chmod(destination, 0o755);
  return destination;
}

async function devAvailable(jetBin, runDir) {
  const result = await runProcess(runDir, [jetBin, "dev", "--help"]);
  return result.code === 0;
}

function dateStamp() {
  return new Date().toISOString().slice(0, 10);
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const matrix = JSON.parse(await fs.readFile(path.join(repoDir, "gauntlet/matrix.json"), "utf8"));
  const entriesDir = path.resolve(process.cwd(), options.entriesDir ?? path.join(repoDir, "gauntlet/entries"));
  const runId = `${dateStamp().replaceAll("-", "")}-${process.pid}-${Date.now().toString(36)}`;
  const runDir = path.join(process.env.HOME ?? ".", ".cache/jet-gauntlet/work", runId);
  await fs.mkdir(runDir, { recursive: true });
  const jetBin = await copyJetBinary(options, runDir);
  const dev = await devAvailable(jetBin, runDir);
  if (!dev) console.warn("WARN jet dev unavailable; skipping Jet dev tier");
  const { loaded, skipped } = await loadEntries(entriesDir, options.entry);
  const results = [];
  for (const item of loaded) {
    results.push(await stageEntry(item.dir, item.entry, runDir, jetBin, options.runs, dev));
  }
  const covered = new Set(results.flatMap((result) => result.entry.cells ?? []));
  const uncovered = (matrix.cells ?? []).map((cell) => cell.id).filter((id) => !covered.has(id));
  const report = {
    generated: new Date().toISOString(),
    run_id: runId,
    options: { entry: options.entry, jet_bin: jetBin, runs: options.runs },
    matrix_version: matrix.version,
    entries_dir: entriesDir,
    skipped,
    uncovered_cells: uncovered,
    entries: results,
  };
  const resultDir = path.join(repoDir, "gauntlet/results");
  await fs.mkdir(resultDir, { recursive: true });
  const resultPath = path.join(resultDir, `${dateStamp()}.json`);
  await fs.writeFile(resultPath, `${JSON.stringify(report, null, 2)}\n`);

  console.log("entry\tlanguage\tstatus\truntime_s\tcold_build_s\tjet_verdicts\tmode_metrics");
  for (const result of results) {
    for (const language of result.languages ?? ["-"]) {
      const row = result.rows?.[language];
      const verdicts = result.comparisons?.[language] ? Object.values(result.comparisons[language]).map((item) => item.verdict).filter(Boolean).join(",") : "-";
      const metrics = result.entry.mode === "service"
        ? `startup=${row?.metrics?.startupSeconds ?? "-"},latency_ms=${row?.metrics?.latencyMs?.median ?? "-"}/${row?.metrics?.latencyMs?.p99 ?? "-"},rss_kb=${row?.metrics?.rssKb ?? "-"},clean_exit=${row?.metrics?.cleanExit ?? "-"}`
        : result.entry.mode === "web" || result.entry.mode === "web-app"
          ? `artifact_bytes=${row?.metrics?.artifactBytes ?? "-"},first_result_s=${row?.metrics?.firstResultSeconds ?? "-"}`
          : "-";
      console.log(`${result.entry.name}\t${language}\t${row?.status ?? result.status}\t${row?.metrics?.runtime_wall_seconds ?? "-"}\t${row?.metrics?.cold_build_seconds ?? "-"}\t${verdicts || "-"}\t${metrics}`);
    }
  }
  for (const item of skipped) console.log(`${item.name}\t-\tskipped\t-\t-\t-`);
  console.log(`results\t${resultPath}`);
  console.log(`uncovered\t${uncovered.length}`);
}

main().catch((error) => {
  console.error(`harness: ${error.message}`);
  process.exitCode = 1;
});
