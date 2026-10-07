#!/usr/bin/env node
// The audited E2 export list is explicit, not inferred by the runtime driver.
// Regenerate it after reviewing a runtime layer's #Export(c) cutover.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const args = process.argv.slice(2);
if (args.length > 1 || args.some((arg) => !["--check", "--print"].includes(arg))) throw new Error("usage: gen-runtime-exports.mjs [--check|--print]");
const sources = ["Compiler/JetRuntime/Source/Alloc.jet", "Compiler/JetRuntime/Source/Trap.jet"];
const symbols = sources.flatMap((path) => [...readFileSync(join(root, path), "utf8").matchAll(/^#Export\(c\)\s+(?:pub\s+)?fn\s+([A-Za-z_]\w*)\s*\(/gm)].map((match) => match[1]));
if (!symbols.length || new Set(symbols).size !== symbols.length) throw new Error("empty or duplicate runtime #Export(c) declarations");
// Lead-reviewed ABI metadata, emitted into the explicit list (not a lowering
// special case): the borrowed exact-Int list index is the one L4a seam.
const text = `${symbols.sort().map((symbol) => symbol === "jet_rt_panic_list_bounds" ? `${symbol} int-index-seam` : symbol).join("\n")}\n`;
const output = join(root, "Compiler/JetRuntime/exports.list");
if (args[0] === "--print") process.stdout.write(text);
else if (args[0] === "--check") {
  if (readFileSync(output, "utf8") !== text) throw new Error("runtime exports.list is stale; review the layer cutover and regenerate it");
} else writeFileSync(output, text);
