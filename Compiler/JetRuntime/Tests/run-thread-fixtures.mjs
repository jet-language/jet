#!/usr/bin/env node
// L8a-1: actual Jet source, checked/lowered by the composed Jet candidate.
// Separate modes permit one held keep-going proof: assemble, lower (including
// negative intrinsic witnesses), audit, link, run. Standalone uses no Rust;
// shared-link/shared-run exercise the complete Jet-authored Shared runtime.
// rust-link/rust-run are retired reference modes (Tower #4923): UNCOVERED, exit 2.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { candidateJetc0 } from "../../JetBackend/Tests/jet-front.mjs";
import { runNativeLibrary } from "../../../Tools/agent/compiler-native-transport.mjs";
import { parseRuntimeExports, runtimeLifecycleList } from "../../../Tools/agent/runtime-route-abi.mjs";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const [output, mode] = process.argv.slice(2);
if (["rust-link", "rust-run"].includes(mode)) {
  console.log(JSON.stringify({ status: "UNCOVERED", cardRequired: true, reason: `${mode}: Rust-spawned attachment fixture (thread-rust-fixture.rs against the Rust native-pack rlib) retired (owner #38/#59; Tower #4923); no rustc invoked; no native replacement covers it` }));
  process.exit(2);
}
if (!output || !["assemble", "lower", "audit", "link", "run", "shared-link", "shared-run"].includes(mode)) {
  throw Error("usage: run-thread-fixtures.mjs OUT assemble|lower|audit|link|run|shared-link|shared-run");
}
const out = resolve(output);
const read = (path) => readFileSync(join(repo, path), "utf8");
const sources = [
  "Compiler/JetBackend/Source/OS/Linux.jet",
  "Compiler/JetRuntime/Source/Alloc.jet", "Compiler/JetRuntime/Source/TrapMessages.jet", "Compiler/JetRuntime/Source/TrapDefectMessages.jet",
  "Compiler/JetRuntime/Source/Trap.jet", "Compiler/JetRuntime/Source/Sync.jet",
  "Compiler/JetRuntime/Source/Thread.jet", "Compiler/JetRuntime/Tests/Fragments.fixture/ThreadTests.jet",
];
function command(bin, args, options = {}) {
  const result = spawnSync(bin, args, { encoding: "utf8", maxBuffer: 1 << 27, ...options });
  if (result.error || result.status !== 0) throw Error(`${bin}: ${result.status ?? result.signal}\n${result.error?.message ?? ""}\n${result.stdout ?? ""}${result.stderr ?? ""}`);
  return result;
}
if (mode === "assemble") {
  mkdirSync(join(out, "good/Source"), { recursive: true });
  // The exact-Int panic-list-index export belongs to L4a, not this word-only
  // fixture. Keep its real body unexported and unreachable; omit only that
  // annotation, so nm can require precisely the six ledgered musl imports.
  const complete = sources.map((path) => read(path)
    .replace(/^#Export\(c\) fn jet_rt_panic_list_bounds/m, "fn jet_rt_panic_list_bounds")
    // Scheduler hooks are outside this deliberately thread-only fixture unit.
    .replace(/^    scheduler_thread_detach\(\)\n/m, "")
    .replace(/^    sentry_thread_detach\(\)\n/m, "")
    .replace(/^    if rt_task_panic_publish\(message, message_len, I64\{0\}\) -> return\n/gm, "")
    // This unit has no task scheduler or Text/Collections implementation.
    // Strip only task-publication branches; retain the real root reports.
    .replace(/^fn trap_native_index_range_stop\(\) \{\n[\s\S]*?^\}/m,
      "fn trap_native_index_range_stop() { trap_native_index_range_root() }")
    .replace(/^#Export\(c\) fn jet_rt_panic_overflow\(\) \{\n[\s\S]*?^    if published -> return\n/m,
      "#Export(c) fn jet_rt_panic_overflow() {\n")
    .replace(/^    \/\/ Outcome\.rs:2463-2466[\s\S]*?^    if published -> return\n/m, "")
    .replace(/^        if \(jet_scheduler_native_propagate\(\) != I64\{0\}\) -> return\n/gm, "")
  ).join("\n");
  // The Shared factory, wrapper, import and private address intrinsic form
  // one explicit block. They belong only to the actual-Shared fixture, never
  // to the standalone musl/Jet witness or its pthread-only import census.
  const sharedBlock = /(?:\/\/ Interim Shared ABI import dies at L8a-4[\s\S]*?)?^#Export\(c\) fn jet_rt_tls_shared_destructor[\s\S]*?(?=^fn thread_tls_destroy)/m;
  if (!sharedBlock.test(complete)) throw Error("missing Shared TLS source block");
  const unit = complete.replace(sharedBlock, "");
  const shared = read("Compiler/JetRuntime/sources.list").split("\n").map(path => path.trim())
    .filter(path => path && !path.startsWith("#")).map(read).join("\n")
    + "\n" + read("Compiler/JetRuntime/Tests/Fragments.fixture/SharedThreadTests.jet");
  // Duplicate #Export(c) names are fatal in the good and shared fixture units.
  for (const text of [unit, shared]) {
    const names = [...text.matchAll(/^#Export\(c\)\s+fn\s+(\w+)\(/gm)].map((match) => match[1]);
    if (new Set(names).size !== names.length) throw Error("duplicate fixture exports");
  }
  mkdirSync(join(out, "shared/Source"), { recursive: true });
  writeFileSync(join(out, "shared/Source/Fixture.jet"), shared);
  writeFileSync(join(out, "shared/package.jet"), read("Compiler/JetRuntime/package.jet"));
  writeFileSync(join(out, "shared/lifecycle.list"), runtimeLifecycleList(parseRuntimeExports(shared, "Source/Fixture.jet")));
  writeFileSync(join(out, "good/Source/Fixture.jet"), unit);
  writeFileSync(join(out, "good/package.jet"), read("Compiler/JetRuntime/package.jet"));
  writeFileSync(join(out, "good/lifecycle.list"), runtimeLifecycleList(parseRuntimeExports(unit, "Source/Fixture.jet")));
  // Real malformed checked inputs: retaining the intrinsic requires the
  // driver to reject a missing C entry or an entry with a different ABI.
  for (const [name, source] of [
    ["missing", unit.replace(/^(#Export\(c\)\s+fn )jet_rt_thread_entry(?=\()/m, "$1fixture_wrong_entry")],
    ["signature", unit.replace("fn jet_rt_thread_entry(start: I64)", "fn jet_rt_thread_entry(start: I64, _extra: I64)")],
  ]) {
    mkdirSync(join(out, name, "Source"), { recursive: true });
    // The unused extra word stays semantically valid so refusal must come
    // from the runtime intrinsic validator, not from the ordinary checker.
    writeFileSync(join(out, name, "Source/Fixture.jet"), source);
    writeFileSync(join(out, name, "lifecycle.list"), runtimeLifecycleList(parseRuntimeExports(source, "Source/Fixture.jet")));
    writeFileSync(join(out, name, "package.jet"), read("Compiler/JetRuntime/package.jet"));
  }
} else if (mode === "lower") {
  const failures = [];
  for (const name of ["good", "shared", "missing", "signature"]) {
    const dir = join(out, name), prefix = join(dir, "fixture");
    try {
      const { problem } = runNativeLibrary({ jet: candidateJetc0(), file: join(dir, "Source/Fixture.jet"), prefix, lifecycle: join(dir, "lifecycle.list"), env: { ...process.env, JET_TOOLCHAIN_ROOT: repo, JET_STORE_DIR: join(out, "store"), RUST_MIN_STACK: "8388608" } });
      if (name === "missing" || name === "signature") {
        const issues = existsSync(prefix + ".issues") ? readFileSync(prefix + ".issues", "utf8") : "";
        if (existsSync(prefix + ".o") || !issues.includes("rt_thread_entry_address")) throw Error(`negative witness did not refuse the thread-entry intrinsic: ${issues}`);
      } else {
        if (problem) throw Error(problem.detail);
      }
    } catch (error) { failures.push(`${name}: ${error.message}`); }
  }
  if (failures.length) throw Error(failures.join("\n"));
} else if (mode === "audit") {
  const object = join(out, "good/fixture.o");
  const imports = command("nm", ["-u", "--format=posix", object]).stdout;
  writeFileSync(join(out, "imports.nm"), imports);
  const expected = ["pthread_attr_destroy", "pthread_attr_init", "pthread_attr_setstacksize", "pthread_create", "pthread_detach", "pthread_join"];
  const actual = imports.trim().split("\n").filter(Boolean).map((line) => line.split(/\s+/)[0]).sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) throw Error(`unledgered/missing imports: ${actual.join(",")}`);
  const symbols = command("nm", ["-g", "--defined-only", "--format=posix", object]).stdout;
  writeFileSync(join(out, "exports.nm"), symbols);
  if (!/^jet_rt_thread_entry T /m.test(symbols)) throw Error("missing C thread entry export");
  writeFileSync(join(out, "disassembly.txt"), command("objdump", ["-dr", object]).stdout);
} else if (mode === "link") {
  const musl = process.env.JET_PACK_MUSL_LIB;
  if (!musl || !existsSync(join(musl, "libc.a"))) throw Error("JET_PACK_MUSL_LIB must name the pack's musl static library directory");
  command(nativeCCompiler, ["-static", "-nostdlib", "-no-pie", join(musl, "crt1.o"), join(musl, "crti.o"),
    join(repo, "Compiler/JetRuntime/Tests/thread-fixture.c"), join(out, "good/fixture.o"), join(musl, "libc.a"), "-lgcc", "-lgcc_eh", join(musl, "crtn.o"), "-o", join(out, "thread-fixture")]);
} else if (mode === "shared-link") {
  command(nativeCCompiler, ["-DJET_SHARED_TLS_FIXTURE", join(repo, "Compiler/JetRuntime/Tests/thread-fixture.c"),
    join(out, "shared/fixture.o"), "-lpthread", "-ldl", "-lm", "-o", join(out, "shared-thread-fixture")]);
} else if (mode === "shared-run") {
  const result = command(join(out, "shared-thread-fixture"), [], { timeout: 30000 });
  writeFileSync(join(out, "shared-result.txt"), `status=${result.status}\nstdout=${JSON.stringify(result.stdout)}\nstderr=${JSON.stringify(result.stderr)}\n`);
  if (result.stdout !== "" || result.stderr !== "") throw Error("Shared attempt fixture stdout/stderr changed");
} else {
  const result = command(join(out, "thread-fixture"), [], { timeout: 3600000 });
  writeFileSync(join(out, "result.txt"), `status=${result.status}\nstdout=${JSON.stringify(result.stdout)}\nstderr=${JSON.stringify(result.stderr)}\n`);
  if (result.stdout !== "tls ok\n" || result.stderr !== "") throw Error("thread fixture stdout/stderr changed");
}
console.log(`thread fixtures ${mode}: ok`);
