#!/usr/bin/env node
// Fresh admitted-pack C consumers; --test instead lowers a canonical runtime
// plus the selected Jet fixture and makes its C witness call the actual export.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {readNativeRuntimePack,nativePackHash} from '../../../Tools/packaging/runtime-pack-cache.mjs';
import {fileObject} from '../../../Tools/packaging/publish-native-runtime-pack.mjs';
import {runNativeLibrary} from '../../../Tools/agent/compiler-native-transport.mjs';
import {parseRuntimeExports,runtimeLifecycleList} from '../../../Tools/agent/runtime-route-abi.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const [output,jet,runtimeDirectory,...extra]=process.argv.slice(2);
const test=extra.length===2&&extra[0]==='--test'?extra[1]:null;
if(!output||!runtimeDirectory||!jet||(extra.length&&(!test||!['ProcessExecution','TerminalProcess'].includes(test))))throw Error('usage: run-process-witnesses.mjs OUT RC-NATIVE-JET RC-RUNTIME-DIR [--test ProcessExecution|TerminalProcess]');
const out=resolve(output),runtime=resolve(runtimeDirectory),nativeJet=resolve(jet);
// Fail closed rather than reusing any old C object, receipt or executable.
mkdirSync(out);
const admitted=readNativeRuntimePack(runtime);
// The full source/backend inventories are the pin, not a collection of copied
// witness objects from an earlier runtime build.
for(const inventory of [admitted.provenance.source,admitted.provenance.backendInputs])
 for(const row of inventory.files)if(nativePackHash(readFileSync(join(root,row.path)))!==row.sha256)
  throw Error(`RC pack/source pin mismatch: ${row.path}`);
const platformInputs=admitted.provenance.cPlatformArchives.map(row=>{
 const handle=fileObject(row.path);
 if(row.language!=='C'||row.role!=='platform'||handle.split(':')[0]!==row.sha256)throw Error(`RC C platform mismatch: ${row.path}`);
 return {path:row.path,handle};
});
const platform=platformInputs.map(row=>row.path);
const musl=dirname(platform.find(path=>path.endsWith('/libc.a'))??'');
if(!musl||!platform.some(path=>path.endsWith('/libc.a')))throw Error('admitted static musl libc.a required');
// The startup-bearing pack.o owns _start; C witnesses supply their own musl
// CRT, so link the admitted standalone Jet provider object instead.
const publishedObject=join(runtime,admitted.key,'jet_runtime_jet.o');
let pack,objectHandle;
const compilerHandle=fileObject(nativeJet);
if(test){
 const unit=join(out,'unit'),prefix=join(unit,'fixture');
 command(process.execPath,[join(root,'Compiler/JetRuntime/Tests/check-runtime.mjs'),unit,'--assemble-only','--test',test],'assemble-fixture');
 const file=join(unit,'unit.jet'),lifecycle=join(unit,'lifecycle.list');
 writeFileSync(lifecycle,runtimeLifecycleList(parseRuntimeExports(readFileSync(file,'utf8'),'unit.jet')));
 const lowered=runNativeLibrary({jet:nativeJet,file,prefix,lifecycle,env:{...process.env,JET_TOOLCHAIN_ROOT:root,JET_STORE_DIR:join(out,'store')}});
 writeFileSync(join(out,'lower-fixture.receipt.json'),JSON.stringify(lowered,null,2)+'\n',{flag:'wx'});
 if(lowered.problem)throw Error(`native ${test} fixture: ${lowered.problem.detail}`);
 pack=prefix+'.o';objectHandle=fileObject(pack);
}else{
 pack=join(out,'jet_runtime_jet.o');
 writeFileSync(pack,admitted.files.get('jet_runtime_jet.o'),{flag:'wx'});
 objectHandle=fileObject(pack);
 if(objectHandle.split(':')[0]!==admitted.provenance.objectSha256)throw Error('admitted Jet object snapshot differs');
}
const crt=['crt1.o','crti.o','crtn.o'].map(name=>{
 const path=join(musl,name);return {path,handle:fileObject(path)};
});
function unchanged(rows){
 for(const row of rows)if(fileObject(row.path)!==row.handle)throw Error(`proof input changed: ${row.path}`);
}
const sharedInputs=[...platformInputs,...crt,{path:pack,handle:objectHandle},{path:nativeJet,handle:compilerHandle}];
writeFileSync(join(out,'admission.json'),JSON.stringify({runtimeKey:admitted.key,publishedObject,linkedObject:pack,object:objectHandle,fixture:test,compiler:{path:nativeJet,handle:compilerHandle},platform:platformInputs,crt},null,2)+'\n',{flag:'wx'});
function command(bin,args,label,options={}){
 const r=spawnSync(bin,args,{cwd:out,encoding:'utf8',maxBuffer:1<<27,timeout:120000,...options});
 writeFileSync(join(out,label+'.receipt.json'),JSON.stringify({bin,args,cwd:options.cwd??out,status:r.status,signal:r.signal,error:r.error?.message??null,stdout:r.stdout,stderr:r.stderr},null,2)+'\n',{flag:'wx'});
 if(r.error||r.status!==0)throw Error(`${label}: ${r.error?.message??r.status}\n${r.stdout??''}${r.stderr??''}`);
 return r;
}
const fixtures=[['ProcessRoutesWitness','process routes: ok\n',false],
 ['ProcessExecutionWitness',readFileSync(join(root,'Compiler/JetRuntime/Tests/ProcessExecutionWitness.stdout'),'utf8'),false],
 ['TerminalProcessWitness',readFileSync(join(root,'Compiler/JetRuntime/Tests/TerminalProcessWitness.stdout'),'utf8'),true],
 ['ProcessSpecWitness',readFileSync(join(root,'Compiler/JetRuntime/Tests/ProcessSpecWitness.stdout'),'utf8'),true]];
