#!/usr/bin/env node
// Refresh the canonical C ABI and compiler-call process boundary in emitted
// runtime Rust. Keep its paired generated routes: never mix seed/full routes.
// Compile with rustc --crate-type cdylib for native loader/worker proofs.
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [input, output] = process.argv.slice(2);
if (!input || !output) throw new Error('usage: prepare-call-runtime.mjs <emitted runtime.rs> <output.rs>');
const lines = readFileSync(input, 'utf8').split('\n');
const start = lines.findIndex(line => line === 'mod jet_c_abi {');
if (start < 0) throw new Error('input lacks the canonical C ABI module');
const end = lines.findIndex((line, index) => index > start && line === '}');
if (end < 0) throw new Error('input C ABI module is unterminated');
// Keep the original module attributes; appending the source would duplicate
// them. Replace through the column-zero module closer, as rtpack.sh does.
const canonical = readFileSync(resolve(repo, 'crates/jet-codegen/src/Prelude/Core/CAbi.rs'), 'utf8');
const canonicalStart = canonical.indexOf('mod jet_c_abi {');
if (canonicalStart < 0) throw new Error('canonical C ABI source has no module');
let prepared = [...lines.slice(0, start), canonical.slice(canonicalStart), ...lines.slice(end + 1)].join('\n');
const core = readFileSync(resolve(repo, 'crates/jet-codegen/src/Prelude/Core.rs'), 'utf8');
const marker = '// Installed only in the isolated persistent compiler worker.';
const contextStart = core.indexOf(marker);
const contextEnd = core.indexOf('struct JetRuntimeExit;', contextStart);
const oldContext = prepared.indexOf(marker);
const oldEnd = prepared.indexOf('struct JetRuntimeExit;', Math.max(oldContext, 0));
if (contextStart < 0 || contextEnd < 0 || oldEnd < 0) throw new Error('compiler-call process boundary missing');
prepared = prepared.slice(0, oldContext < 0 ? oldEnd : oldContext) + core.slice(contextStart, contextEnd) + prepared.slice(oldEnd);
const prefix = 'fn jet_runtime_stop_unwind(';
const currentStop = core.indexOf(prefix);
const oldStop = prepared.indexOf(prefix);
const currentEnd = core.indexOf('\n}', currentStop) + 2;
const oldStopEnd = prepared.indexOf('\n}', oldStop) + 2;
if (currentStop < 0 || oldStop < 0 || currentEnd < 2 || oldStopEnd < 2) throw new Error('runtime stop boundary missing');
prepared = prepared.slice(0, oldStop) + core.slice(currentStop, currentEnd) + prepared.slice(oldStopEnd);
writeFileSync(output, prepared);
console.log(output);
