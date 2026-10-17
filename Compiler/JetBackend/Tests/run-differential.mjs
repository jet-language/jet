#!/usr/bin/env node
// Native-vs-Rust differential over a run-goldens.mjs output directory
// (#4619 criterion 3). run-goldens.mjs keeps both runs of every golden: the
// Rust path's reference (<out>/mir/<slug>.ref mode/status plus raw .stdout and
// .stderr) and the native run (<out>/run/<slug>.stdout, .status.json). It compares them
// with each other, not only with the expected stdout, so a native result that
// differs from the Rust path is its own verdict even when the golden is stale.
//
// Verdicts:
//   agree             native stdout and exit status equal the Rust path's, and
//                     both equal the golden.
//   agree-off-golden  native equals the Rust path, but both differ from the
//                     golden (a front-end or golden defect, not a backend one).
//   native-diverges   native ran and its stdout or exit status differs from
//                     the Rust path's.
//   native-absent     native produced no run (the run-goldens verdict, e.g.
//                     unsupported or lower-crash, is the detail).
//   rust-absent       no Rust-path run to compare with (no .ref, or the mir
//                     stage ran with JET_MIR_EMIT=1).
//   no-golden         the golden expects a compile error or has no stdout.
//
// Gate (exit 1 when any holds):
//   - any native-diverges;
//   - --require-native: a golden whose Rust path passes is not `agree`;
//   - --baseline <differential.tsv>: a stem `agree` in the baseline is not
//     `agree` now.
//
// usage: [JET_GOLDEN_FEATURES=<checkout>/Examples/features] (the corpus
//        run-goldens.mjs ran) node Compiler/JetBackend/Tests/run-differential.mjs
//        <outdir> [--case-list FILE] [--require-native] [--baseline FILE]
// Writes <outdir>/differential.tsv and <outdir>/differential-summary.txt.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const args = process.argv.slice(2);
const outDir = args.shift();
if (!outDir) {
  console.error("usage: run-differential.mjs <outdir> [--case-list FILE] [--require-native] [--baseline FILE]");
  process.exit(64);
}
const features = process.env.JET_GOLDEN_FEATURES ? resolve(process.env.JET_GOLDEN_FEATURES) : join(repo, "Examples/features");
let caseList = null;
let requireNative = false;
let baselinePath = null;
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--case-list") caseList = new Set(readFileSync(args[++i], "utf8").split("\n").map((stem) => stem.trim()).filter(Boolean));
  else if (args[i] === "--require-native") requireNative = true;
  else if (args[i] === "--baseline") baselinePath = args[++i];
  else throw new Error(`unknown option ${args[i]}`);
}

const resultsPath = join(outDir, "results.tsv");
if (!existsSync(resultsPath)) throw new Error(`${resultsPath} missing: run run-goldens.mjs through the run stage first`);
const rows = readFileSync(resultsPath, "utf8").split("\n").filter(Boolean).map((line) => {
  const [stem, verdict, ...detail] = line.split("\t");
  return { stem, verdict, detail: detail.join("\t") };
});
if (caseList) {
  const present = new Set(rows.map((row) => row.stem));
  for (const stem of caseList) if (!present.has(stem)) throw new Error(`case-list stem missing from results.tsv: ${stem}`);
}

// The same slug run-goldens.mjs gives each case.
const slugOf = (stem) => stem.replace(/[^A-Za-z0-9_-]+/gu, "_");
const readJson = (path) => (existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : null);


// The first line where two outputs differ, for the detail column.
function firstDifference(leftName, left, rightName, right) {
  const a = left.toString("utf8").split("\n");
  const b = right.toString("utf8").split("\n");
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if (a[i] !== b[i]) return `line ${i + 1}: ${leftName} ${JSON.stringify((a[i] ?? "<eof>").slice(0, 80))} ${rightName} ${JSON.stringify((b[i] ?? "<eof>").slice(0, 80))}`;
  }
  return left.equals(right) ? "identical" : "stdout bytes differ (identical UTF-8 display)";
}

