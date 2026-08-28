#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import { access, lstat, mkdir, mkdtemp, readdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import { createServer } from "node:net";
import { homedir } from "node:os";
import * as os from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { CdpDriver } from "../canvas-test/driver.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const DEFAULT_SAMPLES = 10;
const COMMAND_TIMEOUT_MS = 180_000;
const SERVER_TIMEOUT_MS = 45_000;
const PAGE_TIMEOUT_MS = 20_000;
const EDIT_TIMEOUT_MS = 15_000;
const OUTPUT_LIMIT = 100_000;

const definitions = [
  {
    id: "jet",
    label: "Jet",
    steps: ["jet new app --target=web", "cd app", "jet dev"],
    scaffold: (jet) => [jet, ["new", "app", "--target=web"]],
    dev: (jet, port) => [jet, ["dev", `--port=${port}`]],
    initialText: "Clicks: 0",
    clickedText: "Clicks: 1",
    source: (app) => join(app, "run.jet"),
    editNeedle: "Clicks: {n}",
    editText: (sample) => `Reload ${sample}: {n}`,
    visibleText: (sample) => `Reload ${sample}: 0`,
  },
  {
    id: "bun-vite",
    label: "Bun + Vite",
    steps: [
      "bun create vite app --template vanilla --no-interactive",
      "cd app",
      "bun install",
      "bun run dev",
    ],
    scaffold: (bun) => [
      bun,
      ["create", "vite", "app", "--template", "vanilla", "--no-interactive"],
    ],
    install: (bun) => [bun, ["install"]],
    dev: (bun, port) => [
      bun,
      ["run", "dev", "--", "--host", "127.0.0.1", "--port", String(port), "--strictPort"],
    ],
    initialText: "Count is 0",
    clickedText: "Count is 1",
    source: (app) => join(app, "src", "counter.js"),
    editNeedle: "Count is",
    editText: (sample) => `Reload ${sample} is`,
    visibleText: (sample) => `Reload ${sample} is 0`,
  },
  {
    id: "npm-vite",
    label: "npm + Vite",
    steps: [
      "npm create vite@latest app -- --template vanilla --no-interactive",
      "cd app",
      "npm install",
      "npm run dev",
    ],
    scaffold: (npm) => [
      npm,
      ["create", "vite@latest", "app", "--", "--template", "vanilla", "--no-interactive"],
    ],
    install: (npm) => [npm, ["install"]],
    dev: (npm, port) => [
      npm,
      ["run", "dev", "--", "--host", "127.0.0.1", "--port", String(port), "--strictPort"],
    ],
    initialText: "Count is 0",
    clickedText: "Count is 1",
    source: (app) => join(app, "src", "counter.js"),
    editNeedle: "Count is",
    editText: (sample) => `Reload ${sample} is`,
    visibleText: (sample) => `Reload ${sample} is 0`,
  },
];

function argument(name, fallback) {
  const prefix = `${name}=`;
  const value = process.argv.find((item) => item.startsWith(prefix));
  return value ? value.slice(prefix.length) : fallback;
}

function samples() {
  const count = Number(argument("--samples", String(DEFAULT_SAMPLES)));
  if (!Number.isInteger(count) || count < 1) {
    throw new Error("--samples must be a positive integer");
  }
  return count;
}

function roundMs(value) {
  return Math.round(value * 10) / 10;
}

function elapsedMs(start) {
  return roundMs(Number(process.hrtime.bigint() - start) / 1_000_000);
}

function delay(ms) {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
}

function tail(value) {
  return value.length > 4_000 ? `…${value.slice(-4_000)}` : value;
}

function appendOutput(current, chunk) {
  const value = current + chunk;
  return value.length > OUTPUT_LIMIT ? value.slice(-OUTPUT_LIMIT) : value;
}

