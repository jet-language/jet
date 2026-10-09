#!/usr/bin/env node
// Heavy-lane proof: seed-only oracle, then real Jet-object all-row NFC KATs.
// No Rust normalization implementation is added: the temporary oracle extracts
// the existing frozen seed functions verbatim and dies with the seed tier.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [mode, output, object] = process.argv.slice(2);
if (mode === "--seed") {
  console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
  process.exit(0);
}
if (!output || !["--seed", "--native"].includes(mode) || (mode === "--native" && !object) || process.argv.length !== (mode === "--native" ? 5 : 4)) {
  throw new Error("usage: run-native-text-normalize.mjs --seed OUT | --native OUT fresh-runtime.o");
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
if (mode === "--seed") {
  const text = readFileSync(join(root, "crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs"), "utf8");
  const start = text.indexOf("const HANGUL_SBASE:");
  const end = text.indexOf("fn jet_text_fold_lookup(", start);
  if (start < 0 || end < 0) throw new Error("seed normalization boundaries missing");
  const source = `include!(${JSON.stringify(join(root, "crates/jet-codegen/src/Prelude/CoreLib/Top/UnicodeTables.rs"))});\n`
    + text.slice(start, end) + `
fn main() {
    let file = std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
    let mut count = 0;
    for line in file.lines() {
        if !line.as_bytes().first().is_some_and(u8::is_ascii_hexdigit) { continue; }
        let columns: Vec<String> = line.split(';').take(5).map(|col| col.split_whitespace()
            .map(|cp| char::from_u32(u32::from_str_radix(cp,16).unwrap()).unwrap()).collect()).collect();
        assert_eq!(columns.len(),5);
        for (i,input) in columns.iter().enumerate() {
            let nfc = jet_text_nfc(input);
            assert_eq!(nfc, columns[if i<3 {1} else {3}], "row {} column {}", count+1,i+1);
            assert_eq!(jet_text_normalize_nfc(input),nfc);
            assert_eq!(jet_text_nfc(&nfc),nfc);
            println!("{}", nfc.chars().map(|cp| format!("{:X}",cp as u32)).collect::<Vec<_>>().join(" "));
        }
        count+=1;
    }
    assert_eq!(count,20034);
    eprintln!("seed NFC Unicode17 20034 rows / 100170 comparisons OK");
}
` + text.slice(text.indexOf("fn jet_text_normalize_nfc("), text.indexOf("fn jet_text_trim_start("));
  const seedSource = join(out, "seed-nfc.rs"), executable = join(out, "seed-nfc");
  writeFileSync(seedSource, source);
  run("rustc", ["--edition=2021", "-Awarnings", seedSource, "-o", executable], "seed-build");
  run(executable, [kat], "seed-execute", oracle);
  console.log("seed NFC Unicode17 20034 rows / 100170 comparisons OK");
} else {
  if (readFileSync(oracle, "utf8").split("\n").length !== 100171) throw new Error("missing complete seed oracle; run --seed in the heavy lane first");
  const executable = join(out, "native-nfc");
  run("cc", ["-std=c11", "-Wall", "-Wextra", "-Werror", join(root,"Compiler/JetRuntime/Tests/NativeTextNormalizeWitness.c"), resolve(object), "-lm", "-pthread", "-o", executable], "native-link");
  run(executable, [kat, oracle], "native-execute");
  const actual = readFileSync(join(out,"native-execute.log"), "utf8");
  const expected = readFileSync(join(root,"Compiler/JetRuntime/Tests/NativeTextNormalizeWitness.stdout"), "utf8");
  if (actual !== `exit=0\n${expected}`) throw new Error("native witness stdout mismatch");
  console.log("NFC2 Unicode17 20034 rows / both routes / ownership OK");
}
