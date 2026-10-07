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

export function validateRuntimeOwnership(symbols, definitions) {
  for (const name of symbols) {
    if (!definitions.has(name)) throw new Error(`Jet runtime export ${name} has no #Export(c) declaration in sources.list`);
  }
  for (const name of definitions.keys()) {
    if (!symbols.has(name)) throw new Error(`Jet runtime #Export(c) ${name} is missing from exports.list; run gen-runtime-exports.mjs`);
  }
}

export function parseRuntimeExports(source, path) {
  const exports = new Map();
  // Read the complete declaration so a generic, write parameter, or failure
  // suffix cannot be silently erased before validation.
  for (const match of source.matchAll(/^#Export\(c\)\s+(?:pub\s+)?fn\s+([A-Za-z_]\w*)\s*([^{}]*?)\{/gm)) {
    const [, name, raw] = match;
    const signature = raw.trim();
    let depth = 0;
    let close = -1;
    if (signature.startsWith("(")) {
      for (let index = 0; index < signature.length; index++) {
        if (signature[index] === "(") depth++;
        else if (signature[index] === ")" && --depth === 0) { close = index; break; }
      }
    }
    if (close < 0) throw new Error(`unparsed runtime #Export(c) ${name} signature in ${path}`);
    if (exports.has(name)) throw new Error(`duplicate runtime #Export(c) ${name}`);
    const params = signature.slice(1, close).trim();
    const items = params ? params.split(",").map((param) => param.trim()) : [];
    if (items.at(-1) === "") items.pop(); // A single trailing comma is valid.
    exports.set(name, { params: items, suffix: signature.slice(close + 1).trim(), path });
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
  const unit = suffix === "" || /^->\s*\(\s*\)$/.test(suffix);
  const valid = actual.length === expected.length && actual.every((type, index) => accepts(expected[index], type)) && (adapter.result.ret ? accepts(adapter.result.ret, result) : unit);
  if (!valid) {
    const wanted = `(${expected.join(", ")})${adapter.result.ret ? ` -> ${adapter.result.ret}` : ""}`;
    const found = `(${definition.params.join(", ")})${suffix ? ` ${suffix}` : ""}`;
    throw new Error(`Jet runtime route ${adapter.symbol}: ${definition.path} export ${adapter.name}${found} does not match C ABI ${wanted}`);
  }
}
