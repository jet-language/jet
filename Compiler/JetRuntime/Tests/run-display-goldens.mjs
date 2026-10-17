#!/usr/bin/env node
// Native display proof against frozen oracle bytes; no live Rust reference.
// Prepare: node run-display-goldens.mjs OUT --prepare [--baseline RESULTS.tsv]
// Prove: node run-display-goldens.mjs OUT --object JET.o --pack JET_ONLY.o --provenance RECEIPT [--frozen RECEIPT_DIR] [--results fresh-results.tsv]
// The object/pack must include --test FloatBits. All selected native goldens
// and the 1M-float native formatting cell remain live.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {createHash} from "node:crypto";
import { collectGoldenEntries } from "../../../Tools/agent/compiler-diff.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
import {readGoldenResults} from "./golden-results.mjs";
const args = process.argv.slice(2);
if (args.includes("--reference")) {
  console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
  process.exit(0);
}
const out = resolve(args.shift() ?? "");
let prepare = false, baseline, object, pack, frozen, sharedResults, provenance;
for (let i = 0; i < args.length; ++i) {
  if (args[i] === "--prepare") prepare = true;
  else if (args[i] === "--baseline") baseline = resolve(args[++i]);
  else if (args[i] === "--object") object = resolve(args[++i]);
  else if (args[i] === "--pack") pack = resolve(args[++i]);
  else if (args[i] === "--frozen") frozen = resolve(args[++i]);
  else if (args[i] === "--results") sharedResults = resolve(args[++i]);
  else if (args[i] === "--provenance") provenance = resolve(args[++i]);
  else throw new Error(`unknown argument ${args[i]}`);
}
if (process.argv.length < 4 || (!prepare && (!object || !pack || !provenance))) throw new Error("usage: run-display-goldens.mjs OUT --prepare [--baseline RESULTS.tsv] | --object JET.o --pack JET_ONLY.o --provenance RECEIPT [--frozen RECEIPT_DIR] [--results fresh-results.tsv]");
mkdirSync(out, { recursive: true });
const exports = ["jet_rt_bool_to_string", "jet_rt_char_to_string", "jet_rt_i64_to_string", "jet_rt_u64_to_string", "jet_rt_int_to_string", "jet_rt_float_to_string", "jet_rt_float_debug", "jet_rt_float32_to_string", "jet_rt_float32_debug"];
const mask = (1n << 64n) - 1n, sign = 1n << 63n, fraction = (1n << 52n) - 1n;
const benchmarkBits = [0n, sign, 0x3ff0000000000000n, 0xbff0000000000000n, 0x3fb999999999999an, 0xbfb999999999999an, 1n, sign | 1n, 0x0010000000000000n, 0x7fefffffffffffffn, 0x3ee4f8b588e368f1n, 0x4341c37937e08000n, 0x7ff0000000000000n, 0xfff0000000000000n, 0x7ff8000000000042n, 0xfff8000000000042n];
const floatWord = (value) => { const buffer = Buffer.alloc(8); buffer.writeDoubleLE(value); return buffer.readBigUInt64LE(); };
if (prepare) {
  const corpus = new Set();
  const add = (bits) => { if (bits >= 0n && bits <= mask) corpus.add(bits.toString(16).padStart(16, "0")); };
  const neighbors = (bits, width = 4n) => { for (let i = -width; i <= width; ++i) { add(bits + i); add((bits + i) ^ sign); } };
  // Every finite binary exponent, mantissa boundaries/midpoints, both signs.
  for (let exponent = 0n; exponent < 2047n; ++exponent) {
    for (const mantissa of [0n, 1n, 2n, (1n << 51n) - 1n, 1n << 51n, (1n << 51n) + 1n, fraction - 1n, fraction]) {
      const bits = (exponent << 52n) | mantissa;
      add(bits); add(bits ^ sign);
    }
  }
  // Decimal powers and threshold neighborhoods, integral .0, signed zeros.
  for (let exponent = -324; exponent <= 308; ++exponent) neighbors(floatWord(Number(`1e${exponent}`)));
  for (let value = 0; value <= 1000; ++value) neighbors(floatWord(value), 1n);
  for (const value of [0, 1e-4, 1e-5, 1e16, 1e15, 1e21, 1e-7, 2 ** 53, Number.MIN_VALUE, Number.MAX_VALUE, 2.2250738585072014e-308, Infinity]) neighbors(floatWord(value), 16n);
  for (const bits of [0x7ff0000000000001n, 0x7ff8000000000000n, 0x7ff8000000000042n, 0x7fffffffffffffffn]) { add(bits); add(bits ^ sign); }
  // Halfway decimal ties and representable integer transitions near 2^53.
  for (const value of [0.5, 1.5, 2.5, 0.125, 0.375, 1.25, 1.75]) neighbors(floatWord(value), 16n);
  for (let offset = -32; offset <= 32; ++offset) neighbors(floatWord(2 ** 53 + offset), 1n);
  let random = 0x6a09e667f3bcc909n;
  for (let i = 0; i < 1000000; ++i) {
    random ^= (random << 13n) & mask; random ^= random >> 7n; random ^= (random << 17n) & mask; random &= mask;
    add(random);
  }
  const corpus32 = new Set();
  const add32 = (bits) => {
    if (bits < 0n || bits > 0xffffffffn) return;
    corpus32.add(bits.toString(16).padStart(16, "0"));
  };
  const neighbors32 = (bits, width = 4n) => {
    for (let i = -width; i <= width; ++i) {
      add32(bits + i); add32((bits + i) ^ 0x80000000n);
    }
  };
  const float32Word = (value) => { const buffer = Buffer.alloc(4); buffer.writeFloatLE(value); return BigInt(buffer.readUInt32LE()); };
  for (let exponent = 0n; exponent < 255n; ++exponent) {
    for (const mantissa of [0n, 1n, 2n, 0x3fffffn, 0x400000n, 0x400001n, 0x7ffffen, 0x7fffffn]) {
      const bits = (exponent << 23n) | mantissa;
      add32(bits); add32(bits ^ 0x80000000n);
    }
  }
  for (let exponent = -45; exponent <= 38; ++exponent) neighbors32(float32Word(Number(`1e${exponent}`)));
  for (const value of [0, 0.1, 1e-4, 1e-5, 1e16, 1e15, 1e21, 1e-7, 2 ** 24, 2 ** -149, 2 ** -126, 0.5, 1.5, 2.5, 0.125, 0.375, 1.25, 1.75, Infinity]) neighbors32(float32Word(value), 16n);
  for (let offset = -32; offset <= 32; ++offset) neighbors32(float32Word(2 ** 24 + offset), 1n);
  for (const bits of [0x7f7fffffn, 0x7f800001n, 0x7fc00000n, 0x7fc00042n, 0x7fffffffn]) { add32(bits); add32(bits ^ 0x80000000n); }
  // A second raw-word form for each edge proves upper32 is ignored.
  for (const hex of [...corpus32]) corpus32.add((BigInt(`0x${hex}`) | 0xdeadbeef00000000n).toString(16));
  let random32 = 0x6a09e667;
  for (let i = 0; i < 1000000; ++i) {
    random32 ^= random32 << 13; random32 ^= random32 >>> 17; random32 ^= random32 << 5;
    add32(BigInt(random32 >>> 0));
  }
  writeFileSync(join(out, "bits32.txt"), [...corpus32].sort().join("\n") + "\n");
  const entries = collectGoldenEntries(join(root, "Examples/features"));
  const passes = baseline ? new Set(readFileSync(baseline, "utf8").split("\n").filter((line) => line.split("\t")[1] === "pass").map((line) => line.split("\t")[0])) : null;
  const cases = [];
  let literals = 0;
  for (const entry of entries) {
    const source = readFileSync(entry.path, "utf8");
    const numbers = [...source.matchAll(/(?<![\w.])(?:[0-9][0-9_]*\.[0-9_]+(?:[eE][+-]?[0-9_]+)?|[0-9][0-9_]*[eE][+-]?[0-9_]+)(?![\w.])/g)];
    for (const match of numbers) { const value = Number(match[0].replaceAll("_", "")); if (!Number.isNaN(value)) { neighbors(floatWord(value)); ++literals; } }
    if ((!passes || passes.has(entry.stem)) && (/float|format|fmt|display|show|print|interp|numeric|hello/.test(entry.stem) || numbers.length)) cases.push(entry.stem);
  }
  if (!cases.length || !literals) throw new Error("empty display golden selection or literal corpus");
  writeFileSync(join(out, "bits.txt"), [...corpus].sort().join("\n") + "\n");
  writeFileSync(join(out, "golden.cases"), cases.join("\n") + "\n");
  writeFileSync(join(out, "corpus.json"), JSON.stringify({ bit_patterns: corpus.size, random_bit_samples: 1000000, binary32_patterns: corpus32.size, binary32_random_samples: 1000000, golden_literals: literals, golden_cases: cases.length, baseline: baseline ?? null }, null, 2) + "\n");
  writeFileSync(join(out, "benchmark_bits.h"), `#define DISPLAY_BENCH_ITERATIONS UINT64_C(1000000)\n#define DISPLAY_BENCH_VALUES ${benchmarkBits.length}\nstatic const uint64_t benchmark_bits[] = {${benchmarkBits.map((bits) => `UINT64_C(0x${bits.toString(16)})`).join(",")}};\n`);
  console.log(`prepared ${corpus.size} IEEE patterns, ${literals} golden literals, ${cases.length} native display goldens`);
  process.exit(0);
}
console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
const frozenOut = frozen ?? out;
const receipts = [];
let failures = 0;
function command(label, program, parameters, input, options = {}) {
  const run = spawnSync(program, parameters, { cwd: root, input, encoding: "utf8", maxBuffer: 1 << 28, timeout: 0, ...options });
  writeFileSync(join(out, `${label}.stdout`), run.stdout ?? "");
  writeFileSync(join(out, `${label}.stderr`), `${run.stderr ?? ""}${run.error ? `\n${run.error.message}\n` : ""}`);
  const ok = run.status === 0;
  if (!ok) ++failures;
  receipts.push({ step: label, status: run.status, signal: run.signal, ok });
  return { ok, text: run.stdout ?? "" };
}
const definedJet = command("object-symbols", process.env.NM ?? "nm", ["--defined-only", object]);
const definedPack = command("pack-symbols", process.env.NM ?? "nm", ["--defined-only", pack]);
const undefinedJet = command("object-imports", process.env.NM ?? "nm", ["-u", object]);
const symbols = (text) => new Map([...text.matchAll(/^\s*(?:[0-9a-fA-F]+\s+)?([A-Za-z])\s+(\S+)$/gm)].map((m) => [m[2], m[1]]));
const jetSymbols = symbols(definedJet.text), packSymbols = symbols(definedPack.text);
const missingExports = [...exports, "jet_rt_test_f64_bits", "jet_rt_test_f64_from_bits"].filter((name) => jetSymbols.get(name) !== "T" || packSymbols.get(name) !== "T");
const unresolved = [...symbols(undefinedJet.text).keys()].filter((name) => !packSymbols.has(name));
// No arithmetic recursion through a Rust exact-Int formatter is admissible.
const exactImports = [...symbols(undefinedJet.text).keys()].filter((name) => name.startsWith("jet_int_"));
if (missingExports.length || unresolved.length || exactImports.length) ++failures;
const cc = command("witness-compile", nativeCCompiler, ["-std=c11", "-O2", "-ffreestanding", "-fno-builtin", "-fno-stack-protector", "-fno-pie", "-I", out, "-c", join(root, "Compiler/JetRuntime/Tests/DisplayWitness.c"), "-o", join(out, "witness.o")]);
const oracle = { ok: true, text: readFileSync(join(frozenOut, "rust.stdout"), "utf8") };
const receipt = JSON.parse(readFileSync(provenance, "utf8"));
const digest = file => createHash("sha256").update(readFileSync(file)).digest("hex");
if (receipt.backend !== "jet-native" || receipt.rustObjects !== 0 || receipt.llvmObjects !== 0 ||
    digest(object) !== receipt.objectSha256 || digest(pack) !== receipt.packSha256) throw Error("display inputs are not the sealed Jet-only runtime");
