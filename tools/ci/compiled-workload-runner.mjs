#!/usr/bin/env node
// Card #1414: execute the frozen seven-workload corpus and emit a complete
// report. This file is a producer only. The report's review fields are filled
// by an independent current-commit/fairness review before release checking.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import crypto from "node:crypto";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), "../..");
const corpus = path.join(root, "tests", "compiled_workloads");
const contractScript = path.join(root, "tools", "ci", "compiled-workload-gate.sh");
let platform = process.env.JET_COMPILED_WORKLOAD_PLATFORM || "";
let reportDir = "";
for (let i = 2; i < process.argv.length; i += 1) {
  if (process.argv[i] === "--platform") platform = process.argv[++i] || "";
  else if (process.argv[i] === "--report-dir") reportDir = process.argv[++i] || "";
  else {
    console.error("usage: compiled-workload-runner.mjs --platform linux|macos|windows --report-dir DIR");
    process.exit(64);
  }
}
if (!["linux", "macos", "windows"].includes(platform) || !reportDir) {
  console.error("usage: compiled-workload-runner.mjs --platform linux|macos|windows --report-dir DIR");
  process.exit(64);
}
const hostPlatform = process.platform === "linux" ? "linux" :
  process.platform === "darwin" ? "macos" :
    process.platform === "win32" ? "windows" : "";
if (platform !== hostPlatform) fail("report platform does not match the host: " + platform + " (host=" + hostPlatform + ")");
reportDir = path.isAbsolute(reportDir) ? reportDir : path.join(root, reportDir);

const jetBin = path.resolve(process.env.JET_COMPILED_WORKLOAD_JET_BIN || path.join(root, "target", "debug", process.platform === "win32" ? "jet.exe" : "jet"));
const exeSuffix = process.platform === "win32" ? ".exe" : "";
const scratchRoot = process.env.JET_COMPILED_WORKLOAD_SCRATCH || path.join(os.homedir(), ".cache", "jet-test-scratch");
const scratchPath = path.resolve(scratchRoot);
const ramBackedTmpPath = process.platform === "linux" ? path.resolve("/tmp") : null;
if (ramBackedTmpPath && (scratchPath === ramBackedTmpPath || scratchPath.startsWith(ramBackedTmpPath + path.sep))) fail("scratch must not use the RAM-backed system temporary directory: " + scratchRoot);
fs.mkdirSync(scratchRoot, { recursive: true });
const work = fs.mkdtempSync(path.join(scratchRoot, "compiled-workload-runner-"));
const preserveScratch = process.env.JET_COMPILED_WORKLOAD_KEEP_SCRATCH === "1";
process.on("exit", () => {
  if (preserveScratch) return;
  try {
    fs.rmSync(work, { recursive: true, force: true });
  } catch {
    // Preserve the report even when a failed run cannot clean its scratch tree.
  }
});
if (fs.existsSync(reportDir)) {
  if (!fs.statSync(reportDir).isDirectory()) fail("report path is not a directory: " + reportDir);
  if (fs.readdirSync(reportDir).length > 0) fail("report directory must be empty: " + reportDir);
} else {
  fs.mkdirSync(reportDir, { recursive: true });
}
fs.mkdirSync(path.join(work, "runs"), { recursive: true });

function fail(message) {
  console.error("compiled workload runner: " + message);
  process.exit(1);
}
const contract = spawnSync("bash", [contractScript, "--contract"], {
  cwd: root,
  encoding: "utf8",
});
if (contract.error || contract.status !== 0) {
  fail("frozen contract rejected: " + cleanText(contract.stderr || contract.stdout || contract.error?.message));
}
function requireFile(file, label) {
  try {
    if (!fs.statSync(file).isFile()) fail("missing " + label + ": " + file);
  } catch {
    fail("missing " + label + ": " + file);
  }
}
function requireInputPath(file, label) {
  try {
    const stat = fs.lstatSync(file);
    if (!stat.isFile() && !stat.isDirectory()) fail("missing " + label + ": " + file);
  } catch {
    fail("missing " + label + ": " + file);
  }
}
function table(file) {
  const lines = fs.readFileSync(file, "utf8").split(/\r?\n/).filter(line => line.length > 0);
  const header = lines.shift().split("\t");
  return lines.map(line => {
    const values = line.split("\t");
    if (values.length !== header.length) fail("row width drifted in " + file);
    return Object.fromEntries(header.map((key, index) => [key, values[index]]));
  });
}
function hashFile(file) {
  const digest = crypto.createHash("sha256");
  const descriptor = fs.openSync(file, "r");
  const buffer = Buffer.allocUnsafe(1024 * 1024);
  try {
    let read;
    do {
      read = fs.readSync(descriptor, buffer, 0, buffer.length, null);
      if (read > 0) digest.update(buffer.subarray(0, read));
    } while (read > 0);
  } finally {
    fs.closeSync(descriptor);
  }
  return digest.digest("hex");
}
function hashBuffer(buffer) {
  return crypto.createHash("sha256").update(buffer).digest("hex");
}
function fixtureModeSpec(input) {
  const control = path.join(input, ".fixture-modes.tsv");
  let stat;
  try {
    stat = fs.lstatSync(control);
  } catch (error) {
    if (error.code === "ENOENT") return { control: "", raw: null, modes: new Map() };
    fail("fixture mode control cannot be inspected: " + error.message);
  }
  if (!stat.isFile()) fail("fixture mode control is not a regular file: " + control);
  const raw = fs.readFileSync(control);
  let text;
  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(raw);
  } catch {
    fail("fixture mode control is not valid UTF-8: " + control);
  }
  const modes = new Map();
  const lines = text.split(/\r?\n/);
  if (lines.at(-1) === "") lines.pop();
  for (const line of lines) {
    const fields = line.split("\t");
    if (fields.length !== 2) fail("fixture mode row must be relative_path<TAB>octal_mode: " + control);
    const relative = fields[0];
    const modeText = fields[1];
    const normalized = path.posix.normalize(relative);
    if (!relative || relative.startsWith("/") || relative.includes("\\") || relative.includes(":") ||
        normalized !== relative || normalized === "." || normalized.startsWith("../") ||
        relative === ".fixture-modes.tsv") {
      fail("invalid fixture mode path: " + relative);
    }
    if (!/^(?:[0-7]{3}|0[0-7]{3})$/.test(modeText) || Number.parseInt(modeText, 8) > 0o777) {
      fail("invalid fixture mode: " + modeText);
    }
    if (modes.has(relative)) fail("duplicate fixture mode path: " + relative);
    const target = path.join(input, ...relative.split("/"));
    let targetStat;
    try {
      targetStat = fs.lstatSync(target);
    } catch {
      fail("fixture mode path does not exist: " + relative);
    }
    if (!targetStat.isFile() && !targetStat.isDirectory()) {
      fail("unsupported fixture mode target: " + relative);
    }
    modes.set(relative, { mode: Number.parseInt(modeText, 8), text: modeText, target });
  }
  return { control, raw, modes };
}
function fixtureModeFor(spec, relative, stat) {
  return spec.modes.get(relative)?.mode ?? (stat.mode & 0o7777);
}
function hashInputPath(input) {
  const rootStat = fs.lstatSync(input);
  if (rootStat.isFile()) return hashFile(input);
  if (!rootStat.isDirectory()) fail("input fixture is not a file or directory: " + input);
  const spec = fixtureModeSpec(input);
  const entries = [];
  function collect(relative, file) {
    const stat = fs.lstatSync(file);
    const type = stat.isDirectory() ? "dir" :
      stat.isSymbolicLink() ? "symlink" :
        stat.isFile() ? "file" : "";
    if (!type) fail("unsupported input fixture entry: " + file);
    if (relative === ".fixture-modes.tsv") return;
    entries.push({ relative, file, type, mode: fixtureModeFor(spec, relative, stat) .toString(8) });
    if (type === "dir") {
      for (const name of fs.readdirSync(file)) {
        collect(path.posix.join(relative, name), path.join(file, name));
      }
    }
  }
  collect(".", input);
  entries.sort((left, right) => left.relative < right.relative ? -1 : left.relative > right.relative ? 1 : 0);
  const digest = crypto.createHash("sha256");
  for (const entry of entries) {
    const link = entry.type === "symlink" ? fs.readlinkSync(entry.file) : "";
    digest.update(entry.relative);
    digest.update("\0");
    digest.update(entry.type);
    digest.update("\0");
    digest.update(entry.mode);
    digest.update("\0");
    digest.update(link);
    digest.update("\0");
    if (entry.type === "file") {
      const descriptor = fs.openSync(entry.file, "r");
      const buffer = Buffer.allocUnsafe(1024 * 1024);
      try {
        let read;
        do {
          read = fs.readSync(descriptor, buffer, 0, buffer.length, null);
          if (read > 0) digest.update(buffer.subarray(0, read));
        } while (read > 0);
      } finally {
        fs.closeSync(descriptor);
      }
    } else if (entry.type === "symlink") {
      digest.update(link);
    }
    digest.update("\0");
  }
  if (spec.raw !== null) {
    digest.update(".fixture-modes.tsv");
    digest.update("\0control\0");
    digest.update(spec.raw);
    digest.update("\0");
    for (const relative of [...spec.modes.keys()].sort()) {
      const declaration = spec.modes.get(relative);
      digest.update("mode\0");
      digest.update(relative);
      digest.update(declaration.mode.toString(8));
      digest.update("\0");
    }
  }
  return digest.digest("hex");
}
function hashInputMethod(input) {
  return fs.lstatSync(input).isDirectory() ? "tree-mode-v1" : "file-bytes-v1";
}
function preserveMode(source, destination, mode) {
  if (process.platform === "win32") return;
  try {
    fs.chmodSync(destination, mode);
  } catch (error) {
    fail("could not preserve mode for " + source + ": " + error.message);
  }
}
function copyPath(source, destination, options = {}) {
  const stat = fs.lstatSync(source);
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  if (stat.isSymbolicLink()) {
    fs.rmSync(destination, { recursive: true, force: true });
    fs.symlinkSync(fs.readlinkSync(source), destination);
    return;
  }
  if (stat.isDirectory()) {
    fs.mkdirSync(destination, { recursive: true, mode: stat.mode & 0o7777 });
    for (const name of fs.readdirSync(source)) {
      if (name === ".jet") continue;
      if (options.excludeFixtureControl && name === ".fixture-modes.tsv") continue;
      copyPath(path.join(source, name), path.join(destination, name), options);
    }
    preserveMode(source, destination, stat.mode & 0o7777);
    return;
  }
  if (!stat.isFile()) fail("unsupported path entry: " + source);
  fs.copyFileSync(source, destination);
  preserveMode(source, destination, stat.mode & 0o7777);
}
function stagePath(source, destination) {
  fs.rmSync(destination, { recursive: true, force: true });
  const stat = fs.lstatSync(source);
  const spec = stat.isDirectory() ? fixtureModeSpec(source) : { modes: new Map() };
  copyPath(source, destination, { excludeFixtureControl: stat.isDirectory() });
  if (stat.isDirectory()) {
    const declarations = [...spec.modes.entries()].sort(([left], [right]) => {
      const leftDepth = left.split("/").length;
      const rightDepth = right.split("/").length;
      return rightDepth - leftDepth || left.localeCompare(right);
    });
    for (const [relative, declaration] of declarations) {
      const target = path.join(destination, ...relative.split("/"));
      preserveMode(declaration.target, target, declaration.mode);
    }
  }
  return spec;
}
function walkFiles(directory) {
  const files = [];
  function collect(current) {
    const stat = fs.lstatSync(current);
    if (stat.isDirectory()) {
      if (path.basename(current) === ".jet") return;
      for (const name of fs.readdirSync(current)) collect(path.join(current, name));
    } else if (stat.isFile()) {
      files.push(current);
    } else if (stat.isSymbolicLink()) {
      return;
    } else {
      fail("unsupported corpus closure entry: " + current);
    }
  }
  collect(directory);
  return files.sort((left, right) => {
    const a = path.relative(root, left).split(path.sep).join("/");
    const b = path.relative(root, right).split(path.sep).join("/");
    return a < b ? -1 : a > b ? 1 : 0;
  });
}
function hashRelativeFiles(files) {
  const digest = crypto.createHash("sha256");
  for (const file of files) {
    digest.update(path.relative(root, file).split(path.sep).join("/"));
    digest.update("\0");
    digest.update(fs.readFileSync(file));
    digest.update("\0");
  }
  return digest.digest("hex");
}
function relativeFiles(relativePaths) {
  return relativePaths.map(file => path.relative(root, file).replaceAll(path.sep, "/")).sort();
}
function cleanText(value) {
  return String(value || "").replace(/[\t\r\n]+/g, " ").trim();
}
function commandText(command, args) {
  return [command, ...args].map(value => JSON.stringify(String(value))).join(" ");
}
function nowNs() {
  return process.hrtime.bigint();
}