async function executable(value) {
  if (value.includes("/") || value.startsWith(".")) {
    try {
      await access(value, fsConstants.X_OK);
      return resolve(value);
    } catch {
      return null;
    }
  }
  const result = spawnSync("which", [value], { encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim().split("\n")[0] : null;
}

function trackedProcess(command, args, options = {}) {
  const child = spawn(command, args, {
    cwd: options.cwd,
    detached: process.platform !== "win32",
    env: { ...process.env, ...options.env },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  let error = null;
  child.stdout?.setEncoding("utf8");
  child.stderr?.setEncoding("utf8");
  child.stdout?.on("data", (chunk) => { stdout = appendOutput(stdout, chunk); });
  child.stderr?.on("data", (chunk) => { stderr = appendOutput(stderr, chunk); });
  child.once("error", (value) => { error = value; });
  const exited = new Promise((resolveExit) => {
    child.once("close", (code, signal) => resolveExit({ code, signal }));
  });
  return {
    child,
    exited,
    output: () => ({ stdout, stderr, error }),
  };
}

function alive(processInfo) {
  return processInfo.child.exitCode === null && processInfo.child.signalCode === null;
}

async function stopProcess(processInfo) {
  if (!processInfo || !alive(processInfo)) {
    return;
  }
  const pid = processInfo.child.pid;
  try {
    if (process.platform !== "win32" && pid) {
      process.kill(-pid, "SIGTERM");
    } else {
      processInfo.child.kill("SIGTERM");
    }
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
  let stopTimer;
  const stopDeadline = new Promise((resolveDeadline) => {
    stopTimer = setTimeout(() => resolveDeadline(false), 2_000);
  });
  const stopped = await Promise.race([processInfo.exited.then(() => true), stopDeadline]);
  clearTimeout(stopTimer);
  if (!stopped && alive(processInfo)) {
    try {
      if (process.platform !== "win32" && pid) {
        process.kill(-pid, "SIGKILL");
      } else {
        processInfo.child.kill("SIGKILL");
      }
    } catch (error) {
      if (error.code !== "ESRCH") throw error;
    }
    await processInfo.exited;
  }
}

async function runProcess(command, args, options = {}) {
  const processInfo = trackedProcess(command, args, options);
  const timeout = options.timeoutMs || COMMAND_TIMEOUT_MS;
  let timeoutTimer;
  const timeoutDeadline = new Promise((resolveDeadline) => {
    timeoutTimer = setTimeout(() => resolveDeadline({ timedOut: true }), timeout);
  });
  const result = await Promise.race([processInfo.exited, timeoutDeadline]);
  clearTimeout(timeoutTimer);
  if (result.timedOut) {
    await stopProcess(processInfo);
    throw new Error(`timed out: ${commandLine(command, args)}`);
  }
  const output = processInfo.output();
  if (output.error) {
    throw new Error(`${commandLine(command, args)}: ${output.error.message}`);
  }
  if (result.code !== 0) {
    throw new Error(
      `${commandLine(command, args)} exited ${result.code ?? result.signal}\n${tail(output.stderr || output.stdout)}`,
    );
  }
  return { ...result, ...output };
}

function commandLine(command, args) {
  return [command, ...args].map((value) => (/^[A-Za-z0-9_./:@=-]+$/.test(value) ? value : JSON.stringify(value))).join(" ");
}

async function freePort() {
  return await new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      server.close((error) => error ? reject(error) : resolvePort(port));
    });
  });
}

async function waitForServer(url, processInfo) {
  const deadline = Date.now() + SERVER_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (!alive(processInfo)) {
      const output = processInfo.output();
      throw new Error(`dev command stopped\n${tail(output.stderr || output.stdout)}`);
    }
    try {
      const response = await fetch(url, { signal: AbortSignal.timeout(500) });
      if (response.ok) {
        await response.arrayBuffer();
        return;
      }
    } catch {
      // The server is still starting.
    }
    await delay(25);
  }
  throw new Error(`timed out waiting for ${url}`);
}

