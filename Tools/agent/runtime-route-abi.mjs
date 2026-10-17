import { runtimeAbiAtom } from "./runtime-abi-symbols.mjs";
// The native route and the Jet definition share a C ABI, not a Jet value type.
import { readFileSync } from "node:fs";
import { join } from "node:path";

// A concrete record application retains its declaration and ordered arguments.
// Generic function monomorphization is separate; these helpers only instantiate
// a declaration's stored field / variant types at a concrete ABI boundary.
export function recordApplication(type) {
  const match = /^((?:[A-Za-z_]\w*::)*(?:[A-Z]\w*|__jet_\w+))(?:<([\s\S]*)>)?$/.exec(type.replace(/\s+/g, ""));
  if (!match) return null;
  // RUST-EXIT S5: seed-only Rust prelude/container codecs, removed at cutover,
  // are not emitted crate declarations.
  // Keep explicit crate::Option (and source-qualified names) nominal: only
  // the actual builtin spelling/provenance bypasses record specialization.
  if (match[2] !== undefined && /^(?:Option|Result|Vec|(?:std|core)::(?:option::Option|result::Result|vec::Vec))$/.test(match[1])) return null;
  if (match[2] === undefined) return { base: match[1], args: [] };
  // Nested declaration arguments are not yet representable in the route key.
  // Reject them explicitly rather than confusing their commas with parameters.
  // () is a real TYPE, not a nested tuple. Parsing it claims no Unit codec.
  if (/[<>()[\]]/.test(match[2].replace(/\(\)/g, ""))) throw new Error(`generic record ${type}: nested or structural arguments are unsupported`);
  const args = match[2].split(",");
  if (args.some((arg) => !arg)) throw new Error(`generic record ${type}: empty argument`);
  return { base: match[1], args };
}

export function recordBindings(header, args, type) {
  const match = /^(?:struct|enum)\s+[A-Za-z_]\w*\s*(?:<([^<>]*)>)?/.exec(header);
  if (!match) throw new Error(`generic record ${type}: unparsed declaration`);
  const params = match[1] === undefined ? [] : match[1].split(",").map((param) => {
    const parsed = /^\s*([A-Za-z_]\w*)(?:\s*:\s*[^=]+)?\s*$/.exec(param);
    if (!parsed) throw new Error(`generic record ${type}: unsupported declaration parameter ${param.trim()}`);
    return parsed[1];
  });
  if (params.length !== args.length) throw new Error(`generic record ${type}: expected ${params.length} declaration arguments, got ${args.length}`);
  if (new Set(params).size !== params.length) throw new Error(`generic record ${type}: duplicate declaration parameter`);
  return new Map(params.map((param, index) => [param, args[index]]));
}

export function substituteRecordType(type, bindings) {
  // Replace complete type identifiers, never substrings of a field/type name.
  return type.replace(/\b[A-Za-z_]\w*\b/g, (name) => bindings.get(name) ?? name);
}

export function recordInstanceName(base, args) {
  // Injective argument encoding: qualified names and underscores cannot alias
  // another argument list. Argument-free names are unchanged.
  return base.split("::").pop() + args.map((arg) => `__${Buffer.from(arg).toString("hex")}`).join("");
}
// Syntax::generated_path (and Jet Emit/Hardware.jet) gives a source nominal
// one injective Rust identity. A caller still requires its actual declaration.
export function generatedRecordPath(name) {
  const escapes = new Map([[95, "u"], [46, "d"], [58, "c"], [47, "s"], [92, "b"], [45, "h"]]);
  let out = "__jet_";
  for (const byte of Buffer.from(name)) {
    out += byte >= 48 && byte <= 57 || byte >= 65 && byte <= 90 || byte >= 97 && byte <= 122
      ? String.fromCharCode(byte) : `_${escapes.get(byte) ?? `x${byte.toString(16).padStart(2, "0")}`}`;
  }
  return out;
}

export function recordApplicationsIn(signature, base) {
  const applications = [];
  const pattern = new RegExp(`\\b(?:[A-Za-z_]\\w*::)*${base.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*<`, "g");
  for (const match of signature.matchAll(pattern)) {
    let end = match.index + match[0].length, depth = 1;
    for (; end < signature.length && depth > 0; end++) {
      if (signature[end] === "<") depth++;
      else if (signature[end] === ">") depth--;
    }
    if (depth !== 0) throw new Error(`generic record ${base}: unterminated application in ${signature}`);
    applications.push(signature.slice(match.index, end).replace(/\s+/g, ""));
  }
  return applications;
}


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

