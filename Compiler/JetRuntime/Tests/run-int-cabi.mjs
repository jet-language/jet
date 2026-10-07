#!/usr/bin/env node
// L4a direct ABI parity, independent BigInt oracle, and exact stopped reports.
// The two libraries must come from the SAME source tree: Rust seed transport
// and native remaining layers. The native executable additionally links the
// Jet-authored runtime object; it cannot silently fall back to Rust numerics.
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [output, rustLibrary, nativeLibrary, jetObject, mode] = process.argv.slice(2);
if (!output || (mode !== "--corpus-only" && (!rustLibrary || !nativeLibrary || !jetObject)) || (mode && mode !== "--corpus-only")) {
  throw Error("usage: run-int-cabi.mjs OUT RUST.staticlib NATIVE.staticlib JetRuntime.o [--corpus-only]");
}
const out = resolve(output);
mkdirSync(out, { recursive: true });
const statements = [];
let count = 0;
const q = JSON.stringify;
const raw = (value) => `parse(${q(String(value))})`;
function witness(label, expression, expected) {
  statements.push(`    value(${q(label)}, ${expression}, ${q(String(expected))});`);
  count++;
}
const values = [0n, 1n, -1n, (1n << 62n) - 1n, -(1n << 62n), 1n << 62n, -(1n << 62n) - 1n,
  (1n << 63n) - 1n, -(1n << 63n), 1n << 64n, -(1n << 64n), 123456789012345678901234567890123456789n];
