#!/usr/bin/env node
// Tiny real-object regression for standalone runtime export roots.
// JET=<admitted compiler> node run-runtime-pack-roots.mjs <scratch-out>
// --prepare-only assembles inputs and the Jet unit without starting a compiler.
import { spawnSync } from "node:child_process";
import { resolve, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [output, mode] = process.argv.slice(2);
if (!output || (mode && mode !== "--prepare-only")) {
  throw new Error("usage: JET=<compiler> node run-runtime-pack-roots.mjs <scratch-out> [--prepare-only]");
}
const compiler = process.env.JET;
if (mode !== "--prepare-only" && !compiler) throw new Error("JET must name the admitted compiler");
const out = resolve(output);
function run(command, args, cwd = out) {
  const result = spawnSync(command, args, { cwd, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed: status=${result.status}, signal=${result.signal}`);
}
run(process.execPath, [join(repo, "Compiler/JetBackend/Tests/run-link.mjs"), out, "--pack-roots"], repo);
const fixtures = join(repo, "Compiler/JetBackend/Tests/Fragments.fixture/Fixtures");
for (const [source, object] of [
  ["object", "runtime.o"], ["archive", "archive.o"],
  ["consumer", "consumer.o"], ["missing", "missing.o"],
]) {
  run("as", ["--64", "-o", join(out, object), join(fixtures, `runtime-pack-roots-${source}.s`)]);
}
run("ar", ["rcs", join(out, "runtime.a"), join(out, "archive.o")]);
if (mode === "--prepare-only") {
  console.log("runtime pack root inputs prepared (no compiler or consumer executed)");
} else {
  run(resolve(compiler), ["run", "unit.jet"]);
  run(join(out, "consumer"), []);
  console.log("runtime pack root consumer: pass");
}
