#!/usr/bin/env node
// Check object-first executables linked with the explicitly admitted fresh Jet
// pack. No freestanding or C-library fallback. An explicit JET_RUNTIME_C_LIB
// optionally exercises supplementary seed-only memory/C rails; it never
// supplies the executable used for the primary fixture verdict.
// usage: node Compiler/JetBackend/Tests/check-native-fixtures.mjs <outdir>
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const outDir = process.argv[2];
if (!outDir) {
  console.error("usage: check-native-fixtures.mjs <outdir>");
  process.exit(64);
}
const runtimeLib = process.env.JET_RUNTIME_C_LIB;
const jetRun = process.env.JET_RUN;
const routeSource = readFileSync(`${dirname(fileURLToPath(import.meta.url))}/../Source/Lower/RuntimeRoutes.jet`, "utf8");
// Match run-lower-fixtures.mjs's focused assembly selection.
const only = new Set((process.env.FX_ONLY ?? "").split(",").filter(Boolean));

// The hand-built fixtures of LowerFixtures.jet, then every
// `// expect-stdout <name>: <JSON string>` line of Tests/Fragments.fixture/Fixtures/*.jet.
const expected = [
  { name: "hello", stdout: "hello, world\n" },
  { name: "arithmetic", stdout: "18\n" },
  { name: "control-flow", stdout: "0\n1\n2\nsum = 15\ntrue\n" },
  { name: "aggregates", stdout: "7\n5\n12\ntrue\np.x = 3\n" },
  { name: "bits", stdout: "0\n2\n0\n0\n1\n" },
];
const fixtureDir = `${dirname(fileURLToPath(import.meta.url))}/Fixtures`;
if (existsSync(fixtureDir)) {
  const excluded = new Set((process.env.FX_EXCLUDE ?? "").split(",").filter(Boolean).map((name) => `${name}.jet`));
  for (const file of readdirSync(fixtureDir).filter((name) => name.endsWith(".jet") && !excluded.has(name)).sort()) {
    for (const line of readFileSync(`${fixtureDir}/${file}`, "utf8").split("\n")) {
      const match = /^\/\/ expect-stdout ([\w-]+): (".*")\s*$/.exec(line);
      if (match) expected.push({ name: match[1], stdout: JSON.parse(match[2]), status: 0 });
      // `// expect-exit <name>: <status>` (after its expect-stdout line): a
      // fixture that stops the program, e.g. 70 for a runtime stop.
      const exit = /^\/\/ expect-exit ([\w-]+): (\d+)\s*$/.exec(line);
      if (exit) expected.find((fixture) => fixture.name === exit[1]).status = Number(exit[2]);
      // Hand-compiled foreign ABI driver has an object but no memory image.
      const objectOnly = /^\/\/ expect-object-only ([\w-]+)\s*$/.exec(line);
      if (objectOnly) expected.find((fixture) => fixture.name === objectOnly[1]).objectOnly = true;
      // Never treat a missing native pack executable as a skipped witness.
      const runtimeOnly = /^\/\/ expect-runtime-only ([\w-]+)\s*$/.exec(line);
      if (runtimeOnly) expected.find((fixture) => fixture.name === runtimeOnly[1]).runtimeOnly = true;
      const requiresRoute = /^\/\/ expect-requires-route ([\w-]+) ([A-Za-z0-9_]+)\s*$/.exec(line);
      if (requiresRoute) expected.find((fixture) => fixture.name === requiresRoute[1]).requiresRoute = requiresRoute[2];
      // `// expect-stderr <name>: <JSON string>` (after its expect-stdout
      // line): the exact stderr, e.g. a stop report; unchecked when absent.
      const stderr = /^\/\/ expect-stderr ([\w-]+): (".*")\s*$/.exec(line);
      if (stderr) expected.find((fixture) => fixture.name === stderr[1]).stderr = JSON.parse(stderr[2]);
    }
  }
}

// Plain and root callback panic reports must remain byte-identical to the
// existing panic-message witness, not merely begin with a diagnostic code.
for (const name of ["panic-task-plain", "panic-root-checked"]) {
  const fixture = expected.find((row) => row.name === name);
  if (fixture) fixture.stderr = expected.find((row) => row.name === "panic-message").stderr;
}

