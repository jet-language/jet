#!/usr/bin/env node
// Run the static x86-64 executables the fixture unit wrote (see
// run-lower-fixtures.mjs) and compare stdout and exit status with the
// fixtures' Jet meaning. With JET_RUNTIME_C_LIB naming the compiled
// runtime's C-ABI static library, also link each `<name>.o` against it with
// the system C compiler (`cc`, or $CC), and load each in-memory image
// `<name>.image` into a process that carries the same runtime
// (load-memory-image.c), and check both the same way. With JET_RUN naming
// the command that runs jet (a jet binary or a wrapper such as safe-jet.sh)
// as well, also link the runtime into a shared library and load each image
// in process from Jet code (`jet run load/load.jet <name> <library>`, the
// Jet loader on the Jet OS layer).
//
// usage: [JET_RUNTIME_C_LIB=libjet_runtime_c.a [JET_RUN=jet]] node Compiler/JetBackend/Tests/check-native-fixtures.mjs <outdir>
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const outDir = process.argv[2];
if (!outDir) {
  console.error("usage: check-native-fixtures.mjs <outdir>");
  process.exit(64);
}
const runtimeLib = process.env.JET_RUNTIME_C_LIB;
const jetRun = process.env.JET_RUN;

const expected = [
  { name: "hello", stdout: "hello, world\n" },
  { name: "arithmetic", stdout: "18\n" },
  { name: "control-flow", stdout: "0\n1\n2\nsum = 15\ntrue\n" },
  { name: "aggregates", stdout: "7\n5\n12\ntrue\np.x = 3\n" },
];

let failures = 0;
function check(label, path, stdout, args = [], options = {}) {
  const run = spawnSync(path, args, { encoding: "utf8", timeout: 10000, ...options });
  const ok = run.status === 0 && run.stdout === stdout;
  if (!ok) failures += 1;
  console.log(`${label}: ${ok ? "ok" : "FAIL"} (exit ${run.status ?? run.signal})`);
  if (!ok) {
    console.log(`  expected stdout ${JSON.stringify(stdout)}`);
    console.log(`  actual stdout   ${JSON.stringify(run.stdout)}`);
    if (run.stderr) console.log(`  stderr ${JSON.stringify(run.stderr)}`);
  }
}

// These witnesses need no runtime library: dirty ABI bytes enter generated
// callees directly, and a tiny C host checks the linked object's data address.
for (const place of ["register", "stack"]) {
  for (const [value, status] of [["false", 0], ["true", 1]]) {
    const name = `bool-${place}-${value}`;
    const run = spawnSync(`${outDir}/${name}.elf`, [], { encoding: "utf8", timeout: 10000 });
    const ok = run.status === status && run.stdout === "";
    if (!ok) failures += 1;
    console.log(`${name}: ${ok ? "ok" : "FAIL"} (exit ${run.status ?? run.signal}, expected ${status})`);
  }
}
check("align64 (static image)", `${outDir}/align64.elf`, "");
const alignmentSource = `${dirname(fileURLToPath(import.meta.url))}/check-alignment.c`;
const alignmentProgram = `${outDir}/align64.hosted`;
const alignmentLink = spawnSync(process.env.CC ?? "cc", [alignmentSource, `${outDir}/align64.o`, "-o", alignmentProgram], { encoding: "utf8" });
if (alignmentLink.status !== 0) {
  console.log(`align64 (linked object): LINK FAILED\n${alignmentLink.stderr}`);
  failures += 1;
} else {
  check("align64 (linked object)", alignmentProgram, "");
}

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
  const link = spawnSync(process.env.CC ?? "cc", [loaderSource, runtimeLib, "-rdynamic", ...keep, "-lpthread", "-ldl", "-lm", "-o", loader], { encoding: "utf8" });
  if (link.status !== 0) {
    console.log(`${fixture.name} (in-memory image): LOADER LINK FAILED\n${link.stderr}`);
    failures += 1;
    return;
  }
  check(`${fixture.name} (in-memory image)`, loader, fixture.stdout, [image, manifest]);
}

// The Jet loader binds imports with dlsym on the runtime's shared library,
// so the static library is linked whole into one.
let sharedRuntime = null;
if (runtimeLib && jetRun) {
  sharedRuntime = resolve(outDir, "libjet_runtime_c.so");
  const link = spawnSync(process.env.CC ?? "cc", ["-shared", "-o", sharedRuntime, "-Wl,--whole-archive", runtimeLib, "-Wl,--no-whole-archive", "-lpthread", "-ldl", "-lm"], { encoding: "utf8" });
  if (link.status !== 0) {
    console.log(`shared runtime: LINK FAILED\n${link.stderr}`);
    failures += 1;
    sharedRuntime = null;
  }
}

for (const fixture of process.argv.includes("--abi") ? [] : expected) {
  const path = `${outDir}/${fixture.name}.elf`;
  if (!existsSync(path)) {
    console.log(`${fixture.name}: MISSING ${path}`);
    failures += 1;
    continue;
  }
  check(`${fixture.name} (static image)`, path, fixture.stdout);
  if (!runtimeLib) continue;
  checkMemoryImage(fixture);
  if (sharedRuntime) {
    check(`${fixture.name} (in-process Jet loader)`, jetRun, fixture.stdout, ["run", "load/load.jet", fixture.name, sharedRuntime], { cwd: outDir, timeout: 600000 });
  }
  const program = `${outDir}/${fixture.name}.hosted`;
  const link = spawnSync(process.env.CC ?? "cc", [`${outDir}/${fixture.name}.o`, runtimeLib, "-lpthread", "-ldl", "-lm", "-o", program], { encoding: "utf8" });
  if (link.status !== 0) {
    console.log(`${fixture.name} (compiled runtime): LINK FAILED\n${link.stderr}`);
    failures += 1;
    continue;
  }
  check(`${fixture.name} (compiled runtime)`, program, fixture.stdout);
}
console.log(failures === 0 ? "all native fixtures match" : `${failures} native fixture check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