// Keep timing and peak-resident-set collection in the runner. The helper is a
// child process so it can sample the workload while this synchronous producer
// remains easy to audit. It intentionally fails when the host has no supported
// RSS collector; a missing memory measurement is not a zero.
const MEASURE_CHILD = String.raw`
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
const command = JSON.parse(process.env.JET_MEASURE_COMMAND);
const timeoutMs = Number(process.env.JET_MEASURE_TIMEOUT_MS);
if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
  process.stderr.write("measurement timeout is invalid");
  process.exit(1);
}
const started = process.hrtime.bigint();
let peak = 0;
let collectorError = "";
function linuxRss(pid) {
  const text = fs.readFileSync("/proc/" + pid + "/status", "utf8");
  const values = [...text.matchAll(/^Vm(?:HWM|RSS):\s+([0-9]+)\s+kB$/gm)].map(match => Number(match[1]) * 1024);
  return values.length ? Math.max(...values) : null;
}
function macosRss(pid) {
  const result = spawnSync("ps", ["-o", "rss=", "-p", String(pid)], { encoding: "utf8" });
  if (result.error || result.status !== 0) return null;
  const value = Number(String(result.stdout).trim().split(/\s+/)[0]);
  return Number.isFinite(value) && value > 0 ? value * 1024 : null;
}
function windowsRss(pid) {
  const script = "(Get-Process -Id " + pid + ").PeakWorkingSet64";
  for (const shell of ["pwsh", "powershell"]) {
    const result = spawnSync(shell, ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8" });
    if (result.error || result.status !== 0) continue;
    const value = Number(String(result.stdout).trim().split(/\s+/)[0]);
    if (Number.isFinite(value) && value > 0) return value;
  }
  return null;
}
function sampleRss(pid) {
  try {
    const value = process.platform === "linux" ? linuxRss(pid) :
      process.platform === "darwin" ? macosRss(pid) :
        process.platform === "win32" ? windowsRss(pid) : null;
    if (value !== null) peak = Math.max(peak, value);
  } catch (error) {
    if (peak === 0) collectorError ||= "RSS collector failed: " + error.message;
  }
}

const useGnuTime = process.platform === "linux";
const rssFile = useGnuTime ? path.join(os.tmpdir(), "jet-workload-rss-" + process.pid + ".txt") : "";
const child = spawn(useGnuTime ? "time" : command[0], useGnuTime ? [
  "-f", "%M", "-o", rssFile, "--", ...command,
] : command.slice(1), {
  cwd: process.env.JET_MEASURE_CWD,
  env: process.env,
  detached: process.platform !== "win32",
  stdio: ["ignore", "pipe", "pipe"],
});
const stdout = [];
const stderr = [];
child.stdout.on("data", chunk => stdout.push(chunk));
child.stderr.on("data", chunk => stderr.push(chunk));
child.on("error", error => stderr.push(Buffer.from(String(error))));
if (!useGnuTime) sampleRss(child.pid);
const sampler = useGnuTime ? null : setInterval(() => sampleRss(child.pid), 5);
const timeout = setTimeout(() => {
  stderr.push(Buffer.from("measurement timed out after " + timeoutMs + "ms"));
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  } else {
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {
      child.kill("SIGKILL");
    }
  }
}, timeoutMs);
child.on("close", (status, signal) => {
  clearTimeout(timeout);
  clearInterval(sampler);
  if (useGnuTime) {
    try {
      peak = Number(fs.readFileSync(rssFile, "utf8").trim()) * 1024;
    } catch (error) {
      collectorError = "GNU time RSS collector failed: " + error.message;
    } finally {
      fs.rmSync(rssFile, { force: true });
    }
  }
  if (peak <= 0) {
    process.stderr.write(collectorError || "RSS collector produced no measurement");
    process.exit(1);
  }
  process.stdout.write(JSON.stringify({
    status: status === null ? 124 : status,
    signal: signal || "",
    stdout: Buffer.concat(stdout).toString("base64"),
    stderr: Buffer.concat(stderr).toString("base64"),
    duration: Number(process.hrtime.bigint() - started),
    rss: peak,
  }));
});
`;

function runMeasured(command, args, cwd, extraEnv = {}) {
  const payload = JSON.stringify([command, ...args]);
  const timeoutMs = Number(process.env.JET_COMPILED_WORKLOAD_TIMEOUT_MS || "180000");
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    fail("JET_COMPILED_WORKLOAD_TIMEOUT_MS must be a positive number");
  }
  const result = spawnSync(process.execPath, ["--input-type=module", "-e", MEASURE_CHILD], {
    cwd,
    env: {
      ...process.env,
      ...extraEnv,
      LC_ALL: "C",
      LANG: "C",
      JET_MEASURE_COMMAND: payload,
      JET_MEASURE_TIMEOUT_MS: String(timeoutMs),
      JET_MEASURE_CWD: cwd,
    },
    maxBuffer: 128 * 1024 * 1024,
    timeout: timeoutMs + 10_000,
    killSignal: "SIGKILL",
  });
  if (result.error) fail("could not measure " + commandText(command, args) + ": " + result.error.message);
  if (result.status !== 0) fail("RSS measurement failed for " + commandText(command, args) + ": " + cleanText(result.stderr));
  let measured;
  try {
    measured = JSON.parse(result.stdout);
  } catch {
    fail("RSS measurement output is invalid for " + commandText(command, args));
  }
  return {
    status: measured.status,
    stdout: Buffer.from(measured.stdout, "base64"),
    stderr: Buffer.from(measured.stderr, "base64"),
    duration: measured.duration,
    rss: measured.rss,
    command: commandText(command, args),
  };
}
function checked(result, expected, label) {
  if (result.status !== 0) fail(label + " exited " + result.status + ": " + cleanText(result.stderr));
  const expectedBytes = fs.readFileSync(expected);
  if (!result.stdout.equals(expectedBytes)) {
    fail(label + " output differs from " + expected);
  }
}
function toolVersion(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8", maxBuffer: 1024 * 1024 });
  if (result.error || result.status !== 0) fail("tool version failed: " + command);
  return cleanText(result.stdout);
}
function toolchainIdentity(language, command, args) {
  const executable = language === "jet" ? command : resolveExecutable(command);
  if (!executable) fail("toolchain executable is unavailable: " + language + "/" + command);
  const rawVersion = toolVersion(executable, args);
  const version = language === "zig" ? "zig " + rawVersion :
    language === "domain" ? "node " + rawVersion : rawVersion;
  if (!version.startsWith(toolchainContract.get(language).version_prefix)) {
    fail("toolchain version drifted for " + language + ": " + version);
  }
  const sha256 = hashFile(executable);
  if (!/^[0-9a-f]{64}$/.test(sha256) || /^0+$/.test(sha256)) {
    fail("toolchain executable identity is invalid: " + executable);
  }
  return {
    executable,
    sha256,
    version: version + ";executable=" + executable + ";sha256=" + sha256,
  };
}
function sourceExtension(relative) {
  const extension = path.extname(relative);
  return extension || ".src";
}
function sourceCandidates(language) {
  return {
    jet: ["main.jet", "src/main.jet", "index.jet"],
    rust: ["main.rs", "src/main.rs"],
    go: ["main.go", "cmd/main/main.go"],
    cxx: ["main.cpp", "main.cc", "main.cxx", "src/main.cpp", "src/main.cc"],
    zig: ["main.zig", "src/main.zig"],
    domain: ["index.mjs", "main.mjs", "index.js", "main.js"],
  }[language] || [];
}
const PEER_LAUNCH_CONTRACT = "compiled-workload-peer-isolation-v1";
let peerLauncherCache = null;
function peerLauncherInfo() {
  const configured = process.env.JET_COMPILED_WORKLOAD_PEER_LAUNCHER || "";
  if (!configured) fail("peer isolation launcher is unavailable");
  const launcher = resolveExecutable(configured);
  if (!launcher) fail("peer isolation launcher is unavailable: " + configured);
  if (process.platform === "win32") {
    if (!launcher.toLowerCase().endsWith(".exe")) fail("Windows peer isolation launcher must be an executable: " + launcher);
  } else {
    let mode;
    if (launcher.toLowerCase().endsWith(".exe")) fail("non-Windows peer isolation launcher must not be a Windows executable: " + launcher);
    try {
      mode = fs.statSync(launcher).mode;
    } catch (error) {
      fail("peer isolation launcher cannot be inspected: " + error.message);
    }
    if (!(mode & 0o111)) fail("peer isolation launcher is not executable: " + launcher);
  }
  if (peerLauncherCache) {
    const current = hashFile(launcher);
    if (current !== peerLauncherCache.sha256) fail("peer isolation launcher changed during run");
    return peerLauncherCache;
  }
  const probe = spawnSync(launcher, ["--contract"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 1024 * 1024,
  });
  if (probe.error || probe.status !== 0 || cleanText(probe.stdout) !== PEER_LAUNCH_CONTRACT) {
    fail("peer isolation launcher contract probe failed: " + launcher);
  }
  const version = toolVersion(launcher, ["--version"]);
  if (!version) fail("peer isolation launcher returned no version: " + launcher);
  peerLauncherCache = { path: launcher, version, sha256: hashFile(launcher) };
  return peerLauncherCache;
}
function peerMeasured(task, command, args, cwd, extraEnv = {}) {
  const launcherInfo = peerLauncherInfo();
  const temporaryDirectory = path.join(cwd, ".tmp");
  fs.mkdirSync(temporaryDirectory, { recursive: true });
  const authority = authorityFields(task.authority);
  const inputKeys = ["argv", "build-input"].filter(key => authority.has(key));
  const allowedKeys = new Set(["argv", "build-input", "cwd", "host", "network", "external-write"]);
  if (authority.size !== 5 || inputKeys.length !== 1 || [...authority.keys()].some(key => !allowedKeys.has(key)) ||
      authority.get(inputKeys[0]) !== "input-root" || authority.get("cwd") !== "scratch" ||
      authority.get("host") !== "ambient" ||
      !["disabled", "loopback-only"].includes(authority.get("network")) ||
      authority.get("external-write") !== "disabled") {
    fail("peer isolation authority is missing or unsupported: " + task.task_id);
  }
  const launchArgs = [
    "--contract=" + PEER_LAUNCH_CONTRACT,
    "--task-id=" + task.task_id,
    "--root=" + cwd,
    "--cwd=" + cwd,
    "--host=" + authority.get("host"),
    "--network=" + authority.get("network"),
    "--external-write=" + authority.get("external-write"),
    "--",
    command,
    ...args,
  ];
  return runMeasured(launcherInfo.path, launchArgs, cwd, {
    TMPDIR: temporaryDirectory,
    TMP: temporaryDirectory,
    TEMP: temporaryDirectory,
    ...extraEnv,
    JET_COMPILED_WORKLOAD_PEER_CONTRACT: PEER_LAUNCH_CONTRACT,
  });
}

