#!/usr/bin/env node
// Each SsaStress/*.fixture program (one Jet file) at O2 with the SSA form
// stressed (every value moved to a fresh slot, Source/SsaOut.jet) and
// verified, against the same program at O0: stdout, the first line of
// stderr and the exit status must be identical (a fixture may end in a
// stop; the lines its report names can differ between levels). Each runs
// as main.jet in its own folder under OUTDIR.
// Usage: node Compiler/JetIR/Tests/run-ssa-stress.mjs OUTDIR JET
import {spawnSync} from 'node:child_process';
import {copyFileSync,mkdirSync,readdirSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const out=process.argv[2]&&resolve(process.argv[2]),jet=process.argv[3]&&resolve(process.argv[3]);
if(!out||!jet)throw Error('usage: run-ssa-stress.mjs OUTDIR JET');
const fixtures=join(dirname(fileURLToPath(import.meta.url)),'SsaStress');
function run(cwd,flags,env){
 const result=spawnSync(jet,['run',...flags,'main.jet'],{cwd,env:{...process.env,...env},encoding:'utf8',maxBuffer:16*1024*1024,timeout:300000});
 if(result.error)throw result.error;
 return {status:result.status,stdout:result.stdout,stderr:result.stderr};
}
let failed=0;
for(const name of readdirSync(fixtures).filter(name=>name.endsWith('.fixture')).sort()){
 const cwd=join(out,name.replace(/\.fixture$/,''));
 mkdirSync(cwd,{recursive:true});
 copyFileSync(join(fixtures,name),join(cwd,'main.jet'));
 const expected=run(cwd,[],{});
 const stressed=run(cwd,['--release'],{JET_OPT_SSA_STRESS:'1',JET_OPT_SSA_VERIFY:'1'});
 const head=text=>text.split('\n')[0];
 if(stressed.status===expected.status&&stressed.stdout===expected.stdout&&head(stressed.stderr)===head(expected.stderr)){
  console.log(`ok ${name}`);
  continue;
 }
 failed+=1;
 console.log(`FAIL ${name}: O0 exit ${expected.status}, O2 stress exit ${stressed.status}`);
 console.log(`  O0 stdout: ${JSON.stringify(expected.stdout.slice(0,400))}`);
 console.log(`  O2 stress stdout: ${JSON.stringify(stressed.stdout.slice(0,400))}`);
 if(stressed.stderr!==expected.stderr)console.log(`  O0 stderr: ${JSON.stringify(expected.stderr.slice(0,400))}\n  O2 stress stderr: ${JSON.stringify(stressed.stderr.slice(0,400))}`);
}
process.exit(failed?1:0);
