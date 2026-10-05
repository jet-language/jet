#!/usr/bin/env node
import { readdirSync } from "node:fs";
// Report for a goldens.sh run: OUTDIR/results.tsv (one row per case) and
// OUTDIR/report.md (totals, failures grouped by cause, generated-compiler-only
// versus shared-with-reference, suspicious batch rows, timings).
// usage: goldens-report.mjs OUTDIR
//        goldens-report.mjs --compare OUTDIR_A OUTDIR_B   (per-case verdict differences,
//        e.g. a --batch on run against a --batch off run of the same list)
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";

const slugOf = (stem) => stem.replace(/[^A-Za-z0-9_-]+/gu, "_");
const readKv = (path) => {
  if (!existsSync(path)) return null;
  const kv = {};
  for (const line of readFileSync(path, "utf8").split("\n")) {
    const at = line.indexOf("=");
    if (at > 0) kv[line.slice(0, at)] = line.slice(at + 1);
  }
  return kv;
};

// One cause line: drop what differs between occurrences of the same defect
// (thread ids, absolute paths, line:column, counts, hashes). A harness panic
// wrapping a host error (`Runner failed: Wrapper { error: Variant("text"), .. }`)
// is reduced to its outer message, innermost variant and text, so the
// boilerplate never crowds the defect out of the cause.
function normalize(message) {
  let text = String(message ?? "");
  const inner = /^([^:{]*):.*?(\w+)\("((?:[^"\\]|\\.){8,})"/su.exec(text);
  if (inner) text = `${inner[1]}: ${inner[2]}: ${inner[3]}`;
  return text
    .replace(/thread '([^']*)' \(\d+\)/gu, "thread '$1'")
    .replace(/(?:\/[\w.@+-]+){2,}/gu, "<path>")
    .replace(/\b[0-9a-f]{12,}\b/gu, "<hex>")
    .replace(/(?<![A-Z]|exit |status |expected |or )\b\d+\b/gu, "N")
    .replace(/\s+/gu, " ")
    .trim()
    .slice(0, 220);
}

// Where a generated-compiler run stopped, from how it ended and its message.
function compileStage(verdict, message) {
  if (verdict === "incomplete") return "diagnostics";
  if (verdict === "no-receipt") return "no-receipt";
  const text = String(message ?? "");
  const rules = [
    [/^startup/u, "startup"],
    [/TIMEOUT/u, "timeout"],
    [/KILLED|SIGKILL/u, "killed(memory)"],
    [/overflowed its stack|STACK OVERFLOW/u, "stack-overflow"],
    [/E3012|FRAME LIMIT/u, "jet-frame-limit"],
    [/image restore|compiler image/iu, "image-restore"],
    [/authorized bootstrap source/u, "source-load"],
    [/\blex/iu, "lex"],
    [/\bpars(e|er|ing)\b/iu, "parse"],
    [/semantic|\bsema\b|declaration checking|resolve/iu, "sema"],
    [/comptime|prep\b/iu, "comptime"],
    [/lower/iu, "lower"],
    [/optimi[sz]/iu, "optimize"],
    [/emit|MIRRust|codegen/iu, "emit"],
    [/codec/iu, "codec"],
    [/retire|completion|callback jobs/iu, "runner-retirement"],
  ];
  for (const [pattern, stage] of rules) if (pattern.test(text)) return stage;
  return verdict === "panic" ? "panic" : verdict;
}