async function waitForText(driver, expected, label, timeoutMs = PAGE_TIMEOUT_MS) {
  const deadline = Date.now() + timeoutMs;
  let lastError = null;
  let lastBody = "";
  while (Date.now() < deadline) {
    try {
      const body = await driver.evaluate("document.body?.innerText || \"\"");
      lastBody = body;
      if (body.includes(expected)) return body;
    } catch (error) {
      lastError = error;
    }
    await delay(25);
  }
  throw new Error(`timed out waiting for ${label}; body=${JSON.stringify(lastBody.slice(0, 240))}${lastError ? `; ${lastError.message}` : ""}`);
}

async function startDev(definition, tool, app, driver) {
  const port = await freePort();
  const [command, args] = definition.dev(tool, port);
  const started = process.hrtime.bigint();
  const processInfo = trackedProcess(command, args, { cwd: app });
  try {
    const url = `http://127.0.0.1:${port}/`;
    await waitForServer(url, processInfo);
    await driver.navigate(url);
    await waitForText(driver, definition.initialText, "initial page");
    await delay(100);
    return { processInfo, port, pageMs: elapsedMs(started) };
  } catch (error) {
    await stopProcess(processInfo);
    throw new Error(`${definition.label}: ${error.message}`);
  }
}

async function checkInteraction(definition, driver) {
  await driver.evaluate("document.querySelector(\"button\")?.click()");
  await waitForText(driver, definition.clickedText, "button interaction");
}

async function directoryBytes(path) {
  let total = 0;
  let entries;
  try {
    entries = await readdir(path, { withFileTypes: true });
  } catch {
    return 0;
  }
  for (const entry of entries) {
    const child = join(path, entry.name);
    if (entry.isDirectory()) {
      total += await directoryBytes(child);
    } else {
      try {
        total += (await (entry.isSymbolicLink() ? lstat(child) : stat(child))).size;
      } catch {
        // A concurrent package-manager cleanup can remove a cache entry.
      }
    }
  }
  return total;
}

async function fileBytes(path) {
  try {
    return (await stat(path)).size;
  } catch {
    return 0;
  }
}

async function installWeight(definition, app) {
  if (definition.id === "jet") return 0;
  const nodeModules = await directoryBytes(join(app, "node_modules"));
  const lock = definition.id === "bun-vite"
    ? await fileBytes(join(app, "bun.lock")) + await fileBytes(join(app, "bun.lockb"))
    : await fileBytes(join(app, "package-lock.json"));
  return nodeModules + lock;
}

async function viteVersion(app) {
  const packageJson = JSON.parse(await readFile(join(app, "node_modules", "vite", "package.json"), "utf8"));
  return packageJson.version;
}

async function measureReloads(definition, app, driver, count) {
  const sourcePath = definition.source(app);
  const original = await readFile(sourcePath, "utf8");
  let current = original;
  let needle = definition.editNeedle;
  const values = [];
  try {
    for (let sample = 1; sample <= count; sample += 1) {
      const replacement = definition.editText(sample);
      const next = current.replace(needle, replacement);
      if (next === current) {
        throw new Error(`${definition.label}: edit marker missing in ${sourcePath}`);
      }
      const started = process.hrtime.bigint();
      await writeFile(sourcePath, next);
      await waitForText(driver, definition.visibleText(sample), `reload ${sample}`, EDIT_TIMEOUT_MS);
      values.push(elapsedMs(started));
      current = next;
      needle = replacement;
    }
  } finally {
    await writeFile(sourcePath, original);
  }
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  const median = sorted.length % 2 === 0
    ? roundMs((sorted[middle - 1] + sorted[middle]) / 2)
    : sorted[middle];
  return { samples: values, medianMs: median };
}

async function ensureBun(npm, cacheHome) {
  const configured = process.env.BUN_BIN;
  if (configured) {
    const path = await executable(configured);
    if (!path) throw new Error(`BUN_BIN is not executable: ${configured}`);
    return { path, provisioned: false, cacheBytes: 0 };
  }
  const direct = await executable("bun");
  if (direct) return { path: direct, provisioned: false, cacheBytes: 0 };

  const cacheRoot = join(cacheHome, "jet-dx-benchmark", "bun");
  const path = join(cacheRoot, "node_modules", ".bin", "bun");
  if (!(await executable(path))) {
    await mkdir(cacheRoot, { recursive: true });
    await runProcess(
      npm,
      ["install", "--prefix", cacheRoot, "--no-save", "--no-audit", "--no-fund", "bun@latest"],
      { cwd: REPO_ROOT },
    );
  }
  const installed = await executable(path);
  if (!installed) throw new Error(`Bun bootstrap did not create ${path}`);
  return { path: installed, provisioned: true, cacheBytes: await directoryBytes(cacheRoot) };
}

