#!/usr/bin/env node
// Gauntlet axis build_system: Jet's build system against Nix, the required
// peer, on one medium project (Tools/gauntlet/axes/build-system/generate.mjs).
// Every cell pairs one Jet command with one Nix command over the same project
// state, measures wall time in fresh processes, interleaves the two peers
// (the order alternates per sample) and validates every built artifact
// against Tools/perf/build-time/midsize.out. The cell commands come from
// measurement-manifest.json axes.build_system.cells; the preparation of each
// cell (edit, cache drop, output delete) is fixed here and never timed.
//
// usage: node Tools/gauntlet/harness/build-system.mjs --jet JET [--work-dir DIR]
//          [--receipt FILE] [--samples N] [--warmups N]
// The caller's environment reaches `jet` unchanged (JET_TOOLCHAIN_ROOT and
// friends); JET_STORE_DIR points the machine-wide store at a directory under the work dir.
import fs from "node:fs";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { EDIT_FILES, SYSTEM, write as writeProject } from "../axes/build-system/generate.mjs";

export const BUILD_SYSTEM_SCHEMA = "gauntlet-axis-build-system-v1";
export const BUILD_SYSTEM_RECEIPT_SCHEMA = "gauntlet-build-system-receipt-v1";
export const BUILD_SYSTEM_CELLS = ["eval", "noop", "incremental", "cold_cache", "scratch"];
const HARNESS = "Tools/gauntlet/harness/build-system.mjs";
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const MANIFEST = path.join(REPO, "Tools/gauntlet/measurement-manifest.json");
const EXPECTED = path.join(REPO, "Tools/perf/build-time/midsize.out");
const STEP_TIMEOUT_MS = 30 * 60 * 1000;

export function buildSystemContractIssues(axis) {
  const issues = [];
  if (!axis) return ["build_system axis is missing"];
  if (axis.status !== "required") issues.push("build_system must be required");
  if (axis.schema !== BUILD_SYSTEM_SCHEMA) issues.push(`build_system schema must be ${BUILD_SYSTEM_SCHEMA}`);
  if (axis.metric !== "wall_ns") issues.push("build_system metric must be wall_ns");
  if (axis.harness !== HARNESS) issues.push(`build_system harness must be ${HARNESS}`);
  const peers = axis.peers ?? [];
  if (JSON.stringify(peers.map((peer) => peer.id)) !== JSON.stringify(["jet", "nix"]) || !peers.every((peer) => peer.required === true)) {
    issues.push("build_system peers must be jet and nix, both required");
  }
  const cells = axis.cells ?? [];
  if (JSON.stringify(cells.map((cell) => cell.id)) !== JSON.stringify(BUILD_SYSTEM_CELLS)) issues.push(`build_system cells must be ${BUILD_SYSTEM_CELLS.join(", ")}`);
  for (const cell of cells) {
    if (!Array.isArray(cell.jet) || !Array.isArray(cell.nix) || cell.jet.length === 0 || cell.nix.length === 0) issues.push(`${cell.id}: both peers need a command`);
    const target = cell.target ?? {};
    const bounded = typeof target.ratio_max === "number" || typeof target.ratio_lt === "number";
    if (!bounded || typeof target.ratio_lt === "number" && target.ratio_lt > 1) issues.push(`${cell.id}: target must bound jet/nix at or below 1.00`);
  }
  if (!(axis.samples >= 1) || !(axis.warmups >= 0)) issues.push("build_system needs samples and warmups");
  return issues;
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : Math.round((sorted[mid - 1] + sorted[mid]) / 2);
}

function verdictOf(target, ratio) {
  if (typeof target.ratio_max === "number") return ratio <= target.ratio_max ? "win" : "loss";
  return ratio < target.ratio_lt ? "win" : "loss";
}

function machine() {
  const cpus = os.cpus();
  return {
    cpu_model: cpus[0]?.model ?? "unknown",
    threads: cpus.length,
    memory_bytes: os.totalmem(),
    kernel: os.release(),
    nix: run(["nix", "--version"]).stdout.trim(),
  };
}

