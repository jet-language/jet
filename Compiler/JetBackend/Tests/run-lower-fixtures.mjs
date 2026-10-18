#!/usr/bin/env node
// Assemble the MIR -> LIR -> x86-64 fixture unit (Tests/LowerFixtures.jet)
// with the sources it needs, as one package-free Jet file, and the in-process
// loader unit (Tests/LoadImages.jet) beside it.
//
// usage: JET_RUNTIME_PACK=<admitted fresh Jet pack> node Compiler/JetBackend/Tests/run-lower-fixtures.mjs <outdir>
// Append --abi to assemble just the ABI/alignment witnesses, or --sanitize
// for just the sanitize-mode witnesses (Tests/SanitizeFixtures.jet).
// --callable-statics assembles the production callable lifetime witness:
// jet run unit.jet <admitted libjet_runtime_c.so> (three "héllo" lines + pass).
// --foreign-descriptors runs descriptor lowering/lint/object witnesses only;
// it needs no runtime pack because it does not link an executable.
// --disjoint-lowering lowers and lints the production disjoint MIR witnesses
// without linking; native execution still requires the admitted runtime pack.
// --tls-refusal checks the canonical TLS transport refusal without a pack.
// --route-carriers lowers/lints the exact-Int and Dyn route carrier witnesses
// (Tests/Fixtures/RouteCarriers.jet) without a pack.
// --list-routes lowers/lints the four stage1-required canonical List witnesses
// without a pack; the ordinary fixture run executes their native images.
// --assemble-only emits the complete unit for a frontend check without a pack.
// It does not execute any native image or qualify a runtime.
// then:  cd <outdir> && jet run unit.jet        (writes <fixture>.elf files)
// then:  node Compiler/JetBackend/Tests/check-native-fixtures.mjs <outdir>
//        (with JET_RUNTIME_C_LIB, it also runs every image through load.jet)
//
// MIR.jet names three Foundation items (Span, Effect, ParamZone with
// param_zone_name) whose files pull in the whole diagnostics registry; their
// exact definitions are copied out of those files instead.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const outDir = process.argv[2];
if (!outDir) {
  console.error("usage: run-lower-fixtures.mjs <outdir>");
  process.exit(64);
}
const runtimePack = process.env.JET_RUNTIME_PACK;
if (!process.argv.includes("--assemble-only") && !sanitizeOnlyArgument() && !process.argv.includes("--foreign-descriptors") && !process.argv.includes("--disjoint-lowering") && !process.argv.includes("--tls-refusal") && !process.argv.includes("--route-carriers") && !process.argv.includes("--list-routes") && !runtimePack) throw new Error("JET_RUNTIME_PACK must explicitly name an admitted fresh Jet pack");
function sanitizeOnlyArgument() { return process.argv.includes("--sanitize"); }
// While other edits are in flight: FX_HEAD (comma-separated repo paths) reads
// those files as committed at HEAD, and FX_EXCLUDE (comma-separated file
// names without `.jet`) leaves those Lower and Fixtures files out.
const headPaths = new Set((process.env.FX_HEAD ?? "").split(",").filter(Boolean));
const excluded = new Set((process.env.FX_EXCLUDE ?? "").split(",").filter(Boolean).map((name) => `${name}.jet`));
// FX_ONLY (comma-separated fixture names) keeps only those one-line
// fx_report* calls in run(), for a small probe of one fixture.
const only = new Set((process.env.FX_ONLY ?? "").split(",").filter(Boolean));
const read = (path) => headPaths.has(path)
  ? execFileSync("git", ["show", `HEAD:${path}`], { cwd: repo, encoding: "utf8", maxBuffer: 1 << 28 })
  : readFileSync(`${repo}/${path}`, "utf8");

// The top-level item starting at the line that begins with `head`, through its
// closing `}` at column 0 (or the line itself for a one-line item).
function item(path, head) {
  const lines = read(path).split("\n");
  const start = lines.findIndex((line) => line.startsWith(head));
  if (start < 0) throw new Error(`${path}: no item starting with ${JSON.stringify(head)}`);
  if (!lines[start].trimEnd().endsWith("{")) return lines[start];
  const end = lines.findIndex((line, index) => index > start && line === "}");
  return lines.slice(start, end + 1).join("\n");
}