async function measure(definition, tools, runRoot, sampleCount) {
  const root = await mkdtemp(join(runRoot, `${definition.id}-`));
  const app = join(root, "app");
  let driver = null;
  let active = null;
  try {
    const setupStarted = process.hrtime.bigint();
    const [scaffoldCommand, scaffoldArgs] = definition.scaffold(tools[definition.id === "jet" ? "jet" : definition.id === "bun-vite" ? "bun" : "npm"]);
    await runProcess(scaffoldCommand, scaffoldArgs, {
      cwd: root,
      env: { CI: "1", npm_config_yes: "true" },
    });
    if (definition.install) {
      const [installCommand, installArgs] = definition.install(tools[definition.id === "bun-vite" ? "bun" : "npm"]);
      await runProcess(installCommand, installArgs, {
        cwd: app,
        env: { CI: "1", npm_config_yes: "true" },
      });
    }
    const setupMs = elapsedMs(setupStarted);
    const installedBytes = await installWeight(definition, app);
    const chromium = tools.chromium;
    driver = await new CdpDriver({ chrome: chromium, chromeTempRoot: runRoot }).launch();
    const cold = await startDev(definition, tools[definition.id === "jet" ? "jet" : definition.id === "bun-vite" ? "bun" : "npm"], app, driver);
    active = cold;
    await checkInteraction(definition, driver);
    await stopProcess(active.processInfo);
    active = null;

    const warm = await startDev(definition, tools[definition.id === "jet" ? "jet" : definition.id === "bun-vite" ? "bun" : "npm"], app, driver);
    active = warm;
    const reloads = await measureReloads(definition, app, driver, sampleCount);
    await stopProcess(active.processInfo);
    active = null;

    return {
      id: definition.id,
      label: definition.label,
      steps: definition.steps.length,
      commands: definition.steps,
      setupMs,
      coldPageMs: cold.pageMs,
      warmPageMs: warm.pageMs,
      reloadSamplesMs: reloads.samples,
      reloadMedianMs: reloads.medianMs,
      installBytes: installedBytes,
      viteVersion: definition.id === "jet" ? null : await viteVersion(app),
      browser: driver.metadata.version,
    };
  } finally {
    if (active) await stopProcess(active.processInfo);
    if (driver) await driver.close();
  }
}

function formatBytes(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KiB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GiB`;
}

function markdown(report) {
  const bunSetup = report.bun.provisioned
    ? `local cache, ${formatBytes(report.bun.cacheBytes)}`
    : "existing executable";
  const vite = report.results.find((result) => result.viteVersion)?.viteVersion || "unknown";
  const sampleWord = report.samples === 1 ? "time" : "times";
  const lines = [
    "# Jet dev DX benchmark",
    "",
    `Run: ${report.generatedAt}`,
    `Machine: ${report.machine.platform}/${report.machine.arch}; ${report.machine.cpu}; Node ${report.machine.node}; Chromium ${report.machine.chromium}`,
    `Tools: ${report.tools.jet}; Bun ${report.tools.bun}; npm ${report.tools.npm}; Vite ${vite}`,
    `Samples: ${report.samples} warm edits per tool. Time unit: milliseconds.`,
    "",
    "| Tool | Steps to first app | Cold page | Warm page | Warm reload median | Project install bytes |",
    "| --- | ---: | ---: | ---: | ---: | ---: |",
    ...report.results.map((result) => `| ${result.label} | ${result.steps} | ${result.coldPageMs} ms | ${result.warmPageMs} ms | ${result.reloadMedianMs} ms | ${formatBytes(result.installBytes)} |`),
    "",
    "Warm reload samples:",
    ...report.results.map((result) => `- ${result.label}: ${result.reloadSamplesMs.join(", ")} ms`),
    "",
    "Qualitative checklist:",
    "- Jet: no project install or config file; the web error overlay keeps the registered diagnostic code plus What, Why, and Fix text.",
    "- Bun + Vite: no Vite config file in the starter; `bun install` is required; the Vite overlay shows JavaScript/source error text and a stack.",
    "- npm + Vite: no Vite config file in the starter; `npm install` is required; the Vite overlay shows JavaScript/source error text and a stack.",
    "",
    "Bun bootstrap: " + bunSetup + ". This is separate from project install weight.",
    "",
    "Reproduce:",
    "",
    "```sh",
    "scripts/agent/jet-env full node scripts/benchmarks/dev-dx.mjs",
    "```",
    "",
    `The harness counts each typed command, including \`cd\`. It starts the browser before the dev command, starts the timer at process spawn, and requires the expected counter text plus a successful button click. It restarts the installed project for the warm page measure. It edits the visible counter source ${report.samples} ${sampleWord} and waits for the new text in the browser. Project install bytes are \`node_modules\` plus the package-manager lockfile; Jet has no project dependency install.`,
  ];
  return `${lines.join("\n")}\n`;
}

