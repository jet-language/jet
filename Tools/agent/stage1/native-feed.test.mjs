import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, writeFileSync, chmodSync, readFileSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
const root=resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
test('env-only factory receives only explicit native-feed and collect-all policy',()=>{
 const out=mkdtempSync(join(tmpdir(),'jet-native-feed-env-'));
 try {
  const wrapper=join(out,'systemd-run'),probe=join(out,'probe');
  writeFileSync(wrapper,'#!/usr/bin/env bash\nwhile [ "$#" -gt 0 ] && [ "$1" != env ]; do shift; done\nexec "$@"\n');chmodSync(wrapper,0o755);
  writeFileSync(probe,'#!/usr/bin/env node\nrequire("node:fs").writeFileSync(process.env.JET_BOOTSTRAP_RECEIPT,JSON.stringify(process.env));\n');chmodSync(probe,0o755);
  const policy={JET_FRONT_NATIVE_OUTPUT:join(out,'native'),JET_FRONT_LIBRARY_OBJECT_OUTPUT:join(out,'library'),JET_FRONT_LIBRARY_LIFECYCLE:join(out,'lifecycle.list'),JET_FRONT_RUNTIME_PACK_OUTPUT:join(out,'pack.o'),JET_FRONT_RUNTIME_PLATFORM_INPUTS:join(out,'libc.a')+'\n'+join(out,'libgcc.a'),JET_RUNTIME_PACK:join(out,'pack.o'),JET_KEEP_GOING:'1',JET_KEEP_GOING_REPORT:join(out,'failures.tsv'),JET_KEEP_GOING_SKIP:'failed_function'};
  const result=spawnSync('bash',['-c','source "$LIB"; jetc_watch(){ return 0; }; jetc "$PROBE" factory "$ROOT" main.jet "$OUTPUT" "$RECEIPT" "$LOG"'],{env:{...process.env,...policy,HOME:out,PATH:out+':'+process.env.PATH,LIB:join(root,'Tools/agent/stage1/lib.sh'),PROBE:probe,ROOT:out,OUTPUT:join(out,'out.rs'),RECEIPT:join(out,'receipt.json'),LOG:join(out,'compile.log'),JET_DEBUG_SENTINEL:'must not leak',JET_ADAPTER_SENTINEL:'must not leak'},encoding:'utf8'});
  assert.equal(result.status,0,result.stderr);
  const observed=JSON.parse(readFileSync(join(out,'receipt.json'),'utf8'));
  for(const [key,value]of Object.entries(policy))assert.equal(observed[key],value,key);
  assert.equal(observed.JET_BOOTSTRAP_MODE,'factory');
  assert.equal(observed.JET_DEBUG_SENTINEL,undefined);assert.equal(observed.JET_ADAPTER_SENTINEL,undefined);
 }finally{rmSync(out,{recursive:true,force:true});}
});
