#!/usr/bin/env node
// ABI-only L9 proof: actual Jet body kernels from an explicit Jet-only runtime
// object/archive (KEY/jet_runtime_jet.o, not startup-bearing jet_runtime.pack.o).
// The delivered-only rail is scripted in check-http-body.c. This does not prove Core
// head/transport integration; that belongs to the cutover.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const args = process.argv.slice(2);
if (args.length !== 2) throw new Error("usage: run-http-body.mjs OUT JET_ONLY_RUNTIME_OBJECT_OR_ARCHIVE");
const [out, object] = args.map((path) => resolve(path));
if (!existsSync(object)) throw new Error(`missing Jet-only runtime proof input ${object}`);
mkdirSync(out, { recursive: true });
const rows = [], failures = [];
function record(name, result, ok) {
  rows.push(`${name}\t${ok ? "pass" : "fail"}\trc=${result.status ?? "none"}\tsignal=${result.signal ?? "none"}`);
  if (!ok) failures.push(name);
}
const program = join(out, "http-body.hosted");
const link = spawnSync(nativeCCompiler, [
  "-std=c11", "-Wall", "-Wextra", "-Werror", join(root, "Compiler/JetRuntime/Tests/check-http-body.c"),
  object, "-Wl,--wrap=jet_rt_session_propagate", "-lpthread", "-ldl", "-lm", "-o", program,
], { cwd: root, encoding: "utf8", timeout: 120000, maxBuffer: 1 << 24 });
writeFileSync(join(out, "link.log"), `${link.stdout ?? ""}${link.stderr ?? ""}`);
record("link", link, link.status === 0 && !link.error && link.signal === null);
if (link.status === 0) {
  for (const [name, argv, status, stdout, stderr] of [
    ["body", [], 0, "http body: ok\n", ""],
    ["tls", ["tls"], 70, "", readFileSync(join(root, "Compiler/JetRuntime/Tests/http-tls-refusal.stderr"), "utf8")],
    ["taken", ["taken"], 70, "", readFileSync(join(root, "Compiler/JetRuntime/Tests/http-body-precondition.stderr"), "utf8")],
  ]) {
    const result = spawnSync(program, argv, { encoding: "utf8", timeout: 30000, maxBuffer: 1 << 24 });
    writeFileSync(join(out, `${name}.stdout`), result.stdout ?? "");
    writeFileSync(join(out, `${name}.stderr`), result.stderr ?? "");
    record(name, result, result.status === status && result.stdout === stdout && result.stderr === stderr && !result.error && result.signal === null);
  }
}
writeFileSync(join(out, "results.tsv"), rows.join("\n") + "\n");
for (const failure of failures) console.error(`${failure}: FAIL (see ${out})`);
if (failures.length) process.exitCode = 1;
else console.log("http body ABI and TLS refusal: PASS");
