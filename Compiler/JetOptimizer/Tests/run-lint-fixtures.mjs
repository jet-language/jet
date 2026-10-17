#!/usr/bin/env node
// Assemble the MIR Lint fixture unit (Tests/LintFixtures.jet) with the sources
// it needs, as one package-free Jet file.
//
// usage: node Compiler/JetOptimizer/Tests/run-lint-fixtures.mjs <outdir> [--canonical-order | --check] [checker]
// then:  cd <outdir> && jet run unit.jet
// --canonical-order selects the stable canonical row-sort regression unit.
// --check selects the one-MIR-checker unit (Tests/CheckFixtures.jet): the
// legality and lint rule sources with the optimizer's check part and
// keep-going scan.
// With a checker (a `jet` binary) it also runs the unit and writes
// <outdir>/receipt.json binding the fixture count to the read sources.
//
// The fixtures reuse the MIR builders and example programs of the lowering
// fixtures (Compiler/JetBackend/Tests/LowerFixtures.jet) without their
// lowering report and entry point. MIR.jet also names Foundation span, call
// registry and parameter-zone items whose files pull in the whole diagnostics
// registry; their exact definitions are copied out of those files.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const outDir = process.argv[2];
const options = process.argv.slice(3);
const canonicalOrder = options.includes("--canonical-order");
const check = options.includes("--check");
const positional = options.filter((option) => option !== "--canonical-order" && option !== "--check");
if (!outDir || (canonicalOrder && check) || positional.length > 1 || positional.some((option) => option.startsWith("--"))) {
  console.error("usage: run-lint-fixtures.mjs <outdir> [--canonical-order | --check] [checker]");
  process.exit(64);
}
const checker = positional[0];
// Every source the unit reads, with its content hash, for the checker receipt.
const sources = new Map();
const read = (path) => {
  const text = readFileSync(`${repo}/${path}`, "utf8");
  sources.set(path, createHash("sha256").update(text).digest("hex"));
  return text;
};