function loadRun(dir) {
  const casesPath = join(dir, "cases.tsv");
  if (!existsSync(casesPath)) throw new Error(`${casesPath} is missing`);
  const env = readKv(join(dir, "run.env")) ?? {};
  const rows = [];
  for (const line of readFileSync(casesPath, "utf8").split("\n")) {
    if (!line) continue;
    const [stem, , , expect, , expectedBase] = line.split("\t");
    const slug = slugOf(stem);
    const caseDir = join(dir, "cases", slug);
    const compile = readKv(join(caseDir, "compile.result"));
    const build = readKv(join(caseDir, "build.result"));
    const run = readKv(join(caseDir, "run.result"));
    const refBuild = readKv(join(dir, "ref", slug, "build.result"));
    const refRun = readKv(join(dir, "ref", slug, "run.result"));
    let verdict;
    let stage = "";
    let cause = "";
    let detail = "";
    // A compile-time diagnostic golden (.err.out) passes in a generated
    // compiler when its receipt reports exactly the expected error codes (the
    // receipt carries reports, not the rendered text).
    const expectedCodes = expect === "err" && expectedBase && existsSync(`${expectedBase}.err.out`)
      ? [...new Set(readFileSync(`${expectedBase}.err.out`, "utf8").match(/\[E\d+\]/gu) ?? [])].map((code) => code.slice(1, -1)).sort().join(" ")
      : "";
    if (!compile) {
      verdict = "NOT-RUN";
    } else if (compile.verdict === "incomplete" && expectedCodes && compile.codes === expectedCodes) {
      verdict = "PASS-DIAG";
    } else if (compile.verdict !== "ok") {
      verdict = "FAIL-COMPILE";
      stage = compileStage(compile.verdict, compile.message);
      cause = normalize(compile.message);
      detail = `cases/${slug}/compile.log`;
    } else if (!build) {
      verdict = "COMPILED";
    } else if (build.verdict !== "ok") {
      verdict = "FAIL-BUILD";
      stage = "rustc";
      cause = normalize(build.message);
      detail = `cases/${slug}/build.log`;
    } else if (!run) {
      verdict = "BUILT";
    } else if (run.verdict === "fail") {
      verdict = "FAIL-RUN";
      stage = "run";
      cause = normalize(run.message);
      detail = `cases/${slug}/diff.txt`;
    } else {
      verdict = run.verdict === "built" ? "BUILT" : "PASS";
    }
    let ref = "-";
    if (refRun) ref = refRun.verdict === "pass" ? "PASS" : refRun.verdict === "built" ? "BUILT" : refRun.verdict === "fail-build" ? "FAIL-BUILD" : "FAIL-RUN";
    rows.push({
      stem, slug, expect, verdict, stage, cause, detail, ref,
      refMessage: refRun?.message ?? refBuild?.message ?? "",
      compileSecs: compile?.secs ? Number(compile.secs) : null,
      buildSecs: build?.secs ? Number(build.secs) : null,
      refSecs: refBuild?.secs ? Number(refBuild.secs) : null,
      mode: compile?.mode ?? "",
      batchVerdict: compile?.batch_verdict ?? "",
      suspicious: compile?.suspicious ?? "",
      compileVerdict: compile?.verdict ?? "",
    });
  }
  return { env, rows };
}

const failed = (verdict) => verdict.startsWith("FAIL") || verdict === "NOT-RUN";
const count = (rows, pick) => {
  const counts = new Map();
  for (const row of rows) counts.set(pick(row), (counts.get(pick(row)) ?? 0) + 1);
  return [...counts].sort((a, b) => b[1] - a[1]).map(([key, n]) => `${key} ${n}`).join(", ");
};
const stats = (values) => {
  const list = values.filter((value) => Number.isFinite(value)).sort((a, b) => a - b);
  if (!list.length) return "n/a";
  const sum = list.reduce((a, b) => a + b, 0);
  return `n=${list.length} total=${sum}s median=${list[Math.floor(list.length / 2)]}s max=${list.at(-1)}s`;
};

