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
  assert.match(body("jet_bootstrap_native"), /jet_bootstrap_mir\(\^request\)/);
  for (const name of ["jet_bootstrap_mir", "jet_bootstrap_library_object"]) {
    const feed = body(name);
    assert.match(feed, /jet_cli_compile_request\(\^request, false\)/, `${name} must preserve the native MIR backend choice`);
    assert.ok(feed.indexOf("jet_cli_compile_request") < feed.indexOf("!result.complete"), `${name} must collect before refusing an incomplete artifact`);
  }
  assert.match(compile, /jet_cli_compile_request\(\^request, true\)/);
  assert.doesNotMatch(entry, /jet_driver_compile(?:_mir|_keep_going)?\(\^request/);
  assert.doesNotMatch(entry, /cli_sys\.get\("JET_KEEP_GOING(?:_REPORT|_SKIP)?"\)/);
  assert.match(request, /jet_driver_compile_keep_going\(\^request, JetDriverKeepGoing\{skip: skip\}, emit_rust\)/);
  assert.match(request, /keep_going_rows\.len\(\) > 0 -> result\.complete = false/);
  assert.match(request, /"phase\\titem\\tcode\\tmessage\\tspan"/);
});
