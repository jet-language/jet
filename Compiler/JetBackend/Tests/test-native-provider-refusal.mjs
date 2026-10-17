#!/usr/bin/env node
// Run only against the native CLI with its admitted, Rust-free pack beside it.
// TLS is refused during lowering; a retained non-native WS route is refused by
// the production linker and both failures must use registered CLI E2104.
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
assert.ok(key && key !== "." && key !== ".." && !key.includes("/"), "pack key must name one directory");
assert.ok(existsSync(join(runtime, key, "jet_runtime.pack.o")), "native CLI must have its admitted pack");
for (const [name, expected] of [
  ["native_tls_refusal", "TLS transport is not available in native builds until Jet's own TLS implementation lands (D-NATIVE-TLS1=B); use plain HTTP or TCP meanwhile"],
  ["native_missing_ws_provider", "undefined symbol `jet_ws_connect`, referenced by"],
]) {
  const cwd = join(out, name); mkdirSync(cwd, {recursive: true});
  writeFileSync(join(cwd, "main.jet"), readFileSync(join(repo, "tests/fixtures", name + ".jet")));
  writeFileSync(join(cwd, "package.jet"), 'name: "' + name + '"\nversion: "0.0.1"\nauthority: { holds: { allow: [Net, Time, IO, Mem.Alloc] } }\n');
  const result = spawnSync(jet, ["build", "main.jet"], {cwd, env: {...process.env, NO_COLOR: "1"}, encoding: "utf8", timeout: 300000, maxBuffer: 64 << 20});
  writeFileSync(join(cwd, "stdout"), result.stdout ?? ""); writeFileSync(join(cwd, "stderr"), result.stderr ?? "");
  assert.equal(result.error, undefined, String(result.error)); assert.equal(result.signal, null);
  assert.notEqual(result.status, 0, "unavailable native member must not build");
  assert.match(result.stderr, /Error \[E2104\]/); assert.ok(result.stderr.includes(expected), result.stderr);
  assert.ok(!existsSync(join(cwd, "main")), "a refusal must not leave an executable");
}
console.log("native TLS and missing-provider E2104 refusals: passed");
