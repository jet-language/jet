#!/usr/bin/env node
// Assemble the MIR Lint fixture unit (Tests/LintFixtures.jet) with the sources
// it needs, as one package-free Jet file.
//
// usage: node Compiler/JetOptimizer/Tests/run-lint-fixtures.mjs <outdir>
// then:  cd <outdir> && jet run unit.jet
// --canonical-order selects the stable canonical row-sort regression unit.
//
// The fixtures reuse the MIR builders and example programs of the lowering
// fixtures (Compiler/JetBackend/Tests/LowerFixtures.jet) without their
// lowering report and entry point. MIR.jet also names Foundation span, call
// registry and parameter-zone items whose files pull in the whole diagnostics
// registry; their exact definitions are copied out of those files.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const outDir = process.argv[2];
if (!outDir) {
  console.error("usage: run-lint-fixtures.mjs <outdir>");
  process.exit(64);
}
const read = (path) => readFileSync(`${repo}/${path}`, "utf8");
const canonicalOrder = process.argv[3] === "--canonical-order";
if (process.argv[3] && !canonicalOrder) throw new Error("unknown fixture selection");

// The top-level item starting at the line that begins with `head`, through its
// closing `}` at column 0 (or the line itself for a one-line item).
function item(path, head) {
  const lines = read(path).split("\n");
  const start = lines.findIndex((line) => line.startsWith(head));
  if (start < 0) throw new Error(`${path}: no item starting with ${JSON.stringify(head)}`);
  if (!lines[start].trimEnd().endsWith("{")) return lines[start];
  const end = lines.findIndex((line, index) => index > start && line === "}");
  return lines.slice(start, end + 1).join("\n");
}

// Units are single-package: drop `use jet_*` items, one line or a bracketed
// block, and the top-level items starting with any of `omit`.
function body(path, omit = []) {
  const kept = [];
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
    if (omit.some((head) => line.startsWith(head))) {
      if (line.trimEnd().endsWith("{")) skippingItem = true;
      continue;
    }
    kept.push(line);
  }
  return `// [unit source: ${path}]\n${kept.join("\n")}\n`;
}

const parts = [
  "// [unit source: Foundation items named by MIR.jet]",
  item("Compiler/JetFoundation/Source/Diagnostics/Diagnostic.jet", "pub struct Span {"),
  item("Compiler/JetFoundation/Source/Registry/CoreCalls.jet", "pub enum Effect "),
  ...[
    "pub enum SinkClass ",
    "pub enum CoreCallFallibility ",
    "pub enum CoreCallPureRoute {",
    "pub enum CoreCallInterpreterRoute ",
    "pub enum CoreCallSymbol ",
    "pub struct CoreMarkerApplication {",
  ].map((head) => item("Compiler/JetFoundation/Source/Registry/CoreCalls.jet", head)),
  item("Compiler/JetFoundation/Source/Types/Types.jet", "pub enum ParamZone {"),
  item("Compiler/JetFoundation/Source/Types/Types.jet", "pub fn param_zone_name("),
  ...[
    "pub struct ByteLayout {",
    "pub struct FieldLayoutFacts {",
    "pub struct LayoutFacts {",
  ].map((head) => item("Compiler/JetFoundation/Source/Target/Layout.jet", head)),
  "",
  body("Compiler/JetFoundation/Source/MIR/MIR.jet"),
  body("Compiler/JetOptimizer/Source/Verification/Lint.jet"),
  body("Compiler/JetBackend/Tests/LowerFixtures.jet", ["fn fx_report(", "fn fx_report_lir(", "fn fx_report_image(", "fn fx_report_executable(", "fn run("]),
  ...(canonicalOrder ? [
    item("Compiler/JetFoundation/Source/Collections.jet", "pub fn compiler_apply_order<T>("),
    item("Compiler/JetFoundation/Source/Collections.jet", "pub fn compiler_transfer_rows<T>("),
    item("Compiler/JetOptimizer/Source/Facts/CanonicalOrdering.jet", "fn mir_pass_sort_functions("),
    item("Compiler/JetOptimizer/Source/Facts/CanonicalOrdering.jet", "fn mir_pass_canonical_order("),
    item("Compiler/JetOptimizer/Source/Facts/CanonicalOrdering.jet", "fn mir_pass_order_before("),
    item("Compiler/JetOptimizer/Source/Facts/CanonicalOrdering.jet", "fn mir_pass_sort_type_instances("),
    item("Compiler/JetCodegen/Source/Codegen/Functions.jet", "fn jet_codegen_sort_functions("),
    item("Compiler/JetCodegen/Source/Codegen/Functions.jet", "pub fn jet_codegen_sorted_order("),
    item("Compiler/JetCodegen/Source/Codegen/Functions.jet", "fn jet_codegen_order_before("),
    body("Compiler/JetOptimizer/Tests/CanonicalOrderingFixtures.jet"),
  ] : [body("Compiler/JetOptimizer/Tests/LintFixtures.jet")]),
];
mkdirSync(outDir, { recursive: true });
writeFileSync(`${outDir}/unit.jet`, parts.join("\n"));
writeFileSync(`${outDir}/package.jet`, 'name: "jet_optimizer_lint_fixtures"\nversion: "0.1.0"\nauthority: { holds: { allow: [IO, Mem.Alloc] } }\n');
console.log(`${outDir}/unit.jet`);
