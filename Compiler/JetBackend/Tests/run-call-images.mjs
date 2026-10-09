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
      const match = /^(?:struct|enum) (\w+)\b/.exec(lines[index]);
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
  // MIR and shared derivation declarations supply their real type identities.
  for (const path of ['Compiler/JetFoundation/Source/MIR/MIR.jet', 'Compiler/JetFoundation/Source/DerivationTypes.jet']) {
    for (const match of read(path).matchAll(/^(?:pub )?(?:struct|enum) (\w+)\b/gm)) provided.add(match[1]);
  }
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
  item('Compiler/JetFoundation/Source/Diagnostics/Diagnostic.jet', 'struct Span {'),
  item('Compiler/JetFoundation/Source/Registry/CoreCalls.jet', 'enum Effect '),
  item('Compiler/JetFoundation/Source/Types/Types.jet', 'enum ParamZone {'),
  item('Compiler/JetFoundation/Source/Types/Types.jet', 'fn param_zone_name('),
  item('Compiler/JetBackend/Source/Image/ELF.jet', 'ELF64_BASE_ADDRESS ::'),
  item('Compiler/JetBackend/Source/Image/ELF.jet', 'ELF64_HEADER_SIZE ::'),
  item('Compiler/JetBackend/Source/Image/ELF.jet', 'ELF64_PROGRAM_HEADER_SIZE ::'),
  item('Compiler/JetBackend/Source/Image/ELF.jet', 'fn elf64_text_offset('),
  ...[
    'Compiler/JetFoundation/Source/DerivationTypes.jet',
    'Compiler/JetFoundation/Source/MIR/MIR.jet',
    'Compiler/JetBackend/Source/LIR/LIR.jet',
    'Compiler/JetBackend/Source/LIR/Lint.jet',
    'Compiler/JetBackend/Source/X64/Encoder.jet',
    'Compiler/JetBackend/Source/X64/InlineAsm.jet',
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
if (process.argv.includes('--values') || process.argv.includes('--session')) {
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
if (process.argv.includes('--session')) {
  uses.add('use core.math as native_comptime_math');
  parts = [
    ...parts.slice(0, -3),
    body('Compiler/JetFoundation/Source/Encoding/Json.jet'),
    ...['X64ValueMember', 'X64ValueVariant', 'X64ValueDescriptor', 'X64ValueBoundary']
      .map(name => item('Compiler/JetBackend/Source/Lower/ComptimeBoundary.jet', `struct ${name} {`)),
    body('Compiler/JetBackend/Source/Image/ComptimeWire.jet'),
    body('Compiler/JetBackend/Source/OS/SessionTransport.jet'),
    body('Compiler/JetBackend/Source/OS/SessionOutput.jet'),
    body('Compiler/JetBackend/Source/Image/SessionHost.jet'),
    item('Compiler/JetCodegen/Source/Codegen/Builder.jet', 'struct JetCodegenClosureSource {'),
    item('Compiler/JetDriver/Source/Driver/Identity.jet', 'trait JetDriverExecutionHost {'),
    item('Compiler/JetDriver/Source/Driver/NativeComptime.jet', 'fn jet_driver_native_value_key('),
    item('Compiler/JetDriver/Source/Driver/NativeComptime.jet', 'fn jet_driver_native_float32_bits('),
    item('Compiler/JetDriver/Source/Driver/NativeComptime.jet', 'fn jet_driver_native_value('),
    item('Compiler/JetCli/Source/Cli/NativeExecutor.jet', 'struct JetCLINativeClosureSources {'),
    item('Compiler/JetCli/Source/Cli/NativeExecutor.jet', 'struct JetCLINativeExecutor {'),
    item('Compiler/JetCli/Source/Cli/NativeExecutor.jet', 'impl JetCLINativeExecutor.JetDriverExecutionHost {'),
    body('Compiler/JetBackend/Tests/SessionHostFixtures.jet'),
  ];
}
mkdirSync(out, { recursive: true });
writeFileSync(resolve(out, 'calls.jet'), [...uses, '', ...parts].join('\n'));
writeFileSync(resolve(out, 'package.jet'), 'name: "jet_native_call_proofs"\nversion: "0.1.0"\nauthority: { holds: { allow: [FFI, Mem.Alloc, IO, Exec, FS, Env, Time.Wait] } }\n');
console.log(resolve(out, 'calls.jet'));
