#!/usr/bin/env node
import assert from 'node:assert/strict';
import { runtimeAbiAtom, emitRuntimeAbiAtom } from '../../../Tools/agent/runtime-abi-symbols.mjs';
import { nativeHandleExports } from '../../../Tools/agent/native-route-handles.mjs';
import { nativeRecordExports } from '../../../Tools/agent/native-route-records.mjs';
for (const [input, expected] of [
  ['JetDelivery', 'jet_delivery'], ['HTTPClient', 'http_client'],
  ['IOError', 'io_error'], ['List.Rec.JetDelivery', 'list_rec_jet_delivery'],
  ['Vec3D', 'vec3_d'], ['jet_rt_record_new_' + 'IOError_NotFound', 'jet_rt_record_new_io_error_not_found'],
  ['jet_rt_already_snake', 'jet_rt_already_snake'],
]) assert.equal(runtimeAbiAtom(input), expected);
assert.throws(() => runtimeAbiAtom('bad/atom'), /invalid runtime ABI atom/);
const type = {runtime:'HTTPClient',jetName:'HTTPClient',source:{path:'Core/net/http.jet',key:'HTTPClient'},clone:true,
  record:{kind:'enum',members:[{name:'NotFound',kind:null},{name:'Reply',kind:'List.Byte'}]}};
assert.ok(nativeHandleExports([type]).some(row => row.name === 'jet_rt_handle_clone_http_client' && row.runtime === 'HTTPClient'));
assert.ok(nativeRecordExports([type]).some(row => row.name === 'jet_rt_record_new_http_client_not_found' && row.member === 'NotFound'));
assert.ok(nativeRecordExports([type]).some(row => row.name === 'jet_rt_record_seq_list_byte_new'));
assert.match(emitRuntimeAbiAtom(), /fn runtime_abi_atom\(name: String\)/);
console.log('runtime ABI atom projection CHECK OK');