let failures = 0;
// `stderr` is compared only for the executables the fixture build links
// itself: the in-process Jet loader shares its stderr with jet.
function check(label, path, fixture, args = [], options = {}, compareStderr = true) {
  const status = fixture.status ?? 0;
  const stderr = compareStderr ? fixture.stderr : undefined;
  const run = spawnSync(path, args, { encoding: "utf8", timeout: 10000, ...options });
  const ok = run.status === status && run.stdout === fixture.stdout && (stderr === undefined || run.stderr === stderr)
    && (!fixture.stderrContains || run.stderr.includes(fixture.stderrContains));
  if (!ok) failures += 1;
  console.log(`${label}: ${ok ? "ok" : "FAIL"} (exit ${run.status ?? run.signal})`);
  if (!ok) {
    console.log(`  expected stdout ${JSON.stringify(fixture.stdout)}, exit ${status}`);
    console.log(`  actual stdout   ${JSON.stringify(run.stdout)}`);
    if (stderr !== undefined) console.log(`  expected stderr ${JSON.stringify(stderr)}`);
    if (run.stderr) console.log(`  stderr ${JSON.stringify(run.stderr)}`);
  }
}

// Dirty ABI transport and datum alignment both use the admitted Jet pack.
for (const place of ["register", "stack"]) {
  for (const [value, status] of [["false", 0], ["true", 1]]) {
    const name = `bool-${place}-${value}`;
    const run = spawnSync(`${outDir}/${name}.exe`, [], { encoding: "utf8", timeout: 10000 });
    const ok = run.status === status && run.stdout === "";
    if (!ok) failures += 1;
    console.log(`${name}: ${ok ? "ok" : "FAIL"} (exit ${run.status ?? run.signal}, expected ${status})`);
  }
}
check("align64 (Jet pack)", `${outDir}/align64.exe`, { stdout: "", status: 0 });
check("word-entry-37 (Jet adapter)", `${outDir}/word-entry-37.exe`, { stdout: "", status: 37 });
for (const [name, status] of [["i32-entry-minus1", 255], ["u32-entry-max", 255], ["bool-entry-true", 1]]) {
  check(`${name} (Jet adapter)`, `${outDir}/${name}.exe`, { stdout: "", status });
}
// align64.o/runtime-exports.o remain separate export/initializer writer witnesses.

// Link one loader per fixture: `-rdynamic` exports the runtime's symbols to
// dlsym, and `-u` keeps every export the image imports.
const loaderSource = `${dirname(fileURLToPath(import.meta.url))}/load-memory-image.c`;
function checkMemoryImage(fixture) {
  const image = `${outDir}/${fixture.name}.image`;
  const manifest = `${outDir}/${fixture.name}.mem`;
  if (!existsSync(image) || !existsSync(manifest)) {
    console.log(`${fixture.name} (in-memory image): MISSING ${image}`);
    failures += 1;
    return;
  }
  const keep = readFileSync(manifest, "utf8")
    .split("\n")
    .filter((line) => line.startsWith("import "))
    .map((line) => `-Wl,-u,${line.split(" ")[2]}`);
  const loader = `${outDir}/${fixture.name}.loader`;
  const link = spawnSync(nativeCCompiler, [loaderSource, runtimeLib, "-rdynamic", ...keep, "-lpthread", "-ldl", "-lm", "-o", loader], { encoding: "utf8" });
  if (link.status !== 0) {
    console.log(`${fixture.name} (in-memory image): LOADER LINK FAILED\n${link.stderr}`);
    failures += 1;
    return;
  }
  check(`${fixture.name} (in-memory image)`, loader, fixture, [image, manifest]);
}

// The Jet loader binds imports with dlsym on the runtime's shared library,
// so the static library is linked whole into one.
let sharedRuntime = null;
if (runtimeLib && jetRun) {
  sharedRuntime = resolve(outDir, "libjet_runtime_c.so");
  const link = spawnSync(nativeCCompiler, ["-shared", "-o", sharedRuntime, "-Wl,--whole-archive", runtimeLib, "-Wl,--no-whole-archive", "-lpthread", "-ldl", "-lm"], { encoding: "utf8" });
  if (link.status !== 0) {
    console.log(`shared runtime: LINK FAILED\n${link.stderr}`);
    failures += 1;
    sharedRuntime = null;
  }
}

