#!/usr/bin/env node
// Assemble the real native selector, loader and call adapter into one hosted
// proof unit. Build this unit, then execute it: jet run's old JIT is not proof
// of native code generation or the audited assembly host primitives.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const out = process.argv[2];
if (!out) throw new Error('usage: run-call-images.mjs <output directory>');
const read = path => readFileSync(resolve(repo, path), 'utf8');
function item(path, prefix) {
  const lines = read(path).split('\n');
  const start = lines.findIndex(line => line.startsWith(prefix));
  if (start < 0) throw new Error(`missing ${prefix} in ${path}`);
  if (!lines[start].trimEnd().endsWith('{')) return lines[start];
  const end = lines.findIndex((line, index) => index > start && line === '}');
  if (end < 0) throw new Error(`unterminated ${prefix}`);
  return lines.slice(start, end + 1).join('\n');
}
const uses = new Set();
function body(path) {
  let skip = false;
  return read(path).split('\n').filter(line => {
    if (skip) { if (line.startsWith(']')) skip = false; return false; }
    if (/^use jet_\w+\.\[\s*$/.test(line)) { skip = true; return false; }
    if (/^use jet_\w+/.test(line)) return false;
    if (/^use core\./.test(line)) { uses.add(line); return false; }
    return true;
  }).join('\n');
}
// Keep the real compiler value schema (including Closure), but omit unrelated
// Foundation algorithms/registries from this focused ABI proof. Walk declared
// type dependencies, never manufacture a test-only comptime value model.
function comptimeSchema(paths) {
  const declarations = new Map();
  for (const path of paths) {
    const lines = read(path).split('\n');
    for (let index = 0; index < lines.length; index += 1) {
      const match = /^(?:pub )?(?:struct|enum) (\w+)\b/.exec(lines[index]);
      if (!match) continue;
      let end = index;
      if (!lines[index].trimEnd().endsWith('}')) {
        end = lines.findIndex((line, at) => at > index && line === '}');
        if (end < 0) throw new Error(`unterminated schema declaration ${match[1]}`);
      }
      declarations.set(match[1], lines.slice(index, end + 1).join('\n'));
      index = end;
    }
  }
  const provided = new Set(['Span', 'Effect', 'ParamZone']);
  // MIR supplies its own real types, including layout/type identities.
  for (const match of read('Compiler/JetFoundation/Source/MIR/MIR.jet').matchAll(/^(?:pub )?(?:struct|enum) (\w+)\b/gm)) provided.add(match[1]);
  const selected = new Map();
  function visit(name) {
    if (provided.has(name) || selected.has(name) || !declarations.has(name)) return;
    const text = declarations.get(name);
    selected.set(name, text);
    for (const match of text.matchAll(/\b[A-Z]\w*\b/g)) visit(match[0]);
  }
  visit('TComptimeValue');
  return [...selected.values()];
}
let parts = [
  item('Compiler/JetFoundation/Source/Diagnostics/Diagnostic.jet', 'pub struct Span {'),
  item('Compiler/JetFoundation/Source/Registry/CoreCalls.jet', 'pub enum Effect '),
  item('Compiler/JetFoundation/Source/Types/Types.jet', 'pub enum ParamZone {'),
  item('Compiler/JetFoundation/Source/Types/Types.jet', 'pub fn param_zone_name('),
  item('Compiler/JetBackend/Source/Image/ELF.jet', 'ELF64_BASE_ADDRESS ::'),
  item('Compiler/JetBackend/Source/Image/ELF.jet', 'ELF64_HEADER_SIZE ::'),
  item('Compiler/JetBackend/Source/Image/ELF.jet', 'ELF64_PROGRAM_HEADER_SIZE ::'),
  item('Compiler/JetBackend/Source/Image/ELF.jet', 'fn elf64_text_offset('),
  ...[
    'Compiler/JetFoundation/Source/MIR/MIR.jet',
    'Compiler/JetBackend/Source/LIR/LIR.jet',
    'Compiler/JetBackend/Source/LIR/Lint.jet',
    'Compiler/JetBackend/Source/X64/Encoder.jet',
    'Compiler/JetBackend/Source/X64/RegAlloc.jet',
    'Compiler/JetBackend/Source/X64/Select.jet',
    'Compiler/JetBackend/Source/Image/Link.jet',
    'Compiler/JetBackend/Source/Image/Memory.jet',
    'Compiler/JetBackend/Source/OS/Linux.jet',
    'Compiler/JetBackend/Source/Image/Loader.jet',
    'Compiler/JetBackend/Source/Image/Calls.jet',
    'Compiler/JetBackend/Tests/CallImages.jet',
  ].map(body),
];
if (process.argv.includes('--values')) {
  const foundation = read('Compiler/Bootstrap/sources.list').split('# Sema:')[0]
    .split('\n').map(line => line.trim()).filter(line => line.startsWith('Compiler/JetFoundation/')
      && line !== 'Compiler/JetFoundation/Source/MIR/MIR.jet');
  parts = [
    ...parts.slice(0, -1),
    ...comptimeSchema(foundation),
    body('Compiler/JetBackend/Source/Image/ComptimeValues.jet'),
    body('Compiler/JetBackend/Tests/CallImages.jet').replace('fn run() {', 'fn ci_calls() {'),
    body('Compiler/JetBackend/Tests/ComptimeScalars.jet'),
    'fn run() {\n    ci_calls()\n    ci_comptime_scalars()\n}',
  ];
}
mkdirSync(out, { recursive: true });
writeFileSync(resolve(out, 'calls.jet'), [...uses, '', ...parts].join('\n'));
writeFileSync(resolve(out, 'package.jet'), 'name: "jet_native_call_proofs"\nversion: "0.1.0"\nauthority: { holds: { allow: [FFI, Mem.Alloc, IO, Exec.Args] } }\n');
console.log(resolve(out, 'calls.jet'));
