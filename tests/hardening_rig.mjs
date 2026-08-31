import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";

const RIG = resolve("scripts/agent/hardening-rig.mjs");
const BASE = join(homedir(), ".cache/jet-test-scratch");

function executable(path, source) {
  writeFileSync(path, source, { mode: 0o755 });
}

function fixture() {
  mkdirSync(BASE, { recursive: true });
  const home = mkdtempSync(join(BASE, "hardening-rig-"));
  const root = join(home, "repo");
  const cache = join(home, "cache");
  const scratch = join(home, "scratch");
  const bin = join(home, "bin");
  mkdirSync(root, { recursive: true });
  mkdirSync(bin, { recursive: true });
  writeFileSync(join(root, ".gitignore"), "target/\n");
  writeFileSync(join(root, "README"), "fixture\n");
  executable(join(bin, "tmp-guard"), "#!/bin/sh\nexit 0\n");
  executable(join(bin, "jet-env"), `#!/bin/sh
if [ "$1" = "sh" ]; then
  shift
  exec sh "$@"
fi
printf '%s\\n' "$*" >> "$JET_HARDENING_FAKE_LOG"
mkdir -p "$JET_HARDENING_FAKE_ROOT/target/debug"
printf 'fake-jet-binary\\n' > "$JET_HARDENING_FAKE_ROOT/target/debug/jet"
if [ "$1" = "jet" ] && [ "$2" = "run" ] && [ "$JET_HARDENING_FAKE_TIER_MISMATCH" = "1" ]; then
  case " $* " in
    *" --release "*) printf 'same\\n' ;;
    *" --interpret "*) printf 'same\\n' ;;
    *) printf 'wrong\\n' ;;
  esac
fi
`);
  executable(join(bin, "proof"), `#!/bin/sh
printf '%s\\n' "$*" >> "$JET_HARDENING_FAKE_LOG"
exit 0
`);
  execFileSync("git", ["init", "-q", root]);
  execFileSync("git", ["-C", root, "add", ".gitignore", "README"]);
  execFileSync("git", [
    "-C", root,
    "-c", "user.name=Hardening Rig Test",
    "-c", "user.email=hardening-rig@test.invalid",
    "commit", "-q", "-m", "fixture",
  ]);
  return { home, root, cache, scratch, bin, log: join(cache, "commands.log") };
}

function layerOneFixture() {
  const fx = fixture();
  const seedPath = join(fx.root, "tests/conformance/corpus/core/math/add.jet");
  mkdirSync(dirname(seedPath), { recursive: true });
  writeFileSync(seedPath, `// core-conformance: core.math.add
fn run() {
    value :: 7
    print(value)
}
`);
  execFileSync("git", ["-C", fx.root, "add", "tests/conformance/corpus/core/math/add.jet"]);
  execFileSync("git", [
    "-C", fx.root,
    "-c", "user.name=Hardening Rig Test",
    "-c", "user.email=hardening-rig@test.invalid",
    "commit", "-q", "-m", "layer one seed",
  ]);
  const towerLog = join(fx.cache, "tower.log");
  const towerArgsLog = join(fx.cache, "tower.args");
  const tower = join(fx.bin, "tower.mjs");
  executable(tower, `import { appendFileSync, readFileSync } from "node:fs";
const payload = readFileSync(0, "utf8");
appendFileSync(process.env.JET_HARDENING_FAKE_TOWER_LOG, payload + "\\n");
appendFileSync(process.env.JET_HARDENING_FAKE_TOWER_ARGS, process.argv.slice(2).join(" ") + "\\n");
process.stdout.write(JSON.stringify({ id: "fake-hardening-card", num: 1, action: "added" }));
`);
  return { ...fx, tower, towerLog, towerArgsLog };
}

function runCycle(fx, simulate = null, extraEnv = {}) {
  const args = [RIG, "cycle", "--json"];
  if (simulate) args.push(`--simulate=${simulate}`);
  const result = spawnSync(process.execPath, args, {
    cwd: resolve("."),
    encoding: "utf8",
    env: {
      ...process.env,
      HOME: fx.home,
      JET_HARDENING_ROOT: fx.root,
      JET_HARDENING_CACHE: fx.cache,
      JET_HARDENING_SCRATCH: fx.scratch,
      JET_HARDENING_JET_ENV: join(fx.bin, "jet-env"),
      JET_HARDENING_TMP_GUARD: join(fx.bin, "tmp-guard"),
      JET_HARDENING_PROOF_PARALLEL: join(fx.bin, "proof"),
      JET_HARDENING_TOWER_CLI: fx.tower,
      CARGO_TARGET_DIR: join(fx.root, "target"),
      JET_HARDENING_FAKE_ROOT: fx.root,
      JET_HARDENING_FAKE_LOG: fx.log,
      JET_HARDENING_TEST_MODE: "1",
      JET_HARDENING_MEM_AVAILABLE_GB: "64",
      JET_HARDENING_PROOF_TARGETS: "dev_corpus_gate",
      JET_HARDENING_SHARDS: "fuzz_sema,sema_soundness_differential",
      JET_HARDENING_BUILD_TIMEOUT_MS: "5000",
      JET_HARDENING_PROOF_TIMEOUT_MS: "5000",
      ...extraEnv,
    },
  });
  assert.equal(result.signal, null, result.stderr);
  assert.doesNotThrow(() => JSON.parse(result.stdout), result.stderr || result.stdout);
  return { process: result, record: JSON.parse(result.stdout) };
}

