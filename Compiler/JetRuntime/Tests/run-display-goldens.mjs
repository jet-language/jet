#!/usr/bin/env node
// Differential proof of the real native Jet display object, not a JS port of
// its digit algorithm. Rust is only the independent seed/verification oracle.
// Prepare: node run-display-goldens.mjs OUT --prepare [--baseline RESULTS.tsv]
// Prove:   node run-display-goldens.mjs OUT --object JET.o --pack PACK.o
// The object/pack must include --test FloatBits. Also runs selected goldens
// and one 1M-float native-versus-Rust formatting cell after byte parity.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { collectGoldenEntries } from "../../../Tools/agent/compiler-diff.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const args = process.argv.slice(2);
const out = resolve(args.shift() ?? "");
let prepare = false, baseline, object, pack;
for (let i = 0; i < args.length; ++i) {
  if (args[i] === "--prepare") prepare = true;
  else if (args[i] === "--baseline") baseline = resolve(args[++i]);
  else if (args[i] === "--object") object = resolve(args[++i]);
  else if (args[i] === "--pack") pack = resolve(args[++i]);
  else throw new Error(`unknown argument ${args[i]}`);
}
if (process.argv.length < 4 || (!prepare && (!object || !pack))) throw new Error("usage: run-display-goldens.mjs OUT --prepare [--baseline RESULTS.tsv] | --object JET.o --pack PACK.o");
mkdirSync(out, { recursive: true });
const exports = ["jet_rt_bool_to_string", "jet_rt_char_to_string", "jet_rt_i64_to_string", "jet_rt_u64_to_string", "jet_rt_int_to_string", "jet_rt_float_to_string", "jet_rt_float_debug"];
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
  for (const value of [0, 1e-4, 1e-5, 1e16, 1e15, Number.MIN_VALUE, Number.MAX_VALUE, 2.2250738585072014e-308, Infinity]) neighbors(floatWord(value), 16n);
  for (const bits of [0x7ff0000000000001n, 0x7ff8000000000000n, 0x7ff8000000000042n, 0x7fffffffffffffffn]) { add(bits); add(bits ^ sign); }
  let random = 0x6a09e667f3bcc909n;
  for (let i = 0; i < 10000; ++i) {
    random ^= (random << 13n) & mask; random ^= random >> 7n; random ^= (random << 17n) & mask; random &= mask;
    add(random);
  }
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
  writeFileSync(join(out, "corpus.json"), JSON.stringify({ bit_patterns: corpus.size, golden_literals: literals, golden_cases: cases.length, baseline: baseline ?? null }, null, 2) + "\n");
  // Values.rs JetDisplay/JetShow/JetDebug f64 all call format!("{:?}").
  writeFileSync(join(out, "benchmark_bits.h"), `#define DISPLAY_BENCH_ITERATIONS UINT64_C(1000000)\n#define DISPLAY_BENCH_VALUES ${benchmarkBits.length}\nstatic const uint64_t benchmark_bits[] = {${benchmarkBits.map((bits) => `UINT64_C(0x${bits.toString(16)})`).join(",")}};\n`);
  writeFileSync(join(out, "oracle.rs"), `use std::io::{self, Read, Write};
fn main() {
 if std::env::args().nth(1).as_deref() == Some("--benchmark") {
  let values: [u64; ${benchmarkBits.length}] = [${benchmarkBits.map((bits) => `0x${bits.toString(16)}`).join(",")}];
  let start = std::time::Instant::now();
  let mut digest = 14695981039346656037u64;
  for iteration in 0..1000000 {
   let text = format!("{:?}", f64::from_bits(values[iteration % values.len()]));
   for byte in text.bytes().chain(std::iter::once(b'\\n')) {
    digest = (digest ^ u64::from(byte)).wrapping_mul(1099511628211);
   }
  }
  let elapsed = start.elapsed().as_nanos();
  println!("benchmark\\t1000000\\t{digest:016x}\\t{elapsed:016x}");
  return;
 }
 let mut input=String::new(); io::stdin().read_to_string(&mut input).unwrap();
 let mut out=io::BufWriter::new(io::stdout().lock());
 for line in input.lines() { let bits=u64::from_str_radix(line,16).unwrap(); let value=f64::from_bits(bits); writeln!(out,"{bits:016x}\\t{value:?}\\t{value:?}").unwrap(); }
}
`);
  console.log(`prepared ${corpus.size} IEEE patterns, ${literals} golden literals, ${cases.length} native display goldens`);
  process.exit(0);
}
const receipts = [];
let failures = 0;
function command(label, program, parameters, input, options = {}) {
  const run = spawnSync(program, parameters, { cwd: root, input, encoding: "utf8", maxBuffer: 1 << 28, timeout: 300000, ...options });
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
const cc = command("witness-compile", process.env.CC ?? "cc", ["-std=c11", "-O2", "-ffreestanding", "-fno-builtin", "-fno-stack-protector", "-fno-pie", "-I", out, "-c", join(root, "Compiler/JetRuntime/Tests/DisplayWitness.c"), "-o", join(out, "witness.o")]);
const rust = command("oracle-compile", process.env.RUSTC ?? "rustc", ["--edition=2021", "-O", join(out, "oracle.rs"), "-o", join(out, "oracle")]);
const linked = cc.ok && command("witness-link", process.env.LD ?? "ld", ["--gc-sections", "-o", join(out, "witness"), "--entry=_start", join(out, "witness.o"), pack]);
const bits = readFileSync(join(out, "bits.txt"), "utf8");
const native = linked?.ok ? command("native", join(out, "witness"), [], bits) : null;
const oracle = rust.ok ? command("rust", join(out, "oracle"), [], bits) : null;
const mismatches = [];
if (native && oracle) {
  const actual = native.text.trimEnd().split("\n"), expected = oracle.text.trimEnd().split("\n");
  for (let i = 0; i < Math.max(actual.length, expected.length); ++i) if (actual[i] !== expected[i]) mismatches.push({ row: i + 1, expected: expected[i] ?? null, actual: actual[i] ?? null });
  if (mismatches.length) ++failures;
}
let performance = null;
if (native?.ok && oracle?.ok && !mismatches.length) {
  const nativeBench = command("native-benchmark", join(out, "witness"), [], "B\n", { timeout: 0 });
  const rustBench = command("rust-benchmark", join(out, "oracle"), ["--benchmark"], undefined, { timeout: 0 });
  const cell = (text) => /^benchmark\t1000000\t([0-9a-f]{16})\t([0-9a-f]{16})\n$/u.exec(text);
  const n = cell(nativeBench.text), r = cell(rustBench.text);
  const ok = nativeBench.ok && rustBench.ok && n !== null && r !== null && n[1] === r[1] && BigInt(`0x${n[2]}`) > 0n && BigInt(`0x${r[2]}`) > 0n;
  if (!ok) ++failures;
  performance = { iterations: 1000000, digest_equal: Boolean(n && r && n[1] === r[1]), native_ns: n ? Number(BigInt(`0x${n[2]}`)) : null, rust_ns: r ? Number(BigInt(`0x${r[2]}`)) : null, ratio: ok ? Number(BigInt(`0x${n[2]}`)) / Number(BigInt(`0x${r[2]}`)) : null, ok };
}
const goldenOut = join(out, "goldens");
const goldenHarness = command("goldens", process.execPath, [join(root, "Compiler/JetBackend/Tests/run-goldens.mjs"), goldenOut, "--case-list", join(out, "golden.cases")], undefined, { timeout: 0, env: { ...process.env, JET_RUNTIME_PACK: pack, JET_MIR_EMIT: "0" } });
const selected = readFileSync(join(out, "golden.cases"), "utf8").trim().split("\n");
let goldenRows = new Map();
try { goldenRows = new Map(readFileSync(join(goldenOut, "results.tsv"), "utf8").trim().split("\n").map((row) => row.split("\t")).map(([stem, verdict]) => [stem, verdict])); }
catch (error) { if (error.code !== "ENOENT") throw error; }
const goldenFailures = selected.filter((stem) => goldenRows.get(stem) !== "pass");
if (goldenFailures.length) ++failures;
const summary = { ...JSON.parse(readFileSync(join(out, "corpus.json"), "utf8")), receipts, missingExports, unresolved, exactImports, mismatches: mismatches.length, byte_equal: Boolean(native?.ok && oracle?.ok && !mismatches.length), performance, golden_passes: selected.length - goldenFailures.length, goldenFailures, goldens_ok: goldenHarness.ok && !goldenFailures.length, failures };
writeFileSync(join(out, "mismatches.json"), JSON.stringify(mismatches, null, 2) + "\n");
writeFileSync(join(out, "summary.json"), JSON.stringify(summary, null, 2) + "\n");
console.log(JSON.stringify(summary, null, 2));
process.exitCode = failures ? 1 : 0;
