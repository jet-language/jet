#!/usr/bin/env node
// The runtime export set is exactly the sources' #Export(c) functions (U3).
// lifecycle.list only orders the hooks those sources mark with a
// `// runtime-init: N` or `// runtime-fini: N` comment; regenerate it from them.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { runtimeExportSignatures, runtimeLifecycleList } from "./runtime-route-abi.mjs";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const args = process.argv.slice(2);
if (args.length > 1 || args.some((arg) => !["--check", "--print"].includes(arg))) throw new Error("usage: gen-runtime-lifecycle.mjs [--check|--print]");
const text = runtimeLifecycleList(runtimeExportSignatures(root));
const output = join(root, "Compiler/JetRuntime/lifecycle.list");
if (args[0] === "--print") process.stdout.write(text);
else if (args[0] === "--check") {
  if (readFileSync(output, "utf8") !== text) throw new Error("Compiler/JetRuntime/lifecycle.list is stale; run node Tools/agent/gen-runtime-lifecycle.mjs");
} else writeFileSync(output, text);