function assertFrozenPeerCommand(task, language, phase, command, args, ledgerOverride = null) {
  const ledger = ledgerOverride || selectedPeers.get(task.task_id);
  if (!ledger) fail("peer missing from ledger: " + task.task_id + "/" + language);
  const frozen = String(phase === "build" ? ledger.build_command : ledger.run_command).trim();
  const tokens = frozen.split(/\s+/);
  const frozenCommand = tokens.shift();
  if (!frozenCommand || !tokens.length && phase === "build") {
    fail("frozen peer " + phase + " command is empty: " + task.task_id + "/" + language);
  }
  if (phase === "build") {
    const commandFamily = command => {
      const base = path.basename(command).toLowerCase().replace(/\.exe$/, "");
      if (base === "c++" || base === "clang++" || base === "g++" || base.endsWith("-g++")) return "cxx";
      return base;
    };
    const actualCommand = commandFamily(command);
    const expectedCommand = commandFamily(frozenCommand);
    if (actualCommand !== expectedCommand) {
      fail("peer " + phase + " tool drifted from ledger: " + task.task_id + "/" + language + " (" + actualCommand + " != " + expectedCommand + ")");
    }
    for (const token of tokens.filter(token => token.startsWith("-") || ["build", "build-exe"].includes(token))) {
      if (!args.includes(token)) fail("peer " + phase + " flag drifted from ledger: " + task.task_id + "/" + language + "/" + token);
    }
  } else if (!tokens.includes("INPUT") || args.length === 0) {
    fail("peer run input boundary drifted from ledger: " + task.task_id + "/" + language);
  }
}
function sourceEntry(sourcePath, language) {
  const stat = fs.lstatSync(sourcePath);
  if (stat.isFile()) return sourcePath;
  if (!stat.isDirectory()) fail("source is not a file or directory: " + sourcePath);
  for (const relative of sourceCandidates(language)) {
    const candidate = path.join(sourcePath, relative);
    if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) return candidate;
  }
  const packageFile = path.join(sourcePath, "package.json");
  if (language === "domain" && fs.existsSync(packageFile)) {
    let packageData;
    try {
      packageData = JSON.parse(fs.readFileSync(packageFile, "utf8"));
    } catch (error) {
      fail("domain source package.json is invalid: " + error.message);
    }
    for (const relative of [packageData.main, packageData.module, packageData.browser]) {
      if (typeof relative !== "string" || !relative) continue;
      const candidate = path.resolve(sourcePath, relative);
      if (candidate !== sourcePath && !candidate.startsWith(sourcePath + path.sep)) {
        fail("domain package entry escaped source root: " + relative);
      }
      if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) return candidate;
    }
  }
  fail("source directory has no conventional " + language + " entry: " + sourcePath);
}
function sourceClosureFiles(sourcePath) {
  const stat = fs.lstatSync(sourcePath);
  if (stat.isFile()) return [sourcePath];
  if (!stat.isDirectory()) fail("source is not a file or directory: " + sourcePath);
  const files = [];
  function collect(file) {
    const entry = fs.lstatSync(file);
    if (entry.isDirectory()) {
      if (path.basename(file) === ".jet") return;
      for (const name of fs.readdirSync(file)) collect(path.join(file, name));
    } else if (entry.isFile() || entry.isSymbolicLink()) {
      files.push(file);
    } else {
      fail("unsupported source closure entry: " + file);
    }
  }
  collect(sourcePath);
  return files.sort((left, right) => {
    const a = path.relative(sourcePath, left).split(path.sep).join("/");
    const b = path.relative(sourcePath, right).split(path.sep).join("/");
    return a < b ? -1 : a > b ? 1 : 0;
  });
}
function stageSource(sourcePath, destinationRoot, language) {
  fs.rmSync(destinationRoot, { recursive: true, force: true });
  fs.mkdirSync(destinationRoot, { recursive: true });
  const stat = fs.lstatSync(sourcePath);
  if (stat.isFile()) {
    const destination = path.join(destinationRoot, "main" + sourceExtension(sourcePath));
    copyPath(sourcePath, destination);
    return { root: destinationRoot, entry: destination, files: [destination] };
  }
  if (!stat.isDirectory()) fail("source is not a file or directory: " + sourcePath);
  for (const name of fs.readdirSync(sourcePath)) {
    copyPath(path.join(sourcePath, name), path.join(destinationRoot, name));
  }
  const entry = sourceEntry(destinationRoot, language);
  return { root: destinationRoot, entry, files: sourceClosureFiles(destinationRoot) };
}
function authorityFields(authority) {
  const fields = new Map();
  for (const field of authority.split(";")) {
    const separator = field.indexOf("=");
    if (separator < 1) fail("malformed task authority: " + authority);
    fields.set(field.slice(0, separator), field.slice(separator + 1));
  }
  return fields;
}
function jetAuthority(task, sourcePath) {
  const fields = authorityFields(task.authority);
  const source = sourceClosureFiles(sourcePath)
    .filter(file => fs.lstatSync(file).isFile())
    .map(file => codeWithoutCommentsAndStrings(fs.readFileSync(file, "utf8")))
    .join("\n");
  const allow = new Set();
  if (fields.has("argv") || fields.has("build-input") || source.includes("core.files")) allow.add("FS");
  if (source.includes("print(")) allow.add("IO");
  if (fields.get("network") === "loopback-only" || /\bcore\.(?:http|net)\b/.test(source)) allow.add("Net");
  if (/\bpanic\s*\(/.test(source)) allow.add("Panic");
  if (/\b(?:String|struct|loop|use core\.|embed_file|print\s*\()/.test(source)) allow.add("Mem.Alloc");
  if (allow.size === 0) fail("task authority resolved to no Jet effects: " + task.task_id);
  return [...allow].sort().join(", ");
}
function writeWorkloadPackage(project, task, sourcePath) {
  const allow = jetAuthority(task, sourcePath);
  fs.writeFileSync(
    path.join(project, "package.jet"),
    "name: \"compiled-workload\"\n" +
      "version: \"0.1.0\"\n" +
      "authority: { holds: { allow: [" + allow + "] } }\n",
  );
}

const manifest = table(path.join(corpus, "manifest.tsv"));
const peers = table(path.join(corpus, "peer_ledger.tsv"));
const adapters = table(path.join(corpus, "adapter_ledger.tsv"));
const peerAdapters = table(path.join(corpus, "peer_adapter_ledger.tsv"));
const metricContract = table(path.join(corpus, "metric_contract.tsv"));
const policyRows = table(path.join(corpus, "measurement_policy.tsv"));
const tierMatrix = table(path.join(corpus, "tier_matrix.tsv"));
const toolchains = table(path.join(corpus, "toolchain_contract.tsv"));
const toolchainContract = new Map(toolchains.map(row => [row.language, row]));
const peerKey = peer => [peer.task_id, peer.selection, peer.language, peer.program].join("|");
const selectedPeers = new Map(peers.filter(row => row.selection === "best-applicable").map(row => [row.task_id, row]));
const adapterByTask = new Map(adapters.map(row => [row.task_id, row]));
const peerAdapterByKey = new Map(peerAdapters.map(row => [row.peer_key, row]));
const taskById = new Map(manifest.map(row => [row.task_id, row]));
const metricUnits = new Map(metricContract.map(row => [row.metric, row.unit]));
const policies = new Map(policyRows.map(row => [row.metric, row]));

function policyTolerance(taskId, metric) {
  const policy = policies.get(metric);
  const peerLanguage = selectedPeers.get(taskId)?.language;
  if (!policy || !peerLanguage) fail("measurement policy or peer missing: " + taskId + "/" + metric);
  return Number(peerLanguage === "rust" ? policy.rust_tolerance_ratio : policy.tolerance_ratio);
}

function policyComparison(taskId, metric) {
  const peerLanguage = selectedPeers.get(taskId)?.language;
  if (!peerLanguage) fail("selected peer missing: " + taskId);
  const tolerance = policyTolerance(taskId, metric);
  return {
    peerLanguage,
    tolerance,
    operator: peerLanguage === "rust" ? ">" : ">=",
  };
}
if (toolchainContract.size !== 6) fail("frozen toolchain contract is incomplete");
const sampleCounts = new Set(policyRows.map(row => Number(row.samples)));
if (sampleCounts.size !== 1 || !Number.isInteger([...sampleCounts][0]) || [...sampleCounts][0] < 1) fail("measurement policy must use one positive sample count");
const sampleCount = [...sampleCounts][0];
if (manifest.length !== 7 || selectedPeers.size !== 7 || adapterByTask.size !== 7) fail("frozen task/peer/adapter count is incomplete");
if (peers.length < manifest.length || peerAdapterByKey.size !== peers.length) fail("all peer candidates must have one adapter row");
if (new Set(peers.map(peerKey)).size !== peers.length) fail("peer ledger keys must be unique");

for (const policy of policyRows) {
  if (policy.tolerance_ratio !== "1.00" || policy.rust_tolerance_ratio !== "1.05") {
    fail("peer ratio law drifted for metric: " + policy.metric);
  }
}
for (const row of manifest) {
  requireInputPath(path.join(corpus, row.input), "input fixture");
  requireFile(path.join(corpus, row.expected), "expected fixture");
  const adapter = adapterByTask.get(row.task_id);
  const peer = selectedPeers.get(row.task_id);
  if (!adapter || !peer) fail("missing selected adapter or peer: " + row.task_id);
  if (peer.source_revision !== adapter.peer_commit || !/^[0-9a-f]{40}$/.test(adapter.peer_commit)) fail("peer identity is not immutable: " + row.task_id);
  requireInputPath(path.join(corpus, adapter.jet_source), "Jet adapter");
  requireInputPath(path.join(corpus, adapter.jet_hostile), "Jet hostile fixture");
  requireInputPath(path.join(corpus, adapter.peer_source), "peer adapter");
  requireInputPath(path.join(corpus, adapter.peer_hostile), "peer hostile fixture");
  requireFile(path.join(corpus, "expected", row.task_id + ".hostile.out"), "hostile expected output");
}
for (const peer of peers) {
  const adapter = peerAdapterByKey.get(peerKey(peer));
  if (!adapter || adapter.task_id !== peer.task_id || adapter.language !== peer.language ||
      adapter.selection !== peer.selection || adapter.peer_commit !== peer.source_revision) {
    fail("peer adapter row does not bind to ledger: " + peerKey(peer));
  }
  requireInputPath(path.join(corpus, adapter.peer_source), "peer candidate adapter");
  requireInputPath(path.join(corpus, adapter.peer_hostile), "peer candidate hostile fixture");
}

const candidateCommitResult = spawnSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" });
if (candidateCommitResult.status !== 0) fail("git rev-parse HEAD failed");
const candidateCommit = candidateCommitResult.stdout.trim();
if (!/^[0-9a-f]{40}$/.test(candidateCommit)) fail("candidate commit is not immutable");
const taskEnvironment = new Map(manifest.map(row => [
  row.task_id,
  "os=" + platform + ";ci=compiled-workload;locale=C;network=" +
    (row.authority.includes("network=loopback-only") ? "loopback-only" : "disabled"),
]));
const environment = "os=" + platform + ";ci=compiled-workload;locale=C;network=per-task-declared";
const machine = os.platform() + "-" + os.arch() + "-" + cleanText(os.release()).replaceAll(" ", "_");
requireFile(jetBin, "Jet compiler");
if (process.platform !== "win32" && !(fs.statSync(jetBin).mode & 0o111)) {
  fail("Jet compiler is not executable: " + jetBin);
}
const jetBinarySha256 = hashFile(jetBin);
const jetToolInfo = toolchainIdentity("jet", jetBin, ["--version"]);
const jetToolVersion = jetToolInfo.version;
const contractFiles = [
  path.join(corpus, "manifest.tsv"),
  path.join(corpus, "domain_contract.tsv"),
  path.join(corpus, "peer_ledger.tsv"),
  path.join(corpus, "peer_adapter_ledger.tsv"),
  path.join(corpus, "metric_contract.tsv"),
  path.join(corpus, "tier_matrix.tsv"),
  path.join(corpus, "canaries.tsv"),
  path.join(corpus, "adapter_ledger.tsv"),
  path.join(corpus, "measurement_policy.tsv"),
  path.join(corpus, "toolchain_contract.tsv"),
];
const sourceFiles = [
  ...walkFiles(path.join(corpus, "adapters")),
  ...walkFiles(path.join(corpus, "fixtures")),
  ...walkFiles(path.join(corpus, "expected")),
  ...walkFiles(path.join(corpus, "task-definitions")),
];
const contractHash = hashRelativeFiles(contractFiles);
const sourceClosureHash = hashRelativeFiles(sourceFiles);
const peerLauncher = peerLauncherInfo();
const toolchainInfoByLanguage = new Map([["jet", jetToolInfo]]);
const identity = [
  ["1", "candidate_commit", candidateCommit],
  ["1", "platform", platform],
  ["1", "environment", environment],
  ["1", "machine", machine],
  ["1", "samples", String(sampleCount)],
  ["1", "jet_tool_version", jetToolVersion],
  ["1", "jet_binary_sha256", jetBinarySha256],
  ["1", "peer_launcher_path", peerLauncher.path],
  ["1", "peer_launcher_version", peerLauncher.version],
  ["1", "peer_launcher_sha256", peerLauncher.sha256],
  ["1", "contract_sha256", contractHash],
  ["1", "source_closure_sha256", sourceClosureHash],
  ["1", "peer_commits", manifest.map(row => row.task_id + ":" + selectedPeers.get(row.task_id).source_revision).join(",")],
];
const samples = [];
const receipts = [];
const tierReceipts = [];
const tierReports = [];
const peerCoverage = [];
const peerMeasurements = [];
const outcomes = [];
const toolchainByKey = new Map();
function addSamples(task, language, metric, values, unit, method) {
  if (values.length !== sampleCount) fail("sample count does not match policy: " + task + "/" + language + "/" + metric);
  values.forEach((value, index) => samples.push(["1", task, language, metric, String(index + 1), String(value), unit, method, platform, "native", "aot"]));
}
function hashClosure(entries) {
  const digest = crypto.createHash("sha256");
  for (const entry of entries) {
    digest.update(entry.name);
    digest.update("\0");
    digest.update(hashFile(entry.file));
    digest.update("\0");
  }
  return digest.digest("hex");
}
function closureSize(entries) {
  return entries.reduce((total, entry) => total + fs.statSync(entry.file).size, 0);
}
function copyClosure(entries, destination) {
  fs.mkdirSync(destination, { recursive: true });
  for (const entry of entries) copyPath(entry.file, path.join(destination, entry.name));
}
function artifactClosure(language, artifact, sourceFiles = [], sourceRoot = path.dirname(artifact)) {
  const entries = [];
  if (language === "domain") {
    const entryRelative = path.relative(sourceRoot, artifact).split(path.sep).join("/");
    if (!entryRelative || entryRelative.startsWith("../")) fail("domain artifact escaped source root: " + artifact);
    sourceFiles.forEach(file => {
      const relative = path.relative(sourceRoot, file).split(path.sep).join("/");
      if (!relative || relative.startsWith("../")) fail("domain source closure escaped root: " + file);
      entries.push({ name: "source/" + relative, file });
    });
    entries.push({ name: "runtime/node", file: process.execPath });
    entries.entryName = "source/" + entryRelative;
  } else {
    entries.push({ name: "program", file: artifact });
    entries.entryName = "program";
  }
  return entries;
}
function artifactHash(language, artifact, sourceFiles = [], sourceRoot) {
  return hashClosure(artifactClosure(language, artifact, sourceFiles, sourceRoot));
}

function targetArtifactProof(artifact, target) {
  const bytes = fs.readFileSync(artifact);
  if (target.startsWith("aarch64-")) {
    if (bytes.length < 20 || bytes.subarray(0, 4).toString("binary") !== "\x7fELF" || bytes.readUInt16LE(18) !== 183) {
      fail("cross-target artifact is not AArch64 ELF: " + artifact);
    }
    return "format=ELF;machine=AArch64;target=" + target + ";bytes=" + bytes.length;
  }
  if (target === "web") {
    if (bytes.length < 4 || bytes.subarray(0, 4).toString("binary") !== "\0asm") {
      fail("web artifact is not Wasm: " + artifact);
    }
    return "format=Wasm;target=web;bytes=" + bytes.length;
  }
  fail("unsupported target proof: " + target);
}
function jetBuild(project, source, target = "", embeddedInput = "", task) {
  if (!task) fail("Jet build authority contract is missing: " + source);
  fs.mkdirSync(project, { recursive: true });
  const sourcePath = path.join(corpus, source);
  const staged = stageSource(sourcePath, project, "jet");
  writeWorkloadPackage(project, task, sourceEntry(sourcePath, "jet"));
  if (embeddedInput) {
    const destination = fs.lstatSync(embeddedInput).isDirectory() ?
      path.join(project, "input-root") : path.join(project, "input.tsv");
    stagePath(embeddedInput, destination);
  }
  const args = ["build", "--profile=release"];
  if (target) args.push("--target=" + target);
  args.push(path.relative(project, staged.entry).split(path.sep).join("/"));
  const env = {
    JET_CACHE_DIR: path.join(project, ".jet-build-cache"),
    JET_RUN_CACHE_DIR: path.join(project, ".jet-run-cache"),
    JET_RUNTIME_CACHE_DIR: path.join(project, ".jet-runtime-cache"),
    ...(target === "aarch64-unknown-linux-gnu" ? {
      RUSTC_LINKER: process.env.RUSTC_LINKER_CROSS || "aarch64-linux-gnu-gcc",
      CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER:
        process.env.RUSTC_LINKER_CROSS || "aarch64-linux-gnu-gcc",
      CC_aarch64_unknown_linux_gnu:
        process.env.CC_CROSS || process.env.RUSTC_LINKER_CROSS || "aarch64-linux-gnu-gcc",
      AR_aarch64_unknown_linux_gnu: process.env.AR_CROSS || "aarch64-linux-gnu-ar",
    } : {}),
  };
  const result = peerMeasured(task, jetBin, args, project, env);
  if (result.status !== 0) fail("Jet build failed for " + source + ": " + cleanText(result.stderr));
  const binary = path.join(project, "build", "main" + exeSuffix);
  if (!target || target !== "web") requireFile(binary, "Jet build artifact");
  return { result, binary, args, env, source: staged };
}
function jetWebBuild(project, source, embeddedInput, task) {
  const build = jetBuild(project, source, "web", embeddedInput, task);
  const js = path.join(project, "build", "app.js");
  const wasm = path.join(project, "build", "app.wasm");
  const runtime = path.join(project, "build", "jet_dom_runtime.js");
  const index = path.join(project, "build", "index.html");
  requireFile(js, "Jet web JavaScript");
  requireFile(wasm, "Jet web Wasm");
  requireFile(runtime, "Jet web DOM runtime");
  requireFile(index, "Jet web host page");
  return { ...build, js, wasm, runtime, index };
}
function hashFiles(files) {
  return hashClosure(files.map(file => ({
    name: path.relative(root, file).split(path.sep).join("/"),
    file,
  })));
}
function resolveExecutable(candidate) {
  if (path.isAbsolute(candidate)) return fs.existsSync(candidate) ? candidate : "";
  for (const directory of (process.env.PATH || "").split(path.delimiter)) {
    const resolved = path.join(directory, candidate);
    if (fs.existsSync(resolved)) return resolved;
  }
  return "";
}
function chromiumExecutable() {
  const configured = process.env.JET_WEB_CHROMIUM || process.env.CHROMIUM || "chromium";
  const resolved = resolveExecutable(configured);
  if (!resolved) fail("cross-platform-notes web tier requires Chromium/Chrome: " + configured);
  const version = spawnSync(resolved, ["--version"], { encoding: "utf8", maxBuffer: 1024 * 1024 });
  const text = cleanText((version.stdout || "") + " " + (version.stderr || ""));
  if (version.status !== 0 || !/(Chromium|Chrome)/.test(text)) fail("configured Chromium/Chrome is unusable: " + resolved);
  return resolved;
}
function webBrowserRun(buildDir, expected, task, peer = false) {
  const chromium = chromiumExecutable();
  const taskId = task.task_id;
  const harness = path.join(buildDir, ".compiled-workload-web-browser.mjs");
  const driverLocal = path.join(buildDir, ".compiled-workload-cdp-driver.mjs");
  const serveLocal = path.join(buildDir, ".compiled-workload-serve.mjs");
  fs.copyFileSync(path.join(root, "scripts", "canvas-test", "driver.mjs"), driverLocal);
  fs.copyFileSync(path.join(root, "scripts", "web-test", "serve.mjs"), serveLocal);
  const driverModule = pathToFileURL(driverLocal).href;
  const serveScript = serveLocal;
  const chromeTemp = path.join(buildDir, ".chrome-temp");
  fs.writeFileSync(harness, [
    'import fs from "node:fs";',
    'import net from "node:net";',
    'import { spawn } from "node:child_process";',
    `const driverModule = ${JSON.stringify(driverModule)};`,
    `const serveScript = ${JSON.stringify(serveScript)};`,
    'const buildDir = process.argv[2];',
    'const expectedPath = process.argv[3];',
    'const expected = fs.readFileSync(expectedPath, "utf8");',
    'const sleep = (ms) => new Promise(resolve => setTimeout(resolve, ms));',
    'const reservation = net.createServer();',
    'await new Promise((resolve, reject) => { reservation.once("error", reject); reservation.listen(0, "127.0.0.1", resolve); });',
    'const port = reservation.address().port;',
    'await new Promise(resolve => reservation.close(resolve));',
    'const server = spawn(process.execPath, [serveScript, "--port", String(port), "--root", buildDir], { stdio: ["ignore", "ignore", "pipe"] });',
    'let serverError = "";',
    'server.stderr.setEncoding("utf8");',
    'server.stderr.on("data", chunk => { serverError += chunk; });',
    'let serving = false;',
    'for (let attempt = 0; attempt < 200; attempt += 1) {',
    '  try { const response = await fetch("http://127.0.0.1:" + port + "/index.html", { cache: "no-store" }); if (response.ok) { serving = true; break; } } catch {}',
    '  await sleep(50);',
    '}',
    'if (!serving) { server.kill("SIGKILL"); throw new Error("web server did not serve index.html: " + serverError); }',
    `const { CdpDriver } = await import(${JSON.stringify(driverModule)});`,
    'const driver = await new CdpDriver({ chrome: process.env.CHROMIUM, chromeTempRoot: process.env.JET_WEB_CHROME_TEMP }).launch();',
    'try {',
    '  await driver.send("Page.addScriptToEvaluateOnNewDocument", { source: "window.__compiledWorkloadLogs = []; console.log = (...args) => window.__compiledWorkloadLogs.push(args.map(value => String(value)).join(\\" \\"));" }, driver.pageSession);',
    '  await driver.navigate("http://127.0.0.1:" + port + "/index.html");',
    '  const expectedLines = expected.replace(/\\r?\\n$/, "").split(/\\r?\\n/);',
    '  let logs = [];',
    '  for (let attempt = 0; attempt < 400; attempt += 1) {',
    '    logs = await driver.evaluate("window.__compiledWorkloadLogs || []");',
    '    if (logs.join("\\n") === expectedLines.join("\\n")) break;',
    '    await sleep(50);',
    '  }',
    '  if (logs.join("\\n") !== expectedLines.join("\\n")) throw new Error("browser output differs: " + logs.join("|"));',
    '  process.stdout.write(expected);',
    '} finally {',
    '  await driver.close();',
    '  server.kill("SIGKILL");',
    '}',
  ].join("\n") + "\n");
  const args = [harness, buildDir, expected];
  try {
    const command = process.execPath;
    const result = peerMeasured(task, command, args, buildDir, { CHROMIUM: chromium, JET_WEB_CHROME_TEMP: chromeTemp });
    checked(result, expected, taskId + "/" + (peer ? "peer" : "jet") + "/web-browser");
    return { ...result, chromium };
  } finally {
    fs.rmSync(chromeTemp, { recursive: true, force: true });
  }
}
function peerWebBuild(source, input, dir, task) {
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  const staged = stageSource(path.join(corpus, source), path.join(dir, "source"), "domain");
  const index = path.join(dir, "index.html");
  const raw = fs.lstatSync(input).isDirectory() ?
    JSON.stringify({ inputRoot: path.basename(input) }) : fs.readFileSync(input, "utf8");
  const modulePath = "./" + path.relative(dir, staged.entry).split(path.sep).join("/");
  fs.writeFileSync(index, [
    "<!doctype html>",
    '<meta charset="utf-8">',
    "<script type=\"module\">",
    `globalThis.__compiledWorkloadInput = ${JSON.stringify(raw)};`,
    `await import(${JSON.stringify(modulePath)});`,
    "</script>",
    "",
  ].join("\n"));
  return { module: staged.entry, index, source: staged };
}

function peerBuild(language, source, dir, crossTarget = "", task, ledgerOverride = null) {
  if (!task) fail("peer build authority contract is missing: " + source);
  fs.mkdirSync(dir, { recursive: true });
  const sourcePath = path.join(corpus, source);
  const staged = stageSource(sourcePath, dir, language);
  const entry = path.relative(dir, staged.entry).split(path.sep).join("/");
  const output = path.join(dir, "peer" + exeSuffix);
  let command;
  let args;
  let env = {};
  if (language === "rust") {
    command = process.env.RUSTC || toolchainContract.get("rust").command;
    args = ["--edition=2021", "-O"];
    if (crossTarget) args.push("--target=" + crossTarget, "-C", "linker=" + (process.env.RUSTC_LINKER_CROSS || "aarch64-linux-gnu-gcc"));
    args.push(entry, "-o", output);
  } else if (language === "go") {
    command = toolchainContract.get("go").command;
    args = ["build", "-trimpath", "-o", output, entry];
    env = {
      GOCACHE: path.join(dir, ".go-cache"),
      GOMODCACHE: path.join(dir, ".go-mod-cache"),
      ...(crossTarget ? { CGO_ENABLED: "0", GOOS: "linux", GOARCH: "arm64" } : {}),
    };
  } else if (language === "cxx") {
    command = toolchainContract.get("cxx").command;
    if (crossTarget) command = process.env.CXX_CROSS || "aarch64-linux-gnu-g++";
    args = ["-std=c++20", "-O2", "-pipe", entry, "-o", output];
  } else if (language === "zig") {
    command = toolchainContract.get("zig").command;
    args = ["build-exe", "-O", "ReleaseFast"];
    if (crossTarget) args.push("-target", "aarch64-linux-gnu");
    args.push(entry, "-femit-bin=" + output);
  } else if (language === "domain") {
    if (crossTarget) fail("domain peer declared for aarch64");
    command = process.execPath;
    args = ["--check", entry];
  } else if (language === "swift") {
    command = toolchainContract.get("swift")?.command || "swiftc";
    args = ["-O", entry, "-o", output];
  } else {
    fail("unsupported peer language: " + language);
  }
  assertFrozenPeerCommand(task, language, "build", command, args, ledgerOverride);
  const result = peerMeasured(task, command, args, dir, env);
  if (result.status !== 0) fail("peer build failed for " + source + ": " + cleanText(result.stderr));
  const artifact = language === "domain" ? staged.entry : output;
  return {
    result,
    closure: artifactClosure(language, artifact, staged.files, staged.root),
    artifact,
    command,
    args,
    source: staged,
  };
}
function peerRun(language, artifact, input, cwd, task, ledgerOverride = null) {
  if (language === "domain") {
    assertFrozenPeerCommand(task, language, "run", process.execPath, [artifact, input], ledgerOverride);
    return peerMeasured(task, process.execPath, [artifact, input], cwd);
  }
  assertFrozenPeerCommand(task, language, "run", artifact, [input], ledgerOverride);
  return peerMeasured(task, artifact, [input], cwd);
}
const DIAGNOSTIC_RUBRIC = [
  ["classification", /\b(?:error|fatal|syntax|parse|diagnostic)\b/i],
  ["location", /(?:^|\s|:)\d+(?::\d+)?(?:\b|$)/m],
  ["cause", /\b(?:expected|unexpected|invalid|unclosed|unknown|mismatch|missing)\b/i],
  ["action", /\b(?:help|fix|suggest|consider|add|remove)\b/i],
  ["source", /(?:main|diagnostic|\.jet|\.rs|\.go|\.c(?:c|xx)?|\.zig|\.m?js|\.swift)\b/i],
];
function diagnosticProbe(language, sourcePath, dir, task, jet, ledgerOverride = null) {
  fs.rmSync(dir, { recursive: true, force: true });
  const staged = stageSource(sourcePath, dir, language);
  const source = staged.entry;
  const invalid = language === "rust" ? "\nfn __diagnostic_probe( {\n" :
    language === "go" ? "\nfunc __diagnostic_probe( {\n" :
      language === "cxx" ? "\nvoid __diagnostic_probe( {\n" :
        language === "zig" ? "\nfn __diagnostic_probe( {\n" :
          language === "swift" ? "\nfunc __diagnostic_probe( {\n" :
            language === "jet" ? "\nfn __diagnostic_probe( {\n" :
              "\nfunction __diagnostic_probe( {\n";
  fs.appendFileSync(source, invalid);
  const entry = path.relative(dir, source).split(path.sep).join("/");
  let command;
  let args;
  if (jet) {
    command = jetBin;
    args = ["check", entry];
  } else if (language === "rust") {
    command = process.env.RUSTC || toolchainContract.get("rust").command;
    args = ["--edition=2021", entry, "-o", path.join(dir, "diagnostic" + exeSuffix)];
  } else if (language === "go") {
    command = toolchainContract.get("go").command;
    args = ["build", "-o", path.join(dir, "diagnostic" + exeSuffix), entry];
  } else if (language === "cxx") {
    command = toolchainContract.get("cxx").command;
    args = ["-std=c++20", entry, "-o", path.join(dir, "diagnostic" + exeSuffix)];
  } else if (language === "zig") {
    command = toolchainContract.get("zig").command;
    args = ["build-exe", entry, "-femit-bin=" + path.join(dir, "diagnostic" + exeSuffix)];
  } else if (language === "swift") {
    command = toolchainContract.get("swift")?.command || "swiftc";
    args = [entry, "-o", path.join(dir, "diagnostic" + exeSuffix)];
  } else {
    command = process.execPath;
    args = ["--check", entry];
  }
  const result = peerMeasured(task, command, args, dir);
  if (result.status === 0) fail("diagnostic probe was accepted: " + language);
  const text = cleanText(result.stdout.toString("utf8") + "\n" + result.stderr.toString("utf8"));
  const checks = Object.fromEntries(DIAGNOSTIC_RUBRIC.map(([name, pattern]) => [name, pattern.test(text)]));
  const passed = Object.values(checks).filter(Boolean).length;
  if (passed < 3) fail("diagnostic rubric failed: " + language + " (" + passed + "/5)");
  const steps = [
    "stage-source-closure",
    "append-invalid-declaration",
    "invoke-compiler-check",
    "capture-stdout-stderr",
    "apply-rubric-v1",
  ];
  const stdoutHash = hashBuffer(result.stdout);
  const stderrHash = hashBuffer(result.stderr);
  return {
    result,
    burden: DIAGNOSTIC_RUBRIC.length - passed,
    passed,
    checks,
    steps,
    method: "diagnostic-rubric-v1:passed=" + passed + ";burden=" + (DIAGNOSTIC_RUBRIC.length - passed) +
      ";steps=" + steps.join(",") + ";replay=" + result.command +
      ";stdout-sha256=" + stdoutHash + ";stderr-sha256=" + stderrHash,
  };
}
function sourceEffort(sourcePath) {
  let lines = 0;
  for (const file of sourceClosureFiles(sourcePath)) {
    const stat = fs.lstatSync(file);
    if (!stat.isFile()) continue;
    const text = fs.readFileSync(file, "utf8");
    lines += text.length === 0 ? 0 : text.split(/\r?\n/).length - (text.endsWith("\n") ? 1 : 0);
  }
  return lines;
}
function codeWithoutCommentsAndStrings(text) {
  const output = [];
  let state = "code";
  let quote = "";
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (state === "code") {
      if (character === "/" && text[index + 1] === "/") {
        output.push(" ", " ");
        state = "line-comment";
        index += 1;
      } else if (character === "/" && text[index + 1] === "*") {
        output.push(" ", " ");
        state = "block-comment";
        index += 1;
      } else if (character === "'" || character === "\"" || character === "`") {
        output.push(" ");
        quote = character;
        state = "string";
      } else {
        output.push(character);
      }
    } else if (state === "line-comment") {
      output.push(character === "\n" ? "\n" : " ");
      if (character === "\n") state = "code";
    } else if (state === "block-comment") {
      if (character === "*" && text[index + 1] === "/") {
        output.push(" ", " ");
        state = "code";
        index += 1;
      } else {
        output.push(character === "\n" ? "\n" : " ");
      }
    } else if (character === "\\") {
      output.push(" ");
      if (text[index + 1] !== undefined) {
        output.push(text[index + 1] === "\n" ? "\n" : " ");
        index += 1;
      }
    } else if (character === quote) {
      output.push(" ");
      state = "code";
    } else {
      output.push(character === "\n" ? "\n" : " ");
    }
  }
  return output.join("");
}
function unsafeInventory(sourcePath, language) {
  const patterns = {
    jet: /\bunsafe\b/g,
    rust: /\bunsafe\b/g,
    go: /\bunsafe\b|\b(?:syscall|cgo)\b/g,
    cxx: /\b(?:reinterpret_cast|malloc|free|new|delete)\b|->|\b(?:const\s+)?(?:auto|char|unsigned\s+char|void|int|long|float|double)\s+\*+\s*[A-Za-z_][A-Za-z0-9_]*/g,
    zig: /@(?:ptrCast|intFromPtr|ptrFromInt|embedFile)\b|\[\*c?\]/g,
    domain: /\b(?:eval|Function|child_process)\b/g,
  };
  const pattern = patterns[language] || /\bunsafe\b/g;
  const sites = [];
  for (const file of sourceClosureFiles(sourcePath)) {
    const stat = fs.lstatSync(file);
    if (!stat.isFile()) continue;
    const source = fs.readFileSync(file, "utf8");
    const scrubbed = codeWithoutCommentsAndStrings(source);
    for (const match of scrubbed.matchAll(pattern)) {
      const line = scrubbed.slice(0, match.index).split("\n").length;
      const lineStart = scrubbed.lastIndexOf("\n", match.index - 1) + 1;
      const lineEnd = scrubbed.indexOf("\n", match.index);
      const lineText = scrubbed.slice(lineStart, lineEnd < 0 ? scrubbed.length : lineEnd);
      const kind = language === "cxx" ? "raw-pointer-or-unsafe" :
        lineText.includes("unsafe") ? "unsafe-token" : "unsafe-construct";
      sites.push({ file, line, kind, token: match[0] });
    }
  }
  sites.sort((left, right) => left.file < right.file ? -1 : left.file > right.file ? 1 : left.line - right.line);
  return {
    count: sites.length,
    sites,
    method: "unsafe-structure-v2:language=" + language + ";sites=" +
      sites.map(site => site.kind + "@" + site.file + ":" + site.line + ":" + site.token).join(","),
  };
}
function deployClosure(task, language, closure, input, expected, destination) {
  const started = nowNs();
  fs.rmSync(destination, { recursive: true, force: true });
  copyClosure(closure, destination);
  const deployedEntries = closure.map(entry => ({ name: entry.name, file: path.join(destination, entry.name) }));
  if (hashClosure(deployedEntries) !== hashClosure(closure)) fail("deployed artifact closure changed: " + task.task_id + "/" + language);
  const deployedInput = path.join(destination, "input-root");
  stagePath(input, deployedInput);
  const artifact = path.join(destination, closure.entryName);
  const result = language === "domain" ?
    peerMeasured(task, path.join(destination, "runtime", "node"), [artifact, deployedInput], destination) :
    peerMeasured(task, artifact, [deployedInput], destination);
  checked(result, expected, task.task_id + "/" + language + "/deployment");
  return { elapsed: Number(nowNs() - started), result };
}

for (const task of manifest) {
  const taskId = task.task_id;
  const adapter = adapterByTask.get(taskId);
  const peer = selectedPeers.get(taskId);
  const taskWork = path.join(work, taskId);
  fs.mkdirSync(taskWork, { recursive: true });
  const input = path.join(corpus, task.input);
  const expected = path.join(corpus, task.expected);
  const hostileExpected = path.join(corpus, "expected", taskId + ".hostile.out");
  const jetTool = jetToolVersion;
  const peerCommand = toolchainContract.get(peer.language).command;
  const peerExecutableCommand = peer.language === "rust" ? (process.env.RUSTC || peerCommand) :
    peer.language === "domain" ? process.execPath : peerCommand;
  const peerArgs = peer.language === "rust" ? ["-Vv"] :
    peer.language === "go" ? ["version"] :
      peer.language === "cxx" ? ["--version"] :
        peer.language === "zig" ? ["version"] : ["--version"];
  const peerToolInfo = toolchainIdentity(peer.language, peerExecutableCommand, peerArgs);
  const peerTool = peerToolInfo.version;
  toolchainInfoByLanguage.set(peer.language, peerToolInfo);
  toolchainByKey.set(taskId + "\tjet", jetTool);
  toolchainByKey.set(taskId + "\t" + peer.language, peerTool);
  const inputHash = hashInputPath(input);
  const expectedHash = hashFile(expected);
  const sampleOrder = [];
  let jetAot = null;
  let peerAot = null;
  let jetAotClosure = null;
  let peerAotClosure = null;
  let jetNormal = null;
  let peerNormal = null;
  let jetHostile = null;
  let peerHostile = null;
  const states = [
    {
      name: "jet",
      language: "jet",
      source: adapter.jet_source,
      hostile: adapter.jet_hostile,
      jet: true,
      sourcePath: path.join(corpus, adapter.jet_source),
      hostilePath: path.join(corpus, adapter.jet_hostile),
      sourceHash: hashInputPath(path.join(corpus, adapter.jet_source)),
      sourceEffort: sourceEffort(path.join(corpus, adapter.jet_source)),
      unsafe: unsafeInventory(path.join(corpus, adapter.jet_source), "jet"),
      buildValues: [],
      editValues: [],
      runtimeValues: [],
      memoryValues: [],
      diagnosticValues: [],
      debuggingValues: [],
      diagnosticMethods: [],
      deploymentValues: [],
      artifactSizes: [],
      normalResult: null,
      hostileResult: null,
      artifact: null,
      closure: null,
      firstOutputHash: "",
      normalCommand: "",
      lastBuildCommand: "",
    },
    {
      name: "peer",
      language: peer.language,
      source: adapter.peer_source,
      hostile: adapter.peer_hostile,
      jet: false,
      sourcePath: path.join(corpus, adapter.peer_source),
      hostilePath: path.join(corpus, adapter.peer_hostile),
      sourceHash: hashInputPath(path.join(corpus, adapter.peer_source)),
      sourceEffort: sourceEffort(path.join(corpus, adapter.peer_source)),
      unsafe: unsafeInventory(path.join(corpus, adapter.peer_source), peer.language),
      buildValues: [],
      editValues: [],
      runtimeValues: [],
      memoryValues: [],
      diagnosticValues: [],
      debuggingValues: [],
      diagnosticMethods: [],
      deploymentValues: [],
      artifactSizes: [],
      normalResult: null,
      hostileResult: null,
      artifact: null,
      closure: null,
      firstOutputHash: "",
      normalCommand: "",
      lastBuildCommand: "",
    },
  ];
  for (let sample = 1; sample <= sampleCount; sample += 1) {
    for (const state of states) {
      sampleOrder.push(state.name + ":" + sample);
      const normalInput = path.join(taskWork, "input-" + state.name + "-" + sample);
      const hostileInput = path.join(taskWork, "hostile-" + state.name + "-" + sample);
      stagePath(input, normalInput);
      stagePath(state.hostilePath, hostileInput);
      const embeddedInput = task.authority.includes("build-input=") ? normalInput : "";
      let build;
      if (state.jet) {
        build = jetBuild(path.join(taskWork, "jet-build-" + sample), state.source, "", embeddedInput, task);
      } else {
        build = peerBuild(state.language, state.source, path.join(taskWork, "peer-build-" + sample), "", task);
      }
      state.artifact = state.jet ? build.binary : build.artifact;
      state.closure = state.jet ? artifactClosure("jet", state.artifact) : build.closure;
      state.artifactSizes.push(closureSize(state.closure));
      state.buildValues.push(build.result.duration);
      state.lastBuildCommand = build.result.command;
      const editProject = path.join(taskWork, "edit-" + state.name + "-" + sample);
      const editInput = path.join(editProject, "input-" + sample);
      stagePath(input, editInput);
      const warm = state.jet ?
        jetBuild(editProject, state.source, "", task.authority.includes("build-input=") ? editInput : "", task) :
        peerBuild(state.language, state.source, editProject, "", task);
      fs.appendFileSync(warm.source.entry, "\n// measured source edit\n");
      const edit = state.jet ?
        peerMeasured(task, jetBin, warm.args, editProject, warm.env) :
        peerMeasured(task, warm.command, warm.args, editProject);
      if (edit.status !== 0) fail((state.jet ? "Jet" : "peer") + " source-edit rebuild failed: " + taskId + ": " + cleanText(edit.stderr));
      state.editValues.push(edit.duration);
      const result = state.jet ?
        peerMeasured(task, state.artifact, [normalInput], taskWork) :
        peerRun(state.language, state.artifact, normalInput, taskWork, task);
      checked(result, expected, taskId + "/" + state.name + "/native");
      if (hashInputPath(input) !== inputHash) fail("normal input fixture changed during run: " + taskId);
      state.runtimeValues.push(result.duration);
      state.memoryValues.push(result.rss);
      if (sample === 1) {
        state.firstOutputHash = hashBuffer(result.stdout);
        state.normalCommand = result.command;
        state.normalResult = result;
      }
      let hostileResult;
      if (state.jet && taskId === "cross-platform-notes") {
        const hostileBuild = jetBuild(path.join(taskWork, "jet-hostile-" + sample), state.source, "", hostileInput, task);
        hostileResult = peerMeasured(task, hostileBuild.binary, [], taskWork);
      } else {
        hostileResult = state.jet ?
          peerMeasured(task, state.artifact, [hostileInput], taskWork) :
          peerRun(state.language, state.artifact, hostileInput, taskWork, task);
      }
      checked(hostileResult, hostileExpected, taskId + "/" + state.name + "/hostile");
      state.hostileResult = hostileResult;
      const diagnostic = diagnosticProbe(
        state.language,
        state.sourcePath,
        path.join(taskWork, "diagnostic-" + state.name + "-" + sample),
        task,
        state.jet,
      );
      state.diagnosticValues.push(diagnostic.burden);
      state.debuggingValues.push(diagnostic.result.duration);
      state.diagnosticMethods.push(
        diagnostic.method + ";debugging-steps=" + diagnostic.steps.length +
          ";debugging-nanoseconds=" + diagnostic.result.duration,
      );
      const deployment = deployClosure(
        task,
        state.jet ? "jet" : state.language,
        state.closure,
        input,
        expected,
        path.join(taskWork, "deploy-" + state.name + "-" + sample),
      );
      state.deploymentValues.push(deployment.elapsed);
    }
  }
  const orderMethod = "paired-interleave-v1:order=" + sampleOrder.join(",");
  for (const state of states) {
    const language = state.jet ? "jet" : state.language;
    addSamples(taskId, language, "source_effort", Array(sampleCount).fill(state.sourceEffort), "count", orderMethod + ";source-closure");
    addSamples(taskId, language, "build_time", state.buildValues, "nanoseconds", orderMethod + ";cold-build");
    addSamples(taskId, language, "edit_time", state.editValues, "nanoseconds", orderMethod + ";warmup-then-source-edit-rebuild");
    addSamples(taskId, language, "runtime", state.runtimeValues, "nanoseconds", orderMethod + ";native-run");
    addSamples(taskId, language, "memory", state.memoryValues, "bytes", orderMethod + ";native-maximum-rss");
    addSamples(taskId, language, "artifact_size", state.artifactSizes, "bytes", orderMethod + ";artifact-closure");
    addSamples(taskId, language, "diagnostics", state.diagnosticValues, "count-and-review",
      orderMethod + ";diagnostic-rubric-output-digests=" + state.diagnosticMethods.join("|"));
    addSamples(taskId, language, "debugging", state.debuggingValues, "nanoseconds-and-steps",
      orderMethod + ";debugging-replay=" + state.diagnosticMethods.join("|"));
    addSamples(taskId, language, "deployment", state.deploymentValues, "count-and-time",
      orderMethod + ";copy-count=1;closure-rehash=1;rerun-count=1");
    addSamples(taskId, language, "unsafe_burden", Array(sampleCount).fill(state.unsafe.count), "count-and-review",
      orderMethod + ";" + state.unsafe.method);
    if (state.jet) {
      jetAot = state.artifact;
      jetAotClosure = state.closure;
      jetNormal = state.normalResult;
      jetHostile = state.hostileResult;
    } else {
      peerAot = state.artifact;
      peerAotClosure = state.closure;
      peerNormal = state.normalResult;
      peerHostile = state.hostileResult;
    }
    const hostileHash = hashBuffer(state.hostileResult.stdout);
    receipts.push([
      "1",
      taskId,
      language,
      state.sourceHash,
      inputHash,
      expectedHash,
      state.firstOutputHash,
      hashInputPath(state.hostilePath),
      hostileHash,
      taskEnvironment.get(taskId),
      machine,
      state.jet ? jetTool : peerTool,
      "build=" + state.lastBuildCommand + ";run=" + state.normalCommand +
        ";source-hash-method=" + hashInputMethod(state.sourcePath) +
        ";input-hash-method=" + hashInputMethod(input) +
        ";hostile-input-hash-method=" + hashInputMethod(state.hostilePath) +
        ";launcher=" + peerLauncher.path + ";launcher-version=" + peerLauncher.version +
        ";launcher-sha256=" + peerLauncher.sha256 + ";authority=" + task.authority,
      peer.source_revision,
      "0",
      String(state.hostileResult.status),
      peerLauncher.path,
      peerLauncher.version,
      peerLauncher.sha256,
      task.authority,
    ]);
  }
  outcomes.push(["1", taskId, peer.language, peer.program, task.input, task.expected, task.declared_outcome, "platform=" + platform + ";candidate=" + candidateCommit + ";authority=" + task.authority + ";peer-launcher=" + peerLauncher.path + ";peer-launcher-sha256=" + peerLauncher.sha256, jetToolVersion, peerTool, peer.dependency_rule, peer.source_boundary, "pass", "pass", "-", "pending", "-"]);

  let jetJitArtifactHash = "-";
  let jetJitOutputHash = "-";
  let jetJitCommand = "not-run:artifact=none:persistent=false";
  if (platform === "linux") {
    const jitProject = path.join(taskWork, "jet-build-" + sampleCount);
    const jitSource = sourceEntry(jitProject, "jet");
    const jitInput = path.join(taskWork, "jit-input");
    stagePath(input, jitInput);
    const jit = peerMeasured(
      task,
      jetBin,
      ["run", path.relative(jitProject, jitSource).split(path.sep).join("/"), "--", jitInput],
      jitProject,
      {
        JET_CACHE_DIR: path.join(jitProject, ".jet-build-cache"),
        JET_RUN_CACHE_DIR: path.join(jitProject, ".jet-run-cache"),
        JET_RUNTIME_CACHE_DIR: path.join(jitProject, ".jet-runtime-cache"),
      },
    );
    checked(jit, expected, taskId + "/jet/jit");
    jetJitOutputHash = hashBuffer(jit.stdout);
    jetJitCommand = jit.command + ";artifact=none:persistent=false";
  }
  let jetCrossHash = "-";
  let peerCrossHash = "-";
  let jetCrossProof = "-";
  let peerCrossProof = "-";
  let jetWebHash = "-";
  let jetWebOutputHash = "-";
  let jetWebProof = "-";
  let peerWebHash = "-";
  let peerWebOutputHash = "-";
  let peerWebProof = "-";
  let jetWebCommand = "not-run";
  let peerWebCommand = "not-run";
  if (platform === "linux") {
    if (taskId === "cross-platform-notes") {
      const webProject = path.join(taskWork, "jet-web");
      const web = jetWebBuild(webProject, adapter.jet_source, input, task);
      const browser = webBrowserRun(path.join(webProject, "build"), expected, task);
      const jetWebClosure = [
        { name: "build/app.js", file: web.js },
        { name: "build/app.wasm", file: web.wasm },
        { name: "build/jet_dom_runtime.js", file: web.runtime },
        { name: "build/index.html", file: web.index },
        { name: "runtime/chromium", file: browser.chromium },
      ];
      jetWebHash = hashClosure(jetWebClosure);
      jetWebProof = targetArtifactProof(web.wasm, "web");
      jetWebOutputHash = hashBuffer(browser.stdout);
      jetWebCommand = browser.command + ";authority=" + task.authority;
      const peerWebProject = path.join(taskWork, "peer-web");
      if (peerSupportsTarget(peer, "web")) {
        const peerWeb = peerWebBuild(adapter.peer_source, input, peerWebProject, task);
        const peerBrowser = webBrowserRun(peerWebProject, expected, task, true);
        const peerWebClosure = artifactClosure("domain", peerWeb.module, peerWeb.source.files, peerWeb.source.root);
        peerWebClosure.push({ name: "build/index.html", file: peerWeb.index });
        peerWebClosure.push({ name: "runtime/chromium", file: peerBrowser.chromium });
        peerWebProof = "format=browser-js;target=web;entry=" + path.basename(peerWeb.module) + ";index=" + path.basename(peerWeb.index);
        peerWebHash = hashClosure(peerWebClosure);
        peerWebOutputHash = hashBuffer(peerBrowser.stdout);
        peerWebCommand = peerBrowser.command + ";authority=" + task.authority;
      }
    } else {
      const jetCross = jetBuild(
        path.join(taskWork, "jet-cross"),
        adapter.jet_source,
        "aarch64-unknown-linux-gnu",
        "",
        task,
      );
      jetCrossHash = artifactHash("jet", jetCross.binary);
      jetCrossProof = targetArtifactProof(jetCross.binary, "aarch64-unknown-linux-gnu");
      if (peerSupportsTarget(peer, "aarch64-unknown-linux-gnu")) {
        const peerCross = peerBuild(
          peer.language,
          adapter.peer_source,
          path.join(taskWork, "peer-cross"),
          "aarch64-unknown-linux-gnu",
          task,
        );
        peerCrossHash = artifactHash(peer.language, peerCross.artifact, peerCross.source.files, peerCross.source.root);
        peerCrossProof = targetArtifactProof(peerCross.artifact, "aarch64-unknown-linux-gnu");
      }
    }
  }


  for (const tier of tierMatrix.filter(row => row.task_id === taskId)) {
    const excluded = tier.requirement === "excluded";
    const inScope = tier.platform === platform || (platform === "linux" && tier.platform === "cross-target");
    const peerTargetApplies = tier.platform !== "cross-target" || peerSupportsTarget(peer, tier.target);
    const evidencePrefix = excluded
      ? "excluded;availability=" + tier.availability + ";reason=" + tier.availability_reason + ";rationale=" + tier.rationale
      : !inScope
        ? "platform-scope=" + platform + ";availability=" + tier.availability + ";reason=" + tier.availability_reason
        : "";
    const authorityEvidence = "authority=" + task.authority + ";launcher=" + peerLauncher.path +
      ";launcher-version=" + peerLauncher.version + ";launcher-sha256=" + peerLauncher.sha256;
    const languages = ["jet", peer.language];
    for (const language of languages) {
      let status = "not-applicable";
      let artifact = "-";
      let output = "-";
      let command = "not-run:" + evidencePrefix + ";" + authorityEvidence;
      if (!excluded && inScope && tier.tier === "jit" && language === "jet") {
        status = "pass";
        artifact = jetJitArtifactHash;
        output = jetJitOutputHash;
        command = jetJitCommand + ";" + authorityEvidence;
      } else if (!excluded && inScope && tier.tier === "jit" && language !== "jet") {
        command = "not-run:reason=peer-native-only;" + authorityEvidence;
      } else if (!excluded && inScope && language !== "jet" && !peerTargetApplies) {
        command = "not-run:reason=peer-target-not-declared;target=" + tier.target +
          ";peer-targets=" + peer.applicable_targets + ";" + authorityEvidence;
      } else if (!excluded && inScope && tier.platform === "cross-target" && tier.target === "web") {
        status = "pass";
        artifact = language === "jet" ? jetWebHash : peerWebHash;
        output = language === "jet" ? jetWebOutputHash : peerWebOutputHash;
        command = (language === "jet" ? jetWebCommand : peerWebCommand) + ";target-proof=" +
          (language === "jet" ? jetWebProof : peerWebProof) + ";" + authorityEvidence;
      } else if (!excluded && inScope && tier.platform === "cross-target") {
        status = "pass";
        artifact = language === "jet" ? jetCrossHash : peerCrossHash;
        command = (language === "jet" ? "jet build --target=" : "peer build --target=") + tier.target +
          ";target-proof=" + (language === "jet" ? jetCrossProof : peerCrossProof) + ";" + authorityEvidence;
      } else if (!excluded && inScope && tier.tier === "aot") {
        status = "pass";
        const closure = language === "jet" ? jetAotClosure : peerAotClosure;
        artifact = hashClosure(closure);
        output = hashBuffer((language === "jet" ? jetNormal : peerNormal).stdout);
        command = (language === "jet" ? "jet build;run" : "peer build;run") + ";" + authorityEvidence;
      }
      const evidence = status === "pass" ? "artifact=" + artifact + ";output=" + output + ";command=" + command : command;
      tierReports.push(["1", taskId, language, tier.platform, tier.target, tier.tier, status, evidence, "-"]);
      tierReceipts.push(["1", taskId, language, tier.platform, tier.target, tier.tier, artifact, output, command, status]);
    }
  }
}

function peerSupportsTarget(peer, target) {
  const alias = target.split("-")[0];
  return peer.applicable_targets.split(",").map(value => value.trim()).some(value => value === target || value === alias);
}

function peerApplies(peer) {
  return peer.applicable_targets.split(",").map(value => value.trim()).includes(platform);
}
function coverageKeyPath(peer) {
  return peerKey(peer).replace(/[^A-Za-z0-9._-]+/g, "_");
}
function coverageStatistics(values) {
  const sorted = [...values].sort((left, right) => left - right);
  const med = sorted.length % 2 ? sorted[(sorted.length - 1) / 2] :
    (sorted[sorted.length / 2 - 1] + sorted[sorted.length / 2]) / 2;
  const mean = values.reduce((left, right) => left + right, 0) / values.length;
  const stdev = Math.sqrt(values.reduce((sum, value) => sum + (value - mean) ** 2, 0) / values.length);
  return {
    median: med,
    min: sorted[0],
    max: sorted.at(-1),
    relativeStdev: mean === 0 ? 0 : stdev / mean,
  };
}
function addPeerCoverageNotApplicable(peer, task, adapter) {
  const key = peerKey(peer);
  const sourceHash = hashInputPath(path.join(corpus, adapter.peer_source));
  const hostileHash = hashInputPath(path.join(corpus, adapter.peer_hostile));
  peerCoverage.push([
    "1", key, task.task_id, peer.selection, peer.language, peer.program, adapter.peer_source,
    peer.source_revision, sourceHash, hashInputPath(path.join(corpus, task.input)),
    hashFile(path.join(corpus, task.expected)), "-", hostileHash, "-", "not-applicable", platform, "-",
    "native", "not-run:host-platform", "peer-targets=" + peer.applicable_targets + ";reason=host-platform",
  ]);
}
function runPeerCoverage(peer, task, adapter) {
  if (!peerApplies(peer)) {
    addPeerCoverageNotApplicable(peer, task, adapter);
    return;
  }
  const adapterPath = path.join(corpus, adapter.peer_source);
  const hostilePath = path.join(corpus, adapter.peer_hostile);
  const input = path.join(corpus, task.input);
  const expected = path.join(corpus, task.expected);
  const hostileExpected = path.join(corpus, "expected", task.task_id + ".hostile.out");
  const key = peerKey(peer);
  const pathKey = coverageKeyPath(peer);
  const inputHash = hashInputPath(input);
  const expectedHash = hashFile(expected);
  const sourceHash = hashInputPath(adapterPath);
  const hostileInputHash = hashInputPath(hostilePath);
  const peerCommand = toolchainContract.get(peer.language)?.command;
  if (!peerCommand) fail("peer coverage toolchain missing: " + peer.language);
  const peerExecutableCommand = peer.language === "domain" ? process.execPath : peerCommand;
  const peerArgs = peer.language === "rust" ? ["-Vv"] :
    peer.language === "go" ? ["version"] :
      peer.language === "cxx" ? ["--version"] :
        peer.language === "zig" ? ["version"] : ["--version"];
  const peerToolInfo = toolchainIdentity(peer.language, peerExecutableCommand, peerArgs);
  toolchainInfoByLanguage.set(peer.language, peerToolInfo);
  const sourceLines = sourceEffort(adapterPath);
  const unsafe = unsafeInventory(adapterPath, peer.language);
  const buildValues = [];
  const editValues = [];
  const runtimeValues = [];
  const memoryValues = [];
  const diagnosticValues = [];
  const debuggingValues = [];
  const deploymentValues = [];
  const artifactSizes = [];
  let firstOutputHash = "";
  let hostileOutputHash = "";
  let firstBuildCommand = "";
  let firstRunCommand = "";
  let firstHostileCommand = "";
  let firstDiagnosticMethod = "";
  for (let sample = 1; sample <= sampleCount; sample += 1) {
    const runRoot = path.join(work, "coverage-" + pathKey + "-" + sample);
    const normalInput = path.join(runRoot, "input");
    const hostileInput = path.join(runRoot, "hostile");
    stagePath(input, normalInput);
    stagePath(hostilePath, hostileInput);
    const build = peerBuild(peer.language, adapter.peer_source, path.join(runRoot, "build"), "", task, peer);
    const closure = build.closure;
    artifactSizes.push(closureSize(closure));
    buildValues.push(build.result.duration);
    const editProject = path.join(runRoot, "edit");
    const warm = peerBuild(peer.language, adapter.peer_source, editProject, "", task, peer);
    fs.appendFileSync(warm.source.entry, "\n// measured source edit\n");
    const edit = peerMeasured(task, warm.command, warm.args, editProject);
    if (edit.status !== 0) fail("peer coverage source-edit rebuild failed: " + key + ": " + cleanText(edit.stderr));
    editValues.push(edit.duration);
    const result = peerRun(peer.language, build.artifact, normalInput, runRoot, task, peer);
    checked(result, expected, task.task_id + "/" + key + "/native");
    if (hashInputPath(input) !== inputHash) fail("normal input fixture changed during peer coverage: " + key);
    runtimeValues.push(result.duration);
    memoryValues.push(result.rss);
    const hostile = peerRun(peer.language, build.artifact, hostileInput, runRoot, task, peer);
    checked(hostile, hostileExpected, task.task_id + "/" + key + "/hostile");
    const diagnostic = diagnosticProbe(
      peer.language,
      adapterPath,
      path.join(runRoot, "diagnostic"),
      task,
      false,
      peer,
    );
    diagnosticValues.push(diagnostic.burden);
    debuggingValues.push(diagnostic.result.duration);
    deploymentValues.push(deployClosure(
      task,
      peer.language,
      closure,
      input,
      expected,
      path.join(runRoot, "deploy"),
    ).elapsed);
    if (sample === 1) {
      firstOutputHash = hashBuffer(result.stdout);
      hostileOutputHash = hashBuffer(hostile.stdout);
      firstBuildCommand = build.result.command;
      firstRunCommand = result.command;
      firstHostileCommand = hostile.command;
      firstDiagnosticMethod = diagnostic.method;
    }
  }
  const coverageCommand = "build=" + firstBuildCommand + ";run=" + firstRunCommand +
    ";hostile=" + firstHostileCommand + ";source-hash-method=" + hashInputMethod(adapterPath) +
    ";input-hash-method=" + hashInputMethod(input) + ";hostile-input-hash-method=" + hashInputMethod(hostilePath);
  const coverageEvidence = "toolchain=" + peerToolInfo.version + ";source-sha256=" + sourceHash +
    ";input-sha256=" + inputHash + ";expected-sha256=" + expectedHash +
    ";output-sha256=" + firstOutputHash + ";hostile-input-sha256=" + hostileInputHash +
    ";hostile-output-sha256=" + hostileOutputHash + ";diagnostic=" + firstDiagnosticMethod;
  peerCoverage.push([
    "1", key, task.task_id, peer.selection, peer.language, peer.program, adapter.peer_source,
    peer.source_revision, sourceHash, inputHash, expectedHash, firstOutputHash, hostileInputHash,
    hostileOutputHash, "measured", platform, "-", "native", coverageCommand, coverageEvidence,
  ]);
  const metricValues = new Map([
    ["source_effort", Array(sampleCount).fill(sourceLines)],
    ["build_time", buildValues],
    ["edit_time", editValues],
    ["runtime", runtimeValues],
    ["memory", memoryValues],
    ["artifact_size", artifactSizes],
    ["diagnostics", diagnosticValues],
    ["debugging", debuggingValues],
    ["deployment", deploymentValues],
    ["unsafe_burden", Array(sampleCount).fill(unsafe.count)],
  ]);
  for (const metric of metricContract) {
    const values = metricValues.get(metric.metric);
    if (!values || values.length !== sampleCount) fail("peer coverage metric missing: " + key + "/" + metric.metric);
    const stats = coverageStatistics(values);
    peerMeasurements.push([
      "1", key, task.task_id, peer.selection, peer.language, metric.metric, String(sampleCount),
      String(stats.median), String(stats.min), String(stats.max), String(stats.relativeStdev), metric.unit,
      "measured", coverageCommand + ";evidence=" + coverageEvidence, platform, "-", "native",
    ]);
  }
}
for (const peer of peers) {
  const task = taskById.get(peer.task_id);
  const adapter = peerAdapterByKey.get(peerKey(peer));
  if (!task || !adapter) fail("peer coverage task or adapter missing: " + peerKey(peer));
  runPeerCoverage(peer, task, adapter);
}

const groups = new Map();
for (const row of samples) {
  const key = row[1] + "\t" + row[2] + "\t" + row[3];
  if (!groups.has(key)) groups.set(key, { task: row[1], language: row[2], metric: row[3], unit: row[6], platform: row[8], target: row[9], tier: row[10], values: [] });
  groups.get(key).values.push(Number(row[5]));
}
function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}
const statistics = [];
for (const group of groups.values()) {
  const policy = policies.get(group.metric);
  if (!policy) fail("measurement policy missing: " + group.metric);
  const values = group.values;
  const sampleLimit = Number(policy.samples);
  const minSamples = Number(policy.min_samples);
  const threshold = Number(policy.min_value);
  if (values.length !== sampleLimit || values.length < minSamples || values.some(value => !Number.isFinite(value) || value < threshold)) fail("sample threshold failed: " + group.task + "/" + group.language + "/" + group.metric);
  const med = median(values);
  const mean = values.reduce((a, b) => a + b, 0) / values.length;
  const stdev = Math.sqrt(values.reduce((a, b) => a + (b - mean) ** 2, 0) / values.length);
  const relative = mean === 0 ? 0 : stdev / Math.abs(mean);
  const mad = median(values.map(value => Math.abs(value - med)));
  const outlierLimit = mad === 0 ? 0 : mad * Number(policy.outlier_mad_multiplier);
  const outliers = values.filter(value => mad === 0 ? value !== med : Math.abs(value - med) > outlierLimit).length;
  if (relative > Number(policy.max_relative_stdev) || outliers > Number(policy.max_outliers)) fail("variance/outlier policy failed: " + group.task + "/" + group.language + "/" + group.metric);
  const comparison = policyComparison(group.task, group.metric);
  statistics.push({
    ...group,
    samples: values.length,
    median: med,
    min: Math.min(...values),
    max: Math.max(...values),
    relative,
    outliers,
    threshold,
    tolerance: comparison.tolerance,
    comparisonPeerLanguage: comparison.peerLanguage,
    comparisonOperator: comparison.operator,
    status: "measured",
    owner: "-",
  });
}
writeRows(path.join(reportDir, "peer_coverage.tsv"), [
  "version", "peer_key", "task_id", "selection", "language", "program", "source", "source_revision",
  "source_sha256", "input_sha256", "expected_sha256", "output_sha256", "hostile_input_sha256",
  "hostile_output_sha256", "status", "platform", "target", "tier", "command", "evidence",
], peerCoverage);
writeRows(path.join(reportDir, "peer_measurements.tsv"), [
  "version", "peer_key", "task_id", "selection", "language", "metric", "samples", "median", "min",
  "max", "relative_stdev", "unit", "status", "evidence", "platform", "target", "tier",
], peerMeasurements);
const statsByKey = new Map(statistics.map(stat => [stat.task + "\t" + stat.language + "\t" + stat.metric, stat]));
for (const stat of statistics) {
  if (stat.language !== "jet") continue;
  const peer = statsByKey.get(stat.task + "\t" + selectedPeers.get(stat.task).language + "\t" + stat.metric);
  if (!peer) fail("peer statistic missing: " + stat.task + "/" + stat.metric);
  const comparison = policyComparison(stat.task, stat.metric);
  const loss = peer.median === 0 ? stat.median > 0 :
    comparison.peerLanguage === "rust" ?
      stat.median > peer.median * comparison.tolerance :
      stat.median >= peer.median * comparison.tolerance;
  if (loss) {
    stat.status = "loss";
    stat.owner = manifest.find(task => task.task_id === stat.task).loss_cards;
  }
}
const tasksWithLoss = new Set(
  statistics.filter(stat => stat.language === "jet" && stat.status === "loss").map(stat => stat.task),
);
for (const outcome of outcomes) {
  if (tasksWithLoss.has(outcome[1])) {
    outcome[12] = "loss";
    outcome[14] = manifest.find(task => task.task_id === outcome[1]).loss_cards;
  }
}
const statisticsRows = statistics.map(stat => [
  "1", stat.task, stat.language, stat.metric, String(stat.samples), String(stat.median),
  String(stat.min), String(stat.max), String(stat.relative), String(stat.outliers),
  String(stat.threshold), String(stat.tolerance), stat.status, stat.owner,
  "samples=" + stat.samples + ";median=" + stat.median + ";relative-stdev=" + stat.relative +
    ";outliers=" + stat.outliers + ";comparison-peer=" + stat.comparisonPeerLanguage +
    ";comparison-operator=" + stat.comparisonOperator + ";tolerance-ratio=" + stat.tolerance,
  stat.platform, stat.target, stat.tier,
]);
const measurementRows = statistics.map(stat => [
  "1", stat.task, stat.language, stat.metric, String(stat.median), stat.unit,
  toolchainByKey.get(stat.task + "\t" + stat.language) || fail("toolchain identity missing: " + stat.task + "/" + stat.language),
  "samples=" + stat.samples + ";median=" + stat.median + ";relative-stdev=" + stat.relative + ";outliers=" + stat.outliers +
    ";comparison-peer=" + stat.comparisonPeerLanguage + ";comparison-operator=" + stat.comparisonOperator + ";tolerance-ratio=" + stat.tolerance,
  stat.status, stat.owner, stat.platform, stat.target, stat.tier,
]);
for (const [language, info] of toolchainInfoByLanguage) {
  identity.push(["1", "toolchain_" + language + "_path", info.executable]);
  identity.push(["1", "toolchain_" + language + "_sha256", info.sha256]);
  identity.push(["1", "toolchain_" + language + "_version", info.version]);
}


