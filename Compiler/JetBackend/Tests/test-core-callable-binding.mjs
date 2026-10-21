#!/usr/bin/env node
// Light Source-only witness: execute the actual callable digest/refusal helpers.
// This does not lower or admit a Core image or exercise the native runtime.
import {spawnSync} from 'node:child_process';
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {resolve, dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const out = process.argv[2];
if (!out || !process.env.JET) throw new Error('usage: JET=<trusted compiler> node test-core-callable-binding.mjs <disk-backed scratch dir>');
mkdirSync(out, {recursive:true});
const load = path => readFileSync(join(root, path), 'utf8');
function declaration(source, marker) {
  const start = source.indexOf(marker);
  if (start < 0 || source.indexOf(marker, start + marker.length) >= 0) throw new Error(`missing or ambiguous Source declaration ${marker}`);
  const firstEnd = source.indexOf('\n', start);
  const end = source.slice(start, firstEnd).endsWith('}') ? firstEnd : source.indexOf('\n}', firstEnd) + 2;
  if (end < firstEnd) throw new Error(`unterminated Source declaration ${marker}`);
  return source.slice(start, end);
}
const lir = load('Compiler/JetBackend/Source/LIR/LIR.jet');
const emission = load('Compiler/JetBackend/Source/Image/Emission.jet');
const json = load('Compiler/JetFoundation/Source/Encoding/Json.jet');
const manifest = load('Compiler/JetBackend/Source/Image/CoreSharedManifest.jet');
const pieces = ['use core.crypto as core_shared_crypto'];
pieces.push(declaration(emission, 'struct LIRFunctionID '));
for (const marker of ['pub struct LIRSignatureID ', 'pub enum LIRABIClass ', 'pub struct LIRLayout ', 'pub enum LIRCallConv ', 'pub struct LIRSignature {', 'fn lir_abi_class_name(', 'pub fn lir_layout_word(', 'pub fn lir_layout_pointer(']) pieces.push(declaration(lir, marker));
for (const marker of ['pub fn foundation_json_escape(', 'pub fn foundation_json_quote(', 'fn foundation_json_append(']) pieces.push(declaration(json, marker));
for (const marker of ['pub struct CoreSharedCallableBinding {', 'fn core_shared_callable_rows_digest(', 'fn core_shared_callable_digest_error(', 'pub fn core_shared_callable_required_error(']) pieces.push(declaration(manifest, marker));
const tests = ['Core callable digest refuses a changed linked offset or signature', 'an absent emitted clone or drop callable is a named refusal'];
for (const [index, name] of tests.entries()) {
  const marker = `#Test(${JSON.stringify(name)})`;
  pieces.push(declaration(manifest, marker).replace(marker, `fn callable_binding_case_${index}() Never!`));
}
pieces.push('fn run() {\n    _ := callable_binding_case_0()\n    _ := callable_binding_case_1()\n    print("Core callable binding: PASS; offset/signature tamper refused, missing clone/drop named (4/4)")\n}');
const fixture = resolve(out, 'callable-binding.jet');
writeFileSync(fixture, pieces.join('\n\n') + '\n');
const result = spawnSync(process.env.JET, ['run', '--threads', '1', fixture], {cwd:resolve(out), env:process.env, encoding:'utf8', maxBuffer:8*1024*1024});
process.stdout.write(result.stdout ?? '');
process.stderr.write(result.stderr ?? '');
if (result.error) throw result.error;
console.log(`Core callable binding fixture: actual exit ${result.status ?? result.signal}`);
process.exit(result.status ?? 1);