for (const [i, a] of values.entries()) {
  witness(`parse-${i}`, raw(a), a);
  witness(`clone-${i}`, `jet_rt_int_clone(${raw(a)})`, a);
  if (a >= -(1n << 63n) && a < (1n << 63n)) {
    const constant = a === -(1n << 63n) ? "INT64_MIN" : a < 0n ? `-INT64_C(${-a})` : `INT64_C(${a})`;
    witness(`from-i64-${i}`, `jet_int_owned_from_i64(${constant})`, a);
    statements.push(`    if (jet_int_owned_to_i64(${raw(a)}) != ${constant}) ++failures;`);
  }
  for (const [j, b] of values.entries()) {
    const binary = { add: a + b, sub: a - b, mul: a * b, bit_and: a & b, bit_or: a | b, bit_xor: a ^ b };
    for (const [name, expected] of Object.entries(binary)) witness(`${name}-${i}-${j}`, `jet_int_${name}(${raw(a)}, ${raw(b)})`, expected);
    witness(`compare-${i}-${j}`, `jet_int_compare(${raw(a)}, ${raw(b)})`, a < b ? -1 : a > b ? 1 : 0);
    if (b !== 0n) {
      const trunc = a / b, remainder = a % b;
      const floor = trunc - (remainder !== 0n && (a < 0n) !== (b < 0n) ? 1n : 0n);
      const modulo = a - floor * b;
      const euclid = trunc + (remainder < 0n ? b < 0n ? 1n : -1n : 0n);
      const rest = a - euclid * b;
      for (const [name, expected] of Object.entries({ div: trunc, rem: remainder, floor_div: floor, mod: modulo, div_euclid: euclid, rem_euclid: rest })) {
        witness(`${name}-${i}-${j}`, `jet_int_${name}(${raw(a)}, ${raw(b)}, NULL, 0, 0)`, expected);
      }
    }
  }
  for (const shift of [0n, 1n, 28n, 29n, 31n, 32n, 62n, 63n, 64n, 127n, 1024n]) {
    witness(`shl-${i}-${shift}`, `jet_int_shl(${raw(a)}, ${shift}, NULL, 0, 0)`, a << shift);
    witness(`shr-${i}-${shift}`, `jet_int_shr(${raw(a)}, ${shift}, NULL, 0, 0)`, a >> shift);
  }
  for (const exponent of [0n, 1n, 2n, 3n, 7n]) witness(`pow-${i}-${exponent}`, `jet_int_pow(${raw(a)}, ${exponent}, NULL, 0, 0)`, a ** exponent);
}
for (const [kind, bits, signed] of [[0, 8, true], [1, 16, true], [2, 32, true], [3, 64, true], [4, 8, false], [5, 16, false], [6, 32, false], [7, 64, false]]) {
  const bounds = signed ? [-(1n << BigInt(bits - 1)), (1n << BigInt(bits - 1)) - 1n] : [0n, (1n << BigInt(bits)) - 1n];
  for (const a of bounds) {
    const word = BigInt.asIntN(64, a);
    const constant = word === -(1n << 63n) ? "INT64_MIN" : word < 0n ? `-INT64_C(${-word})` : `INT64_C(${word})`;
    statements.push(`    word_value(${q(`fixed-${kind}-${a}`)}, jet_int_checked_fixed(${raw(a)}, ${kind}, NULL, 0, 0), ${constant});`);
    count++;
  }
}
const floatInputs = new Set(values.map(String));
for (const a of [(1n << 53n) - 1n, 1n << 53n, (1n << 53n) + 1n, 1n << 63n, ((1n << 53n) - 1n) << 971n, 1n << 1024n, 10n ** 450n]) {
  floatInputs.add(String(a)); floatInputs.add(String(-a));
}
// Every rounding position, both parity ties, just below/above ties, both signs.
for (let exponent = 53n; exponent <= 1023n; exponent++) {
  const unit = 1n << (exponent - 52n), half = unit >> 1n;
  for (const a of [(1n << exponent) + half - 1n, (1n << exponent) + half,
    (1n << exponent) + half + 1n, (1n << exponent) + unit + half]) {
    floatInputs.add(String(a)); floatInputs.add(String(-a));
  }
}
const bytes = new DataView(new ArrayBuffer(8));
for (const [index, text] of [...floatInputs].entries()) {
  bytes.setFloat64(0, Number(BigInt(text)), false);
  const bits = bytes.getBigUint64(0, false).toString(16).padStart(16, "0");
  statements.push(`    float_value("float-${index}", ${raw(text)}, UINT64_C(0x${bits}));`);
  count++;
}
// Clone retention checks the defined atomic refcount word, not merely value.
statements.push('    { int64_t x = parse("9223372036854775808"); IntNode *n = (IntNode *)(uintptr_t)((uint64_t)x & ((UINT64_C(1)<<62)-1)); uint64_t before = n->refs; int64_t cloned = jet_rt_int_clone(x); if (cloned != x || n->refs != before + 1) ++failures; jet_rt_int_drop(cloned); if (n->refs != before) ++failures; jet_rt_int_drop(x); }');
writeFileSync(join(out, "int-corpus.inc"), `static void int_corpus(void) {\n${statements.join("\n")}\n}\n`);
writeFileSync(join(out, "corpus.json"), JSON.stringify({ numericAndFloatCases: count, floatCases: floatInputs.size }, null, 2) + "\n");
if (mode !== "--corpus-only") {
  const failures = [];
  const outputs = new Map();
  for (const [tier, library, extra] of [["rust", rustLibrary, []], ["jet", nativeLibrary, [resolve(jetObject)]]]) {
    const binary = join(out, `${tier}-int-cabi`);
    const built = spawnSync(process.env.CC ?? "cc", ["-std=c11", "-O1", ...(tier === "jet" ? ["-DJET_INT_NATIVE_FIXTURE"] : []), "-I", out, join(root, "Compiler/JetRuntime/Tests/int-cabi.c"), ...extra, resolve(library), "-ldl", "-lpthread", "-lm", "-o", binary], { encoding: "utf8" });
    writeFileSync(join(out, `${tier}-build.log`), `${built.stdout ?? ""}${built.stderr ?? ""}`);
    if (built.error || built.status !== 0) { failures.push(`${tier}: link`); continue; }
    for (const name of ["values", "zero", "exponent", "shift-negative", "shift-wide", "fixed", "native"]) {
      const result = spawnSync(binary, name === "values" ? [] : [name], { encoding: "utf8", maxBuffer: 1 << 27, timeout: 600000 });
      writeFileSync(join(out, `${tier}-${name}.stdout`), result.stdout ?? "");
      writeFileSync(join(out, `${tier}-${name}.stderr`), result.stderr ?? "");
      const status = { status: result.status, signal: result.signal, error: result.error?.message ?? null };
      writeFileSync(join(out, `${tier}-${name}.status.json`), JSON.stringify(status) + "\n");
      outputs.set(`${tier}-${name}`, { ...status, stdout: result.stdout, stderr: result.stderr });
      if (result.error || result.signal || result.status !== (name === "values" ? 0 : 70)) failures.push(`${tier}: ${name}`);
    }
  }
  for (const name of ["values", "zero", "exponent", "shift-negative", "shift-wide", "fixed", "native"]) {
    if (JSON.stringify(outputs.get(`rust-${name}`)) !== JSON.stringify(outputs.get(`jet-${name}`)) || !outputs.has(`rust-${name}`)) failures.push(`parity: ${name}`);
  }
  const undefinedSymbols = spawnSync("nm", ["-u", resolve(jetObject)], { encoding: "utf8" });
  writeFileSync(join(out, "undefined-symbols.txt"), undefinedSymbols.stdout ?? "");
  const memorySeam = "jet_rt_int_retire_zero";
  const exactImports = (undefinedSymbols.stdout ?? "").match(/\b(?:jet_int_\w+|jet_rt_int_\w+)\b/gu) ?? [];
  if (undefinedSymbols.error || undefinedSymbols.status !== 0 || exactImports.some((name) => name !== memorySeam) || !exactImports.includes(memorySeam)) failures.push("Jet object has missing/unregistered exact Int memory seam or numeric fallback");
  const remaining = readFileSync(join(root, "Compiler/JetRuntime/remaining-int.list"), "utf8")
    .split("\n").filter((name) => name && !name.startsWith("#"));
  const demangledImports = spawnSync("nm", ["-C", "-u", resolve(jetObject)], { encoding: "utf8" });
  const remainingDefinitions = spawnSync("nm", ["-C", "--defined-only", resolve(nativeLibrary)], { encoding: "utf8", maxBuffer: 1 << 27 });
  writeFileSync(join(out, "undefined-symbols-demangled.txt"), demangledImports.stdout ?? "");
  const ownershipImport = (demangledImports.stdout ?? "").split("\n")
    .filter((line) => /(?:JetInt|jet_(?:rt_)?int_|JET_INT_HAZARD|JET_INT_RETIRED)/u.test(line));
  const ownershipDefinitions = (remainingDefinitions.stdout ?? "").split("\n")
    .filter((line) => /(?:JetInt|jet_(?:rt_)?int_|JET_INT_HAZARD|JET_INT_RETIRED)/u.test(line));
  writeFileSync(join(out, "remaining-int-ownership.json"), JSON.stringify({
    declaredRustSourceIdentities: remaining,
    observedRemainingRustDefinitions: ownershipDefinitions,
    observedJetObjectOwnershipImports: ownershipImport,
    nativeUserIntDrop: "Lower.jet: .Int -> LowerRepr.Word; lower_drop -> LIRDrop.Trivial",
    approvedCAbiMemorySeam: memorySeam,
    separateLeakItem: "L4a provides jet_rt_int_drop; NativeInteg2 owns wiring general native boxed-Int drop/copy-retain glue. int_discard only frees private refs1 workspaces.",
  }, null, 2) + "\n");
  if (demangledImports.error || demangledImports.status !== 0 || remainingDefinitions.error || remainingDefinitions.status !== 0 || ownershipImport.some((line) => !/\sjet_rt_int_retire_zero$/u.test(line))) {
    failures.push("unreported exact Int ownership import or ownership audit failure");
  }
  writeFileSync(join(out, "result.json"), JSON.stringify({ cases: count, stopCases: 6, concurrentHazardFixture: true, failures }, null, 2) + "\n");
  if (failures.length) throw Error(`exact Int ABI witness failed: ${failures.join(", ")}`);
  console.log(`exact Int ABI: ${count} numeric/float cases + 6 exact stopped reports, clone/drop and concurrent hazard fixture; Rust/Jet identical, only explicit shared-domain retirement import`);
}
