#!/usr/bin/env node
// The audited E2 export list is explicit, not inferred by the runtime driver.
// Regenerate it after reviewing a runtime layer's #Export(c) cutover.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const args = process.argv.slice(2);
if (args.length > 1 || args.some((arg) => !["--check", "--print"].includes(arg))) throw new Error("usage: gen-runtime-exports.mjs [--check|--print]");
const sources = readFileSync(join(root, "Compiler/JetRuntime/sources.list"), "utf8").split("\n")
  .map((line) => line.trim()).filter((line) => line && !line.startsWith("#"));
const declarations = sources.flatMap((path) => [...readFileSync(join(root, path), "utf8").matchAll(/^(?:\/\/ runtime-(init|fini): (\d+)\n)?#Export\(c\)\s+(?:pub\s+)?fn\s+([A-Za-z_]\w*)\s*\(/gm)].map((match) => {
  if (match[1] && Number(match[2]) > 65535) throw new Error(`${path}: lifecycle priority exceeds 65535`);
  return { symbol: match[3], lifecycle: match[1] ? `${match[1]} ${Number(match[2])}` : "" };
}));
const symbols = declarations.map((entry) => entry.symbol);
if (!symbols.length || new Set(symbols).size !== symbols.length) throw new Error("empty or duplicate runtime #Export(c) declarations");
// Lead-reviewed ABI metadata, emitted into the explicit list (not a lowering
// special case): the borrowed exact-Int list index is the one L4a seam.
const text = `${declarations.sort((a, b) => a.symbol.localeCompare(b.symbol)).map(({ symbol, lifecycle }) => {
  if (lifecycle && symbol === "jet_rt_panic_list_bounds") throw new Error("the Int index seam cannot be a lifecycle entry");
  return lifecycle ? `${symbol} ${lifecycle}` : symbol === "jet_rt_panic_list_bounds" ? `${symbol} int-index-seam` : symbol;
}).join("\n")}\n`;
const output = join(root, "Compiler/JetRuntime/exports.list");
if (args[0] === "--print") process.stdout.write(text);
else if (args[0] === "--check") {
  if (readFileSync(output, "utf8") !== text) throw new Error("runtime exports.list is stale; review the layer cutover and regenerate it");
} else writeFileSync(output, text);
