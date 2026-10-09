#!/usr/bin/env node
// One held L9 cell: real 1GiB bounded File Take, including List word widening.
// Inputs must be from the same frozen tree as run-http-body.mjs; no installed
// runtime/reference fallback. This records performance, not an exit-proof PASS.
import { closeSync, existsSync, ftruncateSync, mkdirSync, openSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const args = process.argv.slice(2);
if (args.length !== 3) throw new Error("usage: run-http-body-perf.mjs OUT SAME_TREE_HTTP_BODY_HOSTED FRESH_SAME_TREE_ALLPARTS_RS");
const [out, hosted, allparts] = args.map((path) => resolve(path));
for (const path of [hosted, allparts]) if (!existsSync(path)) throw new Error(`missing same-tree input ${path}`);
mkdirSync(out, { recursive: true });
const source = join(out, "reference.rs"), reference = join(out, "reference");
writeFileSync(source, readFileSync(allparts, "utf8") + "\n" + readFileSync(join(root, "Compiler/JetRuntime/Tests/http-body-reference.rs"), "utf8"));
const built = spawnSync(process.env.RUSTC ?? "rustc", ["--edition", "2021", "--crate-name", "http_body_reference", "-O", "-Awarnings", source, "-o", reference], {
  cwd: root, encoding: "utf8", timeout: 600000, maxBuffer: 1 << 25,
});
writeFileSync(join(out, "reference-build.log"), `${built.stdout ?? ""}${built.stderr ?? ""}`);
if (built.status !== 0 || built.signal !== null || built.error) throw new Error("same-tree reference build failed; see reference-build.log");
const input = join(out, "one-gib.bin"), fd = openSync(input, "w");
ftruncateSync(fd, 1 << 30);
closeSync(fd);
const samples = [], failures = [];
const expected = "bytes=1073741824 checksum=0\n";
// Alternate order; one warm-up per implementation is excluded from medians.
for (let trial = 0; trial < 4; trial += 1) {
  const order = trial % 2 ? ["rust", "jet"] : ["jet", "rust"];
  for (const implementation of order) {
    const argv = implementation === "jet" ? ["perf", input] : [input];
    const program = implementation === "jet" ? hosted : reference;
    const start = process.hrtime.bigint();
    const result = spawnSync(program, argv, { encoding: "utf8", timeout: 600000, maxBuffer: 1 << 20 });
    const nanos = Number(process.hrtime.bigint() - start);
    const ok = result.status === 0 && result.signal === null && !result.error && result.stdout === expected && result.stderr === "";
    writeFileSync(join(out, `${implementation}-${trial}.stdout`), result.stdout ?? "");
    writeFileSync(join(out, `${implementation}-${trial}.stderr`), result.stderr ?? "");
    samples.push({ implementation, trial, warmup: trial === 0, nanos, ok, status: result.status, signal: result.signal, error: result.error?.message ?? null });
    if (!ok) failures.push(`${implementation}-${trial}`);
  }
}
function median(implementation) {
  const times = samples.filter((row) => row.implementation === implementation && !row.warmup && row.ok).map((row) => row.nanos).sort((a, b) => a - b);
  return times.length === 3 ? times[1] : null;
}
const jet = median("jet"), rust = median("rust");
writeFileSync(join(out, "perf.json"), JSON.stringify({
  bytes: 1 << 30, chunkBytes: 65536, carrier: "owned stride8 byte List", hosted, allparts,
  samples, jetMedianNanos: jet, rustMedianNanos: rust,
  jetOverRust: jet !== null && rust !== null ? jet / rust : null,
}, null, 2) + "\n");
for (const failure of failures) console.error(`${failure}: runtime/output failure`);
if (failures.length) process.exitCode = 1;
else console.log(`1GiB streaming: Jet ${jet}ns; Rust ${rust}ns; ratio ${jet / rust} (recorded, not a parity claim)`);
