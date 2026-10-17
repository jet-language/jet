#!/usr/bin/env node
// Static S3 linkage inventory, NOT evidence of MIR reachability or pack admission.
import { readFileSync, readdirSync, writeFileSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";
const [repoArg, output] = process.argv.slice(2);
if (!repoArg || !output) throw new Error("usage: self-host-link-plan.mjs <repo> <LINKPLAN.tsv>");
const repo = resolve(repoArg);
const read = (path) => readFileSync(join(repo, path), "utf8");
const manifest = read("Compiler/Bootstrap/sources.list").split("\n").map((row) => row.trim()).filter((row) => row && !row.startsWith("#"));
const files = new Map(manifest.map((path) => [path, read(path)]));
const routeText = read("Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet");
const routes = new Map([...routeText.matchAll(/LowerRoute\{symbol: "([^"]+)", export: "([^"]+)"/g)].map((match) => [match[1], match[2]]));
const registry = read("Compiler/JetFoundation/Source/Registry/CoreCallRows.jet");
const core = new Map();
for (const match of registry.matchAll(/CoreCallRecord\{module_name: "([^"]+)", member: "([^"]+)", [^\n]*?symbol: CoreCallSymbol\.(?:Prelude|Rust)\{name: "([^"]*)"\}/g)) core.set(`${match[1]}.${match[2]}`, match[3]);
const receiverCandidates = new Map();
for (const match of registry.matchAll(/CoreCallRecord\{module_name: "", member: "([^"]+)", receiver_types: \[String\]\{((?:"[^"]*"(?:, )?)*)\}, [^\n]*?symbol: CoreCallSymbol\.(?:Prelude|Rust)\{name: "([^"]+)"\}/g)) {
  if (!receiverCandidates.has(match[1])) receiverCandidates.set(match[1], []);
  receiverCandidates.get(match[1]).push({ receiver: match[2].replace(/"/g, ""), symbol: match[3] });
}
import { codeOnly } from "./self-host-source.mjs";
const rows = new Map();
function add(kind, route, symbol, path, offset, refusal = "") {
  const key = `${kind}\t${route}\t${symbol}`;
  if (!rows.has(key)) rows.set(key, { kind, route, symbol, sites: [], refusal });
  const line = files.get(path).slice(0, offset).split("\n").length;
  rows.get(key).sites.push(`${path}:${line}`);
}
for (const [path, text] of files) {
  const code = codeOnly(text);
  const aliases = new Map();
  for (const match of code.matchAll(/^use (core(?:\.[A-Za-z_]\w*)+)(?: as ([A-Za-z_]\w*))?\s*$/gm)) {
    aliases.set(match[2] ?? match[1].split(".").at(-1), match[1]);
  }
  // Group imports are recorded explicitly as unresolved instead of dropped.
  for (const match of code.matchAll(/^use (core(?:\.[A-Za-z_]\w*)+)\.\[([^\]]*)\]/gm)) {
    for (const item of match[2].split(",").map((part) => part.trim()).filter(Boolean)) {
      const [member, alias] = item.split(/\s+as\s+/);
      const local = alias ?? member;
      const regex = new RegExp(`\\b${local}\\s*(?:<[^;\\n()]*>)?\\s*\\(`, "g");
      for (const call of code.matchAll(regex)) add("core-imported-call", `${match[1]}.${member}`, core.get(`${match[1]}.${member}`) ?? "UNRESOLVED", path, call.index);
    }
  }
  for (const call of code.matchAll(/\b([A-Za-z_]\w*)\.([A-Za-z_]\w*)\s*(?:<[^;\n()]*>)?\s*\(/g)) {
    const module = aliases.get(call[1]);
    if (module) {
      const route = `${module}.${call[2]}`;
      const refused = route.startsWith("core.testing.") || route === "core.archive.zip_open" || route === "core.web.invalidate" || /^core.compute\.(ones|device_auto|device_vulkan|profile_show|matmul|dense)$/.test(route);
      add("core-module-call", route, core.get(route) ?? "UNRESOLVED", path, call.index, refused ? "semantic O0 refusal in Lower.jet" : "");
    }
    else {
      for (const candidate of receiverCandidates.get(call[2]) ?? []) {
        add("receiver-candidate-untyped", `${candidate.receiver}.${call[2]}`, candidate.symbol, path, call.index);
      }
    }
  }
  // This is a compiler emitter's route reference, not a direct import of the
  // compiler binary itself. It is useful for the subsequent jetc1 runtime plan.
  for (const match of text.matchAll(/\blower_\w*call\w*\s*\([^;\n]*?"(jet_[A-Za-z0-9_{}]+)"/g)) {
    if (code.slice(match.index, match.index + 6) !== "lower_") continue;
    add("emitted-runtime-reference", match[1], match[1], path, match.index);
  }
  for (const match of text.matchAll(/#\[(?:[^\]]*\bFFI\(([^)]+)\)[^\]]*)\]\s*(?:pub )?fn ([A-Za-z_]\w*)/g)) {
    add("foreign-definition", `FFI(${match[1]})`, match[2], path, match.index, "foreign functions are linked, not lowered (baseline Lower.jet)");
    const calls = new RegExp(`\\b${match[2]}\\s*\\(`, "g");
    for (const [callerPath, callerText] of files) {
      const callerCode = codeOnly(callerText);
      for (const call of callerCode.matchAll(calls)) {
        if (/\bfn\s*$/.test(callerCode.slice(Math.max(0, call.index - 12), call.index))) continue;
        add("foreign-call", `FFI(${match[1]})`, match[2], callerPath, call.index, "foreign functions are linked, not lowered (baseline Lower.jet)");
      }
    }
  }
}
// Provider source evidence only. Matching a Jet definition does NOT prove its
// export ABI is admitted, and matching Rust source does NOT approve retaining it.
const jetDefs = new Map();
const rustDefs = new Map();
function collect(directory, extension, target) {
  const absolute = join(repo, directory);
  if (!existsSync(absolute)) return;
  for (const entry of readdirSync(absolute, { withFileTypes: true })) {
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory()) collect(path, extension, target);
    else if (entry.name.endsWith(extension)) {
      const text = read(path);
      for (const match of text.matchAll(/\bfn\s+(jet_[A-Za-z0-9_]+)\s*(?:<[^>]*>)?\s*\(/g)) {
        if (!target.has(match[1])) target.set(match[1], path);
      }
    }
  }
}
collect("Core", ".jet", jetDefs);
collect("crates/jet-codegen/src/Prelude", ".rs", rustDefs);
const result = [...rows.values()].sort((a, b) => b.sites.length - a.sites.length || a.route.localeCompare(b.route) || a.kind.localeCompare(b.kind));
const table = [
  "# [INFERENCE] Static compiler source scan; no MIR reachability, type resolution, native object imports or pack admission measured.",
  `# repo=${repo}; sources.list_files=${files.size}; route_rows=${routes.size}; core_registry_rows=${core.size}`,
  "# Core module aliases are file-local; comments and string bodies excluded. Receiver-candidate rows are syntactic possibilities, NOT resolved Core calls. Interpolation expressions and dynamically built symbols require r2 MIR measurement.",
  "# emitted-runtime-reference rows describe the compiler's output programs, not necessarily the compiler binary's own link requirements.",
  "# native_route_exists checks RuntimeRoutes.jet ONLY; n may mean bespoke lowering or Jet source, not a proven blocker.",
  "# provider records source definitions only: Rust-seed dies at cutover, Jet-source is not an admitted export. FFI(asm) is Jet inline assembly, not a runtime provider.",
  "kind\troute\tsymbol\tcompiler_call_sites\tnative_route_exists\tadapter_export\tprovider_INFERENCE\tprovider_source\tbaseline_refusal\tcall_sites",
];
for (const row of result) {
  const symbol = row.symbol;
  const exported = routes.get(symbol) ?? symbol.split("::").at(-1);
  const jet = jetDefs.get(exported) ?? jetDefs.get(symbol);
  const rust = rustDefs.get(exported) ?? rustDefs.get(symbol);
  const ffi = row.kind.startsWith("foreign-");
  const provider = ffi ? (row.route === "FFI(asm)" ? "Jet-inline-asm" : "foreign-external-source") : jet && rust ? "Jet-source+Rust-seed" : jet ? "Jet-source" : rust ? "Rust-seed" : "unresolved";
  table.push([row.kind, row.route, symbol, row.sites.length, routes.has(symbol) ? "y" : "n", routes.get(symbol) ?? "", provider,
    ffi ? row.sites[0].replace(/:\d+$/, "") : [jet, rust].filter(Boolean).join(";"), row.refusal, row.sites.join(";")].join("\t"));
}
writeFileSync(output, table.join("\n") + "\n");
console.log(`LINKPLAN [INFERENCE] rows=${result.length} core_calls=${result.filter((row) => row.kind.startsWith("core-")).length} no_table_route=${result.filter((row) => !routes.has(row.symbol)).length} ffi=${result.filter((row) => row.kind.startsWith("foreign-")).length}; output=${output}`);
