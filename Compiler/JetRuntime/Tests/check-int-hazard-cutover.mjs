#!/usr/bin/env node
// Source proof only: seed bodies unchanged, native numeric hazard sites routed.
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const int = readFileSync(join(root, 'Compiler/JetRuntime/Source/Int.jet'), 'utf8');
if (/Import\(c\)/.test(int) || !int.includes('jet_rt_int_retire_zero(node)')) throw Error('Int still imports an external retirement domain');
// Only the Jet ownership gate is live; retain the retired Rust source recipe.
console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
process.exit(0);
const path = 'crates/jet-foundation/src/Numeric.rs';
const current = readFileSync(join(root, path), 'utf8');
const baseline = spawnSync('git', ['show', '417ce17ff:' + path], { cwd: root, encoding: 'utf8', maxBuffer: 1 << 24 });
if (baseline.status !== 0) throw Error('missing exact pre-cutover Numeric.rs baseline417ce17ff');
function blocks(text, declaration) {
  const result = [];
  let at = 0;
  while ((at = text.indexOf(declaration, at)) >= 0) {
    const start = text.indexOf('{', at);
    if (start < 0) throw Error('missing opener: ' + declaration);
    let depth = 1, end = start + 1;
    // The compared seed hazard bodies contain no braced string literals.
    for (; end < text.length && depth; ++end) {
      if (text[end] === '{') ++depth;
      else if (text[end] === '}') --depth;
    }
    if (depth) throw Error('unbalanced body: ' + declaration);
    result.push({ body: text.slice(start, end), prefix: text.slice(Math.max(0, at - 800), at) });
    at = end;
  }
  return result;
}
const seed = ['struct JetIntHazardRecord', 'fn jet_int_hazard_record()', 'struct JetIntReadGuard', 'impl JetIntReadGuard', 'impl Drop for JetIntReadGuard', 'fn jet_int_push_retired(', 'fn jet_int_is_hazard(', 'unsafe fn jet_int_deallocate(', 'fn jet_int_retire_zero(', 'fn jet_int_reclaim_retired()', 'fn jet_int_try_retain(', 'unsafe fn jet_int_release(', 'pub fn clone_from_atomic<F>'];
for (const declaration of seed) {
  const old = blocks(baseline.stdout, declaration), next = blocks(current, declaration);
  if (old.length !== 1 || !next.length || old[0].body !== next[0].body) throw Error('seed body changed: ' + declaration);
  if (!next[0].prefix.includes('#[cfg(not(jet_native_pack))]')) throw Error('seed hazard site not excluded from native: ' + declaration);
}
for (const name of ['JET_INT_HAZARDS', 'JET_INT_HAZARD_RECORD', 'JET_INT_RETIRED']) {
  const at = current.indexOf('static ' + name + ':');
  if (at < 0 || !current.slice(Math.max(0, at - 180), at).includes('#[cfg(not(jet_native_pack))]')) throw Error('native Rust domain storage remains: ' + name);
}
const native = [['struct JetIntReadGuard', 'record: i64'], ['impl JetIntReadGuard', 'jet_rt_int_hazard_enter()'], ['impl Drop for JetIntReadGuard', 'jet_rt_int_hazard_clear(self.record)'], ['fn jet_int_retire_zero(', 'jet_rt_int_retire_zero('], ['pub fn clone_from_atomic<F>', 'jet_rt_int_clone_atomic_reload(']];
for (const [declaration, forward] of native) {
  const sites = blocks(current, declaration);
  if (sites.length !== 2 || !sites[1].prefix.includes('#[cfg(jet_native_pack)]') || !sites[1].body.includes(forward)) throw Error('native forwarding site missing: ' + declaration);
  if (/\bloop\b|\.compare_exchange|JET_INT_(?:HAZARDS|RETIRED)/.test(sites[1].body)) throw Error('native forwarding site contains hazard policy: ' + declaration);
}
const nextDrop = blocks(current, 'impl Drop for JetInt {')[0];
const oldDrop = blocks(baseline.stdout, 'impl Drop for JetInt {')[0];
const oldRelease = oldDrop.body.slice(oldDrop.body.indexOf('if jet_int_is_pointer'), oldDrop.body.lastIndexOf('}') - 1).trim();
if (!nextDrop.body.includes(oldRelease) || !nextDrop.body.includes('jet_rt_int_drop(self.0 as i64)')) throw Error('JetInt Drop seed/native forwarding drift');
const equality = blocks(current, 'pub fn atomic_equal<F>')[0];
if (equality.body !== blocks(baseline.stdout, 'pub fn atomic_equal<F>')[0].body) throw Error('atomic_equal policy changed instead of forwarding existing guard sites');
const abi = readFileSync(join(root, 'crates/jet-codegen/src/Prelude/Core/CAbi.rs'), 'utf8');
const at = abi.indexOf('pub extern "C" fn jet_rt_int_retire_zero(');
if (at < 0 || !abi.slice(at - 180, at).includes('#[cfg(not(jet_native_pack))]')) throw Error('duplicate native CAbi retirement provider');
console.log(`Int hazard source cutover: ${seed.length} byte-identical seed bodies, 3 seed-only stores, ${native.length} native forwarding sites, Drop/equality/retire unified; native behavior NOT measured`);
