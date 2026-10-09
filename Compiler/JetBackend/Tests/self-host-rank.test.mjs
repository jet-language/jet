#!/usr/bin/env node
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

// Synthetic unit fixtures only; these are not self-host coverage evidence.
const scratch = process.env.JET_SCRATCH_ROOT;
if (!scratch) throw new Error("JET_SCRATCH_ROOT must name disk-backed scratch");
const root = mkdtempSync(join(scratch, "self-host-rank-test-"));
const script = join(dirname(fileURLToPath(import.meta.url)), "self-host-rank.mjs");
const put = (path, text) => { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, text); };
const run = (path, golden) => spawnSync(process.execPath, [script, path], {
  encoding: "utf8", env: { ...process.env, SELFHOST_GOLDEN_LOWER: golden ?? "" },
});
try {
  const empty = join(root, "empty");
  mkdirSync(empty);
  assert.notEqual(run(empty).status, 0, "absent measurement must not become 0% coverage");
  const probe = join(root, "probe");
  put(join(probe, "compiler.map.json"), JSON.stringify({ source_segments: [
    { generated: { start: 0, end: 100 }, source_file: "Compiler/JetLexer/Source/Scan.jet" },
    { generated: { start: 100, end: 200 }, source_file: "Compiler/JetParser/Source/Entry.jet" },
  ] }));
  const functions = "1\tlexer_a\ta\t1\t1\t2\t1\n2\tparser_b\tb\t1\t101\t102\t1\n3\tparser_c\tc\t1\t110\t111\t1\n";
  const sources = "1\t/unit/selfhost.jet\n";
  for (const pkg of ["JetLexer", "JetParser"]) {
    const slice = join(probe, "slices", pkg);
    put(join(slice, "self.functions.tsv"), functions);
    put(join(slice, "self.sources.tsv"), sources);
  }
  const lexer = join(probe, "slices/JetLexer/run");
  put(join(lexer, "self.progress"), "fn\t0\tlexer_a\nfn\t1\tparser_b\n");
  put(join(lexer, "self.issues"), "parser_b\tunsupported route 123\n");
  const parser = join(probe, "slices/JetParser/run");
  put(join(parser, "self.progress"), "fn\t0\tparser_b\nfn\t1\tparser_c\n");
  put(join(parser, "self.partial-issues"), "parser_b\tunsupported route 456\n");
  put(join(parser, "self.prepare-issues"), "entry\tprepare: missing owner 123\n");
  let result = run(probe);
  assert.equal(result.status, 0, result.stderr);
  assert.match(readFileSync(join(probe, "PACKAGES.tsv"), "utf8"), /Compiler\/JetLexer\t1\t1\t100.0/);
  assert.match(readFileSync(join(probe, "PACKAGES.tsv"), "utf8"), /Compiler\/JetParser\t2\t0\t0.0/);
  assert.match(readFileSync(join(probe, "RANKED.tsv"), "utf8"), /functions=3; lowered_clean=1/);
  assert.match(readFileSync(join(probe, "OVERLAP.tsv"), "utf8"), /overlap-unmeasured/);
  assert.match(readFileSync(join(probe, "OVERLAP.tsv"), "utf8"), /missing owner N\tglobal/);
  assert.match(readFileSync(join(probe, "OVERLAP.tsv"), "utf8"), /did not complete/);
  const golden = join(root, "golden");
  put(join(golden, "one.issues"), "golden_fn\tunsupported route 999\n");
  result = run(probe, golden);
  assert.equal(result.status, 0, result.stderr);
  assert.match(readFileSync(join(probe, "OVERLAP.tsv"), "utf8"), /unsupported route N\t1\t1\tshared/);
  // Completion records distinguish a finished last function from a crash.
  put(join(parser, "self.completed"), "parser_b\nparser_c\n");
  result = run(probe, golden);
  assert.equal(result.status, 0, result.stderr);
  assert.match(readFileSync(join(probe, "PACKAGES.tsv"), "utf8"), /Compiler\/JetParser\t2\t1\t50.0/);
  assert.doesNotMatch(readFileSync(join(probe, "OVERLAP.tsv"), "utf8"), /did not complete/);
  put(join(probe, "self-all.functions.tsv"), functions);
  put(join(probe, "self-all.sources.tsv"), sources);
  result = spawnSync(process.execPath, [join(dirname(script), "self-host-slices.mjs"), probe], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(join(probe, "slice-order.txt"), "utf8"), "JetLexer\nJetParser\n");
  assert.equal(readFileSync(join(probe, "slices/JetParser/roots.txt"), "utf8"), "2\n3\n");
  const mapped = join(root, "map/compiler.map.json");
  const assembled = join(root, "map/selfhost.jet");
  const prefix = "// [unit source: Compiler/JetLexer/Source/Scan.jet]\n// π\n";
  put(assembled, prefix + "// [unit source: Compiler/JetParser/Source/Entry.jet]\nfn parser() {}\n");
  result = spawnSync(process.execPath, [join(dirname(script), "self-host-map.mjs"), assembled, mapped], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const recovered = JSON.parse(readFileSync(mapped, "utf8")).source_segments;
  assert.equal(recovered[1].generated.start, Buffer.byteLength(prefix));
  assert.equal(recovered[0].generated.end, recovered[1].generated.start);
  console.log("SELFHOST RANK TEST OK: absent evidence, package mapping, cross-slice deduplication, incomplete functions, completion records, golden overlap, global preparation, smallest-first slices, UTF-8 byte spans");
} finally {
  rmSync(root, { recursive: true, force: true });
}
