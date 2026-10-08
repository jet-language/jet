#!/usr/bin/env node
// L2 exit witness after E1/E2: native panic/trap stems run against
// the Jet-object pack. Pin golden stdout/stderr bytes, both reference streams,
// and the EXACT exit code of each executed Rust reference.
// --plan only records the stem selection; it does not build or execute anything.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { collectGoldenEntries, goldenFailureExit } from "../../../Tools/agent/compiler-diff.mjs";

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
    const refStdoutPath = join(out, "mir", `${slug}.stdout`);
    const refStderrPath = join(out, "mir", `${slug}.stderr`);
    const statusPath = join(out, "run", `${slug}.status.json`);
    const stderrPath = join(out, "run", `${slug}.stderr`);
    const stdoutPath = join(out, "run", `${slug}.stdout`);
    const goldenStdoutPath = join(expected, `${stem}.out`);
    const failure = existsSync(join(expected, `${stem}.err.out`));
    const goldenPath = join(expected, `${stem}.${failure ? "err" : "stderr"}.out`);
    if (![refPath, refStdoutPath, refStderrPath, statusPath, stdoutPath, stderrPath].every(existsSync)) {
      failures.push(`${stem}: missing reference/native result`);
      rows.push(`${stem}\tmissing`);
      continue;
    }
    const reference = JSON.parse(readFileSync(refPath, "utf8"));
    const native = JSON.parse(readFileSync(statusPath, "utf8"));
    const stderr = readFileSync(stderrPath);
    const stdout = readFileSync(stdoutPath);
    const stdoutBytesMatch = stdout.equals(existsSync(goldenStdoutPath) ? readFileSync(goldenStdoutPath) : Buffer.alloc(0));
    const stdoutReferenceMatch = stdout.equals(readFileSync(refStdoutPath));
    const goldenStderr = readFileSync(goldenPath);
    const expectedExit = failure ? goldenFailureExit(stem, goldenStderr.toString("utf8")) : 0;
    const bytesMatch = stderr.equals(goldenStderr);
    const referenceMatch = stderr.equals(readFileSync(refStderrPath));
    const exitMatch = expectedExit !== null && reference.mode === "run" && Number.isInteger(reference.status) &&
      reference.status === expectedExit && native.status === reference.status && native.signal === null && native.error === null;
    const ok = bytesMatch && referenceMatch && stdoutBytesMatch && stdoutReferenceMatch && exitMatch;
    rows.push(`${stem}\t${ok ? "pass" : "wrong"}\texpected=${expectedExit ?? "ambiguous"}\tref=${reference.status}\tnative=${native.status ?? native.signal}\tgolden-stdout=${stdoutBytesMatch}\treference-stdout=${stdoutReferenceMatch}\tgolden-stderr=${bytesMatch}\treference-stderr=${referenceMatch}`);
    if (!ok) failures.push(`${stem}: ${expectedExit === null ? "ambiguous failure exit report" : "exact exit/stdout/stderr mismatch"}`);
  }
  writeFileSync(join(out, "trap-results.tsv"), rows.join("\n") + "\n");
  for (const failure of failures) console.error(failure);
  if (harness.error) console.error(harness.error.message);
  if (failures.length || harness.status !== 0) process.exitCode = 1;
}