// lifecycle.list text for a set of parsed #Export(c) definitions: one
// `symbol init|fini N` row per `// runtime-init|fini: N` hook, symbol order.
// A library object built from a subset of the runtime sources passes the rows
// of exactly the definitions it compiles.
export function runtimeLifecycleList(definitions) {
  const rows = [...definitions].filter(([, { lifecycle }]) => lifecycle)
    .map(([symbol, { lifecycle }]) => `${symbol} ${lifecycle}`).sort();
  return [
    "# Runtime object lifecycle hooks: <#Export(c) symbol> init|fini <priority 0..65535>.",
    "# Lower init priorities run first (.init_array); fini rows run at exit (.fini_array).",
    "# Rows only order hooks; the export set is exactly the sources' #Export(c) functions.",
    "# Generated by Tools/agent/gen-runtime-lifecycle.mjs from `// runtime-init|fini: N` comments.",
    ...rows,
  ].join("\n") + "\n";
}

export function parseRuntimeExports(source, path) {
  const exports = new Map();
  // Read the complete declaration so a generic, write parameter, or failure
  // suffix cannot be silently erased before validation.
  for (const match of source.matchAll(/^(?:\/\/ runtime-(init|fini): (\d+)\n)?#Export\(c\)\s+(?:pub\s+)?fn\s+([A-Za-z_]\w*)\s*([^{}]*?)\{/gm)) {
    const [, lifecycleKind, lifecyclePriority, name, signature] = match;
    if (lifecycleKind && Number(lifecyclePriority) > 65535) throw new Error(`${path}: lifecycle priority exceeds 65535`);
    const parsed = /^\(([\s\S]*)\)\s*([\s\S]*)$/.exec(signature.trim());
    if (!parsed) throw new Error(`unparsed runtime #Export(c) ${name} signature in ${path}`);
    if (exports.has(name)) throw new Error(`duplicate runtime #Export(c) ${name}`);
    const [, params, suffix] = parsed;
    const items = params.trim() ? params.split(",").map((param) => param.trim()).filter(Boolean) : [];
    const widened = maskedWordParameters.get(name);
    if (widened) {
      // The reviewed allowance requires the actual parameter's mask in this
      // export body. Comments, strings and another function are not evidence.
      const begin = match.index + match[0].length;
      const code = source.slice(begin).replace(/\/\/[^\n]*|\/\*[\s\S]*?\*\/|"(?:[^"\\]|\\.)*"/g, "");
      let depth = 1, end = 0;
      for (; end < code.length && depth > 0; end++) {
        if (code[end] === "{") depth++;
        else if (code[end] === "}") depth--;
      }
      if (depth !== 0) throw new Error(`${path}: unterminated masked export ${name}`);
      const body = code.slice(0, end);
      for (const [index, rust] of widened) {
        const param = /^([A-Za-z_]\w*)\s*:\s*I64$/.exec(items[index]);
        if (!param) continue; // An already-narrow primitive needs no widening.
        const bits = { u8: 8, u16: 16, u32: 32 }[rust];
        if (!new RegExp(`\\b${param[1]}\\s*\\.\\s*bit_and\\s*\\(\\s*I64\\s*\\{\\s*${2 ** bits - 1}\\s*\\}\\s*\\)`).test(body)) {
          throw new Error(`${path}: ${name} parameter ${index} requires the reviewed low-${bits}-bit mask`);
        }
      }
    }
    exports.set(name, { params: items, suffix: suffix.trim(), path, lifecycle: lifecycleKind ? `${lifecycleKind} ${Number(lifecyclePriority)}` : "" });
  }
  return exports;
}

// Signedness is not part of register passing. Pointer words may use either
// I64 or U64 in vetted runtime internals; narrow boolean carriers stay narrow.
function accepts(rust, jet) {
  const type = rust.replace(/\s+/g, "");
  if (type === "i64" || type === "u64" || type === "usize" || type === "isize" || type === "JetCString" || type.startsWith("*mut") || type.startsWith("*const")) return jet === "I64" || jet === "U64";
  if (type === "bool" || type === "u8") return jet === "Bool" || jet === "U8";
  if (type === "f64") return jet === "Float";
  return false;
}

// Reviewed PARAMETER-only widening. HostNet's resolver constructor masks the
// low byte before testing the flag (e5157d499); 0x100 is false, 0x101 is true.
// Return/out carriers and every unlisted symbol remain strict.
// HostNetAddress/Resolver18eb2f82f discard high port/flow/scope bits at the
// boundary with the same explicit unsigned masks.
const maskedWordParameters = new Map([
  ["jet_rt_record_new_jet_resolve_error", new Map([[1, "u8"]])],
  ["jet_net_raw_socket_from_parts", new Map([[1, "u16"], [2, "u32"], [3, "u32"]])],
  ["jet_net_raw_resolve_ordered", new Map([[1, "u16"]])],
]);