function run(argv, options = {}) {
  const started = process.hrtime.bigint();
  const result = spawnSync(argv[0], argv.slice(1), {
    cwd: options.cwd,
    env: options.env ?? process.env,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    timeout: STEP_TIMEOUT_MS,
  });
  const wall_ns = Number(process.hrtime.bigint() - started);
  return { status: result.status, stdout: result.stdout ?? "", stderr: result.stderr ?? String(result.error ?? ""), wall_ns };
}

function must(argv, options, what) {
  const result = run(argv, options);
  if (result.status !== 0) throw new Error(`${what}: \`${argv.join(" ")}\` exited ${result.status}: ${result.stderr.trim().split("\n").slice(-3).join(" | ")}`);
  return result;
}

class Workspace {
  constructor(workDir, jet) {
    this.workDir = path.resolve(workDir);
    this.project = path.join(this.workDir, "project");
    this.jetDir = path.join(this.project, "jet");
    this.nixDir = path.join(this.project, "nix");
    this.jetStore = path.join(this.workDir, "jet-store");
    this.nixStore = `local?root=${path.join(this.workDir, "nix-root")}`;
    this.nixStoreRoot = path.join(this.workDir, "nix-root");
    this.nixCache = `file://${path.join(this.workDir, "nix-cache")}`;
    this.flake = `path:${this.nixDir}#packages.${SYSTEM}.default`;
    this.jet = path.resolve(jet);
    this.jetEnv = { ...process.env, JET_STORE_DIR: this.jetStore };
    this.expected = fs.readFileSync(EXPECTED, "utf8");
    this.nixOut = null;
    this.edits = 0;
  }

  expand(argv) {
    const values = { "{jet}": this.jet, "{flake}": this.flake, "{nix_store}": this.nixStore, "{nix_cache}": this.nixCache };
    return argv.map((word) => Object.entries(values).reduce((text, [key, value]) => text.split(key).join(value), word));
  }

  // A fresh project and caches, the private Nix store seeded with the pinned
  // nixpkgs source (flake evaluation needs it; it is a toolchain input), then
  // one validated build by each peer.
  async setUp() {
    await fsp.rm(this.workDir, { recursive: true, force: true });
    await fsp.mkdir(this.jetStore, { recursive: true });
    writeProject(this.project);
    const lock = JSON.parse(fs.readFileSync(path.join(this.nixDir, "flake.lock"), "utf8"));
    const { owner, repo, rev } = lock.nodes.nixpkgs.locked;
    const nixpkgs = must(["nix", "eval", "--raw", "--expr", `(builtins.getFlake "github:${owner}/${repo}/${rev}").outPath`], {}, "resolve the pinned nixpkgs").stdout.trim();
    must(["nix", "copy", "--to", this.nixStore, nixpkgs], {}, "seed the private Nix store");
    this.validateJet(must([this.jet, "build"], { cwd: this.jetDir, env: this.jetEnv }, "first jet build"));
    this.validateNix(must(this.expand(["nix", "build", "--no-link", "--print-out-paths", "--option", "substitute", "false", "{flake}"]), {}, "first nix build"), "/");
  }

  validateJet(result) {
    const artifact = path.join(this.jetDir, ".jet/build/medium");
    const output = must([artifact], {}, "run the jet artifact").stdout;
    if (output !== this.expected) throw new Error(`the jet artifact printed ${JSON.stringify(output)}`);
    return result;
  }

  validateNix(result, storeRoot) {
    const out = result.stdout.trim().split("\n").at(-1);
    if (!out?.startsWith("/nix/store/")) throw new Error(`nix build printed no output path: ${JSON.stringify(result.stdout)}`);
    const output = must([path.join(storeRoot, out, "bin/medium")], {}, "run the nix artifact").stdout;
    if (output !== this.expected) throw new Error(`the nix artifact printed ${JSON.stringify(output)}`);
    this.nixOut = out;
    return result;
  }

  // One new unused function in the edit file: a real source change that
  // keeps the program's output.
  edit(peer) {
    this.edits += 1;
    const n = this.edits;
    const text = peer === "jet" ? `\nedit_probe_${n}: fn() -> Int {\n    return ${n}\n}\n` : `\nint64_t edit_probe_${n}(void) { return ${n}; }\n`;
    fs.appendFileSync(path.join(this.project, EDIT_FILES[peer]), text);
  }

