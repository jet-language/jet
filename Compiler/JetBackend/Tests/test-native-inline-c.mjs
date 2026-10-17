#!/usr/bin/env node
// Run only against the native CLI with its admitted pack beside it. Two
// #FFI(c) bodies share C state across translation units: the native driver
// compiles both, proves the whole inline bridge with undefined symbols
// refused, archives it and links it (FFI.rs emit_inline_build_rs). A body that
// references a symbol no inline unit defines must be refused before linking.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [outArg, jetArg] = process.argv.slice(2);
if (!outArg || !jetArg) throw new Error("expected OUT NATIVE_JET_WITH_ADMITTED_PACK");
const out = resolve(outArg), jet = resolve(jetArg);
const runtime = join(dirname(jet), "jet-runtime");
const key = readFileSync(join(runtime, "native.key"), "utf8").trim();
assert.ok(existsSync(join(runtime, key, "jet_runtime.pack.o")), "native CLI must have its admitted pack");
const shared = readFileSync(join(repo, "tests/fixtures/native_inline_c_shared_state.jet"), "utf8");
const manifest = name => 'name: "' + name + '"\nversion: "0.0.1"\nauthority: { holds: { allow: [FFI.C, IO, Mem.Alloc] } }\n';
function build(name, source) {
  const cwd = join(out, name); mkdirSync(cwd, { recursive: true });
  writeFileSync(join(cwd, "main.jet"), source);
  writeFileSync(join(cwd, "package.jet"), manifest(name));
  const result = spawnSync(jet, ["build", "main.jet"], { cwd, env: { ...process.env, NO_COLOR: "1" }, encoding: "utf8", timeout: 300000, maxBuffer: 64 << 20 });
  writeFileSync(join(cwd, "stdout"), result.stdout ?? ""); writeFileSync(join(cwd, "stderr"), result.stderr ?? "");
  assert.equal(result.error, undefined, String(result.error)); assert.equal(result.signal, null);
  return { cwd, result };
}
const ok = build("native_inline_c_shared_state", shared);
assert.equal(ok.result.status, 0, ok.result.stderr);
const ran = spawnSync(join(ok.cwd, "main"), [], { encoding: "utf8", timeout: 60000 });
assert.equal(ran.status, 0, ran.stderr);
assert.equal(ran.stdout, "bump=42 peek=42\n");
// The same program with the defining body removed leaves `peek` unresolved.
const missing = build("native_inline_c_missing_definition", shared
  .replace("int64_t jet_fixture_shared_counter = 0;", "extern int64_t jet_fixture_shared_counter;"));
assert.notEqual(missing.result.status, 0, "an unresolved inline C symbol must not build");
assert.ok(missing.result.stderr.includes("inline C bodies leave undefined symbols"), missing.result.stderr);
assert.ok(missing.result.stderr.includes("jet_fixture_shared_counter"), missing.result.stderr);
assert.ok(!existsSync(join(missing.cwd, "main")), "a refusal must not leave an executable");
console.log("native inline C shared state and whole-bridge refusal: passed");