export function validateRuntimeRoute(adapter, definition) {
  const parameters = adapter.params.flatMap((param) => param.ctys);
  const expected = [...parameters, ...adapter.result.outs.map((out) => `*mut ${out.cty}`)];
  const actual = definition.params.map((param) => /^[A-Za-z_]\w*\s*:\s*(I64|U64|Float|Bool|U8)$/.exec(param)?.[1]);
  const suffix = definition.suffix;
  const result = suffix === "" ? null : /^->\s*(I64|U64|Float|Bool|U8)$/.exec(suffix)?.[1];
  const unit = suffix === "" || /^->\s*\(\s*\)$/.test(suffix);
  const allowed = maskedWordParameters.get(adapter.name);
  const valid = actual.length === expected.length && actual.every((type, index) =>
    accepts(expected[index], type) || (index < parameters.length && type === "I64" && allowed?.get(index) === expected[index]))
    && (adapter.result.ret ? accepts(adapter.result.ret, result) : unit);
  if (!valid) {
    const wanted = `(${expected.join(", ")})${adapter.result.ret ? ` -> ${adapter.result.ret}` : ""}`;
    const found = `(${definition.params.join(", ")})${suffix ? ` ${suffix}` : ""}`;
    throw new Error(`Jet runtime route ${adapter.symbol}: ${definition.path} export ${adapter.name}${found} does not match C ABI ${wanted}`);
  }
}

// Lower/CForeign.jet: lower_c_import_call calls these raw helpers directly,
// not through a Core route or a fabricated Rust kernel. This table checks the
// actual C ABI of the existing calls (pack/len 126/128, unpack 156, free 173).
export const loweringRuntimeAbi = [
  { name: "jet_rt_c_string_pack", params: ["i64"], ret: "i64", site: "Compiler/JetBackend/Source/Lower/CForeign.jet:126" },
  { name: "jet_rt_string_byte_len", params: ["i64"], ret: "i64", site: "Compiler/JetBackend/Source/Lower/CForeign.jet:128" },
  { name: "jet_rt_c_string_unpack", params: ["i64"], ret: "i64", site: "Compiler/JetBackend/Source/Lower/CForeign.jet:156" },
  { name: "jet_rt_c_string_free", params: ["i64", "i64"], ret: null, site: "Compiler/JetBackend/Source/Lower/CForeign.jet:173" },
];

export function validateLoweringRuntimeAbi(definitions) {
  // Earlier runtime compositions without CString do not acquire new symbols.
  if (!definitions.has("jet_rt_c_string_pack") && !definitions.has("jet_rt_c_string_unpack") && !definitions.has("jet_rt_c_string_free")) return;
  for (const { name, params, ret, site } of loweringRuntimeAbi) {
    const definition = definitions.get(name);
    if (!definition) throw new Error(`${site}: missing Jet lowering helper ${name}`);
    validateRuntimeRoute({ symbol: `${name} (${site})`, name, params: params.map((cty) => ({ ctys: [cty] })), result: { ret, outs: [] } }, definition);
  }
}

// Native opaque providers have no Rust record declaration. Admit their
// ownership metadata only from the canonical physical ABI, never a marker
// struct or a guessed Rust type. Pipe24 is shared by these two nominals.
export function nativeProviderHandles(definitions) {
  const types = ["ProcessStdoutStream", "ProcessStderrStream"];
  const lifecycle = types.flatMap((type) => [`jet_rt_handle_clone_${runtimeAbiAtom(type)}`, `jet_rt_handle_drop_${runtimeAbiAtom(type)}`]);
  if (!lifecycle.some((name) => definitions.has(name))) return [];
  const required = [
    ...types.flatMap((type) => [
      { name: `jet_rt_handle_clone_${runtimeAbiAtom(type)}`, params: ["i64"], ret: "i64" },
      { name: `jet_rt_handle_drop_${runtimeAbiAtom(type)}`, params: ["i64"], ret: null },
    ]),
    { name: "jet_process_pipe_take_raw", params: ["i64"], ret: "i64" },
    { name: "jet_process_stream_line_bytes_raw", params: ["i64", "i64", "i64", "i64"], ret: "i64" },
  ];
  for (const { name, params, ret } of required) {
    const definition = definitions.get(name);
    if (!definition) throw new Error(`native Pipe24 provider requires ${name}`);
    validateRuntimeRoute({ symbol: name, name, params: params.map((cty) => ({ ctys: [cty] })), result: { ret, outs: [] } }, definition);
  }
  return types.flatMap((runtime) => [
    { jetName: `<corelib>/Core/process::Core/process/process.jet::${runtime}`, runtime },
    { jetName: runtime, runtime },
  ]);
}
