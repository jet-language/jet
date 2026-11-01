// Known-failing Jetpack claims: a listed failure keeps the tier green, a listed
// pass fails it. Text fixtures only: no compiler or claim runs.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { classify } from "./known-claims.mjs";

const script = resolve(dirname(fileURLToPath(import.meta.url)), "known-claims.mjs");
const map = { tests: ["recipe admission builds", "store opens", "plugin loads"], test_areas: ["Engine", "Engine", "Compiler"] };
const ledger = "# repo-relative case or golden example ID\tcard\treason\n"
  + "tests/ui/some_case\t#1\ta testkit row the classifier ignores\n"
  + "jetpack-claim:Engine/recipe admission builds\t#3590\trecipe admission is not ported\n";

async function cli(mode, results) {
  const root = await mkdtemp(resolve(tmpdir(), "jetpack-known-claims-"));
  try {
    await writeFile(resolve(root, "map.json"), JSON.stringify(map));
    await writeFile(resolve(root, "results"), results);
    await writeFile(resolve(root, "known-failing.tsv"), ledger);
    return spawnSync(process.execPath, [script, mode, resolve(root, "map.json"), resolve(root, "results"), resolve(root, "known-failing.tsv")], { encoding: "utf8" });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("a listed failing claim is KNOWN-FAILING and keeps both tiers green", async () => {
  const run = await cli("run", "pass store opens\nfail recipe admission builds\npass plugin loads\n");
  assert.equal(run.status, 0, run.stdout + run.stderr);
  assert.match(run.stdout, /^KNOWN-FAILING recipe admission builds \(#3590 recipe admission is not ported\)$/m);
  assert.match(run.stdout, /run claims: 2 passed, 0 failed, 1 known-failing, 0 unexpected-pass$/m);
  const aot = await cli("aot", "store opens: pass\nrecipe admission builds: FAIL\nplugin loads: pass\n2 passed, 1 failed, 0 skipped\n");
  assert.equal(aot.status, 0, aot.stdout + aot.stderr);
  assert.match(aot.stdout, /^KNOWN-FAILING recipe admission builds/m);
});

test("a listed claim that passes is UNEXPECTED-PASS and fails both tiers", async () => {
  const run = await cli("run", "pass store opens\npass recipe admission builds\npass plugin loads\n");
  assert.equal(run.status, 1);
  assert.match(run.stdout, /^UNEXPECTED-PASS recipe admission builds \(#3590 recipe admission is not ported\): remove the stale known-failing row jetpack-claim:Engine\/recipe admission builds$/m);
  const aot = await cli("aot", "store opens: pass\nrecipe admission builds: pass\nplugin loads: pass\n3 passed, 0 failed, 0 skipped\n");
  assert.equal(aot.status, 1);
  assert.match(aot.stdout, /^UNEXPECTED-PASS recipe admission builds/m);
});

test("an unlisted failure, or a claim listed under another area, still fails", () => {
  const unlisted = classify("run", map, "fail store opens\n", ledger);
  assert.equal(unlisted.ok, false);
  assert.ok(unlisted.lines.includes("FAIL store opens"));
  const otherArea = classify("run", map, "fail plugin loads\n", "jetpack-claim:Engine/plugin loads\t#7\tlisted under the wrong area\n");
  assert.equal(otherArea.ok, false);
  assert.ok(otherArea.lines.includes("FAIL plugin loads"));
});

test("the AOT tier needs a result for every claim", () => {
  const verdict = classify("aot", map, "store opens: pass\nrecipe admission builds: FAIL\n", ledger);
  assert.equal(verdict.ok, false);
  assert.ok(verdict.lines.includes("MISSING plugin loads: the AOT run reported no result"));
});

test("malformed or duplicate claim rows are refused", async () => {
  assert.throws(() => classify("run", map, "", "jetpack-claim:Engine/store opens\tno-card\treason\n"), /needs/);
  assert.throws(() => classify("run", map, "", "jetpack-claim:Engine/store opens\t#1\tone\njetpack-claim:Engine/store opens\t#1\ttwo\n"), /duplicate/);
  assert.throws(() => classify("run", { tests: ["store opens"] }, "", ""), /test_areas/);
});