if (hashFile(jetBin) !== jetBinarySha256) fail("Jet compiler changed during run");
if (peerLauncherInfo().sha256 !== peerLauncher.sha256) fail("peer isolation launcher changed during run");
function writeRows(file, header, rows) {
  fs.writeFileSync(file, [header, ...rows].map(row => row.join("\t")).join("\n") + "\n");
}
writeRows(path.join(reportDir, "identity.tsv"), ["version", "key", "value"], identity);
writeRows(path.join(reportDir, "samples.tsv"), ["version", "task_id", "language", "metric", "sample", "value", "unit", "method", "platform", "target", "tier"], samples);
writeRows(path.join(reportDir, "outcomes.tsv"), ["version", "task_id", "peer_language", "peer_program", "input", "expected", "outcome", "toolchain_id", "jet_tool_version", "peer_tool_version", "dependency_rule", "source_boundary", "jet_status", "peer_status", "loss_owner", "review_status", "review_evidence"], outcomes);
writeRows(path.join(reportDir, "measurements.tsv"), ["version", "task_id", "language", "metric", "value", "unit", "toolchain_id", "evidence", "status", "loss_owner", "platform", "target", "tier"], measurementRows);
writeRows(path.join(reportDir, "statistics.tsv"), ["version", "task_id", "language", "metric", "samples", "median", "min", "max", "relative_stdev", "outliers", "threshold", "tolerance_ratio", "status", "loss_owner", "evidence", "platform", "target", "tier"], statisticsRows);
writeRows(path.join(reportDir, "tiers.tsv"), ["version", "task_id", "language", "platform", "target", "tier", "status", "evidence", "loss_owner"], tierReports);
writeRows(path.join(reportDir, "receipts.tsv"), ["version", "task_id", "language", "source_sha256", "input_sha256", "expected_sha256", "output_sha256", "hostile_input_sha256", "hostile_output_sha256", "environment", "machine", "tool_version", "command", "peer_commit", "exit_code", "hostile_exit_code", "peer_launcher_path", "peer_launcher_version", "peer_launcher_sha256", "authority"], receipts);
writeRows(path.join(reportDir, "tier_receipts.tsv"), ["version", "task_id", "language", "platform", "target", "tier", "artifact_sha256", "output_sha256", "command", "status"], tierReceipts);
console.log("compiled workload report: produced platform=" + platform + " report=" + reportDir);