// The top-level item starting at the line that begins with `head`, through its
// closing `}` at column 0 (or the line itself for a one-line item). A
// signature that wraps (its first line ends in `(`) runs to that `}` too.
function item(path, head) {
  const lines = read(path).split("\n");
  const start = lines.findIndex((line) => line.startsWith(head));
  if (start < 0) throw new Error(`${path}: no item starting with ${JSON.stringify(head)}`);
  if (!/[{(]$/.test(lines[start].trimEnd())) return lines[start];
  const end = lines.findIndex((line, index) => index > start && line === "}");
  return lines.slice(start, end + 1).join("\n");
}

// Units are single-package: drop `use jet_*` items, one line or a bracketed
// block, and the top-level items starting with any of `omit`. Every `omit`
// head must still name an item, so a renamed or removed item cannot leave a
// stale list behind.
function body(path, omit = []) {
  const kept = [];
  const omitted = new Set();
  let skippingUse = false;
  let skippingItem = false;
  for (const line of read(path).split("\n")) {
    if (skippingUse) {
      if (line.startsWith("]")) skippingUse = false;
      continue;
    }
    if (skippingItem) {
      if (line === "}") skippingItem = false;
      continue;
    }
    if (/^use jet_\w+\.\[\s*$/.test(line)) {
      skippingUse = true;
      continue;
    }
    if (/^use jet_\w+/.test(line)) continue;
    const head = omit.find((prefix) => line.startsWith(prefix));
    if (head) {
      omitted.add(head);
      if (line.trimEnd().endsWith("{")) skippingItem = true;
      continue;
    }
    kept.push(line);
  }
  const stale = omit.filter((head) => !omitted.has(head));
  if (stale.length) throw new Error(`${path}: no item starting with ${stale.map((head) => JSON.stringify(head)).join(", ")}`);
  return `// [unit source: ${path}]\n${kept.join("\n")}\n`;
}

const parts = [
  "// [unit source: Foundation items named by MIR.jet]",
  item("Compiler/JetFoundation/Source/Diagnostics/Diagnostic.jet", "struct Span {"),
  item("Compiler/JetFoundation/Source/Registry/CoreCalls.jet", "enum Effect "),
  ...[
    "enum SinkClass ",
    "enum CoreCallFallibility ",
    "enum CoreCallPureRoute {",
    "enum CoreCallInterpreterRoute ",
    "enum CoreCallSymbol ",
    "struct CoreMarkerApplication {",
  ].map((head) => item("Compiler/JetFoundation/Source/Registry/CoreCalls.jet", head)),
  item("Compiler/JetFoundation/Source/Types/Types.jet", "enum ParamZone {"),
  item("Compiler/JetFoundation/Source/Types/Types.jet", "fn param_zone_name("),
  item("Compiler/JetFoundation/Source/Registry/CoreModules.jet", "fn core_source_value_hook("),
  // The Rust emitter behind `jet run` needs a declaration row for every nominal
  // type the unit names (as in run-lower-fixtures.mjs).
  item("Compiler/JetFoundation/Source/Types/Types.jet", "struct SymbolID {"),
  ...[
    "struct ByteLayout {",
    "struct FieldLayoutFacts {",
    "struct LayoutFacts {",
  ].map((head) => item("Compiler/JetFoundation/Source/Target/Layout.jet", head)),
  "",
  body("Compiler/JetFoundation/Source/DerivationTypes.jet"),
  // mir_typed_head_kind is a Sema-only projection over the semantic Type graph
  // (as in run-lower-fixtures.mjs); the MIR fixtures never call it.
  body("Compiler/JetFoundation/Source/MIR/MIR.jet", ["fn mir_typed_head_kind("]),
  body("Compiler/JetOptimizer/Source/Verification/Lint.jet"),
  // Only the MIR builders and example programs: these items lower, link or
  // report native images, or name lowering-only fixtures (Tests/Fixtures).
  body("Compiler/JetBackend/Tests/LowerFixtures.jet", [
    "fn fx_generic_runtime_rejection_checks(",
    "fn fx_report(",
    "fn fx_report_runtime(",
    "fn fx_report_lir(",
    "fn fx_report_image(",
    "fn fx_report_executable(",
    "fn run(",
    "fn fx_lazy_clone_case(",
    "fn fx_lazy_clone_checks(",
    "fn fx_nested_size_case(",
    "fn fx_nested_size_checks(",
  ]),
  ...(canonicalOrder ? [
    item("Compiler/JetFoundation/Source/Collections.jet", "fn compiler_apply_order<T>("),
    item("Compiler/JetFoundation/Source/Collections.jet", "fn compiler_transfer_rows<T>("),
    item("Compiler/JetOptimizer/Source/Facts/CanonicalOrdering.jet", "fn mir_pass_canonical_order("),
    item("Compiler/JetOptimizer/Source/Facts/CanonicalOrdering.jet", "fn mir_pass_order_before("),
    item("Compiler/JetOptimizer/Source/Facts/CanonicalOrdering.jet", "fn mir_pass_sort_functions("),
    item("Compiler/JetOptimizer/Source/Facts/CanonicalOrdering.jet", "fn mir_pass_sort_type_instances("),
    item("Compiler/JetCodegen/Source/Codegen/Functions.jet", "fn jet_codegen_sort_functions("),
    item("Compiler/JetCodegen/Source/Codegen/Functions.jet", "fn jet_codegen_sorted_order("),
    item("Compiler/JetCodegen/Source/Codegen/Functions.jet", "fn jet_codegen_order_before("),
    body("Compiler/JetOptimizer/Tests/CanonicalOrderingFixtures.jet"),
  ] : check ? [
    item("Compiler/JetFoundation/Source/MIR/CanonicalPass.jet", "fn canonical_pass_mir_terminator("),
    item("Compiler/JetFoundation/Source/MIR/CanonicalPass.jet", "fn canonical_pass_mir_operation_kind("),
    body("Compiler/JetOptimizer/Source/Analysis/ControlFlow.jet"),
    body("Compiler/JetOptimizer/Source/Verification/Legality.jet"),
    ...[
      "enum MIROptimizationErrorKind {",
      "struct MIROptimizationError {",
      "fn mir_pass_error(",
      "fn mir_pass_name(",
      "struct MIRPassCheckInputs {",
      "fn mir_pass_check_inputs(",
      "fn mir_pass_check_owned_part(",
      "fn mir_pass_lint_error(",
      "fn mir_optimizer_function_failures(",
    ].map((head) => item("Compiler/JetOptimizer/Source/Pipeline.jet", head)),
    body("Compiler/JetOptimizer/Tests/CheckFixtures.jet"),
  ] : [body("Compiler/JetOptimizer/Tests/LintFixtures.jet")]),
];
mkdirSync(outDir, { recursive: true });
writeFileSync(`${outDir}/unit.jet`, parts.join("\n"));
writeFileSync(`${outDir}/package.jet`, 'name: "jet_optimizer_lint_fixtures"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n');
console.log(`${outDir}/unit.jet`);
if (checker) {
  const binary = resolve(checker);
  read("Compiler/JetOptimizer/Tests/run-lint-fixtures.mjs");
  const expected = [...parts.at(-1).matchAll(check ? /\bcf_expect\("/g : /\blf_expect\("/g)].length;
  const head = execFileSync("git", ["rev-parse", "HEAD"], { cwd: repo, encoding: "utf8" }).trim();
  const clean = spawnSync("git", ["diff", "--quiet", "HEAD", "--", ...sources.keys()], { cwd: repo });
  if (clean.error || ![0, 1].includes(clean.status)) throw clean.error ?? new Error("cannot bind fixture sources to HEAD");
  const run = spawnSync(binary, ["run", resolve(outDir, "unit.jet")], {
    cwd: resolve(outDir), encoding: "utf8", timeout: 180_000, maxBuffer: 16 * 1024 * 1024,
  });
  writeFileSync(`${outDir}/stdout.log`, run.stdout ?? "");
  writeFileSync(`${outDir}/stderr.log`, run.stderr ?? "");
  const summary = /(\d+) passed, (\d+) failed/.exec(run.stdout ?? "");
  const failures = [];
  if (run.error) failures.push(run.error.message);
  if (run.status !== 0) failures.push(`checker status ${run.status}, signal ${run.signal}`);
  if (!summary || Number(summary[1]) !== expected || Number(summary[2]) !== 0 || !run.stdout.includes(check ? "all check fixtures passed" : "all lint fixtures passed")) {
    failures.push(`fixture summary does not prove all ${expected} controls passed`);
  }
  const receipt = {
    head: clean.status === 0 ? head : null, source_head: head, source_dirty: clean.status !== 0,
    coverage: "MIR alias.exclusive/view.valid/send.boundary mutation controls", control_kind: "hand-built-mir",
    checker: binary, checker_sha256: createHash("sha256").update(readFileSync(binary)).digest("hex"),
    sources: Object.fromEntries(sources), checked_cases: summary ? Number(summary[1]) : 0,
    expected_cases: expected, verdict: failures.length ? "FAIL" : "PASS", failures,
    status: run.status, signal: run.signal,
  };
  writeFileSync(`${outDir}/receipt.json`, `${JSON.stringify(receipt, null, 2)}\n`);
  process.stdout.write(run.stdout ?? "");
  process.stderr.write(run.stderr ?? "");
  process.exitCode = failures.length ? (run.error ? 2 : 1) : 0;
}
