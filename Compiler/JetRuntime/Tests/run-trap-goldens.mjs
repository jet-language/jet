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
import {readGoldenResults} from "./golden-results.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [output, baseline, mode, sharedResults] = process.argv.slice(2);
if (!output || !baseline || (mode && mode !== "--plan" && mode !== "--results") ||
    (mode === "--results" ? !sharedResults || process.argv.length !== 6 : process.argv.length > 5)) {
  throw new Error("usage: run-trap-goldens.mjs OUT BASELINE.results.tsv [--plan | --results fresh-results.tsv]");
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
  const harness = mode === "--results" ? null : spawnSync(process.execPath, [join(root, "Compiler/JetBackend/Tests/run-goldens.mjs"), out, "--front", "jet", "--case-list", cases], {
    cwd: root, stdio: "inherit", env: { ...process.env, JET_MIR_EMIT: "0" },
  });
  const nativeRoot = sharedResults ? dirname(resolve(sharedResults)) : out;
  const results = readGoldenResults(sharedResults ? resolve(sharedResults) : join(out, "results.tsv"));
  const failures = [];
  const rows = [];
  for (const stem of stems) {
    const slug = stem.replace(/[^A-Za-z0-9_-]+/gu, "_");
    const statusPath = join(nativeRoot, "run", `${slug}.status.json`);
    const stderrPath = join(nativeRoot, "run", `${slug}.stderr`);
    const stdoutPath = join(nativeRoot, "run", `${slug}.stdout`);
    const goldenStdoutPath = join(expected, `${stem}.out`);
    const failure = existsSync(join(expected, `${stem}.err.out`));
    const goldenPath = join(expected, `${stem}.${failure ? "err" : "stderr"}.out`);
    if (![statusPath, stdoutPath, stderrPath].every(existsSync)) {
      failures.push(`${stem}: missing native result`);
      rows.push(`${stem}\tmissing`);
      continue;
    }
    const native = JSON.parse(readFileSync(statusPath, "utf8"));
    const stderr = readFileSync(stderrPath);
    const stdout = readFileSync(stdoutPath);
    const stdoutBytesMatch = stdout.equals(existsSync(goldenStdoutPath) ? readFileSync(goldenStdoutPath) : Buffer.alloc(0));
    const goldenStderr = readFileSync(goldenPath);
    const expectedExit = failure ? goldenFailureExit(stem, goldenStderr.toString("utf8")) : 0;
    const bytesMatch = stderr.equals(goldenStderr);
    const resultMatch = results.get(stem) === "pass";
    const exitMatch = expectedExit !== null && native.status === expectedExit && native.signal === null && native.error === null;
    const ok = resultMatch && bytesMatch && stdoutBytesMatch && exitMatch;
    rows.push(`${stem}\t${ok ? "pass" : "wrong"}\texpected=${expectedExit ?? "ambiguous"}\tnative=${native.status ?? native.signal}\tgolden-stdout=${stdoutBytesMatch}\tgolden-stderr=${bytesMatch}\tresult=${results.get(stem) ?? "missing"}`);
    if (!ok) failures.push(`${stem}: ${expectedExit === null ? "ambiguous failure exit report" : "exact exit/stdout/stderr mismatch"}`);
  }
  writeFileSync(join(out, "trap-results.tsv"), rows.join("\n") + "\n");
  for (const failure of failures) console.error(failure);
  if (harness?.error) console.error(harness.error.message);
  if (failures.length || (harness && harness.status !== 0)) process.exitCode = 1;
}
