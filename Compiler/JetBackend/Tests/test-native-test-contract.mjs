#!/usr/bin/env node
// Exercise process-state output checks and fatal recovery with the real native
// host, compiler, runtime pack and executable. No protocol producer is mocked.
import {spawnSync} from 'node:child_process';
import {mkdirSync,writeFileSync,readFileSync,rmSync} from 'node:fs';
import {resolve,join,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
const repo=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const [outArg,hostArg]=process.argv.slice(2);
if(!hostArg)throw new Error('expected OUT HOSTED_JET');
const out=resolve(outArg),host=resolve(hostArg);mkdirSync(out,{recursive:true});
const header='use core.files as contract_files\nuse core.process as contract_process\nCHECK :: Output.Check{name: "state-check", entry: verify}\n#Persist state := 0\n';
const success=header+'#Test("first") {\n    state = 41\n    contract_files.append("events.txt", "first\\n") ?? panic("write first event")\n}\n#Test("second") {\n    state += 1\n    contract_files.append("events.txt", "second\\n") ?? panic("write second event")\n}\nfn verify() -> Bool {\n    contract_files.append("events.txt", "check\\n") ?? panic("write check event")\n    state == 42\n}\n';
const fatal=header+'#Test("already reported") {\n    state = 41\n    contract_files.append("events.txt", "already\\n") ?? panic("write completed event")\n}\n#Test("fatal child") {\n    contract_files.append("events.txt", "fatal\\n") ?? panic("write fatal event")\n    contract_process.exit(73)\n}\n#Test("unreported body") {\n    contract_files.append("events.txt", "remaining\\n") ?? panic("write remaining event")\n}\nfn verify() -> Bool {\n    contract_files.append("events.txt", "check\\n") ?? panic("write check event")\n    state == 41\n}\n';
const receipts=[];
for(const [name,source,code,events]of [['same-process',success,0,'first\nsecond\ncheck\n'],['fatal-recovery',fatal,1,'already\nfatal\nfatal\nremaining\n']]){
 const work=join(out,name);rmSync(work,{recursive:true,force:true});mkdirSync(work,{recursive:true});
 writeFileSync(join(work,'case.jet'),source);
 writeFileSync(join(work,'package.jet'),'name: "native_test_contract"\nversion: "0.0.1"\nedition: "2028"\nauthority: { allow: [Env, Exec, Exec.Args, FS, FS.Read, IO, Mem.Alloc, Mem.Rc] }\n');
 const run=spawnSync(host,['test','--serial','--capture=all','--color=never','case.jet'],{cwd:work,env:{...process.env,JET_TOOLCHAIN_ROOT:repo,JET_TEST_JSON:'1',NO_COLOR:'1',TERM:'dumb'},encoding:'utf8',timeout:600000,maxBuffer:64<<20});
 writeFileSync(join(work,'stdout'),run.stdout??'');writeFileSync(join(work,'stderr'),run.stderr??'');
 let report=null;for(const line of (run.stdout??'').split('\n')){try{const value=JSON.parse(line);if(value.schema==='jet.test.v1')report=value;}catch{}}
 let observedEvents=null;try{observedEvents=readFileSync(join(work,'events.txt'),'utf8');}catch{}
 const expectedNames=name==='same-process'?['first','second']:['already reported','fatal child','unreported body','state-check'];
 const names=report?.tests?.map(test=>test.name);
 const reason='output check not run: the test batch ended in a fatal trap';
 const outputCheck=name==='same-process'||(report?.tests?.at(-1)?.ok===false&&report.tests.at(-1).stderr.includes('E3015')&&report.tests.at(-1).stderr.toLowerCase().includes(reason));
 const ok=run.signal===null&&run.status===code&&observedEvents===events&&JSON.stringify(names)===JSON.stringify(expectedNames)&&report?.passed===2&&report?.failed===(name==='same-process'?0:2)&&outputCheck;
 receipts.push({name,ok,code:run.status,signal:run.signal,events:observedEvents,report});
 console.log(`${ok?'PASS':'FAIL'}\t${name}`);
}
writeFileSync(join(out,'results.json'),JSON.stringify({ok:receipts.every(row=>row.ok),receipts},null,2)+'\n');
process.exitCode=receipts.every(row=>row.ok)?0:1;
