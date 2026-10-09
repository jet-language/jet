#!/usr/bin/env node
// One held L9 cell: real 1GiB bounded File Take, including List word widening.
// Inputs must be from the same frozen tree as run-http-body.mjs; no installed
// runtime/reference fallback. This records performance, not an exit-proof PASS.
import { closeSync, existsSync, ftruncateSync, mkdirSync, openSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join, resolve } from "node:path";

const args = process.argv.slice(2);
console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
if (args.length < 2 || args.length > 3) throw new Error("usage: run-http-body-perf.mjs OUT SAME_TREE_HTTP_BODY_HOSTED [FROZEN_PERF_JSON]");
const [out, hosted, frozenPerf] = args.map((path) => resolve(path));
if (!existsSync(hosted)) throw new Error(`missing same-tree input ${hosted}`);
const frozen = frozenPerf ? JSON.parse(readFileSync(frozenPerf, "utf8")) : null;
mkdirSync(out, { recursive: true });
const input = join(out, "one-gib.bin"), fd = openSync(input, "w");
ftruncateSync(fd, 1 << 30);
closeSync(fd);
const samples = [], failures = [];
const expected = "bytes=1073741824 checksum=0\n";
// One Jet warm-up is excluded; Rust timing is only an optional frozen receipt.
for (let trial = 0; trial < 4; trial += 1) {
  const implementation = "jet";
  const start = process.hrtime.bigint();
  const result = spawnSync(hosted, ["perf", input], { encoding: "utf8", timeout: 600000, maxBuffer: 1 << 20 });
  const nanos = Number(process.hrtime.bigint() - start);
  const ok = result.status === 0 && result.signal === null && !result.error && result.stdout === expected && result.stderr === "";
  writeFileSync(join(out, `${implementation}-${trial}.stdout`), result.stdout ?? "");
  writeFileSync(join(out, `${implementation}-${trial}.stderr`), result.stderr ?? "");
  samples.push({ implementation, trial, warmup: trial === 0, nanos, ok, status: result.status, signal: result.signal, error: result.error?.message ?? null });
  if (!ok) failures.push(`${implementation}-${trial}`);
}
function median(implementation) {
  const times = samples.filter((row) => row.implementation === implementation && !row.warmup && row.ok).map((row) => row.nanos).sort((a, b) => a - b);
  return times.length === 3 ? times[1] : null;
}
const jet = median("jet"), rust = frozen?.rustMedianNanos ?? null;
writeFileSync(join(out, "perf.json"), JSON.stringify({
  bytes: 1 << 30, chunkBytes: 65536, carrier: "owned stride8 byte List", hosted, frozenPerf: frozenPerf ?? null,
  samples, jetMedianNanos: jet, rustMedianNanos: rust, rustReference: "parked: Rust reference retired (owner #38/#59); Tower #4923",
  jetOverRust: jet !== null && rust !== null ? jet / rust : null,
}, null, 2) + "\n");
for (const failure of failures) console.error(`${failure}: runtime/output failure`);
if (failures.length) process.exitCode = 1;
else console.log(`1GiB streaming: Jet ${jet}ns; frozen Rust ${rust ?? "not supplied"}ns (recorded, not a parity claim)`);
