#!/usr/bin/env node
// The Jet host's native tests preserve canonical status and report without Rust.
import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,rmSync,copyFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const repo=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const [outArg,hostedArg]=process.argv.slice(2);
if (hostedArg === '--rust-reference') {
 console.log('test-host reference: parked: Rust reference retired (owner #38/#59); Tower #4923');
 process.exit(0);
}
if(!outArg||!hostedArg||process.argv.length!==4)throw new Error('expected OUT HOSTED_JET');
const out=resolve(outArg),hosted=resolve(hostedArg);
mkdirSync(out,{recursive:true});
const examples=['basics/onboarding/run.jet','tooling/test_members.jet','tooling/expected_fail.jet'];
const policies=[['all',['--capture=all']],['serial',['--capture=all','--serial']],['shuffle',['--capture=all','--shuffle=17']],['none',['--capture=none']],['failed',['--capture=failed']],['filter',['--capture=all','--filter=ordinary pass']]];
const results=[];
for(const example of examples)for(const [policy,args]of policies){
 const name=example.replaceAll('/','_').replace('.jet','')+'-'+policy;
 const work=join(out,'work');
 const runs=[];
 for(const [side,binary]of [['native',hosted]]){
  rmSync(work,{recursive:true,force:true});mkdirSync(work,{recursive:true});
  copyFileSync(join(repo,'Examples/features',example),join(work,'case.jet'));
  writeFileSync(join(work,'package.jet'),'name: "native_test_example"\nversion: "0.0.1"\nedition: "2028"\n');
  const run=spawnSync(binary,['test','--color=never',...args,'case.jet'],{cwd:work,env:{...process.env,JET_TOOLCHAIN_ROOT:repo,JET_TEST_JSON:'1',NO_COLOR:'1',TERM:'dumb'},encoding:'utf8',timeout:600000,maxBuffer:64<<20});
  writeFileSync(join(out,`${name}.${side}.stdout`),run.stdout??'');writeFileSync(join(out,`${name}.${side}.stderr`),run.stderr??'');
  runs.push({side,code:run.status,signal:run.signal,stdout:run.stdout??'',stderr:run.stderr??''});
 }
 const [native]=runs;
 const expectedCode=example==='tooling/expected_fail.jet'&&policy!=='filter'?1:0;
 let reportObserved=true;
 if(['all','serial','shuffle','filter'].includes(policy)){
  reportObserved=runs.every(run=>run.stdout.split('\n').some(line=>{try{const value=JSON.parse(line);return value.schema==='jet.test.v1'&&Array.isArray(value.tests)&&(policy==='filter'||value.tests.length>0);}catch{return false;}}));
 }
 const ok=reportObserved&&native.signal===null&&native.code===expectedCode;
 results.push({example,policy,ok,native:native.code,nativeSignal:native.signal,rustReference:'parked; Tower #4923'});
 console.log(`${ok?'PASS':'FAIL'}\t${example}\t${policy}`);
 writeFileSync(join(out,'results.json'),JSON.stringify({examples,results,ok:results.every(row=>row.ok)},null,2)+'\n');
}
console.log('test-host reference: parked: Rust reference retired (owner #38/#59); Tower #4923');
process.exitCode=results.every(row=>row.ok)?0:1;
