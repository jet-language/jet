#!/usr/bin/env node
// Resolve static no-table-route references against a pinned dev Core tree.
// Categories are source evidence only: class a is NOT native execution proof.
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { codeOnly, functionsIn } from "./self-host-source.mjs";
const [devArg, compilerArg, planPath, output] = process.argv.slice(2);
if (!output) throw new Error("usage: self-host-resolve-links.mjs <dev tree> <compiler tree> <LINKPLAN.tsv> <LINKPLAN-RESOLVED.tsv>");
const dev = resolve(devArg), compiler = resolve(compilerArg);
const read = (repo, path) => readFileSync(join(repo, path), "utf8");
const routes = new Set([...read(dev, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet").matchAll(/LowerRoute\{symbol: "([^"]+)"/g)].map((match) => match[1]));
const registry = new Map();
for (const match of read(dev, "Compiler/JetFoundation/Source/Registry/CoreCallRows.jet").matchAll(/CoreCallRecord\{module_name: "([^"]+)", member: "([^"]+)", [^\n]*?symbol: CoreCallSymbol\.(?:Prelude|Rust)\{name: "([^"]*)"\}/g)) registry.set(`${match[1]}.${match[2]}`, match[3]);
const byQualified = new Map(), bySymbol = new Map(), texts = new Map();
function gather(directory) {
  for (const entry of readdirSync(join(dev, directory), { withFileTypes: true })) {
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory()) { gather(path); continue; }
    if (!entry.name.endsWith(".jet")) continue;
    const text = read(dev, path);
    texts.set(path, text);
    const parts = path.slice(5, -4).split("/");
    if (parts.length > 1 && parts.at(-1) === parts.at(-2)) parts.pop();
    const module = `core.${parts.join(".")}`;
    const aliases = new Map([...codeOnly(text).matchAll(/^use (core(?:\.[A-Za-z_]\w*)+)(?: as ([A-Za-z_]\w*))?\s*$/gm)].map((match) => [match[2] ?? match[1].split(".").at(-1), match[1]]));
    for (const fn of functionsIn(text, path)) {
      fn.module = module; fn.aliases = aliases;
      byQualified.set(`${module}.${fn.name}`, fn);
      if (fn.name.startsWith("jet_")) bySymbol.set(fn.name, fn);
    }
  }
}
gather("Core");
const namedCalls = (body) => [...body.matchAll(/\b([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*)\s*(?:<[^;\n()]*>)?\s*\(/g)].map((match) => match[1]);
function resolveBody(start) {
  const visited = new Set(), leaves = new Set(), sourceBodies = new Set();
  const work = [start];
  while (work.length) {
    const fn = work.pop();
    const identity = `${fn.path}:${fn.line}`;
    if (visited.has(identity)) continue;
    visited.add(identity); sourceBodies.add(identity);
    for (const call of namedCalls(fn.body)) {
      let target = call;
      const parts = call.split(".");
      const explicitCore = call.startsWith("core.");
      const aliased = fn.aliases.get(parts[0]);
      if (aliased) target = `${aliased}.${parts.slice(1).join(".")}`;
      if (explicitCore || aliased) {
        // An explicit Core provider call is a raw registered leaf, even when
        // its public Jet wrapper has the same name (e.g. core.files.stat).
        const symbol = registry.get(target);
        if (symbol) { leaves.add(symbol); continue; }
        const body = byQualified.get(target);
        if (body && body !== fn) work.push(body);
        else leaves.add(`UNRESOLVED:${target}`);
      } else {
        const local = byQualified.get(`${fn.module}.${call}`);
        if (local) work.push(local);
        else if (call.startsWith("jet_")) leaves.add(call);
      }
    }
  }
  return { leaves, sourceBodies };
}
const plan = readFileSync(planPath, "utf8").split("\n").filter((row) => row && !row.startsWith("#"));
const header = plan.shift().split("\t");
const records = plan.map((row) => Object.fromEntries(row.split("\t").map((cell, index) => [header[index], cell]))).filter((row) => row.native_route_exists === "n");
const counts = { a: 0, b: 0, c: 0, ffi: 0 };
const result = [];
const blockers = [];
for (const row of records) {
  let category, body, leaves = new Set(), sources = new Set();
  const foreign = row.kind.startsWith("foreign-");
  if (foreign) { category = "ffi"; sources.add(row.provider_source); }
  else {
    body = byQualified.get(row.route) ?? bySymbol.get(row.symbol);
    if (!body) category = "c";
    else {
      ({ leaves, sourceBodies: sources } = resolveBody(body));
      category = leaves.size ? "b" : "a";
    }
  }
  counts[category]++;
  const missing = [...leaves].filter((leaf) => !routes.has(leaf)).sort();
  const blocker = category === "c" || (category === "b" && missing.length > 0);
  const confidence = row.kind === "receiver-candidate-untyped" ? "untyped-candidate-NOT-proven-import" : row.kind === "emitted-runtime-reference" ? "output-program-runtime-NOT-proven-compiler-import" : "source-call-inferred";
  const line = [row.kind, row.route, row.symbol, row.compiler_call_sites, category, [...sources].sort().join(";"), [...leaves].sort().map((leaf) => `${leaf}:${routes.has(leaf) ? "route-y" : "route-n"}`).join(";"), missing.join(";"), blocker ? "candidate" : "no-source-link-gap", confidence];
  result.push(line);
  if (blocker) blockers.push(line);
}
const preamble = [
  "# [INFERENCE] Static source-body classification. a=Jet body without identified raw leaf; b=Jet body reaching raw leaf; c=no matching Jet body; ffi=separate foreign metadata.",
  `# dev_tree=${dev}; compiler_tree=${compiler}; a=${counts.a}; b=${counts.b}; c=${counts.c}; ffi=${counts.ffi}; total=${records.length}`,
  "# a does NOT prove native lowering; b route-y does NOT prove provider admission. Typed MIR must confirm receiver identity, dynamic/interpolation calls and compiler reachability.",
  "# A c row for an untyped receiver candidate or emitted-runtime reference is NOT an established jetc1 import blocker.",
  "kind\troute\tsymbol\tstatic_call_sites\tclass\tJet_source_bodies\traw_leaves_and_routes\tmissing_leaf_routes\tlink_gap_INFERENCE\tconfidence",
];
const write = (path, rows) => writeFileSync(path, [...preamble, ...rows.map((row) => row.join("\t"))].join("\n") + "\n");
write(output, result);
write(join(dirname(output), "LINKPLAN-BLOCKERS.tsv"), blockers);

// Direct named-call reverse graph over compiler definitions: function pointer
// calls/closure dispatch and interpolation expressions await the checked MIR.
const manifest = read(compiler, "Compiler/Bootstrap/sources.list").split("\n").map((row) => row.trim()).filter((row) => row && !row.startsWith("#"));
const definitions = [];
for (const path of manifest) definitions.push(...functionsIn(read(compiler, path), path));
const names = new Map();
for (const fn of definitions) {
  fn.id = `${fn.name}@${fn.path}:${fn.line}`;
  if (!names.has(fn.name)) names.set(fn.name, []);
  names.get(fn.name).push(fn);
}
const reverse = new Map();
for (const fn of definitions) for (const call of new Set(namedCalls(fn.body))) {
  for (const callee of names.get(call) ?? []) {
    if (!reverse.has(callee.id)) reverse.set(callee.id, new Set());
    reverse.get(callee.id).add(fn.id);
  }
}
const ffiRows = [];
for (const symbol of new Set(records.filter((row) => row.kind.startsWith("foreign-")).map((row) => row.symbol))) {
  const foreignDefs = names.get(symbol) ?? [];
  const direct = new Set(foreignDefs.flatMap((fn) => [...(reverse.get(fn.id) ?? [])]));
  const reached = new Set(direct), work = [...direct];
  while (work.length) for (const caller of reverse.get(work.pop()) ?? []) {
    if (!reached.has(caller)) { reached.add(caller); work.push(caller); }
  }
  ffiRows.push([symbol, direct.size, reached.size, [...direct].sort().join(";"), [...reached].sort().join(";")]);
}
writeFileSync(join(dirname(output), "FFI-REACH.tsv"), ["# [INFERENCE] Static named-call reachability, not entry-rooted MIR; dynamic dispatch may add edges.", "symbol\tdirect_compiler_functions\ttransitive_compiler_functions\tdirect_functions\ttransitive_functions", ...ffiRows.map((row) => row.join("\t"))].join("\n") + "\n");
console.log(`LINKPLAN RESOLVED [INFERENCE] a=${counts.a} b=${counts.b} c=${counts.c} ffi=${counts.ffi} total=${records.length} candidate_gaps=${blockers.length} ffi_symbols=${ffiRows.length}; output=${output}`);
