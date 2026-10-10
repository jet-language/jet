#!/usr/bin/env node
// Heavy-lane proof: real Jet-object all-row NFC KATs against the frozen seed
// oracle scalars. The Rust --seed oracle is retired: it reports UNCOVERED and
// exits 2 before any rustc invocation.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [mode, output, object] = process.argv.slice(2);
if (mode === "--seed") {
  console.log(JSON.stringify({ status: "UNCOVERED", cardRequired: true, reason: "--seed Rust NFC seed oracle (Text.rs normalization compiled by rustc) retired (owner #38/#59; Tower #4923): oracle scalars not regenerated; no rustc invoked" }));
  process.exit(2);
}
if (mode !== "--native" || !output || !object || process.argv.length !== 5) {
  throw new Error("usage: run-native-text-normalize.mjs --native OUT fresh-runtime.o");
}
const out = resolve(output); mkdirSync(out, { recursive: true });
const kat = join(root, "tests/data/unicode/NormalizationTest.txt");
const bytes = readFileSync(kat);
if (createHash("sha256").update(bytes).digest("hex") !== "5019ffd530751a741900c849c0e010332f142a3612234639bd200b82138a87db") throw new Error("NormalizationTest17 checksum mismatch");
const rows = bytes.toString("utf8").split("\n").filter((line) => /^[0-9A-F]+;/u.test(line));
if (rows.length !== 20034) throw new Error("NormalizationTest17 row count mismatch");
function run(command, args, label, stdoutFile) {
  const result = spawnSync(command, args, { cwd: root, encoding: "utf8", maxBuffer: 1 << 27 });
  writeFileSync(join(out, `${label}.log`), `exit=${result.status}\n${result.stdout ?? ""}${result.stderr ?? ""}`);
  if (result.error || result.status !== 0) throw new Error(`${label}: ${result.error?.message ?? `exit ${result.status}`} (see ${out}/${label}.log)`);
  if (stdoutFile) writeFileSync(stdoutFile, result.stdout);
  console.log(`${label}: exit=0`);
}
const oracle = process.env.JET_NFC_FROZEN_SCALARS ? resolve(process.env.JET_NFC_FROZEN_SCALARS) : join(out, "seed-nfc.scalars");
if (mode === "--native") {
  if (readFileSync(oracle, "utf8").split("\n").length !== 100171) throw new Error("missing complete frozen seed oracle scalars (JET_NFC_FROZEN_SCALARS or <out>/seed-nfc.scalars)");
  const executable = join(out, "native-nfc");
  run(nativeCCompiler, ["-std=c11", "-Wall", "-Wextra", "-Werror", join(root,"Compiler/JetRuntime/Tests/NativeTextNormalizeWitness.c"), resolve(object), "-lm", "-pthread", "-o", executable], "native-link");
  run(executable, [kat, oracle], "native-execute");
  const actual = readFileSync(join(out,"native-execute.log"), "utf8");
  const expected = readFileSync(join(root,"Compiler/JetRuntime/Tests/NativeTextNormalizeWitness.stdout"), "utf8");
  if (actual !== `exit=0\n${expected}`) throw new Error("native witness stdout mismatch");
  console.log("NFC2 Unicode17 20034 rows / both routes / ownership OK");
}
