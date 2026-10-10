#!/usr/bin/env node
// Fresh C consumers of one admitted RC pack. Never rebuild a runtime object or
// invoke rustc; the native Jet field case uses the supplied same-pin RC jet.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {readNativeRuntimePack,nativePackHash} from '../../../Tools/packaging/runtime-pack-cache.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const [output,jet,runtimeDirectory,...extra]=process.argv.slice(2);
if(!output||!runtimeDirectory||!jet||extra.length)throw Error('usage: run-process-witnesses.mjs OUT RC-NATIVE-JET RC-RUNTIME-DIR');
const out=resolve(output),runtime=resolve(runtimeDirectory),nativeJet=resolve(jet);
mkdirSync(out,{recursive:true});
const admitted=readNativeRuntimePack(runtime);
// The full source/backend inventories are the pin, not a collection of copied
// witness objects from an earlier runtime build.
for(const inventory of [admitted.provenance.source,admitted.provenance.backendInputs])
 for(const row of inventory.files)if(nativePackHash(readFileSync(join(root,row.path)))!==row.sha256)
  throw Error(`RC pack/source pin mismatch: ${row.path}`);
const platform=admitted.provenance.cPlatformArchives.map(row=>{
 if(row.language!=='C'||nativePackHash(readFileSync(row.path))!==row.sha256)throw Error(`RC C platform mismatch: ${row.path}`);
 return row.path;
});
const musl=dirname(platform.find(path=>path.endsWith('/libc.a'))??'');
if(!musl||!platform.some(path=>path.endsWith('/libc.a')))throw Error('admitted static musl libc.a required');
// The startup-bearing pack.o owns _start; C witnesses supply their own musl
// CRT, so link the admitted standalone Jet provider object instead.
const pack=join(runtime,admitted.key,'jet_runtime_jet.o');
function command(bin,args,label,options={}){
 const r=spawnSync(bin,args,{cwd:out,encoding:'utf8',maxBuffer:1<<27,timeout:120000,...options});
 writeFileSync(join(out,label+'.receipt.json'),JSON.stringify({status:r.status,signal:r.signal,error:r.error?.message??null,stdout:r.stdout,stderr:r.stderr},null,2)+'\n');
 if(r.error||r.status!==0)throw Error(`${label}: ${r.error?.message??r.status}\n${r.stdout??''}${r.stderr??''}`);
 return r;
}
const fixtures=[['ProcessRoutesWitness','process routes: ok\n',false],
 ['ProcessExecutionWitness',readFileSync(join(root,'Compiler/JetRuntime/Tests/ProcessExecutionWitness.stdout'),'utf8'),false],
 ['TerminalProcessWitness',readFileSync(join(root,'Compiler/JetRuntime/Tests/TerminalProcessWitness.stdout'),'utf8'),true],
 ['ProcessSpecWitness',readFileSync(join(root,'Compiler/JetRuntime/Tests/ProcessSpecWitness.stdout'),'utf8'),true]];
for(const [name,expected,wrap]of fixtures){
 const object=join(out,name+'.o'),binary=join(out,name);
 command(join(root,'Tools/agent/jet-env'),[process.env.CC??'cc','-std=c11','-O1','-fno-stack-protector','-fno-PIE','-fno-builtin','-Wall','-Wextra','-Werror','-c',join(root,'Compiler/JetRuntime/Tests',name+'.c'),'-o',object],name+'-compile');
 command(join(root,'Tools/agent/jet-env'),[process.env.CC??'cc','-nostdlib','-static','-no-pie',join(musl,'crt1.o'),join(musl,'crti.o'),object,...(wrap?['-Wl,--wrap=jet_rt_free']:[]),'-Wl,--start-group',pack,...platform,'-Wl,--end-group',join(musl,'crtn.o'),'-o',binary],name+'-link');
 const r=command(binary,[],name+'-execute');
 if(r.stdout!==expected||r.stderr!=='')throw Error(`${name}: native witness output differs`);
}
const fieldDirectory=join(out,'fields');mkdirSync(fieldDirectory,{recursive:true});
writeFileSync(join(fieldDirectory,'package.jet'),'name: "process_fields_witness"\nversion: "0.0.1"\nedition: "2028"\n');
writeFileSync(join(fieldDirectory,'run.jet'),readFileSync(join(root,'Compiler/JetBackend/Tests/Fixtures/ProcessReceiptFields.jet')));
const fields=command(nativeJet,['run',join(fieldDirectory,'run.jet')],'native-process-fields',{cwd:fieldDirectory,env:{...process.env,JET_TOOLCHAIN_ROOT:root}});
if(fields.stdout!=='native process receipt fields: ok\n'||fields.stderr!=='')throw Error('native ProcessReceipt typed field output differs');
writeFileSync(join(out,'pack-identity.json'),JSON.stringify({runtimeKey:admitted.key,packSha256:admitted.provenance.packSha256,objectSha256:admitted.provenance.objectSha256,rustObjects:0},null,2)+'\n');
console.log('plain process native pack witnesses: ok');
