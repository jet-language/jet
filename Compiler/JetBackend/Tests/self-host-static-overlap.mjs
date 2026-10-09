#!/usr/bin/env node
// Compiler syntax/reference overlap with observed golden refusal roots.
// All rows are static [INFERENCE]; none measures native failure or unlocks.
import { readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { codeOnly } from "./self-host-source.mjs";
const [repoArg, clustersPath, output] = process.argv.slice(2);
if (!output) throw new Error("usage: self-host-static-overlap.mjs <repo> <CLUSTERS.md> <OVERLAP-STATIC.tsv>");
const repo = resolve(repoArg);
const read = (path) => readFileSync(join(repo, path), "utf8");
const files = read("Compiler/Bootstrap/sources.list").split("\n").map((row) => row.trim()).filter((row) => row && !row.startsWith("#")).map((path) => {
  const text = read(path), code = codeOnly(text);
  const aliases = new Map([...code.matchAll(/^use (core(?:\.[A-Za-z_]\w*)+)(?: as ([A-Za-z_]\w*))?\s*$/gm)].map((match) => [match[2] ?? match[1].split(".").at(-1), match[1]]));
  return { path, text, code, aliases, pkg: path.split("/").slice(0, 2).join("/") };
});
const escaped = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const typeUse = (name) => new RegExp(`\\b${escaped(name)}\\s*(?:<|\\{|\\.(?:new|with_capacity)\\s*\\()`, "g");
const methodUse = (name) => new RegExp(`\\.${escaped(name)}\\s*(?:<[^;\\n()]*>)?\\s*\\(`, "g");
const reference = (name) => new RegExp(`\\b${escaped(name)}\\b`, "g");
const taxonomies = [
  ["Set locals", typeUse("Set"), "type-or-constructor-candidate"],
  ["Queue locals", typeUse("Queue"), "type-or-constructor-candidate"],
  ["PriorityQueue locals", typeUse("PriorityQueue"), "type-or-constructor-candidate"],
  ["ID locals", typeUse("ID"), "type-or-constructor-candidate"],
  ["iterator source kinds", /\.(?:iter|into_iter|keys|values|entries|lines|chars|scalars|chunks|windows)\s*(?:<[^;\n()]*>)?\s*\(/g, "iterator-source-candidate-not-proven-unsupported-kind"],
  ["flat_map", methodUse("flat_map"), "method-call-receiver-untyped"],
  ["try_collect", methodUse("try_collect"), "method-call-receiver-untyped"],
  ["TraitBox / dynamic trait values", /\bdyn\s+[A-Za-z_]\w*/g, "dynamic-trait-syntax-candidate"],
  ["ClosureMethod try_map", methodUse("try_map"), "method-call-receiver-untyped"],
  ["ClosureMethod para_map", methodUse("para_map"), "method-call-receiver-untyped"],
  ["String? comparison", null, "declared-optional-identifier-comparison-candidate", "String?"],
  ["Instant arithmetic", null, "declared-Instant-identifier-arithmetic-candidate", "Instant"],
  ["Shared / STM / transaction", /\b(?:Shared|SharedGuard|SharedSnapshot|SharedWeak)\s*(?:<|\{)|\b(?:stm|transaction)\s*(?:\(|\{)/g, "carrier-type-or-transaction-syntax-candidate"],
  ["task race / any", /\.(?:race|any|join_all|select|spawn)\s*(?:<[^;\n()]*>)?\s*\(|\bTask\s*</g, "task-and-untyped-method-candidate"],
  ["TypedText", null, "typed-text-literal-or-value-type-candidate", "TypedText"],
  ["F32", /\bF32\b/g, "sized-float-type-reference-excludes-lowercase-bool-field-f32"],
  ["inline asm", /\bFFI\s*\(\s*asm\s*\)/g, "foreign-asm-declaration"],
];
const clusterText = readFileSync(clustersPath, "utf8").split("## Exhaustive first-refusal table")[1];
if (!clusterText) throw new Error("missing exhaustive refusal table");
const roots = [...clusterText.matchAll(/^\| (.*?) \| (\d+) \|/gm)].map((match) => ({ root: match[1], goldens: Number(match[2]) }));
if (roots.length === 0) throw new Error("no observed refusal roots");
function coreSites(route, file) {
  const variants = new Set([route]);
  for (const [alias, module] of file.aliases) if (route.startsWith(`${module}.`)) variants.add(`${alias}.${route.slice(module.length + 1)}`);
  const matches = [];
  for (const variant of variants) matches.push(...file.code.matchAll(new RegExp(`\\b${escaped(variant)}\\s*(?:<[^;\\n()]*>)?\\s*\\(`, "g")));
  return matches;
}
function matcher(root) {
  if (/comparison of String\?/.test(root)) return { mode: "declared-optional-identifier-comparison-candidate", special: "String?" };
  if (/operator .* on Instant/.test(root)) return { mode: "declared-Instant-identifier-arithmetic-candidate", special: "Instant" };
  if (/^MIR producer:|^Error:|^pack:|^runtime pack lacks|^unbound type parameter/.test(root)) return { mode: "not-inferable-from-source-syntax" };
  const route = /\b(?:CoreCall|StaticPreludeCall|HandleMethod|HostCall|MathBuiltin|BuiltinMethod|PreciseBuiltin)\s+(core\.[A-Za-z_.]+)/.exec(root)?.[1];
  if (route && !route.startsWith("core.handle.") && !route.startsWith("core.builtin.") && !route.startsWith("core.host.")) return { mode: "qualified-core-call-candidate", core: route };
  const method = /\b(?:CoreCall|StaticPreludeCall|HandleMethod|HostCall|MathBuiltin|BuiltinMethod|PreciseBuiltin)\s+(?:core\.)?[A-Za-z_.:]+\.([A-Za-z_]\w*)/.exec(root)?.[1];
  if (method) return { mode: "method-call-receiver-untyped", regex: methodUse(method) };
  const operation = /(?:semantic operation|MIR operation) ([A-Za-z_]\w*)/.exec(root)?.[1];
  if (operation) return { mode: "MIR-tag-reference-NOT-use-of-that-operation-by-compiler", regex: reference(operation) };
  const ty = /(?:local of (?:List element )?type|return type|type) ([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)?)/.exec(root)?.[1];
  if (ty) return { mode: "type-reference-NOT-proven-refused-local", regex: reference(ty) };
  const builtin = /builtin method ([A-Za-z_]\w*)/.exec(root)?.[1];
  if (builtin) return { mode: "method-call-receiver-untyped", regex: methodUse(builtin.replace(/^list_/, "")) };
  if (/iterator source kind/.test(root)) return { mode: taxonomies[4][2], regex: taxonomies[4][1] };
  if (/flat_map/.test(root)) return { mode: "method-call-receiver-untyped", regex: methodUse("flat_map") };
  if (/String\?/.test(root)) return { mode: "declared-optional-identifier-comparison-candidate", special: "String?" };
  if (/Instant/.test(root)) return { mode: "declared-Instant-identifier-arithmetic-candidate", special: "Instant" };
  if (/Duration/.test(root)) return { mode: "type-reference-NOT-proven-arithmetic", regex: reference("Duration") };
  if (/global constant stm|global constant transaction|Shared/.test(root)) return { mode: taxonomies[12][2], regex: taxonomies[12][1] };
  if (/Task|task |stop-and-cancellation|native frames|Select/.test(root)) return { mode: taxonomies[13][2], regex: taxonomies[13][1] };
  if (/callee kind/.test(root)) return { mode: "foreign-declaration-candidate", regex: /\bFFI\s*\(\s*(?:asm|c)\s*\)/g };
  if (/DataEntriesToMap/.test(root)) return { mode: "MIR-tag-reference-NOT-use-of-that-operation-by-compiler", regex: reference("DataEntriesToMap") };
  if (/Dyn/.test(root)) return { mode: "nominal-type-reference", regex: reference("Dyn") };
  if (/Browser/.test(root)) return { mode: "Browser-reference-NOT-proven-entry-initialization", regex: reference("Browser") };
  if (/field ID is not a stored field of/.test(root)) return { mode: "nominal-type-reference", regex: reference(root.split(" of ").at(-1)) };
  if (/Core operation|Core\.compute operation/.test(root)) {
    const name = /::([A-Za-z_]\w*)/.exec(root.slice(root.lastIndexOf("::")))?.[1];
    if (name) return { mode: "unqualified-call-candidate", regex: new RegExp(`\\b${escaped(name)}\\s*\\(`, "g") };
  }
  return { mode: "not-inferable-from-source-syntax" };
}
function specialSites(kind, file) {
  if (kind === "TypedText") {
    const matches = [...file.code.matchAll(/:\s*(?:SQL|Sh)\b|->\s*(?:SQL|Sh)\b/g)];
    for (const match of file.text.matchAll(/\b(?:sql|sh|time|date|datetime)"/g)) {
      const prefix = match[0].slice(0, -1);
      if (file.code.slice(match.index, match.index + prefix.length) === prefix) matches.push(match);
    }
    return matches;
  }
  const type = kind === "String?" ? "String\\s*\\?" : "Instant";
  const names = new Set([...file.code.matchAll(new RegExp(`\\b([A-Za-z_]\\w*)\\s*:\\s*${type}`, "g"))].map((match) => match[1]));
  if (kind === "Instant") for (const match of file.code.matchAll(/\b([A-Za-z_]\w*)\s*(?:::|:=)\s*(?:[A-Za-z_]\w*\.)?instant\s*\(/g)) names.add(match[1]);
  const operators = kind === "String?" ? "(?:==|!=|<=|>=|<|>)" : "[+-]";
  const matches = [];
  for (const match of file.code.matchAll(new RegExp(`\\b([A-Za-z_]\\w*(?:\\.[A-Za-z_]\\w*)*)\\s*${operators}(?!=)`, "g"))) {
    if (!names.has(match[1].split(".").at(-1))) continue;
    const rest = file.code.slice(match.index + match[0].length).trimStart();
    if (kind === "String?" && /^(?:None|\\.None)\b/.test(rest)) continue;
    matches.push(match);
  }
  return matches;
}

function count(root, goldenCount, rule, kind) {
  let total = 0;
  const packages = new Map(), samples = [];
  for (const file of files) {
    const matches = rule.special ? specialSites(rule.special, file) : rule.core ? coreSites(rule.core, file) : rule.regex ? [...file.code.matchAll(new RegExp(rule.regex.source, rule.regex.flags))] : [];
    if (!matches.length) continue;
    total += matches.length;
    packages.set(file.pkg, (packages.get(file.pkg) ?? 0) + matches.length);
    for (const match of matches.slice(0, 2)) if (samples.length < 5) samples.push(`${file.path}:${file.code.slice(0, match.index).split("\n").length}`);
  }
  return { root, kind, goldenCount, mode: rule.mode, count: rule.regex || rule.core || rule.special ? total : null,
    packages: [...packages].sort((a, b) => b[1] - a[1]).map(([pkg, n]) => `${pkg}:${n}`).join(";"), samples: samples.join(";") };
}
const rows = [
  ...taxonomies.map(([root, regex, mode, special]) => count(root, "", { regex, mode, special }, "requested-construct")),
  ...roots.map(({ root, goldens }) => count(root, goldens, matcher(root), "observed-refusal-root")),
].sort((a, b) => (b.count ?? -1) - (a.count ?? -1) || a.root.localeCompare(b.root));
const preamble = [
  "# [INFERENCE] Static compiler-source overlap, NOT observed compiler native failures or projected unlock counts.",
  `# manifest_files=${files.length}; observed_golden_roots=${roots.length}; requested_constructs=${taxonomies.length}; repo=${repo}`,
  "# Comments/string bodies excluded. MIR-tag references are NOT uses of those operations by the compiler. String?/Instant rows match operators on declared identifier names, not all annotations; scope/receiver identity still requires typed MIR. F32 excludes the compiler's lowercase f32 Bool field.",
  "# UNMEASURED means no faithful syntactic matcher, NOT zero compiler uses. Alias Core calls are file-local; receiver methods remain untyped candidates.",
  "kind\troot\tcompiler_candidate_sites\tgolden_first_refusal_cases\tevidence_mode\ttop_packages\tsample_sites",
];
writeFileSync(output, [...preamble, ...rows.map((row) => [row.kind, row.root, row.count ?? "UNMEASURED", row.goldenCount, row.mode, row.packages, row.samples].join("\t"))].join("\n") + "\n");
console.log(`OVERLAP STATIC [INFERENCE] files=${files.length} roots=${roots.length} constructs=${taxonomies.length} matched_rules=${rows.filter((row) => row.count !== null).length} unmeasured_rules=${rows.filter((row) => row.count === null).length}`);
console.log("TOP REQUESTED CONSTRUCTS (candidate sites, not native refusals):");
for (const row of rows.filter((row) => row.kind === "requested-construct").slice(0, 10)) console.log(`${row.root}\t${row.count}\t${row.packages}\t${row.mode}`);