function assertCleaned(fx, record) {
  assert.equal(record.cleanup.scratch_removed, true);
  assert.equal(record.cleanup.children, true);
  assert.equal(existsSync(join(fx.cache, "rig.lock")), false);
  assert.equal(existsSync(join(fx.root, "target/.jet-hardening-build.lock")), false);
  const residue = existsSync(fx.scratch)
    ? readdirSync(fx.scratch).filter((name) => name.startsWith("cycle-"))
    : [];
  assert.deepEqual(residue, []);
}

test("bounded hardening cycle refuses unsafe starts and cleans every exit", () => {
  const main = fixture();
  try {
    const first = runCycle(main);
    assert.equal(first.process.status, 0);
    assert.equal(first.record.status, "PASS", JSON.stringify(first.record.refusal));
    assert.equal(first.record.build.exit, 0);
    assert.equal(first.record.proof.exit, 0);
    assert.equal(first.record.config.suite_concurrency, 2);
    assert.equal(first.record.config.cargo_build_jobs, 4);
    assert.equal(first.record.config.target_cap_gib, 80);
    assertCleaned(main, first.record);

    const second = runCycle(main);
    assert.equal(second.record.status, "PASS");
    assert.deepEqual(second.record.build, {
      skipped: true,
      reason: "same clean commit already built",
      binary_sha256: second.record.binary_sha256,
    });
    assertCleaned(main, second.record);
    assert.match(readFileSync(main.log, "utf8"), /cargo build -p jet/);
    assert.match(readFileSync(main.log, "utf8"), /-j 2 dev_corpus_gate fuzz_sema sema_soundness_differential/);
  } finally {
    rmSync(main.home, { recursive: true, force: true });
  }

  const skipped = ["dirty", "busy", "tmp-guard", "memory", "target", "cache"];
  for (const simulation of skipped) {
    const fx = fixture();
    try {
      const outcome = runCycle(fx, simulation);
      assert.equal(outcome.process.status, 0, simulation);
      assert.equal(outcome.record.status, "SKIPPED", simulation);
      assert.ok(outcome.record.refusal.reason, simulation);
      assertCleaned(fx, outcome.record);
    } finally {
      rmSync(fx.home, { recursive: true, force: true });
    }
  }

  for (const simulation of ["build-failure", "test-failure", "timeout", "signal"]) {
    const fx = fixture();
    try {
      const outcome = runCycle(fx, simulation);
      assert.equal(outcome.process.status, 1, simulation);
      assert.equal(outcome.record.status, "RED", simulation);
      assert.equal(existsSync(join(fx.cache, "failure.json")), true, simulation);
      assertCleaned(fx, outcome.record);
    } finally {
      rmSync(fx.home, { recursive: true, force: true });
    }
  }

  const stale = fixture();
  try {
    const outcome = runCycle(stale, "stale-lease");
    assert.equal(outcome.record.status, "PASS");
    assert.ok(outcome.record.transitions.some((row) => row.phase === "stale_lease_recovered"));
    assertCleaned(stale, outcome.record);
  } finally {
    rmSync(stale.home, { recursive: true, force: true });
  }
});

test("layer-1 findings are bounded, deterministic, cleaned, and written only through Tower CLI", () => {
  const fx = layerOneFixture();
  const env = {
    JET_HARDENING_TEST_MODE: "0",
    JET_HARDENING_DRY_RUN: "0",
    JET_HARDENING_FAKE_TOWER_LOG: fx.towerLog,
    JET_HARDENING_FAKE_TOWER_ARGS: fx.towerArgsLog,
    JET_HARDENING_FAKE_TIER_MISMATCH: "1",
    JET_HARDENING_INCLUDE_DIFFERENTIAL: "0",
    JET_HARDENING_ORACLE_MAX_CASES: "5",
    JET_HARDENING_ORACLE_BATCH_SIZE: "2",
    JET_HARDENING_ORACLE_TIMEOUT_MS: "5000",
  };
  try {
    const first = runCycle(fx, null, env);
    assert.equal(first.process.status, 1);
    assert.equal(first.record.status, "RED");
    assert.equal(first.record.oracle.status, "FINDINGS");
    assert.equal(first.record.oracle.attempted, 5);
    assert.equal(first.record.oracle.valid_case_count, 5);
    assert.equal(first.record.oracle.finding_payloads.length, 5);
    assert.equal(first.record.tower.actions.length, 5);
    assert.ok(first.record.tower.actions.every((action) => action.status === "WRITTEN"));
    assertCleaned(fx, first.record);
    const firstBundles = first.record.oracle.serialized_bundles;

    const second = runCycle(fx, null, env);
    assert.equal(second.record.status, "RED");
    assert.equal(second.record.oracle.serialized_bundles, firstBundles);
    assert.equal(readFileSync(fx.towerLog, "utf8").trim().split("\n").length, 10);
    const towerArgs = readFileSync(fx.towerArgsLog, "utf8").trim().split("\n");
    assert.equal(towerArgs.length, 10);
    assert.ok(towerArgs.every((args) => args.includes("card add --stdin --json --by hardening-rig")));
    assert.ok(towerArgs.every((args) => !args.includes("--force")));
    assertCleaned(fx, second.record);
    assert.equal(existsSync(join(fx.root, ".tower")), false);
  } finally {
    rmSync(fx.home, { recursive: true, force: true });
  }
});
