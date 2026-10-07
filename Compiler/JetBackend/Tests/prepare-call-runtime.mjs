#!/usr/bin/env node
// Refresh the canonical C ABI and compiler-call process boundary in emitted
// runtime Rust. Keep its paired generated routes: never mix seed/full routes.
// Compile with rustc --crate-type cdylib for native loader/worker proofs.
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [input, output, mode] = process.argv.slice(2);
if (!input || !output || (mode && mode !== '--allparts')) {
  throw new Error('usage: prepare-call-runtime.mjs <emitted runtime.rs> <output.rs> [--allparts]');
}
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
// A full-pack proof must include the current canonical adapters together.
// Seed proofs retain their emitted, paired route set instead.
if (mode === '--allparts') {
  // A frozen native MIR producer may predate callback scope sharing. Refresh
  // this shared canonical fragment with the adapters that call it; Scheduler
  // itself must still come from the producer's checked stop-rail dependency.
  const bodyStart = prepared.indexOf('// One thread-confined native callback boundary, shared by emitted Core and');
  const panicMarker = 'pub struct JetNativeTaskPanic(pub String);';
  const bodyEnd = prepared.indexOf(panicMarker, bodyStart);
  if (bodyStart < 0 || bodyEnd < 0) throw new Error('allparts input lacks the native task panic boundary');
  let stop = bodyEnd + panicMarker.length;
  const callback = prepared.indexOf('pub fn jet_native_callback_body', stop);
  const nextCore = prepared.indexOf('// Installed only in the isolated persistent compiler worker.', stop);
  if (callback >= 0 && callback < nextCore) {
    stop = prepared.indexOf('\n}', callback) + 2;
    if (stop < 2) throw new Error('input native callback body is unterminated');
    const comment = prepared.indexOf('// Every native callback body which belongs', bodyEnd);
    if (comment < 0 || comment > callback) throw new Error('input native callback scope marker is missing');
  }
  const body = readFileSync(resolve(repo, 'crates/jet-codegen/src/Prelude/Core/NativeTaskBody.rs'), 'utf8');
  prepared = prepared.slice(0, bodyStart) + body + prepared.slice(stop);
  for (const [module, file] of [
    ['jet_c_abi_routes', 'CAbiRoutes.rs'],
    ['jet_c_abi_iter', 'CAbiIter.rs'],
    ['jet_c_abi_task', 'CAbiTask.rs'],
    ['jet_c_abi_parallel', 'CAbiParallel.rs'],
  ]) {
    const rows = prepared.split('\n');
    const begin = rows.findIndex(line => line === `mod ${module} {` || line === `pub(crate) mod ${module} {`);
    if (begin >= 0) {
      const close = rows.findIndex((line, index) => index > begin && line === '}');
      if (close < 0) throw new Error(`input ${module} module is unterminated`);
      // The canonical text carries its own cfg; remove the old one with it.
      const first = begin > 0 && rows[begin - 1] === '#[cfg(not(target_arch = "wasm32"))]' ? begin - 1 : begin;
      prepared = [...rows.slice(0, first), ...rows.slice(close + 1)].join('\n');
    }
    prepared += '\n' + readFileSync(resolve(repo, `crates/jet-codegen/src/Prelude/Core/${file}`), 'utf8');
  }
}
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