const archives = receipt.cPlatformArchives.map(archive => {
  if (archive.language !== "C" || archive.role !== "platform" || digest(archive.path) !== archive.sha256) throw Error("display platform archive is not sealed C");
  return archive.path;
});
// This witness defines its own _start and executes the object's init array.
// Link the C-client object once, never the startup-bearing program pack.
const linked = cc.ok && command("witness-link", process.env.LD ?? "ld", ["--gc-sections", "-o", join(out, "witness"), "--entry=_start", join(out, "witness.o"), object, "--start-group", ...archives, "--end-group"]);
const bits = readFileSync(join(out, "bits.txt"), "utf8");
const native = linked?.ok ? command("native", join(out, "witness"), [], bits) : null;
const mismatches = [];
if (native && oracle) {
  const actual = native.text.trimEnd().split("\n"), expected = oracle.text.trimEnd().split("\n");
  for (let i = 0; i < Math.max(actual.length, expected.length); ++i) if (actual[i] !== expected[i]) mismatches.push({ row: i + 1, expected: expected[i] ?? null, actual: actual[i] ?? null });
  if (mismatches.length) ++failures;
}
const oracle32 = { ok: true, text: readFileSync(join(frozenOut, "rust32.stdout"), "utf8") };
const bits32 = readFileSync(join(out, "bits32.txt"), "utf8");
const native32 = linked?.ok ? command("native32", join(out, "witness"), [], bits32.split("\n").filter(Boolean).map((line) => `F${line}\n`).join("")) : null;
const mismatches32 = [];
if (native32 && oracle32) {
  const actual = native32.text.trimEnd().split("\n"), expected = oracle32.text.trimEnd().split("\n");
  for (let i = 0; i < Math.max(actual.length, expected.length); ++i) if (actual[i] !== expected[i]) mismatches32.push({ row: i + 1, expected: expected[i] ?? null, actual: actual[i] ?? null });
  if (mismatches32.length) ++failures;
}
let performance = null;
if (native?.ok && oracle?.ok && !mismatches.length) {
  const nativeBench = command("native-benchmark", join(out, "witness"), [], "B\n", { timeout: 0 });
  const rustBench = { ok: true, text: readFileSync(join(frozenOut, "rust-benchmark.stdout"), "utf8") };
  const cell = (text) => /^benchmark\t1000000\t([0-9a-f]{16})\t([0-9a-f]{16})\n$/u.exec(text);
  const n = cell(nativeBench.text), r = cell(rustBench.text);
  const ok = nativeBench.ok && rustBench.ok && n !== null && r !== null && n[1] === r[1] && BigInt(`0x${n[2]}`) > 0n && BigInt(`0x${r[2]}`) > 0n;
  if (!ok) ++failures;
  performance = { label: "runtime object unoptimized; Rust timing/digest frozen", frozen_receipt: frozenOut, iterations: 1000000, digest_equal: Boolean(n && r && n[1] === r[1]), native_ns: n ? Number(BigInt(`0x${n[2]}`)) : null, rust_ns: r ? Number(BigInt(`0x${r[2]}`)) : null, ratio: ok ? Number(BigInt(`0x${n[2]}`)) / Number(BigInt(`0x${r[2]}`)) : null, ok };
}
const goldenOut = join(out, "goldens");
const goldenHarness = sharedResults ? null : command("goldens", process.execPath, [join(root, "Compiler/JetBackend/Tests/run-goldens.mjs"), goldenOut, "--front", "jet", "--case-list", join(out, "golden.cases")], undefined, { timeout: 0, env: { ...process.env, JET_RUNTIME_PACK: pack, JET_MIR_EMIT: "0" } });
const selected = readFileSync(join(out, "golden.cases"), "utf8").trim().split("\n");
let goldenRows = new Map();
try { goldenRows = readGoldenResults(sharedResults ?? join(goldenOut, "results.tsv")); }
catch (error) { if (error.code !== "ENOENT") throw error; }
const goldenFailures = selected.filter((stem) => goldenRows.get(stem) !== "pass");
if (goldenFailures.length) ++failures;
const summary = { ...JSON.parse(readFileSync(join(out, "corpus.json"), "utf8")), receipts, missingExports, unresolved, exactImports, mismatches: mismatches.length, byte_equal: Boolean(native?.ok && oracle?.ok && !mismatches.length), binary32_mismatches: mismatches32.length, binary32_byte_equal: Boolean(native32?.ok && oracle32?.ok && !mismatches32.length), performance, golden_passes: selected.length - goldenFailures.length, goldenFailures, goldens_ok: (sharedResults ? true : goldenHarness.ok) && !goldenFailures.length, failures };
writeFileSync(join(out, "mismatches.json"), JSON.stringify(mismatches, null, 2) + "\n");
writeFileSync(join(out, "mismatches32.json"), JSON.stringify(mismatches32, null, 2) + "\n");
writeFileSync(join(out, "summary.json"), JSON.stringify(summary, null, 2) + "\n");
console.log(JSON.stringify(summary, null, 2));
process.exitCode = failures ? 1 : 0;