  async prepare(cellId, peer) {
    if (cellId === "incremental") this.edit(peer);
    if (cellId === "cold_cache" && peer === "jet") await fsp.rm(path.join(this.jetDir, ".jet"), { recursive: true, force: true });
    if (cellId === "scratch" && peer === "jet") {
      await fsp.rm(path.join(this.jetDir, ".jet"), { recursive: true, force: true });
      await fsp.rm(this.jetStore, { recursive: true, force: true });
      await fsp.mkdir(this.jetStore, { recursive: true });
    }
    if (cellId === "cold_cache" && peer === "nix") {
      // The substituter holds the current build; the private store does not.
      must(["nix", "copy", "--to", this.nixCache, this.nixOut], {}, "populate the binary cache");
      // An output the private store never held has nothing to delete.
      const deleted = run(["nix", "store", "delete", "--store", this.nixStore, this.nixOut]);
      if (fs.existsSync(path.join(this.nixStoreRoot, this.nixOut))) throw new Error(`could not delete ${this.nixOut} from the private store: ${deleted.stderr.trim()}`);
    }
  }

  measure(cell, peer) {
    const argv = this.expand(cell[peer]);
    const options = peer === "jet" ? { cwd: this.jetDir, env: this.jetEnv } : { cwd: this.nixDir };
    const result = must(argv, options, `${cell.id} (${peer})`);
    if (cell.id === "eval") {
      if (peer === "nix" && !result.stdout.trim().endsWith(".drv")) throw new Error(`nix eval printed ${JSON.stringify(result.stdout)}`);
    } else if (peer === "jet") {
      this.validateJet(result);
    } else {
      this.validateNix(result, cell.id === "cold_cache" ? this.nixStoreRoot : "/");
    }
    return result.wall_ns;
  }
}

export async function measureBuildSystem(axis, { jet, workDir, samples = axis.samples, warmups = axis.warmups }) {
  const receipt = {
    schema: BUILD_SYSTEM_RECEIPT_SCHEMA,
    started_at: new Date().toISOString(),
    machine: machine(),
    jet: { path: path.resolve(jet), version: run([jet, "--version"]).stdout.trim() },
    samples,
    warmups,
    cells: [],
    failures: [],
    passed: false,
  };
  const space = new Workspace(workDir, jet);
  const rows = new Map(axis.cells.map((cell) => [cell.id, { id: cell.id, target: cell.target, jet: [], nix: [] }]));
  try {
    await space.setUp();
    for (let round = 0; round < warmups + samples; round += 1) {
      const order = round % 2 === 0 ? ["jet", "nix"] : ["nix", "jet"];
      for (const cell of axis.cells) {
        for (const peer of order) {
          await space.prepare(cell.id, peer);
          const wall = space.measure(cell, peer);
          if (round >= warmups) rows.get(cell.id)[peer].push(wall);
        }
      }
    }
  } catch (error) {
    receipt.failures.push(String(error.message ?? error));
  }
  for (const row of rows.values()) {
    const complete = row.jet.length === samples && row.nix.length === samples;
    const summary = (values) => values.length ? { samples_ns: values, median_ns: median(values), min_ns: Math.min(...values), max_ns: Math.max(...values) } : null;
    const jetSummary = summary(row.jet);
    const nixSummary = summary(row.nix);
    const ratio = complete ? Number((jetSummary.median_ns / nixSummary.median_ns).toFixed(4)) : null;
    receipt.cells.push({
      id: row.id,
      target: row.target,
      status: complete ? "measured" : "unmeasured",
      jet: jetSummary,
      nix: nixSummary,
      ratio,
      verdict: complete ? verdictOf(row.target, ratio) : "unmeasured",
    });
  }
  receipt.finished_at = new Date().toISOString();
  receipt.passed = receipt.failures.length === 0 && receipt.cells.every((cell) => cell.verdict === "win");
  return receipt;
}

