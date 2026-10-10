#!/usr/bin/env node
// T1 admission on fresh same-tree inputs. Independent checks keep going;
// caller owns candidate/host/pack construction and gate-2 machine admission.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [outArg, jetObject, hosted] = process.argv.slice(2);
if (jetObject === "--rust-reference") {
  console.log("test runtime Rust reference: parked: Rust reference retired (owner #38/#59); Tower #4923");
  process.exit(0);
}
if (!outArg || !jetObject || !hosted || process.argv.length !== 5) throw new Error("usage: run-test-runtime.mjs OUT JET_ONLY_RUNTIME_OBJECT HOSTED_JET");
const out = resolve(outArg);
mkdirSync(out, { recursive: true });
const results = [];
function step(name, binary, args) {
  const result = spawnSync(binary, args, { cwd: repo, env: process.env, encoding: "utf8", timeout: 3600000, maxBuffer: 64 << 20 });
  writeFileSync(join(out, `${name}.stdout`), result.stdout ?? "");
  writeFileSync(join(out, `${name}.stderr`), result.stderr ?? "");
  results.push({ name, status: result.status, signal: result.signal, error: result.error?.message ?? null });
  console.log(`${name}: ${result.status ?? result.error?.message ?? result.signal}`);
  return result;
}
const nm = step("undefined-symbols", process.env.NM ?? "nm", ["--undefined-only", resolve(jetObject)]);
if (nm.status === 0) {
  const allowed = new Set(readFileSync(join(repo, "Compiler/JetRuntime/imports.list"), "utf8").split("\n").map(line => line.trim()).filter(line => line && !line.startsWith("#")));
  const unexpected = nm.stdout.split("\n").map(line => line.trim().split(/\s+/u).at(-1)).filter(symbol => symbol && !allowed.has(symbol));
  if (unexpected.length) results.push({ name: "unexpected-imports", status: 1, symbols: unexpected });
}
const fixture = join(out, "test-unit-runtime");
const build = step("fixture-build", nativeCCompiler, [
  "-std=c11", "-D_POSIX_C_SOURCE=200809L", "-Wall", "-Wextra", "-Werror",
  join(repo, "Compiler/JetRuntime/Tests/test-unit-runtime.c"), resolve(jetObject),
  "-ldl", "-lpthread", "-lm", "-lrt", "-lutil", "-o", fixture,
]);
if (build.status === 0) step("unit-outcomes", process.execPath, [join(repo, "Compiler/JetRuntime/Tests/test-unit-runtime.mjs"), fixture]);
step("native-three-examples", process.execPath, [join(repo, "Compiler/JetBackend/Tests/test-native-test-host.mjs"), join(out, "cli"), resolve(hosted)]);
writeFileSync(join(out, "results.json"), JSON.stringify({ results, ok: results.every(row => row.status === 0 && !row.signal && !row.error) }, null, 2) + "\n");
process.exitCode = results.every(row => row.status === 0 && !row.signal && !row.error) ? 0 : 1;
