import { runtimeAbiAtom } from "./runtime-abi-symbols.mjs";
// Native handle exports describe existing Lower operations, not a second
// representation or renderer. gen-c-abi-routes owns source classification;
// only rows with an actual Jet declaration may enter this partition.
const operations = ["clone", "eq", "display", "show", "debug"];

export function nativeHandleExports(types) {
  const exports = [];
  const names = new Set();
  for (const type of types) {
    if (!type.source) continue;
    if (!type.source.path || !type.source.key || !type.jetName || !type.runtime) {
      throw new Error("native handle requires its actual source declaration and concrete identity");
    }
    for (const op of ["drop", ...operations.filter((op) => type[op])]) {
      const name = `jet_rt_handle_${runtimeAbiAtom(op)}_${runtimeAbiAtom(type.runtime)}`;
      if (names.has(name)) throw new Error(`duplicate native handle export ${name}`);
      names.add(name);
      exports.push({ name, runtime: type.runtime, jetName: type.jetName, op });
    }
  }
  return exports;
}

// The native pack removes ALL source-owned adapters. A surviving caller asks
// Lower for typed glue; symbols with no caller have no replacement definition.
// The Rust seed keeps its original adapter behind cfg(not(jet_native_pack));
// its ledger row dies at cutover.
export function partitionNativeHandleExports(types, callers) {
  if (!(callers instanceof Map)) throw new Error("native handle callers must be keyed by exact C symbol");
  const descriptors = nativeHandleExports(types);
  const synthesized = [];
  const deleted = [];
  for (const descriptor of descriptors) {
    const sites = callers.get(descriptor.name) ?? [];
    if (!Array.isArray(sites) || sites.some((site) => typeof site !== "string" || !site)) {
      throw new Error(`native handle ${descriptor.name}: invalid caller sites`);
    }
    if (sites.length) synthesized.push({ ...descriptor, callers: [...new Set(sites)] });
    else deleted.push(descriptor);
  }
  return { descriptors, synthesized, deleted };
}
