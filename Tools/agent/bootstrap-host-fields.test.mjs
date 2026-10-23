import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const identity = readFileSync(new URL("../../Compiler/JetDriver/Source/Driver/Identity.jet", import.meta.url), "utf8");
const native = readFileSync(new URL("../../Compiler/Bootstrap/Host/Native.rs", import.meta.url), "utf8");

test("bootstrap host initializes every declared JetDriverCompileRequest field", () => {
  const declaration = identity.match(/^struct JetDriverCompileRequest \{\n([\s\S]*?)^\}/m);
  assert.ok(declaration, "JetDriverCompileRequest declaration must exist");
  const fields = [...declaration[1].matchAll(/^\s+([^\s:]+)\s*:/gm)].map((match) => match[1]);
  assert.ok(fields.length > 0, "request field inventory must not be empty");
  const factory = native.match(/^fn emit_bootstrap_host_factory\([\s\S]*?^\}/m);
  assert.ok(factory, "bootstrap host factory must exist");
  const bindings = new Map([...factory[0].matchAll(/let ([a-z_]+)\s*=\s*symbols\.field_symbol\("JetDriverCompileRequest", "([^"]+)"\)\?/g)]
    .map((match) => [match[2], match[1]]));
  for (const field of fields) {
    const binding = bindings.get(field);
    assert.ok(binding, `${field} requires a checked field-symbol binding`);
    assert.match(factory[0], new RegExp(`" +\\{${binding}\\}:`), `${field} requires an emitted initializer row`);
    assert.match(factory[0], new RegExp(`^ +${binding} = ${binding},$`, "m"), `${field} requires a format argument`);
  }
});

test("bootstrap native and library feeds share the CLI keep-going collector", () => {
  const entry = readFileSync(new URL("../../Compiler/Bootstrap/Host/Entry.jet", import.meta.url), "utf8");
  const request = readFileSync(new URL("../../Compiler/JetCli/Source/Cli/Request.jet", import.meta.url), "utf8");
  const body = (name) => {
    const declaration = entry.match(new RegExp(`^fn ${name}\\([^\\n]*\\{\\n([\\s\\S]*?)^\\}`, "m"));
    assert.ok(declaration, `${name} must exist`);
    return declaration[1];
  };
  const compile = body("jet_bootstrap_compile");
  assert.match(compile, /JET_FRONT_NATIVE_OUTPUT[\s\S]*jet_bootstrap_native\(\^request, prefix\)/);
  assert.match(body("jet_bootstrap_native"), /jet_cli_lowering_inputs\(\^request, None\)/);
  const mir = body("jet_bootstrap_mir");
  assert.match(mir, /jet_cli_compile_request\(\^request, false\)/, "the MIR observation feed keeps the MIR compile");
  assert.ok(mir.indexOf("jet_cli_compile_request") < mir.indexOf("!result.complete"), "MIR observation must collect before refusing an incomplete artifact");
  const library = body("jet_bootstrap_library_object");
  assert.match(library, /jet_cli_lowering_inputs\(\^request, None\)/);
  assert.match(library, /jet_cli_runtime_library\(\^front, prefix, compiler_threads\)/);
  assert.match(compile, /jet_cli_compile_request\(\^request, true\)/);
  assert.doesNotMatch(entry, /jet_driver_compile(?:_mir|_keep_going)?\(\^request/);
  assert.doesNotMatch(entry, /cli_sys\.get\("JET_KEEP_GOING(?:_REPORT|_SKIP)?"\)/);
  assert.match(request, /jet_driver_compile_keep_going\(\^request, JetDriverKeepGoing\{skip: skip\}, emit_rust\)/);
  assert.match(request, /keep_going_rows\.len\(\) > 0 -> result\.complete = false/);
  assert.match(request, /"phase\\titem\\tcode\\tmessage\\tspan"/);
});

test("bootstrap library feed delegates ABI and optional pack production to the CLI", () => {
  const entry = readFileSync(new URL("../../Compiler/Bootstrap/Host/Entry.jet", import.meta.url), "utf8");
  const request = readFileSync(new URL("../../Compiler/JetCli/Source/Cli/Request.jet", import.meta.url), "utf8");
  const library = entry.match(/^fn jet_bootstrap_library_object\([^\n]*\{\n([\s\S]*?)^\}/m);
  assert.ok(library, "bootstrap library feed must exist");
  assert.match(library[1], /jet_cli_runtime_library\(\^front, prefix, compiler_threads\)/);
  assert.doesNotMatch(library[1], /x64_runtime_library|write_bytes|\.exports/);
  const producer = request.match(/^fn jet_cli_runtime_library\([^\n]*\{\n([\s\S]*?)^\}/m);
  assert.ok(producer, "canonical CLI runtime producer must exist");
  assert.match(producer[1], /!result\.complete/);
  assert.match(producer[1], /ir_build_library\(/);
  for (const artifact of [".o", ".exports", ".abi.json"]) assert.ok(producer[1].includes(`{prefix}${artifact}`), `${artifact} must remain a canonical output`);
  assert.ok(producer[1].includes("JET_FRONT_LIBRARY_LIFECYCLE"), "JET_FRONT_LIBRARY_LIFECYCLE must be honored by the shared producer");
  const pack = request.match(/^fn jet_cli_runtime_pack\([^\n]*\{\n([\s\S]*?)^\}/m);
  assert.ok(pack, "canonical CLI runtime pack linker must exist");
  for (const key of ["JET_FRONT_RUNTIME_PACK_OUTPUT", "JET_FRONT_RUNTIME_PLATFORM_INPUTS"]) assert.ok(pack[1].includes(key), `${key} must be honored by the shared producer`);
  assert.match(pack[1], /x64_link_runtime_pack\(names, \^bytes, roots\)/);
});
