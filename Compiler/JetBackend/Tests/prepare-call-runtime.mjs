#!/usr/bin/env node
// Refresh only the canonical hand-written C ABI in previously emitted runtime
// Rust. Keep its paired generated route set: seed and full runtimes must not
// be mixed. Compile the result with rustc --crate-type cdylib for loader proofs.
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
writeFileSync(output, [...lines.slice(0, start), canonical.slice(canonicalStart), ...lines.slice(end + 1)].join('\n'));
console.log(output);
