import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
const script=new URL('./run-lower-fixtures.mjs',import.meta.url).pathname;
function assemble(t,flags){
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'jet-lower-assembly-'));
 t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
 const env={...process.env};for(const name of ['JET_RUNTIME_PACK','FX_ONLY','FX_HEAD','FX_EXCLUDE'])delete env[name];
 const result=spawnSync(process.execPath,[script,root,...flags],{env,encoding:'utf8'});
 return {root,result};
}
test('assemble-only includes the full bits/container fixture unit without inventing a pack',t=>{
 const {root,result}=assemble(t,['--assemble-only']);
 assert.equal(result.status,0,result.stderr);
 const unit=fs.readFileSync(path.join(root,'unit.jet'),'utf8');
 assert.ok(unit.includes('fx_report("bits", fx_bits())'));
 for(const suffix of ['list','option','bytes','base64','fixed','map','paths']){
  assert.ok(unit.includes(`fn fx_container_codecs_${suffix}(`));
  assert.ok(unit.includes(`fx_report("container-codecs-${suffix}",`));
 }
 assert.ok(fs.existsSync(path.join(root,'load/load.jet')));
 assert.equal(fs.existsSync(path.join(root,'runtime-pack.txt')),false);
});
test('ordinary full fixture execution still requires an admitted pack',t=>{
 const {root,result}=assemble(t,[]);
 assert.notEqual(result.status,0);
 assert.match(result.stderr,/JET_RUNTIME_PACK must explicitly name an admitted fresh Jet pack/);
 assert.equal(fs.existsSync(path.join(root,'unit.jet')),false);
});