for(const [name,expected,wrap]of fixtures.filter(row=>!test||row[0]===test+'Witness')){
 const object=join(out,name+'.o'),binary=join(out,name);
 const source=join(root,'Compiler/JetRuntime/Tests',name+'.c'),sourceHandle=fileObject(source);
 command(join(root,'Tools/agent/jet-env'),[nativeCCompiler,'-std=c11','-O1','-fno-stack-protector','-fno-PIE','-fno-builtin','-Wall','-Wextra','-Werror',...(test?['-DJET_PROCESS_JET_FIXTURES=1']:[]),'-c',source,'-o',object],name+'-compile');
 const cObjectHandle=fileObject(object);
 unchanged([...sharedInputs,{path:source,handle:sourceHandle}]);
 command(join(root,'Tools/agent/jet-env'),[nativeCCompiler,'-nostdlib','-static','-no-pie',crt[0].path,crt[1].path,object,...(wrap?['-Wl,--wrap=jet_rt_free']:[]),'-Wl,--start-group',pack,...platform,'-Wl,--end-group',crt[2].path,'-o',binary],name+'-link');
 const executableHandle=fileObject(binary);
 unchanged([...sharedInputs,{path:source,handle:sourceHandle},{path:object,handle:cObjectHandle}]);
 const r=command(binary,[],name+'-execute');
 unchanged([...sharedInputs,{path:source,handle:sourceHandle},{path:object,handle:cObjectHandle},{path:binary,handle:executableHandle}]);
 if(r.stdout!==expected||r.stderr!=='')throw Error(`${name}: native witness output differs`);
 writeFileSync(join(out,name+'.identity.json'),JSON.stringify({runtimeKey:admitted.key,object:objectHandle,compiler:compilerHandle,witness:{path:source,handle:sourceHandle},cObject:cObjectHandle,executable:executableHandle},null,2)+'\n',{flag:'wx'});
}
if(test){
 writeFileSync(join(out,'fixture-result.json'),JSON.stringify({status:0,test,object:objectHandle,compiler:compilerHandle,witness:test+'Witness'},null,2)+'\n',{flag:'wx'});
 console.log(`native process Jet fixture ${test}: ok`);
 process.exit(0);
}
const fieldDirectory=join(out,'fields');mkdirSync(fieldDirectory);
const fieldSource=join(root,'Compiler/JetBackend/Tests/Fragments.fixture/Fixtures/ProcessReceiptFields.jet'),fieldHandle=fileObject(fieldSource);
writeFileSync(join(fieldDirectory,'package.jet'),'name: "process_fields_witness"\nversion: "0.0.1"\nedition: "2028"\n',{flag:'wx'});
const fieldCopy=join(fieldDirectory,'run.jet');
writeFileSync(fieldCopy,readFileSync(fieldSource),{flag:'wx'});
const fieldCopyHandle=fileObject(fieldCopy);
if(fieldCopyHandle.split(':')[0]!==fieldHandle.split(':')[0])throw Error('native field fixture snapshot differs');
const fields=command(nativeJet,['run',fieldCopy],'native-process-fields',{cwd:fieldDirectory,env:{...process.env,JET_TOOLCHAIN_ROOT:root}});
unchanged([...sharedInputs,{path:fieldSource,handle:fieldHandle},{path:fieldCopy,handle:fieldCopyHandle}]);
if(fields.stdout!=='native process receipt fields: ok\n'||fields.stderr!=='')throw Error('native ProcessReceipt typed field output differs');
writeFileSync(join(out,'pack-identity.json'),JSON.stringify({runtimeKey:admitted.key,packSha256:admitted.provenance.packSha256,object:objectHandle,compiler:compilerHandle,fieldFixture:fieldHandle,fieldCopy:fieldCopyHandle,rustObjects:0},null,2)+'\n',{flag:'wx'});
console.log('plain process native pack witnesses: ok');