for (const fixture of process.argv.includes("--abi") ? [] : expected.filter(({ name }) => only.size === 0 || only.has(name))) {
  if (fixture.requiresRoute && !routeSource.includes(`symbol: "${fixture.requiresRoute}"`)) {
    console.log(`${fixture.name}: REFUSED (requires canonical route ${fixture.requiresRoute}; lowering refusal witness runs in unit.jet)`);
    continue;
  }
  const path = `${outDir}/${fixture.name}.exe`;
  if (!existsSync(path)) {
    console.log(`${fixture.name}: MISSING required Jet pack executable ${path}`);
    failures += 1;
    continue;
  }
  check(`${fixture.name} (Jet pack)`, path, fixture);
  if (!runtimeLib || fixture.objectOnly) continue;
  checkMemoryImage(fixture);
  if (sharedRuntime) {
    check(`${fixture.name} (in-process Jet loader)`, jetRun, fixture, ["run", "load/load.jet", fixture.name, sharedRuntime], { cwd: outDir, timeout: 600000 }, false);
  }
  const program = `${outDir}/${fixture.name}.hosted`;
  const taskHost = fixture.nativeTask ? [`${dirname(fileURLToPath(import.meta.url))}/check-trap-task.c`, "-Wl,--wrap=jet_rt_main",
    ...(fixture.taskPass ? ["-DJET_FX_TASK_PASS"] : []),
    ...(fixture.taskMessage ? [`-DJET_FX_TASK_MESSAGE=${JSON.stringify(fixture.taskMessage)}`] : [])] : [];
  const link = spawnSync(nativeCCompiler, [`${outDir}/${fixture.name}.o`, ...taskHost, runtimeLib, "-lpthread", "-ldl", "-lm", "-o", program], { encoding: "utf8" });
  if (link.status !== 0) {
    console.log(`${fixture.name} (compiled runtime): LINK FAILED\n${link.stderr}`);
    failures += 1;
    continue;
  }
  check(`${fixture.name} (compiled runtime)`, program, fixture);
  if (fixture.name === "flat-map-owned" || fixture.name === "list-try-collect") {
    const rail = `${dirname(fileURLToPath(import.meta.url))}/check-collector-alloc.c`;
    const balanced = `${outDir}/${fixture.name}.balanced`;
    const balanceLink = spawnSync(nativeCCompiler, [`${outDir}/${fixture.name}.o`, rail, runtimeLib, "-Wl,--wrap=main,--wrap=jet_rt_alloc,--wrap=jet_rt_free", "-lpthread", "-ldl", "-lm", "-o", balanced], { encoding: "utf8" });
    if (balanceLink.status !== 0) {
      console.log(`${fixture.name} (allocator balance): LINK FAILED\n${balanceLink.stderr}`);
      failures += 1;
    } else {
      check(`${fixture.name} (allocator balance)`, balanced, { ...fixture, stderr: "collector-alloc: balanced live=0\n" });
    }
  }
}

// Stop points (#4720): stop-points.o linked with the compiled runtime and a
// scripted session rail (check-stop-points.c) that stops it at chosen polls;
// stderr counts the program's String from static text and its drops.
if (runtimeLib && !process.argv.includes("--abi") && (only.size === 0 || only.has("stop-points")) && existsSync(`${outDir}/stop-points.o`)) {
  const rail = `${dirname(fileURLToPath(import.meta.url))}/check-stop-points.c`;
  const program = `${outDir}/stop-points.rail`;
  const link = spawnSync(nativeCCompiler, [`${outDir}/stop-points.o`, rail, runtimeLib, "-Wl,--wrap=jet_rt_string_from_static,--wrap=jet_rt_string_drop", "-lpthread", "-ldl", "-lm", "-o", program], { encoding: "utf8" });
  if (link.status !== 0) {
    console.log(`stop-points (session rail): LINK FAILED\n${link.stderr}`);
    failures += 1;
  } else {
    const cases = [
      ["no stop", {}, "42\n0\n1\n6\n10\nkept\n", "polls=8 made=1 dropped=1\n"],
      ["stop at run's entry Check", { JET_FX_STOP_AT: "1" }, "", "polls=1 made=0 dropped=0\n"],
      ["stop at twice's entry Check, propagated", { JET_FX_STOP_AT: "2" }, "", "polls=2 made=0 dropped=0\n"],
      ["stop at label's entry Check, before its String", { JET_FX_STOP_AT: "3" }, "42\n", "polls=3 made=0 dropped=0\n"],
      ["stop at the first loop Check", { JET_FX_STOP_AT: "4" }, "42\n", "polls=4 made=1 dropped=1\n"],
      ["stop at the second loop Check", { JET_FX_STOP_AT: "5" }, "42\n0\n", "polls=5 made=1 dropped=1\n"],
      ["pending inside the Shield, delivered at the next Check", { JET_FX_STOP_AT: "7" }, "42\n0\n1\n6\n", "polls=8 made=1 dropped=1\n"],
      ["signal before label's Propagate: result kept, dropped once at the next Check", { JET_FX_SIGNAL_AT_PROPAGATE: "2" }, "42\n", "polls=4 made=1 dropped=1\n"],
    ];
    for (const [label, env, stdout, stderr] of cases) {
      check(`stop-points (session rail: ${label})`, program, { name: "stop-points", stdout, stderr }, [], { env: { ...process.env, ...env } });
    }
  }
}
if (runtimeLib && !process.argv.includes("--abi")) {
  for (const [name, description] of [
    ["record-sequences", "borrowed storage, duplicate order, byte boundaries"],
    ["codecs", "canonical scalar policy, typed parse, error ownership"],
  ]) {
    const source = `${dirname(fileURLToPath(import.meta.url))}/check-${name}.c`;
    const program = `${outDir}/${name}.hosted`;
    const link = spawnSync(nativeCCompiler, [source, runtimeLib, "-lpthread", "-ldl", "-lm", "-o", program], { encoding: "utf8" });
    if (link.status !== 0) {
      console.log(`${name}: LINK FAILED\n${link.stderr}`);
      failures += 1;
    } else {
      check(`${name} (${description})`, program, { stdout: "", status: 0 });
    }
  }
}