function report(dir) {
  const { env, rows } = loadRun(dir);
  writeFileSync(join(dir, "results.tsv"), [
    "stem\tjetc\tstage\tcause\treference\tcompile_secs\tmode\tdetail",
    ...rows.map((row) => [row.stem, row.verdict, row.stage || "-", row.cause || "-", row.ref, row.compileSecs ?? "-", row.mode || "-", row.detail || "-"].join("\t")),
  ].join("\n") + "\n");

  const refRan = rows.some((row) => row.ref !== "-");
  const fails = rows.filter((row) => failed(row.verdict));
  const jetcOnly = fails.filter((row) => row.ref === "PASS" || row.ref === "BUILT");
  const shared = fails.filter((row) => row.ref.startsWith("FAIL"));
  const jetcBetter = rows.filter((row) => !failed(row.verdict) && row.ref.startsWith("FAIL"));
  const out = [];
  out.push(`# goldens: ${basename(env.list ?? dir)} through ${env.compiler ?? "?"}`);
  out.push("");
  out.push(`cases ${rows.length}; compiler batch mode ${env.batch ?? "?"}; reference ${env.ref ?? "?"} (${env.ref_jet ?? "-"}); started ${env.started ?? "?"}, finished ${env.finished ?? "(running or interrupted)"}`);
  out.push("");
  out.push("## Totals");
  out.push(`- generated compiler: ${count(rows, (row) => row.verdict)}`);
  out.push(`- compile verdicts: ${count(rows, (row) => row.compileVerdict || "not-run")}`);
  out.push(`- stop stages of failures: ${fails.length ? count(fails, (row) => `${row.verdict}/${row.stage || "-"}`) : "none"}`);
  if (refRan) {
    out.push(`- reference: ${count(rows, (row) => row.ref)}`);
    out.push(`- failures only the generated compiler has (reference passes): ${jetcOnly.length}; shared with the reference: ${shared.length}; reference-only failures: ${jetcBetter.length}`);
  }
  out.push("");

  const groups = new Map();
  for (const row of fails) {
    const key = `${row.verdict} [${row.stage || "-"}] ${row.cause || "(no message)"}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(row);
  }
  out.push(`## Failure groups (${groups.size} causes, largest first)`);
  for (const [key, members] of [...groups].sort((a, b) => b[1].length - a[1].length)) {
    const only = members.filter((row) => row.ref === "PASS" || row.ref === "BUILT").length;
    out.push("");
    out.push(`### ${members.length} × ${key}`);
    if (refRan) out.push(`generated-compiler-only: ${only}/${members.length}`);
    for (const row of members.slice(0, 25)) {
      out.push(`- ${row.stem} (ref ${row.ref}${row.mode && row.mode !== "fresh" ? `, ${row.mode}` : ""}${row.batchVerdict ? `, batch verdict ${row.batchVerdict}` : ""}) ${row.detail}`);
    }
    if (members.length > 25) out.push(`- … ${members.length - 25} more (results.tsv)`);
  }
  if (refRan && jetcBetter.length) {
    out.push("");
    out.push("## Reference-only failures (generated compiler passes, reference fails)");
    for (const row of jetcBetter) out.push(`- ${row.stem}: ref ${row.ref} ${normalize(row.refMessage)}`);
  }
  if (refRan) {
    const refFails = rows.filter((row) => row.ref.startsWith("FAIL"));
    out.push("");
    out.push(`## Reference failures by cause (${refFails.length})`);
    const refGroups = new Map();
    for (const row of refFails) {
      const key = `${row.ref} ${normalize(row.refMessage) || "(no message)"}`;
      refGroups.set(key, [...(refGroups.get(key) ?? []), row.stem]);
    }
    for (const [key, stems] of [...refGroups].sort((a, b) => b[1].length - a[1].length)) {
      out.push(`- ${stems.length} × ${key}: ${stems.slice(0, 8).join(", ")}${stems.length > 8 ? ", …" : ""}`);
    }
  }
  const reruns = rows.filter((row) => row.batchVerdict || row.suspicious);
  if (reruns.length) {
    out.push("");
    out.push("## Suspicious batch rows (rerun in a fresh process)");
    for (const row of reruns) {
      out.push(`- ${row.stem}: batch ${row.batchVerdict || "?"} -> fresh ${row.compileVerdict || "?"}${row.suspicious ? ` (${row.suspicious})` : ""}${row.batchVerdict && row.batchVerdict !== row.compileVerdict ? "  STATE LEAK CANDIDATE" : ""}`);
    }
  }
  out.push("");
  out.push("## Timing");
  const jetcDir = join(dir, "jetc");
  const startups = existsSync(jetcDir)
    ? readdirSync(jetcDir).filter((name) => name.endsWith(".startup_secs")).sort()
      .map((name) => `${name.replace(".startup_secs", "")} ${readFileSync(join(jetcDir, name), "utf8").trim()}s`)
    : [];
  if (startups.length) out.push(`- compiler startup to first case (batch process): ${startups.join(", ")}`);
  out.push(`- generated compiler per case: ${stats(rows.map((row) => row.compileSecs))}`);
  out.push(`- backend build per case: ${stats(rows.map((row) => row.buildSecs))}`);
  out.push(`- reference build per case (uncached): ${stats(rows.map((row) => row.refSecs))}`);
  out.push("");
  writeFileSync(join(dir, "report.md"), out.join("\n"));
  const summary = out.slice(0, out.indexOf(`## Failure groups (${groups.size} causes, largest first)`));
  console.log(summary.join("\n"));
  console.log(`groups: ${groups.size}; full report: ${join(dir, "report.md")}`);
  return fails.length ? 1 : 0;
}

function compare(dirA, dirB) {
  const a = new Map(loadRun(dirA).rows.map((row) => [row.stem, row]));
  const b = new Map(loadRun(dirB).rows.map((row) => [row.stem, row]));
  let same = 0;
  const differ = [];
  for (const [stem, rowA] of a) {
    const rowB = b.get(stem);
    if (!rowB) continue;
    const left = `${rowA.verdict} ${rowA.stage} ${rowA.cause}`;
    const right = `${rowB.verdict} ${rowB.stage} ${rowB.cause}`;
    if (left === right) same += 1;
    else differ.push(`- ${stem}\n    A: ${left}\n    B: ${right}`);
  }
  console.log(`compared ${same + differ.length} common case(s): ${same} identical, ${differ.length} different`);
  if (differ.length) console.log(differ.join("\n"));
  return differ.length ? 1 : 0;
}

const args = process.argv.slice(2);
if (args[0] === "--compare" && args.length === 3) process.exit(compare(args[1], args[2]));
if (args.length !== 1) {
  console.error("usage: goldens-report.mjs OUTDIR | --compare OUTDIR_A OUTDIR_B");
  process.exit(64);
}
process.exit(report(args[0]));