async function version(command, args) {
  return (await runProcess(command, args, { cwd: REPO_ROOT, timeoutMs: 15_000 })).stdout.trim().replace(/\s+/g, " ");
}

async function main() {
  const sampleCount = samples();
  const cacheHome = process.env.XDG_CACHE_HOME || join(homedir(), ".cache");
  const scratchBase = process.env.JET_DX_BENCH_ROOT || join(cacheHome, "jet-dx-benchmark");
  const scratchPath = resolve(scratchBase);
  if (scratchPath === "/tmp" || scratchPath.startsWith("/tmp/")) {
    throw new Error(`JET_DX_BENCH_ROOT points at RAM-backed /tmp: ${scratchPath}`);
  }
  await mkdir(scratchPath, { recursive: true });
  const runRoot = await mkdtemp(join(scratchPath, "run-"));
  const keep = process.env.JET_DX_KEEP === "1";
  try {
    const jet = process.env.JET_BIN || (await executable(join(REPO_ROOT, "target", "debug", "jet"))) || await executable("jet");
    const npm = process.env.NPM_BIN || await executable("npm");
    const chromium = process.env.CHROMIUM || await executable("chromium") || await executable("chromium-browser");
    if (!jet) throw new Error("Jet binary not found; build target/debug/jet first or set JET_BIN");
    if (!npm) throw new Error("npm not found; set NPM_BIN");
    if (!chromium) throw new Error("Chromium not found; run this benchmark in the full Jet shell or set CHROMIUM");
    const bun = await ensureBun(npm, cacheHome);
    const tools = { jet, npm, bun: bun.path, chromium };
    const results = [];
    for (const definition of definitions) {
      results.push(await measure(definition, tools, runRoot, sampleCount));
    }
    const report = {
      schema: 1,
      generatedAt: new Date().toISOString(),
      samples: sampleCount,
      machine: {
        platform: process.platform,
        arch: process.arch,
        cpu: os.cpus()[0]?.model || "unknown CPU",
        node: process.version,
        chromium: results[0].browser,
      },
      tools: {
        jet: await version(jet, ["--version"]),
        npm: await version(npm, ["--version"]),
        bun: await version(bun.path, ["--version"]),
      },
      bun: {
        provisioned: bun.provisioned,
        cacheBytes: bun.cacheBytes,
      },
      results,
    };
    if (process.argv.includes("--json")) {
      process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    } else {
      process.stdout.write(markdown(report));
    }
  } finally {
    if (!keep) await rm(runRoot, { recursive: true, force: true });
    else process.stderr.write(`kept benchmark files at ${runRoot}\n`);
  }
}

main().catch((error) => {
  process.stderr.write(`benchmark failed: ${error.stack || error.message}\n`);
  process.exitCode = 1;
});