// Stop points (#4720): stop-points.o linked with the compiled runtime and a
// scripted session rail (check-stop-points.c) that stops it at chosen polls;
// stderr counts the program's String from static text and its drops.
if (runtimeLib && !process.argv.includes("--abi") && existsSync(`${outDir}/stop-points.o`)) {
  const rail = `${dirname(fileURLToPath(import.meta.url))}/check-stop-points.c`;
  const program = `${outDir}/stop-points.rail`;
  const link = spawnSync(nativeCCompiler, [`${outDir}/stop-points.o`, rail, runtimeLib, "-Wl,--wrap=jet_rt_string_from_static,--wrap=jet_rt_string_drop", "-lpthread", "-ldl", "-lm", "-o", program], { encoding: "utf8" });
  if (link.status !== 0) {
    console.log(`stop-points (session rail): LINK FAILED\n${link.stderr}`);
    failures += 1;
  } else {
    const cases = [
      ["no stop", {}, "42\n0\n1\n6\n10\nkept\n", "polls=8 made=1 dropped=1\n"],
      ["stop at run's entry Check", { JET_FX_STOP_AT: "1" }, "", "polls=1 made=0 dropped=0\n"],
      ["stop at twice's entry Check, propagated", { JET_FX_STOP_AT: "2" }, "", "polls=2 made=0 dropped=0\n"],
      ["stop at label's entry Check, before its String", { JET_FX_STOP_AT: "3" }, "42\n", "polls=3 made=0 dropped=0\n"],
      ["stop at the first loop Check", { JET_FX_STOP_AT: "4" }, "42\n", "polls=4 made=1 dropped=1\n"],
      ["stop at the second loop Check", { JET_FX_STOP_AT: "5" }, "42\n0\n", "polls=5 made=1 dropped=1\n"],
      ["pending inside the Shield, delivered at the next Check", { JET_FX_STOP_AT: "7" }, "42\n0\n1\n6\n", "polls=8 made=1 dropped=1\n"],
      ["signal before label's Propagate: result kept, dropped once at the next Check", { JET_FX_SIGNAL_AT_PROPAGATE: "2" }, "42\n", "polls=4 made=1 dropped=1\n"],
    ];
    for (const [label, env, stdout, stderr] of cases) {
      check(`stop-points (session rail: ${label})`, program, { name: "stop-points", stdout, stderr }, [], { env: { ...process.env, ...env } });
    }
  }
}
if (existsSync(`${outDir}/lists-swap-oob.elf`)) {
  const ordinary = spawnSync(`${outDir}/lists-oob.elf`, [], { encoding: "utf8", timeout: 10000 });
  const swap = spawnSync(`${outDir}/lists-swap-oob.elf`, [], { encoding: "utf8", timeout: 10000 });
  const same = ordinary.status === 70 && swap.status === ordinary.status && swap.stderr === ordinary.stderr;
  if (!same) failures += 1;
  console.log(`lists-swap ordinary-index stop parity: ${same ? "ok" : "FAIL"}`);
}
if (runtimeLib && !process.argv.includes("--abi")) {
  const source = `${dirname(fileURLToPath(import.meta.url))}/check-record-sequences.c`;
  const program = `${outDir}/record-sequences.hosted`;
  const link = spawnSync(nativeCCompiler, [source, runtimeLib, "-lpthread", "-ldl", "-lm", "-o", program], { encoding: "utf8" });
  if (link.status !== 0) {
    console.log(`record-sequences: LINK FAILED\n${link.stderr}`);
    failures += 1;
  } else {
    check("record-sequences (borrowed storage, duplicate order)", program, { stdout: "", status: 0 });
  }
}
if (existsSync(`${outDir}/lists-swap-oob.elf`)) {
  const ordinary = spawnSync(`${outDir}/lists-oob.elf`, [], { encoding: "utf8", timeout: 10000 });
  const swap = spawnSync(`${outDir}/lists-swap-oob.elf`, [], { encoding: "utf8", timeout: 10000 });
  const same = ordinary.status === 70 && swap.status === ordinary.status && swap.stderr === ordinary.stderr;
  if (!same) failures += 1;
  console.log(`lists-swap ordinary-index stop parity: ${same ? "ok" : "FAIL"}`);
}
console.log(failures === 0 ? "all native fixtures match" : `${failures} native fixture check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
