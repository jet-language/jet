#!/usr/bin/env node
// L2 exit witness after E1/E2: native panic/trap stems run against
// the Jet-object pack. In addition to golden stderr bytes, pin the EXACT exit
// code of each Rust reference, not the backend harness's loose 70-or-1 rule.
// --plan only records the stem selection; it does not build or execute anything.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { collectGoldenEntries } from "../../../Tools/agent/compiler-diff.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [output, baseline, mode] = process.argv.slice(2);
if (!output || !baseline || (mode && mode !== "--plan") || process.argv.length > 5) {
  throw new Error("usage: run-trap-goldens.mjs OUT BASELINE.results.tsv [--plan]");
}
const out = resolve(output);
const expected = join(root, "Examples/features/expected");
const passingStderr = readFileSync(baseline, "utf8").trim().split("\n").map((row) => row.split("\t"))
  .filter(([stem, verdict]) => verdict === "pass" &&
    (existsSync(join(expected, `${stem}.err.out`)) || existsSync(join(expected, `${stem}.stderr.out`))))
  .map(([stem]) => stem);
// The original 140-pass native baseline contains no stderr golden. Do not let
// that accidentally turn L2 parity into an empty proof: include every golden
// whose expected stop is one of the three registered rows this layer owns.
const trapStems = collectGoldenEntries(join(root, "Examples/features")).map(({ stem }) => stem).filter((stem) => {
  const path = join(expected, `${stem}.err.out`);
  return existsSync(path) && /^Stop \[(?:E3001|E3010|E3011)\]:/u.test(readFileSync(path, "utf8"));
});
const stems = [...new Set([...passingStderr, ...trapStems])].sort();
if (!stems.length) throw new Error("empty trap stem selection");
mkdirSync(out, { recursive: true });
const cases = join(out, "trap.cases");
writeFileSync(cases, stems.join("\n") + "\n");
console.log(`${passingStderr.length} baseline stderr stems; ${trapStems.length} registered trap stems:\n${stems.join("\n")}`);
if (mode !== "--plan") {
  if (!process.env.JET_RUNTIME_PACK) throw new Error("JET_RUNTIME_PACK must name the E2 pack containing the Jet trap object");
  const harness = spawnSync(process.execPath, [join(root, "Compiler/JetBackend/Tests/run-goldens.mjs"), out, "--case-list", cases], {
    cwd: root,
    stdio: "inherit",
    // Exit parity requires actual reference execution, not emit-only MIR.
    env: { ...process.env, JET_MIR_EMIT: "0" },
  });
  const failures = [];
  const rows = [];
  for (const stem of stems) {
    const slug = stem.replace(/[^A-Za-z0-9_-]+/gu, "_");
    const refPath = join(out, "mir", `${slug}.ref`);
    const statusPath = join(out, "run", `${slug}.status.json`);
    const stderrPath = join(out, "run", `${slug}.stderr`);
    const goldenPath = join(expected, `${stem}.${existsSync(join(expected, `${stem}.err.out`)) ? "err" : "stderr"}.out`);
    if (![refPath, statusPath, stderrPath].every(existsSync)) {
      failures.push(`${stem}: missing reference/native result`);
      rows.push(`${stem}\tmissing`);
      continue;
    }
    const reference = JSON.parse(readFileSync(refPath, "utf8"));
    const native = JSON.parse(readFileSync(statusPath, "utf8"));
    const stderr = readFileSync(stderrPath);
    const bytesMatch = stderr.equals(readFileSync(goldenPath));
    const referenceMatch = typeof reference.stderr === "string" && stderr.equals(Buffer.from(reference.stderr));
    const exitMatch = reference.mode === "run" && Number.isInteger(reference.status) &&
      native.status === reference.status && native.signal === null && native.error === null;
    const ok = bytesMatch && referenceMatch && exitMatch;
    rows.push(`${stem}\t${ok ? "pass" : "wrong"}\tref=${reference.status}\tnative=${native.status ?? native.signal}\tgolden-stderr=${bytesMatch}\treference-stderr=${referenceMatch}`);
    if (!ok) failures.push(`${stem}: exact exit/stderr mismatch`);
  }
  writeFileSync(join(out, "trap-results.tsv"), rows.join("\n") + "\n");
  for (const failure of failures) console.error(failure);
  if (harness.error) console.error(harness.error.message);
  if (failures.length || harness.status !== 0) process.exitCode = 1;
}
