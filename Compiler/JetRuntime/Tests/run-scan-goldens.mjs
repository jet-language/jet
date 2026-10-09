#!/usr/bin/env node
// Consume the SAME scan.cases native-golden receipt, then execute the raw ABI
// and Float-bit witnesses against the fresh pack. Optional saved tier receipts
// are historical comparisons, never a live Rust/reference provider.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { candidateJetc0, runJetFront } from "../../JetBackend/Tests/jet-front.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [output, native, ...optional] = process.argv.slice(2);
const frozen = new Map();
for (let at = 0; at < optional.length; at += 2) {
  const flag = optional[at], file = optional[at + 1];
  if (!["--frozen-reference", "--frozen-evaluator"].includes(flag) || !file || frozen.has(flag)) {
    throw new Error("usage: run-scan-goldens.mjs OUT NATIVE.results.tsv [--frozen-reference FILE] [--frozen-evaluator FILE]");
  }
  frozen.set(flag, file);
}
if (!output || !native) {
  throw new Error("usage: run-scan-goldens.mjs OUT NATIVE.results.tsv [--frozen-reference FILE] [--frozen-evaluator FILE]");
}
const out = resolve(output);
mkdirSync(out, { recursive: true });
const stems = readFileSync(join(root, "Compiler/JetRuntime/Tests/scan.cases"), "utf8").trim().split("\n");
const failures = [];
const tiers = {};
const inputs = [["native", native], ...[...frozen].map(([flag, file]) => [flag.slice("--frozen-".length), file])];
for (const [tier, file] of inputs) {
  try {
    const rows = readFileSync(resolve(file), "utf8").trim().split("\n").map(line => line.split("\t"));
    if (new Set(rows.map(([stem]) => stem)).size !== rows.length) failures.push(`${tier}: duplicate result rows`);
    const results = new Map(rows);
    tiers[tier] = stems.map(stem => ({ stem, verdict: results.get(stem) ?? "missing" }));
    failures.push(...tiers[tier].filter(({ verdict }) => verdict !== "pass").map(({ stem, verdict }) => `${tier}: ${stem}: ${verdict}`));
  } catch (error) { failures.push(`${tier}: ${error.message}`); }
}
const object = process.env.JET_RUNTIME_JET_OBJECT;
const pack = process.env.JET_RUNTIME_PACK;
if (!object || !pack || !existsSync(object) || !existsSync(pack)) throw new Error("fresh JET_RUNTIME_JET_OBJECT and JET_RUNTIME_PACK required");
for (const [name, file] of [["object", object], ["pack", pack]]) {
  const nm = spawnSync("nm", ["-g", "--defined-only", "--format=posix", file], { encoding: "utf8", maxBuffer: 1 << 27 });
  writeFileSync(join(out, `${name}.symbols`), `${nm.stdout ?? ""}${nm.stderr ?? ""}`);
  if (nm.error || nm.status !== 0) { failures.push(`${name} nm: ${nm.error?.message ?? nm.status}`); continue; }
  const rows = nm.stdout.split("\n").filter(Boolean).map(line => line.trim().split(/\s+/u));
  for (const symbol of ["jet_rt_text_pattern_match", "jet_rt_binary_pattern_match", "jet_rt_cursor_take_pattern", "jet_rt_reader_take_pattern", "jet_reader_read_f32_le", "jet_reader_read_f32_be"]) {
    const count = rows.filter(([name, type]) => name === symbol && type === "T").length;
    if (count !== 1) failures.push(`${name}: ${symbol}: ${count} strong definitions`);
  }
}
const undefinedPack = spawnSync("nm", ["-u", "--format=posix", pack], { encoding: "utf8" });
writeFileSync(join(out, "pack.undefined"), `${undefinedPack.stdout ?? ""}${undefinedPack.stderr ?? ""}`);
if (undefinedPack.error || undefinedPack.status !== 0 || undefinedPack.stdout?.trim()) failures.push("runtime pack has unresolved imports or nm failed");
const raw = join(out, "raw");
const assemble = spawnSync(process.execPath, [join(root, "Compiler/JetRuntime/Tests/check-runtime.mjs"), raw, "--assemble-only", "--test", "Scan"], { cwd: root, stdio: "inherit" });
if (assemble.error || assemble.status !== 0) failures.push(`raw assembly: ${assemble.error?.message ?? assemble.status}`);
else {
  try {
    const corpus = readFileSync(join(root, "Compiler/JetRuntime/Tests/scan-floats.tsv"), "utf8").split("\n").filter(line => line && !line.startsWith("#")).map(line => line.split("\t"));
    let tests = "fn scan_float_corpus() {\n";
    for (const [index, [text, expected]] of corpus.entries()) {
      if (!/^-?[0-9]+$/u.test(expected)) throw new Error(`invalid IEEE word ${index}`);
      const bytes = Buffer.from(text, "utf8");
      tests += `    text${index} :: rt_text_builder_with_capacity(I64{${bytes.length}})\n`;
      for (let at = 0; at < bytes.length; at += 8) {
        const chunk = bytes.subarray(at, at + 8);
        const word = chunk.reduceRight((word, byte) => (word << 8n) | BigInt(byte), 0n);
        tests += `    rt_text_push_word(text${index}, I64{${BigInt.asIntN(64, word)}}, I64{${chunk.length}})\n`;
      }
      tests += `    scan_test_float(text${index}, I64{${expected}})\n`;
    }
    // 100k-digit inputs are built at run time, keeping the compiled fixture
    // small. In the first case only the last digit (well after 768) breaks
    // the tie; in the second, a 9-tail approaches the tie strictly from below.
    for (const [index, [prefixText, fill, last, expected]] of [
      ["1.00000000000000011102230246251565404236316680908203125", 48, 49, "4607182418800017409"],
      ["1.00000000000000011102230246251565404236316680908203124", 57, 57, "4607182418800017408"],
      ["1.", 48, 49, "4607182418800017408"],
    ].entries()) {
      const bytes = Buffer.from(prefixText, "utf8");
      tests += `    long${index} :: rt_text_builder_with_capacity(I64{100000})\n`;
      for (let at = 0; at < bytes.length; at += 8) {
        const chunk = bytes.subarray(at, at + 8);
        const word = chunk.reduceRight((word, byte) => (word << 8n) | BigInt(byte), 0n);
        tests += `    rt_text_push_word(long${index}, I64{${BigInt.asIntN(64, word)}}, I64{${chunk.length}})\n`;
      }
      tests += `    loop rt_text_len(long${index}) < (I64{99999}) {\n        rt_text_push_word(long${index}, I64{${fill}}, I64{1})\n    }\n`;
      tests += `    rt_text_push_word(long${index}, I64{${last}}, I64{1})\n    scan_test_float(long${index}, I64{${expected}})\n`;
    }
    tests += "}\nfn run() {\n    scan_tests()\n    scan_float_corpus()\n}\n";
    const executableRoot = join(out, "raw-native");
    mkdirSync(executableRoot, { recursive: true });
    const program = join(executableRoot, "unit.jet");
    writeFileSync(program, `${readFileSync(join(raw, "unit.jet"), "utf8")}\n${tests}`);
    const prefix = join(executableRoot, "scan-test");
    const problem = runJetFront({ candidate: candidateJetc0(), file: program, root: executableRoot, cwd: root, prefix, pack, store: join(raw, "store") });
    if (problem) failures.push(`raw compile: ${problem.detail}`);
    else {
      const run = spawnSync(`${prefix}.exe`, [], { encoding: "utf8", timeout: 30000 });
      writeFileSync(join(raw, "run.stdout"), run.stdout ?? "");
      writeFileSync(join(raw, "run.stderr"), run.stderr ?? "");
      writeFileSync(join(raw, "run.status.json"), `${JSON.stringify({ status: run.status, signal: run.signal, error: run.error?.message ?? null })}\n`);
      if (run.error || run.status !== 0 || run.signal || run.stdout || run.stderr) failures.push(`raw run: ${run.error?.message ?? `rc=${run.status}, signal=${run.signal}, stdout=${JSON.stringify(run.stdout)}, stderr=${JSON.stringify(run.stderr)}`}`);
    }
  } catch (error) { failures.push(`raw: ${error.message}`); }
}
const receipt = { object, pack, tiers, retired: ["live Rust reference", "Rust MIR evaluator"], failures };
writeFileSync(join(out, "scan-receipt.json"), `${JSON.stringify(receipt, null, 2)}\n`);
console.log(JSON.stringify(receipt, null, 2));
if (failures.length) process.exitCode = 1;
