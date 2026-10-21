#!/usr/bin/env node
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { runtimeAbiAtom, emitRuntimeAbiAtom } from '../../../Tools/agent/runtime-abi-symbols.mjs';
import { nativeHandleExports } from '../../../Tools/agent/native-route-handles.mjs';
import { nativeRecordExports } from '../../../Tools/agent/native-route-records.mjs';
import { runtimeExportSignatures } from '../../../Tools/agent/runtime-route-abi.mjs';
for (const [input, expected] of [
  ['JetDelivery', 'jet_delivery'], ['HTTPClient', 'http_client'],
  ['IOError', 'io_error'], ['List.Rec.JetDelivery', 'list_rec_jet_delivery'],
  ['Vec3D', 'vec3_d'], ['jet_rt_record_new_' + 'IOError_NotFound', 'jet_rt_record_new_io_error_not_found'],
  ['jet_rt_already_snake', 'jet_rt_already_snake'],
]) assert.equal(runtimeAbiAtom(input), expected);
assert.throws(() => runtimeAbiAtom('bad/atom'), /invalid runtime ABI atom/);
const type = {runtime:'HTTPClient',jetName:'HTTPClient',source:{path:'Core/Net/http.jet',key:'HTTPClient'},clone:true,
  record:{kind:'enum',members:[{name:'NotFound',kind:null},{name:'Reply',kind:'List.Byte'}]}};
assert.ok(nativeHandleExports([type]).some(row => row.name === 'jet_rt_handle_clone_http_client' && row.runtime === 'HTTPClient'));
assert.ok(nativeRecordExports([type]).some(row => row.name === 'jet_rt_record_new_http_client_not_found' && row.member === 'NotFound'));
assert.ok(nativeRecordExports([type]).some(row => row.name === 'jet_rt_record_seq_list_byte_new'));
assert.match(emitRuntimeAbiAtom(), /fn runtime_abi_atom\(name: String\)/);
// The runtime is one source unit: internal calls must not re-import its
// exported C addresses, and external addresses have one declaration owner.
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const exports = runtimeExportSignatures(root);
const imports = new Map();
const sources = readFileSync(join(root, 'Compiler/JetRuntime/sources.list'), 'utf8')
  .split('\n').map(line => line.trim()).filter(line => line && !line.startsWith('#'));
for (const path of sources) {
  const source = readFileSync(join(root, path), 'utf8');
  assert.doesNotMatch(source,
    /\b(?:scheduler_observe_(?:register|enter|finish|control|wait|identity|parked|pool|quiescent)|main_(?:env_seed|stdout_flush|stderr_flush|stderr_write|observe_init|observe_start|observe_report)|observe_(?:mem_resources|env_get|env_get_os)|process_errno_location)\s*\(/,
    `${path}: every retired C alias caller must use its actual provider`);
  for (const match of source.matchAll(/^#Import\(c\)\s+(?:pub\s+)?fn\s+([A-Za-z_]\w*)\s*[^=]*=\s*"([^"]+)"/gm)) {
    const [, name, symbol] = match;
    assert.ok(!exports.has(symbol),
      `${path}:${name} must call the existing Jet provider ${symbol}, not re-import its C symbol`);
    assert.ok(!imports.has(symbol),
      `${path}:${name} duplicates the external C declaration ${imports.get(symbol)} for ${symbol}`);
    imports.set(symbol, `${path}:${name}`);
  }
}
assert.ok(imports.size > 0, 'the runtime must retain its real external C seams');
console.log('runtime ABI atom projection and C symbol ownership CHECK OK');
