#!/usr/bin/env node
// Standing local proofq trigger, not a CI/nightly job. Polls every 60 seconds.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { hash } from "./safety-gate.mjs";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
export const INTERVAL_MS = 6 * 60 * 60 * 1000;
export function due(state, artifactHash, now) {
  return !state || state.artifacts_sha256 !== artifactHash || now - state.at >= INTERVAL_MS;
}
const quote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;
export function renderJob(template, values) {
  return ["#!/usr/bin/env bash", "# proofq: mem=8G time=120M kind=test", ...Object.entries(values).map(([name, value]) => `export ${name}=${quote(value)}`), template].join("\n") + "\n";
}

export function tick({ artifacts, stateDir, queueRoot, scratchRoot, preflight, cheap, count = 50 }) {
  mkdirSync(stateDir, { recursive: true });
  if (existsSync(join(queueRoot, "PAUSE"))) return "proofq paused";
  for (const directory of ["queue", "running", "queue-critical-hold", "queue-gate-hold"]) {
    if (existsSync(join(queueRoot, directory)) && readdirSync(join(queueRoot, directory)).some((file) => /^SafetyNet.*\.sh$/u.test(file))) return "standing safety job already queued/running";
  }
  const bytes = readFileSync(artifacts);
  const artifactHash = hash(bytes);
  const manifest = JSON.parse(bytes);
  if (!manifest.dev_head) throw new Error("artifacts manifest has no admitted head");
  const statePath = join(stateDir, "standing.json");
  const state = existsSync(statePath) ? JSON.parse(readFileSync(statePath, "utf8")) : null;
  const now = Date.now();
  const pin = spawnSync("git", ["-C", repo, "rev-parse", "HEAD"], { encoding: "utf8" });
  if (pin.status !== 0) throw new Error("cannot pin tooling tree");
  const sha = pin.stdout.trim();
  if (!due(state, artifactHash, now) && state.tooling_sha === sha) return "not due";
  const seed = state && ![0, 3].includes(state.preverify_rc) ? state.seed : (state?.seed ?? 3) + 1;
  const name = `SafetyNet8-standing-${manifest.dev_head.slice(0, 9)}-s${seed}-${now}`;
  const evidence = join(scratchRoot, name);
  mkdirSync(evidence, { recursive: true });
  const snapshot = join(evidence, "artifacts.json");
  writeFileSync(snapshot, bytes);
  // Persist failed attempts too: a red pre-flight must not loop every minute.
  // Retry the same failing seed after an input/tooling change or the next cadence.
  const attempt = { artifacts_sha256: artifactHash, head: manifest.dev_head, tooling_sha: sha, seed, at: now, evidence, preverify_rc: 2 };
  writeFileSync(statePath, JSON.stringify(attempt, null, 2) + "\n");
  const run = (label, bin, args) => {
    const result = spawnSync(bin, args, { cwd: repo, encoding: "utf8", maxBuffer: 1 << 26, timeout: 7200000 });
    writeFileSync(join(evidence, `${label}.log`), `${result.stdout ?? ""}${result.stderr ?? ""}`);
    if (result.error || result.signal) throw new Error(`${label}: ${result.signal ?? result.error?.message}`);
    return result.status;
  };
  // Exact-tree cheap faithful equivalents first. The gate is an artifact consumer,
  // not a compiler build: ASan links/runs are serial and bounded to 8G by cheap.sh.
  const env = join(repo, "Tools/agent/jet-env");
  if (run("preverify-tests", cheap, [env, process.execPath, "--test", "Tools/agent/safety-gate.test.mjs"]) !== 0) throw new Error("tooling pre-verification failed");
  const rc = run("preverify", cheap, [env, process.execPath, "Tools/agent/safety-gate.mjs", join(evidence, "preverify"), snapshot, String(seed), String(count)]);
  attempt.preverify_rc = rc;
  writeFileSync(statePath, JSON.stringify(attempt, null, 2) + "\n");
  if (![0, 3].includes(rc)) throw new Error(`pre-verification failed (exit ${rc}); all findings retained in ${evidence}`);
  // GAP is a known unavailable coverage receipt, never converted to PASS.
  // Current owner instruction explicitly permits running available steps with gaps.
  const temporary = join(evidence, `${name}.sh`);
  writeFileSync(temporary, renderJob(readFileSync(join(repo, "Tools/agent/safety-gate-job.sh"), "utf8"), {
    WT: repo, SHA: sha, ARTIFACTS: snapshot, ARTIFACTS_SHA256: artifactHash,
    OUT: join(evidence, "gate"), SEED: seed, COUNT: count,
  }));
  if (run("pq-preflight", process.execPath, [preflight, "--job", temporary]) !== 0) throw new Error(`pq-preflight rejected ${temporary}`);
  mkdirSync(join(queueRoot, "queue"), { recursive: true });
  renameSync(temporary, join(queueRoot, "queue", `${name}.sh`));
  attempt.job = name;
  writeFileSync(statePath, JSON.stringify(attempt, null, 2) + "\n");
  return `queued ${name}; preverify ${rc === 3 ? "GAP (not green)" : "PASS"}`;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const home = process.env.HOME;
  const options = {
    artifacts: process.env.SAFETY_ARTIFACTS ?? join(home, ".cache/jet-dev/dev-artifacts.json"),
    stateDir: process.env.SAFETY_STATE ?? join(home, ".cache/jet-dev/scratch/SafetyNet8/standing"),
    queueRoot: process.env.PQ_ROOT ?? join(home, ".cache/jet-dev/proofq"),
    scratchRoot: process.env.SAFETY_SCRATCH ?? "/mnt/jetscratch/candidates/SafetyNet8",
    preflight: join(home, ".cache/jet-dev/proofq/bin/pq-preflight.mjs"),
    cheap: join(home, ".cache/jet-dev/cheap.sh"),
    count: Number(process.env.SAFETY_COUNT ?? 50),
  };
  // Kernel-released singleton lock: a killed/restarted watcher cannot leave a
  // stale directory lock that silently parks the standing safety net.
  if (!process.argv.includes("--locked")) {
    mkdirSync(options.stateDir, { recursive: true });
    const locked = spawnSync("flock", ["-n", join(options.stateDir, "trigger.lock"), process.execPath, fileURLToPath(import.meta.url), "--locked", ...process.argv.slice(2)], { stdio: "inherit" });
    process.exit(locked.status ?? 2);
  }
  const watch = process.argv.slice(2).includes("--watch");
  do {
    try { console.log(tick(options)); }
    catch (error) { console.error(error.stack ?? String(error)); if (!watch) process.exit(2); }
    if (watch) await new Promise((done) => setTimeout(done, 60000));
  } while (watch);
}
