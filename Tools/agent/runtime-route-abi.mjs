// The native route and the Jet definition share a C ABI, not a Jet value type.
import { readFileSync } from "node:fs";
import { join } from "node:path";

export function runtimeExportSignatures(root) {
  const sources = readFileSync(join(root, "Compiler/JetRuntime/sources.list"), "utf8")
    .split("\n").map((line) => line.trim()).filter((line) => line && !line.startsWith("#"));
  const exports = new Map();
  for (const path of sources) {
    const source = readFileSync(join(root, path), "utf8");
    for (const [name, definition] of parseRuntimeExports(source, path)) {
      if (exports.has(name)) throw new Error(`duplicate runtime #Export(c) ${name}`);
      exports.set(name, definition);
    }
  }
  if (!exports.size) throw new Error("empty runtime #Export(c) declarations");
  return exports;
}

export function parseRuntimeExports(source, path) {
  const exports = new Map();
  // Read the complete declaration so a generic, write parameter, or failure
  // suffix cannot be silently erased before validation.
  for (const match of source.matchAll(/^#Export\(c\)\s+(?:pub\s+)?fn\s+([A-Za-z_]\w*)\s*([^{}]*?)\{/gm)) {
    const [, name, signature] = match;
    const parsed = /^\(([\s\S]*)\)\s*([\s\S]*)$/.exec(signature.trim());
    if (!parsed) throw new Error(`unparsed runtime #Export(c) ${name} signature in ${path}`);
    if (exports.has(name)) throw new Error(`duplicate runtime #Export(c) ${name}`);
    const [, params, suffix] = parsed;
    exports.set(name, { params: params.trim() ? params.split(",").map((param) => param.trim()).filter(Boolean) : [], suffix: suffix.trim(), path });
  }
  return exports;
}

// Signedness is not part of register passing. Pointer words may use either
// I64 or U64 in vetted runtime internals; narrow boolean carriers stay narrow.
function accepts(rust, jet) {
  const type = rust.replace(/\s+/g, "");
  if (type === "i64" || type === "u64" || type === "usize" || type === "isize" || type === "JetCString" || type.startsWith("*mut") || type.startsWith("*const")) return jet === "I64" || jet === "U64";
  if (type === "bool" || type === "u8") return jet === "Bool" || jet === "U8";
  if (type === "f64") return jet === "F64";
  return false;
}

export function validateRuntimeRoute(adapter, definition) {
  const expected = [...adapter.params.flatMap((param) => param.ctys), ...adapter.result.outs.map((out) => `*mut ${out.cty}`)];
  const actual = definition.params.map((param) => /^[A-Za-z_]\w*\s*:\s*(I64|U64|F64|Bool|U8)$/.exec(param)?.[1]);
  const suffix = definition.suffix;
  const result = suffix === "" ? null : /^->\s*(I64|U64|F64|Bool|U8)$/.exec(suffix)?.[1];
  const valid = actual.length === expected.length && actual.every((type, index) => accepts(expected[index], type)) && (adapter.result.ret ? accepts(adapter.result.ret, result) : suffix === "");
  if (!valid) {
    const wanted = `(${expected.join(", ")})${adapter.result.ret ? ` -> ${adapter.result.ret}` : ""}`;
    const found = `(${definition.params.join(", ")})${suffix ? ` ${suffix}` : ""}`;
    throw new Error(`Jet runtime route ${adapter.symbol}: ${definition.path} export ${adapter.name}${found} does not match C ABI ${wanted}`);
  }
}