const cases = [];
for (const row of rows) {
  if (caseList && !caseList.has(row.stem)) continue;
  const slug = slugOf(row.stem);
  const c = { stem: row.stem, harness: row.verdict, rustPass: false, verdict: "", detail: "" };
  cases.push(c);
  if (row.verdict === "no-golden") {
    c.verdict = "no-golden";
    continue;
  }
  const ref = readJson(join(outDir, "mir", `${slug}.ref`));
  const refStdoutPath = join(outDir, "mir", `${slug}.stdout`);
  const refStderrPath = join(outDir, "mir", `${slug}.stderr`);
  if (!ref || ref.mode !== "run" || !existsSync(refStdoutPath) || !existsSync(refStderrPath)) {
    c.verdict = "rust-absent";
    c.detail = !ref ? "no reference run" : ref.mode !== "run" ? `reference mode ${ref.mode}` : "missing raw reference streams";
    continue;
  }
  const expected = readFileSync(join(features, "expected", `${row.stem}.out`));
  const refStdout = readFileSync(refStdoutPath);
  c.rustPass = ref.status === 0 && refStdout.equals(expected);
  const stdoutPath = join(outDir, "run", `${slug}.stdout`);
  const native = readJson(join(outDir, "run", `${slug}.status.json`));
  if ((row.verdict !== "pass" && row.verdict !== "wrong") || !existsSync(stdoutPath) || !native) {
    c.verdict = "native-absent";
    c.detail = `${row.verdict}${row.detail ? `: ${row.detail}` : ""}`;
    continue;
  }
  const stdout = readFileSync(stdoutPath);
  const status = native.status ?? `signal ${native.signal}`;
  if (stdout.equals(refStdout) && status === ref.status) {
    c.verdict = c.rustPass ? "agree" : "agree-off-golden";
    if (!c.rustPass) c.detail = ref.status !== 0 ? `both exit ${ref.status}` : firstDifference("golden", expected, "both", stdout);
    continue;
  }
  c.verdict = "native-diverges";
  const parts = [];
  if (status !== ref.status) {
    const stderrPath = join(outDir, "run", `${slug}.stderr`);
    const stderr = existsSync(stderrPath) ? readFileSync(stderrPath, "utf8") : "";
    parts.push(`exit rust ${ref.status} native ${status}${stderr ? ` (${stderr.split("\n")[0].slice(0, 100)})` : ""}`);
  }
  if (!stdout.equals(refStdout)) parts.push(firstDifference("rust", refStdout, "native", stdout));
  c.detail = parts.join("; ");
}

const counts = new Map();
for (const c of cases) counts.set(c.verdict, (counts.get(c.verdict) ?? 0) + 1);
writeFileSync(join(outDir, "differential.tsv"), cases.map((c) => `${c.stem}\t${c.verdict}\t${c.rustPass ? "rust-pass" : "rust-fail"}\t${c.detail}`).join("\n") + "\n");

const failures = [];
for (const c of cases.filter((c) => c.verdict === "native-diverges")) failures.push(`native-diverges\t${c.stem}\t${c.detail}`);
if (requireNative) {
  for (const c of cases.filter((c) => c.rustPass && c.verdict !== "agree" && c.verdict !== "native-diverges")) failures.push(`rust-pass-not-native\t${c.stem}\t${c.verdict}: ${c.detail}`);
}
if (baselinePath) {
  const now = new Map(cases.map((c) => [c.stem, c.verdict]));
  for (const line of readFileSync(baselinePath, "utf8").split("\n").filter(Boolean)) {
    const [stem, verdict] = line.split("\t");
    if (verdict !== "agree" || !now.has(stem) || now.get(stem) === "agree" || now.get(stem) === "native-diverges") continue;
    failures.push(`regressed\t${stem}\tbaseline agree, now ${now.get(stem)}`);
  }
}

const order = ["agree", "agree-off-golden", "native-diverges", "native-absent", "rust-absent", "no-golden"];
const summary = [
  `cases: ${cases.length}`,
  ...order.filter((verdict) => counts.has(verdict)).map((verdict) => `${verdict}: ${counts.get(verdict)}`),
  `rust-path passes: ${cases.filter((c) => c.rustPass).length}`,
  `rust-path passes agreed natively: ${cases.filter((c) => c.rustPass && c.verdict === "agree").length}`,
  "",
  `gate: ${failures.length ? `FAIL (${failures.length})` : "PASS"}`,
  ...failures,
];
writeFileSync(join(outDir, "differential-summary.txt"), summary.join("\n") + "\n");
console.log(summary.join("\n"));
process.exit(failures.length ? 1 : 0);
