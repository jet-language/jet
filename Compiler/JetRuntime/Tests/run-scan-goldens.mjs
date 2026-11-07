#!/usr/bin/env node
// Run the scan.cases programs with the native CLI ($JET) against their goldens,
// then execute the raw ABI and Float-bit witnesses against the fresh runtime.
// Optional saved tier receipts are historical comparisons, never a live
// Rust/reference provider.
// Usage: JET=<native CLI> JET_FRONT_JETC0=<native CLI> JET_RUNTIME_JET_OBJECT=<object>
//   JET_RUNTIME_PACK=<pack> node run-scan-goldens.mjs OUT [--frozen-reference FILE] [--frozen-evaluator FILE]
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { candidateJetc0 } from "../../JetBackend/Tests/jet-front.mjs";
import { runNativeLibrary } from "../../../Tools/agent/compiler-native-transport.mjs";
import { parseRuntimeExports, runtimeLifecycleList } from "../../../Tools/agent/runtime-route-abi.mjs";
import { faultReportObject } from "../../../Tools/packaging/fault-report-object.mjs";
import { nativeCCompiler } from "../../../Tools/packaging/native-c-compiler.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const usage = "usage: run-scan-goldens.mjs OUT [--frozen-reference FILE] [--frozen-evaluator FILE]";
const [output, ...optional] = process.argv.slice(2);
const frozen = new Map();
for (let at = 0; at < optional.length; at += 2) {
  const flag = optional[at], file = optional[at + 1];
  if (!["--frozen-reference", "--frozen-evaluator"].includes(flag) || !file || frozen.has(flag)) throw new Error(usage);
  frozen.set(flag, file);
}
if (!output) throw new Error(usage);
const out = resolve(output);
mkdirSync(out, { recursive: true });
const object = process.env.JET_RUNTIME_JET_OBJECT;
const pack = process.env.JET_RUNTIME_PACK;
if (!object || !pack || !existsSync(object) || !existsSync(pack)) throw new Error("fresh JET_RUNTIME_JET_OBJECT and JET_RUNTIME_PACK required");
// Each row is a repo-relative program whose golden stdout sits beside it as .out.
const programs = readFileSync(join(root, "Compiler/JetRuntime/Tests/scan.cases"), "utf8").trim().split("\n");
const failures = [];
const tiers = {};
tiers.native = programs.map(program => {
  const run = spawnSync(process.env.JET ?? "jet", ["run", program], { cwd: root, encoding: "utf8", timeout: 300000, maxBuffer: 1 << 24 });
  const golden = join(root, program.replace(/\.jet$/u, ".out"));
  const verdict = run.error ? `error ${run.error.message}`
    : run.status !== 0 ? `rc=${run.status ?? run.signal}`
    : !existsSync(golden) ? "golden missing"
    : run.stdout !== readFileSync(golden, "utf8") ? "stdout differs" : "pass";
  return { stem: program, verdict };
});
failures.push(...tiers.native.filter(({ verdict }) => verdict !== "pass").map(({ stem, verdict }) => `native: ${stem}: ${verdict}`));
for (const [flag, file] of frozen) {
  const tier = flag.slice("--frozen-".length);
  try {
    const rows = readFileSync(resolve(file), "utf8").trim().split("\n").map(line => line.split("\t"));
    if (new Set(rows.map(([stem]) => stem)).size !== rows.length) failures.push(`${tier}: duplicate result rows`);
    const results = new Map(rows);
    tiers[tier] = programs.map(stem => ({ stem, verdict: results.get(stem) ?? "missing" }));
    failures.push(...tiers[tier].filter(({ verdict }) => verdict !== "pass").map(({ stem, verdict }) => `${tier}: ${stem}: ${verdict}`));
  } catch (error) { failures.push(`${tier}: ${error.message}`); }
}
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
// The pack's strong imports are exactly its link contract (Image/RuntimePack.jet):
// the program's `main`, the fault report each image link writes, and the
// layout names Jet's linker defines (Image/StaticLink.jet link_linker_names).
const staticLink = readFileSync(join(root, "Compiler/JetBackend/Source/Image/StaticLink.jet"), "utf8");
const faultReportName = /^LINK_FAULT_REPORT: "([^"]+)"$/mu.exec(staticLink)?.[1];
const linkerNames = /^link_linker_names: fn\(\) -> \[String\] Never! \{\n {4}\[String\]\{([^}]*)\}/mu.exec(staticLink)?.[1];
if (!faultReportName || !linkerNames) failures.push("pack import contract not found in Image/StaticLink.jet");
else {
  const contract = new Set(["main", faultReportName, ...[...linkerNames.matchAll(/"([^"]+)"/gu)].map(match => match[1])]);
  const undefinedPack = spawnSync("nm", ["-u", "--format=posix", pack], { encoding: "utf8" });
  writeFileSync(join(out, "pack.undefined"), `${undefinedPack.stdout ?? ""}${undefinedPack.stderr ?? ""}`);
  if (undefinedPack.error || undefinedPack.status !== 0) failures.push(`pack nm -u: ${undefinedPack.error?.message ?? undefinedPack.status}`);
  else {
    const strong = undefinedPack.stdout.split("\n").filter(Boolean).map(line => line.trim().split(/\s+/u)).filter(([, type]) => type === "U").map(([name]) => name);
    const outside = strong.filter(name => !contract.has(name));
    if (outside.length) failures.push(`runtime pack imports outside its link contract: ${outside.join(", ")}`);
  }
}
// The raw witnesses call runtime internals, so the runtime sources, ScanTests.jet
// and the generated float corpus lower to one fixture object, linked by a C host.
const unit = join(out, "unit");
try {
  const paths = readFileSync(join(root, "Compiler/JetRuntime/sources.list"), "utf8").split("\n").map(line => line.trim()).filter(line => line && !line.startsWith("#"));
  paths.push("Compiler/JetRuntime/Tests/Fragments.fixture/ScanTests.jet");
  const corpus = readFileSync(join(root, "Compiler/JetRuntime/Tests/scan-floats.tsv"), "utf8").split("\n").filter(line => line && !line.startsWith("#")).map(line => line.split("\t"));
  let tests = "scan_float_corpus: fn() {\n";
  for (const [index, [text, expected]] of corpus.entries()) {
    if (!/^-?[0-9]+$/u.test(expected)) throw new Error(`invalid IEEE word ${index}`);
    const bytes = Buffer.from(text, "utf8");
    tests += `    text${index}: rt_text_builder_with_capacity(I64{${bytes.length}})\n`;
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
    tests += `    long${index}: rt_text_builder_with_capacity(I64{100000})\n`;
    for (let at = 0; at < bytes.length; at += 8) {
      const chunk = bytes.subarray(at, at + 8);
      const word = chunk.reduceRight((word, byte) => (word << 8n) | BigInt(byte), 0n);
      tests += `    rt_text_push_word(long${index}, I64{${BigInt.asIntN(64, word)}}, I64{${chunk.length}})\n`;
    }
    tests += `    loop rt_text_len(long${index}) < (I64{99999}) {\n        rt_text_push_word(long${index}, I64{${fill}}, I64{1})\n    }\n`;
    tests += `    rt_text_push_word(long${index}, I64{${last}}, I64{1})\n    scan_test_float(long${index}, I64{${expected}})\n`;
  }
  tests += "}\n\n#Export(c) jet_rt_scan_goldens_fixture: fn() {\n    scan_tests()\n    scan_float_corpus()\n}\n";
  const source = `${paths.map(path => readFileSync(join(root, path), "utf8")).join("\n")}\n${tests}`;
  mkdirSync(join(unit, "Source"), { recursive: true });
  writeFileSync(join(unit, "Source/Fixture.jet"), source);
  writeFileSync(join(unit, "package.jet"), readFileSync(join(root, "Compiler/JetRuntime/package.jet")));
  writeFileSync(join(unit, "lifecycle.list"), runtimeLifecycleList(parseRuntimeExports(source, "Source/Fixture.jet")));
  const { problem } = runNativeLibrary({ jet: candidateJetc0(), file: join(unit, "Source/Fixture.jet"), prefix: join(unit, "fixture"), lifecycle: join(unit, "lifecycle.list"), env: { ...process.env, JET_TOOLCHAIN_ROOT: root, JET_STORE_DIR: join(out, "store") } });
  if (problem) throw new Error(`lower: ${problem.verdict}: ${problem.detail}`);
  // The fixture object imports jet_rt_fault_report, which Jet's linker writes
  // into each image; this platform link takes it from the shared report source.
  const faultReport = faultReportObject(mkdtempSync(join(out, "fault-report-")));
  const executable = join(out, "scan-fixture");
  const link = spawnSync(nativeCCompiler, ["-std=c11", "-Wall", "-Wextra", "-Werror", join(root, "Compiler/JetRuntime/Tests/scan-fixture.c"), join(unit, "fixture.o"), faultReport, "-lpthread", "-ldl", "-lm", "-o", executable], { encoding: "utf8", maxBuffer: 1 << 24 });
  if (link.error || link.status !== 0) throw new Error(`link: ${link.error?.message ?? link.status}\n${link.stdout ?? ""}${link.stderr ?? ""}`);
  const run = spawnSync(executable, [], { encoding: "utf8", timeout: 30000 });
  writeFileSync(join(out, "run.receipt.json"), `${JSON.stringify({ status: run.status, signal: run.signal, error: run.error?.message ?? null, stdout: run.stdout, stderr: run.stderr }, null, 2)}\n`);
  if (run.error || run.status !== 0 || run.signal || run.stdout || run.stderr) failures.push(`raw run: ${run.error?.message ?? `rc=${run.status}, signal=${run.signal}, stdout=${JSON.stringify(run.stdout)}, stderr=${JSON.stringify(run.stderr)}`}`);
} catch (error) { failures.push(`raw: ${error.message}`); }
const receipt = { object, pack, tiers, retired: ["live Rust reference", "Rust evaluator"], failures };
writeFileSync(join(out, "scan-receipt.json"), `${JSON.stringify(receipt, null, 2)}\n`);
console.log(JSON.stringify(receipt, null, 2));
if (failures.length) process.exitCode = 1;
