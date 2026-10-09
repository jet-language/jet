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