function table(receipt) {
  const ms = (summary) => summary ? (summary.median_ns / 1e6).toFixed(1) : "-";
  const lines = ["cell\tjet_ms\tnix_ms\tjet/nix\ttarget\tverdict"];
  for (const cell of receipt.cells) {
    const target = typeof cell.target.ratio_max === "number" ? `<= ${cell.target.ratio_max.toFixed(2)}` : `< ${cell.target.ratio_lt.toFixed(2)}`;
    lines.push(`${cell.id}\t${ms(cell.jet)}\t${ms(cell.nix)}\t${cell.ratio ?? "-"}\t${target}\t${cell.verdict}`);
  }
  for (const failure of receipt.failures) lines.push(`failure: ${failure}`);
  return `${lines.join("\n")}\n`;
}

export async function runBuildSystemAxisAdapter(axis, { runDir, jetBin, repoDir }) {
  const axisDir = path.join(runDir, "axes", "build_system");
  await fsp.mkdir(axisDir, { recursive: true });
  const receiptPath = path.join(axisDir, "receipt.json");
  const args = [path.join(repoDir, axis.harness), "--jet", jetBin, "--work-dir", path.join(axisDir, "work"), "--receipt", receiptPath];
  const exit = await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, args, { cwd: repoDir, stdio: ["ignore", "inherit", "inherit"] });
    child.on("error", reject);
    child.on("exit", (code) => resolve(code));
  });
  let receipt = null;
  try { receipt = JSON.parse(await fsp.readFile(receiptPath, "utf8")); } catch { /* reported below */ }
  const blockers = receipt ? [...receipt.failures] : [`the build-system harness exited ${exit} without a receipt`];
  const metrics = {};
  const cells = {};
  for (const cell of receipt?.cells ?? []) {
    metrics[cell.id] = { jet_wall_ns: cell.jet?.median_ns ?? null, nix_wall_ns: cell.nix?.median_ns ?? null };
    cells[cell.id] = { status: cell.status, jet: cell.jet?.median_ns ?? null, peer: cell.nix?.median_ns ?? null, ratio: cell.ratio, verdict: cell.verdict };
    if (cell.verdict !== "win") blockers.push(`${cell.id}: jet/nix ${cell.ratio ?? "unmeasured"} misses ${JSON.stringify(cell.target)}`);
  }
  const complete = Boolean(receipt) && (receipt.cells ?? []).length === axis.cells.length && receipt.cells.every((cell) => cell.status === "measured");
  const verdict = !complete ? "unmeasured" : receipt.cells.every((cell) => cell.verdict === "win") ? "win" : "loss";
  return {
    id: "build_system",
    required: axis.status === "required",
    status: complete ? "complete" : "incomplete",
    contract: axis,
    schema: axis.schema,
    metric: axis.metric,
    receipt: receiptPath,
    metrics,
    comparisons: { nix: { verdict, cells } },
    verdicts: { nix: verdict },
    publication: { status: receipt?.passed ? "ready" : "blocked", blockers },
  };
}

function parseArgs(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (flag === "--jet") options.jet = value;
    else if (flag === "--work-dir") options.workDir = value;
    else if (flag === "--receipt") options.receipt = value;
    else if (flag === "--samples") options.samples = Number(value);
    else if (flag === "--warmups") options.warmups = Number(value);
    else throw new Error(`unknown flag ${flag}`);
    index += 1;
  }
  if (!options.jet) throw new Error(`usage: node ${HARNESS} --jet JET [--work-dir DIR] [--receipt FILE] [--samples N] [--warmups N]`);
  return options;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const options = parseArgs(process.argv.slice(2));
  const axis = JSON.parse(fs.readFileSync(MANIFEST, "utf8")).axes.build_system;
  const issues = buildSystemContractIssues(axis);
  if (issues.length) throw new Error(issues.join("; "));
  const workDir = options.workDir ?? fs.mkdtempSync(path.join(os.tmpdir(), "jet-build-system-"));
  const receipt = await measureBuildSystem(axis, {
    jet: options.jet,
    workDir,
    samples: options.samples ?? axis.samples,
    warmups: options.warmups ?? axis.warmups,
  });
  if (options.receipt) {
    await fsp.mkdir(path.dirname(path.resolve(options.receipt)), { recursive: true });
    await fsp.writeFile(options.receipt, `${JSON.stringify(receipt, null, 2)}\n`);
  }
  process.stdout.write(table(receipt));
  process.exit(receipt.failures.length ? 1 : 0);
}
