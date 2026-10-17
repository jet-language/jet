#!/usr/bin/env node
import { runtimeAbiAtom } from "../agent/runtime-abi-symbols.mjs";
// Admission receipt for the pack and resident-child library from one rustc run.
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { spawnSync } from 'node:child_process';

const [sourceRoot, outputDirectory, producer] = process.argv.slice(2);
if (!sourceRoot || !outputDirectory || !producer) {
  throw new Error('usage: write-runtime-manifest.mjs <source root> <runtime directory> <producer executable>');
}
const root = resolve(sourceRoot);
const directory = resolve(outputDirectory);
function command(binary, args) {
  const result = spawnSync(binary, args, { encoding: 'utf8', maxBuffer: 16 << 20 });
  if (result.status !== 0) throw new Error(`${binary} failed: ${result.stderr ?? result.error ?? result.status}`);
  return result.stdout;
}
function digest(path) { return createHash('sha256').update(readFileSync(path)).digest('hex'); }
function artifact(name) { return { name, sha256: digest(join(directory, name)) }; }
const tree = command('git', ['-C', root, 'rev-parse', 'HEAD']).trim();
const exports = command('nm', ['-D', '--defined-only', join(directory, 'libjet_runtime.so')])
  .split('\n').map(row => row.trim().split(/\s+/).at(-1))
  .filter(name => /^jet_rt_(?:session|execution|comptime)_/.test(name)).sort();
const required = ['session_create', 'session_begin', 'session_end', 'session_reserve',
  'session_publish', 'session_resolve', 'session_take', 'session_retire', 'session_destroy',
  'session_poll', 'session_propagate', 'session_status', 'session_shield_enter',
  'session_shield_leave', 'execution_begin', 'execution_end', 'execution_stop_write',
  'comptime_begin', 'comptime_enter', 'comptime_step', 'comptime_leave', 'comptime_end'];
for (const name of required) {
  if (!exports.includes(`jet_rt_${runtimeAbiAtom(name)}`)) throw new Error(`resident runtime lacks jet_rt_${runtimeAbiAtom(name)}`);
}
const runtime = artifact('libjet_runtime.so');
const manifest = { format: 'jet-native-runtime-v1', source_tree_sha: tree,
  producer_sha256: digest(resolve(producer)), rlib: artifact('libjet_runtime_native.rlib'),
  runtime, pack: artifact('jet_runtime.pack.o'), exports };
writeFileSync(join(directory, 'MANIFEST.json'), `${JSON.stringify(manifest, null, 2)}\n`);
writeFileSync(join(directory, 'libjet_runtime.so.sha256'), `${runtime.sha256}\n`);
console.log(join(directory, 'MANIFEST.json'));