// Units are single-package: drop `use jet_*` items, one line or a bracketed
// block; `use core.*` items are collected for the head of the unit.
const coreUses = new Set();
function body(path) {
  const kept = [];
  let skipping = false;
  for (const line of read(path).split("\n")) {
    if (callableStatics && path === "Compiler/JetBackend/Tests/LowerFixtures.jet" && line.startsWith("fn run() {")) break;
    if (skipping) {
      if (line.startsWith("]") || line === "}") skipping = false;
      continue;
    }
    // This Sema-only projection is not part of the backend fixture closure.
    // Keeping it would require the entire semantic Type declaration graph.
    if (path === "Compiler/JetFoundation/Source/MIR/MIR.jet" && line.startsWith("fn mir_typed_head_kind(")) {
      skipping = true;
      continue;
    }
    if (/^use jet_\w+\.\[\s*$/.test(line)) {
      skipping = true;
      continue;
    }
    if (/^use jet_\w+/.test(line)) continue;
    if (/^use core\./.test(line)) {
      coreUses.add(line);
      continue;
    }
    const report = /fx_report\w*\("([\w-]+)"/.exec(line);
    if (only.size > 0 && report && !only.has(report[1])) continue;
    kept.push(line);
  }
  return `// [unit source: ${path}]\n${kept.join("\n")}\n`;
}

// Every lowering file (Lower.jet and its sibling family files) and every
// fixture file under Tests/Fixtures, in name order.
const sorted = (dir) => existsSync(`${repo}/${dir}`) ? readdirSync(`${repo}/${dir}`).filter((name) => name.endsWith(".jet") && !excluded.has(name)).sort().map((name) => `${dir}/${name}`) : [];
// Feed the exact audited OS templates into the hand-MIR witnesses; do not
// maintain a second copy that could diverge from the OS layer.
const osSource = read("Compiler/JetBackend/Source/OS/Linux.jet");
const asmLeaves = ["os_syscall", "os_store_word", "os_load_word", "os_load_byte", "os_call_address", "os_call_address_two", "os_call_address_unit", "os_call_address_one_unit", "os_call_address_two_unit", "os_call_address_three_unit", "os_syscall_i64", "os_load_i64", "os_store_i64", "os_cas_i64", "os_spin_pause", "os_load_u8", "os_store_u8", "os_write_word_i64", "os_syscall_stack4_i64", "os_tls_base_i64", "os_fetch_add_i64", "os_swap_i64", "os_call_address_i64", "os_call_address_two_i64", "os_call1_unit_i64", "os_call2_unit_i64", "os_call3_unit_i64", "os_clone_i64"];
const asmSpans = asmLeaves.map((name) => {
  const start = osSource.indexOf(`fn ${name}(`);
  const open = osSource.indexOf('"""', start);
  const close = osSource.indexOf('"""', open + 3);
  if (start < 0 || open < 0 || close < 0) throw new Error(`missing OS asm leaf ${name}`);
  return `    if name == "${name}" -> return Span{start: ${Buffer.byteLength(osSource.slice(0, start))}, end: ${Buffer.byteLength(osSource.slice(0, close + 3))}}`;
});
const asmWitnessSource = `fn fx_asm_os_source() -> String Never! { ${JSON.stringify(osSource).replaceAll("{", "{{").replaceAll("}", "}}")} }\nfn fx_asm_os_span(name: String) -> Span Never! {\n${asmSpans.join("\n")}\n    panic("unknown OS asm witness {name}")\n}\n`;
const abiOnly = process.argv.includes("--abi");
const sanitizeOnly = process.argv.includes("--sanitize");
const callableStatics = process.argv.includes("--callable-statics");
const foreignDescriptors = process.argv.includes("--foreign-descriptors");
const disjointLowering = process.argv.includes("--disjoint-lowering");
const tlsRefusal = process.argv.includes("--tls-refusal");
const listRoutes = process.argv.includes("--list-routes");
const routeCarriers = process.argv.includes("--route-carriers");
if (routeCarriers && (abiOnly || sanitizeOnly || callableStatics || foreignDescriptors || disjointLowering || tlsRefusal || listRoutes)) throw new Error("--route-carriers cannot be combined with other focused modes");
if (listRoutes && (abiOnly || sanitizeOnly || callableStatics || foreignDescriptors || disjointLowering || tlsRefusal)) throw new Error("--list-routes cannot be combined with other focused modes");
if (tlsRefusal && (abiOnly || sanitizeOnly || callableStatics || foreignDescriptors || disjointLowering)) throw new Error("--tls-refusal cannot be combined with other focused modes");
if (disjointLowering && (abiOnly || sanitizeOnly || callableStatics || foreignDescriptors)) throw new Error("--disjoint-lowering cannot be combined with other focused modes");
if (foreignDescriptors && (abiOnly || sanitizeOnly || callableStatics)) throw new Error("--foreign-descriptors cannot be combined with other focused modes");
if (callableStatics && (abiOnly || sanitizeOnly)) throw new Error("--callable-statics cannot be combined with --abi or --sanitize");
const small = abiOnly || sanitizeOnly;
const parts = [
  ...(small ? [
    item("Compiler/JetFoundation/Source/MIR/MIR.jet", "struct MIRTypeID "),
    body("Compiler/JetFoundation/Source/Text/RustDebug.jet"),
  ] : [
    "// [unit source: Foundation items named by MIR.jet]",
    item("Compiler/JetFoundation/Source/Diagnostics/Diagnostic.jet", "struct Span {"),
    item("Compiler/JetFoundation/Source/Registry/CoreCalls.jet", "enum Effect "),
    // MIR.jet's Core-call and layout facts (as run-lint-fixtures.mjs): the Rust emitter needs a declaration row for
    // every nominal type the unit names.
    ...[
      "enum SinkClass ",
      "enum CoreCallFallibility ",
      "enum CoreCallPureRoute {",
      "enum CoreCallInterpreterRoute ",
      "enum CoreCallSymbol ",
      "struct CoreMarkerApplication {",
    ].map((head) => item("Compiler/JetFoundation/Source/Registry/CoreCalls.jet", head)),
    item("Compiler/JetFoundation/Source/Types/Types.jet", "enum ParamZone {"),
    item("Compiler/JetFoundation/Source/Types/Types.jet", "fn param_zone_name("),
    item("Compiler/JetFoundation/Source/Types/Types.jet", "struct SymbolID {"),
    item("Compiler/JetFoundation/Source/Registry/CoreModules.jet", "fn core_source_value_hook("),
    // Lower.jet's bare spelling of a source-owned Core nominal.
    item("Compiler/JetFoundation/Source/Registry/CoreModules.jet", "struct CoreSourceOwnedType {"),
    item("Compiler/JetFoundation/Source/Registry/CoreModules.jet", "fn core_source_owns_type("),
    item("Compiler/JetFoundation/Source/Registry/CoreModules.jet", "fn core_source_owns_type_in("),
    item("Compiler/JetFoundation/Source/Registry/CoreEnumRows.jet", "CORE_SOURCE_OWNED_TYPES ::"),
    ...[
      "struct ByteLayout {",
      "struct FieldLayoutFacts {",
      "struct LayoutFacts {",
    ].map((head) => item("Compiler/JetFoundation/Source/Target/Layout.jet", head)),
    body("Compiler/JetFoundation/Source/DerivationTypes.jet"),
    body("Compiler/JetFoundation/Source/MIR/MIR.jet"),
    body("Compiler/JetFoundation/Source/Text/RustDebug.jet"),
    body("Compiler/JetFoundation/Source/Encoding/Json.jet"),
  ]),
  // Lower.jet, X64/Select.jet and Opt/Opt.jet (the streaming per-function
  // compile) move rows, admit and partition compile threads and re-raise
  // worker panics with these Foundation helpers; their files otherwise hold
  // the Sema collection registry, so only the named items are copied.
  "// [unit source: Foundation helpers named by Lower.jet, X64/Select.jet and Opt/Opt.jet]",
  item("Compiler/JetFoundation/Source/Collections.jet", "fn compiler_take_rows<"),
  item("Compiler/JetFoundation/Source/Threads.jet", "struct CompilerThreads {"),
  item("Compiler/JetFoundation/Source/Threads.jet", "fn compiler_threads_admit("),
  item("Compiler/JetFoundation/Source/Threads.jet", "fn compiler_threads_partition("),
  item("Compiler/JetFoundation/Source/Threads.jet", "fn compiler_threads_failure_text("),
  item("Compiler/JetFoundation/Source/Threads.jet", "fn compiler_threads_single("),
  body("Compiler/JetBackend/Source/LIR/LIR.jet"),
  body("Compiler/JetBackend/Source/LIR/Lint.jet"),
  body("Compiler/JetBackend/Source/UiABI.jet"),
  ...(sanitizeOnly ? [body("Compiler/JetBackend/Source/LIR/Sanitize.jet")] : []),
  ...(!small ? [
    ...(!listRoutes ? [body("Compiler/JetBackend/Source/LIR/Print.jet")] : []),
    body("Compiler/JetBackend/Source/ErrLayout.jet"),
    ...sorted("Compiler/JetBackend/Source/Lower").map(body),
    ...(!listRoutes ? sorted("Compiler/JetBackend/Source/Opt").map(body) : []),
  ] : []),
  ...(!small && !listRoutes ? [asmWitnessSource] : []),
  body("Compiler/JetBackend/Source/X64/Encoder.jet"),
  body("Compiler/JetBackend/Source/X64/InlineAsm.jet"),
  ...(!listRoutes ? [
  body("Compiler/JetBackend/Source/X64/RegAlloc.jet"),
  body("Compiler/JetBackend/Source/X64/Select.jet"),
  body("Compiler/JetBackend/Source/Image/Link.jet"),
  body("Compiler/JetBackend/Source/Image/Runtime.jet"),
  body("Compiler/JetBackend/Source/Image/RuntimeStrings.jet"),
  body("Compiler/JetBackend/Source/Image/ELF.jet"),
  body("Compiler/JetBackend/Source/Image/Object.jet"),
  body("Compiler/JetBackend/Source/Image/Dwarf.jet"),
  body("Compiler/JetBackend/Source/Image/RuntimeObject.jet"),
  ...(!small ? [body("Compiler/JetBackend/Source/Image/RuntimeLibrary.jet")] : []),
  ...(!small ? [body("Compiler/JetBackend/Source/Image/Memory.jet")] : []),
  body("Compiler/JetBackend/Source/Image/Archive.jet"),
  body("Compiler/JetBackend/Source/Image/StaticLink.jet"),
  body("Compiler/JetBackend/Source/Image/RuntimePack.jet"),
  ] : []),
  ...(foreignDescriptors || disjointLowering || tlsRefusal || routeCarriers || listRoutes ? [] : [
    body("Compiler/JetBackend/Tests/TestPack.jet"),
    ...(sanitizeOnly ? [body("Compiler/JetBackend/Tests/SanitizeFixtures.jet"), "fn run() { sanitize_fixtures() }"] : [body("Compiler/JetBackend/Tests/AbiFixtures.jet")]),
  ]),
  ...(listRoutes || disjointLowering ? [
    ...["fn fx_capacity_raw_type(", "fn fx_capacity_raw(", "fn fx_capacity_foreign(", "fn fx_capacity_provider_call("].map((head) => item("Compiler/JetBackend/Tests/Fixtures/Builtins.jet", head)),
  ] : []),
  ...(abiOnly ? ["fn run() { abi_fixtures() }"] : sanitizeOnly ? [] : listRoutes ? [
    body("Compiler/JetBackend/Tests/LowerFixtures.jet").split("fn fx_hello(")[0],
    item("Compiler/JetBackend/Tests/Fixtures/Tuple.jet", "fn fx_tuple_type("),
    item("Compiler/JetBackend/Tests/Fixtures/Tuple.jet", "fn fx_tuple_field_id("),
    body("Compiler/JetBackend/Tests/Fixtures/Lists.jet"),
    'fn run() {\n    assert(fx_generic_runtime_rejection_checks())\n    assert(fx_list_routes_lowering_checks())\n    print("List route lowering fixtures passed")\n}',
  ] : disjointLowering ? [
    body("Compiler/JetBackend/Tests/LowerFixtures.jet").split("fn fx_hello(")[0],
    item("Compiler/JetBackend/Tests/Fixtures/Tuple.jet", "fn fx_tuple_type("),
    item("Compiler/JetBackend/Tests/Fixtures/Tuple.jet", "fn fx_tuple_field_id("),
    item("Compiler/JetBackend/Tests/Fixtures/Closures.jet", "fn fx_capture_facts("),
    item("Compiler/JetBackend/Tests/Fixtures/Closures.jet", "fn fx_capture_place("),
    body("Compiler/JetBackend/Tests/Fixtures/Lists.jet"),
    body("Compiler/JetBackend/Tests/Fixtures/Disjoint.jet"),
    'fn run() { assert(fx_disjoint_lowering_checks()); print("disjoint lowering fixtures passed") }',
  ] : tlsRefusal ? [
    body("Compiler/JetBackend/Tests/LowerFixtures.jet").split("fn fx_hello(")[0],
    body("Compiler/JetBackend/Tests/Fixtures/TLSRefusal.jet"),
    'fn run() { assert(fx_report_tls_refusals("tls-refusal")); print("TLS refusal fixtures passed") }',
  ] : routeCarriers ? [
    body("Compiler/JetBackend/Tests/LowerFixtures.jet").split("fn fx_hello(")[0],
    item("Compiler/JetBackend/Tests/Fixtures/Tuple.jet", "fn fx_tuple_type("),
    body("Compiler/JetBackend/Tests/Fixtures/RouteCarriers.jet"),
    body("Compiler/JetBackend/Tests/Fixtures/NativeValues.jet"),
    'fn run() { assert(fx_route_carrier_checks()); assert(fx_route_native_values_checks()); print("route carrier fixtures passed") }',
  ] : foreignDescriptors ? [
    body("Compiler/JetBackend/Tests/LowerFixtures.jet").split("fn fx_hello(")[0],
    body("Compiler/JetBackend/Tests/Fixtures/Foreign.jet"),
    'fn run() { assert(fx_foreign_descriptor_checks()); print("foreign descriptor fixtures passed") }',
  ] : [body("Compiler/JetBackend/Tests/LowerFixtures.jet"), ...sorted("Compiler/JetBackend/Tests/Fixtures").map(body)]),
  ...(callableStatics ? [
    body("Compiler/JetBackend/Source/OS/Linux.jet"),
    body("Compiler/JetBackend/Source/Image/Loader.jet"),
    body("Compiler/JetBackend/Source/Image/Calls.jet"),
    body("Compiler/JetBackend/Tests/CallableStatics.jet"),
    "fn run() { callable_statics() }",
  ] : []),
];
mkdirSync(outDir, { recursive: true });
if (runtimePack) writeFileSync(`${outDir}/runtime-pack.txt`, `${resolve(runtimePack)}\n`);
writeFileSync(`${outDir}/unit.jet`, [...coreUses, "", ...parts].join("\n"));

// The loader unit needs only the image type, the OS layer and the loader. It
// is its own package (load/), run from <outdir> so it finds the images.
coreUses.clear();
const loadParts = [
  // X64MemoryImage.relocs rows are Link.jet's data relocations.
  "// [unit source: Compiler/JetBackend/Source/Image/Link.jet]",
  item("Compiler/JetBackend/Source/Image/Link.jet", "struct X64ImageDataReloc "),
  "// [unit source: Compiler/JetBackend/Source/Image/Memory.jet]",
  item("Compiler/JetBackend/Source/Image/Memory.jet", "struct X64MemoryImage {"),
  "",
  body("Compiler/JetBackend/Source/OS/Linux.jet"),
  body("Compiler/JetBackend/Source/Image/Loader.jet"),
  body("Compiler/JetBackend/Tests/LoadImages.jet"),
];
const packageSource = (name) => `name: "${name}"\nversion: "0.1.0"\nauthority: { holds: { allow: [Exec, Exec.Args, FFI, FS.Read, FS.Write, IO, Mem.Alloc] } }\n`;
writeFileSync(`${outDir}/package.jet`, packageSource("jet_backend_lower_fixtures"));
mkdirSync(`${outDir}/load`, { recursive: true });
writeFileSync(`${outDir}/load/load.jet`, [...coreUses, "", ...loadParts].join("\n"));
writeFileSync(`${outDir}/load/package.jet`, packageSource("jet_backend_load_images"));
console.log(`${outDir}/unit.jet`);
console.log(`${outDir}/load/load.jet`);
