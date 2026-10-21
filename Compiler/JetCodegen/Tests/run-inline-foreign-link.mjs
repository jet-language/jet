#!/usr/bin/env node
// Compile and execute the production Rust/Jet inline-bridge build-script templates.
// Usage: node Compiler/JetCodegen/Tests/run-inline-foreign-link.mjs OUTDIR [SELFHOST.JET]
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {dirname,resolve,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {runtimeExportSignatures} from '../../../Tools/agent/runtime-route-abi.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const out=process.argv[2]&&resolve(process.argv[2]);
if(!out)throw Error('usage: run-inline-foreign-link.mjs OUTDIR [SELFHOST.JET]');
mkdirSync(out,{recursive:true});
const cc=process.env.CC??'cc',cxx=process.env.CXX??'c++',ar=process.env.AR??'ar',rustc=process.env.RUSTC??'rustc';
const rows=[];
function command(label,exe,args,cwd,env={}){
 const result=spawnSync(exe,args,{cwd,env:{...process.env,...env},encoding:'utf8',maxBuffer:16*1024*1024});
 writeFileSync(join(out,label+'.stdout'),result.stdout??'');writeFileSync(join(out,label+'.stderr'),result.stderr??'');
 if(result.error)throw result.error;
 return result;
}
const version=command('rust-version',rustc,['-vV'],root);
assert.equal(version.status,0);const target=version.stdout.match(/^host: (.+)$/m)?.[1];assert(target);
const rustSource=readFileSync(join(root,'crates/jet-pkg-model/src/FFI.rs'),'utf8');
const pkgSource=readFileSync(join(root,'Jetpack/Integrations/Source/Foreign/RustBridge/WrapperEmit.jet'),'utf8');
const codegenSource=readFileSync(join(root,'Compiler/JetCodegen/Source/Codegen/ForeignFunctions.jet'),'utf8');
const rustTemplate=rustSource.split('fn emit_inline_build_rs(')[1].match(/r#"([\s\S]*?)"#/)[1];
const pkgTemplate=pkgSource.split('fn pkg_ffi_emit_inline_build_rs(')[1].match(/\n    """\n([\s\S]*?)\n[ \t]*"""/)[1];
const codegenBody=codegenSource.split('fn jet_codegen_inline_native_build_script(')[1].split('fn jet_codegen_prepare_inline_native_artifacts(')[0];
function render(kind,files){
 const cpp=files.some(row=>row.cpp),fileRows=files.map(row=>`(${JSON.stringify(row.name)}, ${JSON.stringify(row.cpp?cxx:cc)}, ${row.cpp}, "")`).join(', ');
 let text=kind==='rust'?rustTemplate:kind==='package'?pkgTemplate:codegenBody.split('\n').flatMap(line=>{
  if(line.includes('if config.has_cpp')&&!cpp)return [];
  const match=line.match(/&lines\.push\("((?:\\.|[^"\\])*)"\)/);return match?[JSON.parse('"'+match[1]+'"')]:[];
 }).join('\n');
 const quote=JSON.stringify;
 const fields={target:quote(target),proof_suffix:quote('so'),undefined:quote('-Wl,--no-undefined'),archiver:quote(ar),files:fileRows,runtime:cpp?'println!("cargo:rustc-link-lib=dylib=stdc++");':'',target_debug:quote(target),proof_suffix_debug:quote('so'),undefined_debug:quote('-Wl,--no-undefined'),archiver_debug:quote(ar),file_rows:fileRows,file_rows_text:fileRows};
 for(const [name,value]of Object.entries(fields))text=text.replaceAll('{'+name+':?}',value).replaceAll('{'+name+'}',value);
 for(const [name,value]of Object.entries({'toolchain.target':target,'config.proof_suffix':'so','config.undefined_symbol_flag':'-Wl,--no-undefined','toolchain.ar.path':ar,'config.archive_name':'libjet_inline_native.a','config.cxx_runtime':'stdc++'}))text=text.replaceAll('{jet_codegen_inline_native_rust_quote('+name+')}',quote(value));
 return text.replaceAll('{{','{').replaceAll('}}','}')+'\n';
}
const session=readFileSync(join(root,'Compiler/JetBackend/Source/OS/SessionTransport.jet'),'utf8');
const sessionBody=name=>session.match(new RegExp('fn '+name+'\\([^]*?"""([^]*?)"""'))[1];
const cases=[
 {name:'shared-mutex',files:[{name:'lock.c',source:sessionBody('os_session_lock'),cpp:false},{name:'unlock.c',source:sessionBody('os_session_unlock'),cpp:false}],main:'void os_session_lock(void); void os_session_unlock(void); int main(void){for(int i=0;i<3;i++){os_session_lock();os_session_unlock();}return 0;}'},
 {name:'mixed-cpp',files:[{name:'owner.c',source:'#include <stdint.h>\nint64_t inline_shared_word=40;',cpp:false},{name:'read.cpp',source:'#include <stdint.h>\n#include <string>\nextern "C" { extern int64_t inline_shared_word; int64_t inline_read(void){return inline_shared_word+(int64_t)std::string("a").size();}}',cpp:true}],main:'#include <stdint.h>\nint64_t inline_read(void); int main(void){return inline_read()!=41;}'},
 {name:'missing-definition',files:[{name:'missing.c',source:'#include <stdint.h>\nextern int64_t inline_missing_definition; int64_t inline_read(void){return inline_missing_definition;}',cpp:false}],reject:true},
];
for(const kind of ['rust','package','codegen'])for(const fixture of cases){
 const label=kind+'-'+fixture.name,folder=join(out,label);mkdirSync(folder,{recursive:true});mkdirSync(join(folder,'objects'),{recursive:true});
 try{
  for(const file of fixture.files)writeFileSync(join(folder,file.name),file.source);
  writeFileSync(join(folder,'build.rs'),render(kind,fixture.files));
  let result=command(label+'-rustc',rustc,['--edition=2021','build.rs','-o','build-script'],folder);assert.equal(result.status,0,result.stderr);
  result=command(label+'-build',join(folder,'build-script'),[],folder,{TARGET:target,OUT_DIR:join(folder,'objects')});
  if(fixture.reject){assert.notEqual(result.status,0,'undefined-symbol proof unexpectedly accepted a missing definition');assert.match(result.stderr,/inline_missing_definition/);}
  else{
   assert.equal(result.status,0,result.stderr);writeFileSync(join(folder,'main.c'),fixture.main);
   result=command(label+'-consumer-object',cc,['-c','main.c','-o','main.o'],folder);assert.equal(result.status,0,result.stderr);
   result=command(label+'-consumer-link',fixture.files.some(row=>row.cpp)?cxx:cc,['main.o',join(folder,'objects/libjet_inline_native.a'),'-o','consumer'],folder);assert.equal(result.status,0,result.stderr);
   result=command(label+'-consumer-run',join(folder,'consumer'),[],folder);assert.equal(result.status,0,result.stderr);
  }
  rows.push({label,status:'pass'});
 }catch(error){rows.push({label,status:'fail',error:error.message});}
}
if(process.argv[3]){
 const unit=resolve(process.argv[3]),source=readFileSync(unit,'utf8'),providers=runtimeExportSignatures(root),entries=[];
 const pattern=/^\s*#\[[^\n]*\bFFI\((c|cpp)\)[^\n]*\]\s*\n\s*(?:pub\s+)?fn\s+([A-Za-z_]\w*)[^]*?\{\s*"""([^]*?)"""\s*\}/gm;
 for(const match of source.matchAll(pattern))entries.push({language:match[1],name:match[2],source:match[3],line:source.slice(0,match.index).split('\n').length});
 const definitions=new Map(),objects=[],failures=[];
 for(const [index,entry]of entries.entries()){
  const folder=join(out,'census');mkdirSync(folder,{recursive:true});const path=join(folder,index+(entry.language==='cpp'?'.cpp':'.c')),object=join(folder,index+'.o');
  writeFileSync(path,'#include <stdint.h>\n#include <stdbool.h>\n'+entry.source);
  const result=command('census-'+index+'-compile',entry.language==='cpp'?cxx:cc,['-fPIC','-c',path,'-o',object],root);
  if(result.status!==0){failures.push({name:entry.name,line:entry.line,error:result.stderr});continue;}
  const defined=command('census-'+index+'-defined','nm',['--defined-only','--extern-only',object],root),undefinedSymbols=command('census-'+index+'-undefined','nm',['--undefined-only',object],root);
  assert.equal(defined.status,0);assert.equal(undefinedSymbols.status,0);
  const symbols=text=>text.split('\n').map(line=>line.trim().split(/\s+/).at(-1)).filter(Boolean);
  for(const symbol of symbols(defined.stdout))definitions.set(symbol,entry.name);
  objects.push({...entry,object,undefined:symbols(undefinedSymbols.stdout)});
 }
 const references=objects.flatMap(entry=>entry.undefined.map(symbol=>({from:entry.name,line:entry.line,symbol,definedBy:definitions.get(symbol)??null,nativeProvider:providers.get(symbol)?.path??null}))),projectMissing=references.filter(row=>/^(?:jet_|os_)/.test(row.symbol)&&!row.definedBy&&!row.nativeProvider);
 const bridgeProof=objects.length?command('census-bridge-proof',objects.some(entry=>entry.language==='cpp')?cxx:cc,['-shared','-Wl,--no-undefined',...objects.map(entry=>entry.object),'-o',join(out,'census/bridge.so')],root):{status:null};
 const census={unit,entries:entries.map(({source,...entry})=>entry),compiled:objects.length,failures,bridgeProofStatus:bridgeProof.status,crossTranslationUnitReferences:references.filter(row=>row.definedBy),runtimeReferences:references.filter(row=>/^jet_/.test(row.symbol)),allUndefinedReferences:references,missingProjectSymbols:projectMissing};
 writeFileSync(join(out,'census.json'),JSON.stringify(census,null,2)+'\n');
 rows.push({label:'selfhost-inline-census',status:entries.length&&failures.length===0&&projectMissing.length===0&&bridgeProof.status===0?'pass':'fail',entries:entries.length,compiled:objects.length,compileFailures:failures.length,missingProjectSymbols:projectMissing.length,bridgeProofStatus:bridgeProof.status});
}
writeFileSync(join(out,'receipt.json'),JSON.stringify(rows,null,2)+'\n');console.log(JSON.stringify(rows,null,2));process.exitCode=rows.some(row=>row.status!=='pass')?1:0;
