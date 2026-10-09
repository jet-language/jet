#!/usr/bin/env node
// Exercise the real verify-full control flow with real tiny fixture children,
// never a compiler build, release token, runtime corpus, or full-gate proof.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const repo=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const source=fs.readFileSync(path.join(repo,'Tools/agent/verify-full.sh'));
const parent=process.env.TMPDIR&&!/^\/tmp(?:\/|$)/.test(process.env.TMPDIR)?process.env.TMPDIR:path.join(os.homedir(),'.cache/jet-dev/scratch');
fs.mkdirSync(parent,{recursive:true});
const sandbox=fs.mkdtempSync(path.join(parent,'verify-full-collect-'));
const output=process.env.JET_COLLECT_REGRESSION_OUTPUT;
function put(name,body){const file=path.join(sandbox,name);fs.mkdirSync(path.dirname(file),{recursive:true});fs.writeFileSync(file,body,{mode:0o755});return file;}
const outcomes=[];
try{
 put('Tools/agent/verify-full.sh',source);
 for(const name of ['Tools/agent/clean-nix-tmp.sh','Tools/ci/tower-hygiene-gate.sh'])put(name,'#!/usr/bin/env bash\nexit 0\n');
 put('Tools/agent/verify-nix-eval-stopline.sh','#!/usr/bin/env bash\nprintf "nix-authority\\n" >> "$FIXTURE_COMMANDS"\nif [ "$FIXTURE_MODE" = authority ]; then exit 36; fi\nexit 0\n');
 put('Tools/ci/test-shards.sh',`#!/usr/bin/env bash
printf '%s\\n' '-p fixture --test alpha' '-p fixture --test beta' '-p fixture --test gamma' '-p fixture --lib'
if [ "\$FIXTURE_MODE" = missing ]; then printf '%s\\n' '-p jet --test canvas_scenarios' '-p jet --test cffi_native_matrix'; fi
`);
 const node=put('bin/node',`#!${process.execPath}
const fs=require('node:fs');const args=process.argv.slice(2),text=args.join(' ');fs.appendFileSync(process.env.FIXTURE_COMMANDS,'node '+text+'\\n');
if(args[0]==='--version'){console.log('v22.0.0');process.exit(0);}
if(process.env.FIXTURE_MODE==='integrity'&&text.includes('check-unsafe-ratchet'))process.exit(35);
if(process.env.FIXTURE_MODE==='preflights'){
 if(text.includes('check-agent-doc-flags'))process.exit(31);
 if(text.includes('verify-jet-shell-parity'))process.exit(33);
 if(text.includes('compiler-diff')&&args.includes('--canary'))process.exit(32);
 if(text.includes('compiler-diff')&&args.includes('--corpus'))process.exit(args.includes('--keep-going')?34:92);
}
if(text.includes('compiler-diff')&&args.includes('--corpus')&&!args.includes('--keep-going'))process.exit(92);
process.exit(0);
`);
 put('bin/cargo',`#!${process.execPath}
const fs=require('node:fs');const args=process.argv.slice(2),text=args.join(' '),mode=process.env.FIXTURE_MODE;fs.appendFileSync(process.env.FIXTURE_COMMANDS,'cargo '+text+'\\n');
if(args[0]==='metadata'){console.log(JSON.stringify({workspace_members:['fixture'],packages:[{id:'fixture',name:'fixture',targets:[{kind:['lib'],doctest:mode!=='disabled-docs'}]},{id:'unselected-dependency',name:'unselected-dependency',targets:[{kind:['lib'],doctest:true}]}]}));process.exit(0);}
if(args.includes('--keep-going')||!args.includes('--no-fail-fast'))process.exit(93);
if(['masked-signal','masked-exit','ordinary-101'].includes(mode)&&args.includes('alpha')&&!args.includes('--no-run')){
 const child=require('node:child_process').spawnSync(process.execPath,['-e',mode==='masked-signal'?'process.kill(process.pid,\"SIGSEGV\")':mode==='masked-exit'?'process.exit(137)':'process.exit(101)'],{stdio:'pipe'});
 if(mode==='masked-signal'&&child.signal!=='SIGSEGV')process.exit(94);
 if(mode!=='masked-signal'&&child.status!==(mode==='masked-exit'?137:101))process.exit(95);
 console.error(\"  process didn't exit successfully: fixture-test-child (\"+(child.signal?'signal: 11, SIGSEGV: invalid memory reference':'exit status: '+child.status)+')');
 process.exit(101);
}
if(args.includes('alpha')&&args.includes('--no-run')){
 if(mode==='unsafe')process.kill(process.pid,'SIGKILL');
 if(mode==='segv')process.kill(process.pid,'SIGSEGV');
 if(mode==='abort')process.kill(process.pid,'SIGABRT');
 if(mode==='ordinary'){console.error('first independent compile failure');process.exit(17);}
}
if(mode==='ordinary'&&args.includes('gamma')&&!args.includes('--no-run')){console.error('second independent assertion failure');process.exit(23);}
if(mode==='lib-failure'&&args.includes('--lib')&&args.includes('--no-run')){console.error('cfg(test)-only compile failure');process.exit(27);}
console.log('fixture child PASS');process.exit(0);
`);
 for(const [name,version] of [['chromium','Chromium 123'],['firefox','Firefox 123'],['geckodriver','geckodriver 0.35'],['cc','fixture cc'],['ar','fixture ar'],['rustc','fixture rustc']])put(`bin/${name}`,`#!/usr/bin/env bash\nprintf '%s\\n' '${version}'\n`);
 for(const mode of ['ordinary','preflights','unsafe','segv','abort','integrity','missing','lib-failure','authority','disabled-docs','masked-signal','masked-exit','ordinary-101']){
  const evidence=path.join(sandbox,`evidence-${mode}`),commands=path.join(sandbox,`commands-${mode}.log`);fs.writeFileSync(commands,'');
  const env={...process.env,PATH:`${sandbox}/bin:${process.env.PATH}`,FIXTURE_MODE:mode,FIXTURE_COMMANDS:commands,JET_VERIFY_TMPDIR:path.join(sandbox,'temporary'),JET_CI_EVIDENCE_DIR:evidence,JET_CANVAS_CHROMIUM:path.join(sandbox,'bin/chromium'),JET_CANVAS_FIREFOX:path.join(sandbox,'bin/firefox'),JET_CANVAS_GECKODRIVER:path.join(sandbox,'bin/geckodriver'),JET_CANVAS_NODE:node,JET_CFFI_CC:path.join(sandbox,'bin/cc'),JET_CFFI_AR:path.join(sandbox,'bin/ar'),JET_CFFI_RUSTC:path.join(sandbox,'bin/rustc'),JETPACK_DOGFOOD_ROOT:path.join(sandbox,'dogfood'),JETPACK_DOGFOOD_TARGET_DIR:path.join(sandbox,'dogfood-target'),JET_CANDIDATE_COMPILER:'fixture-candidate'};
  for(const name of ['JET_TEST_SHARD','JET_TEST_SHARD_COUNT','JET_VERIFY_CANVAS_PREREQUISITES_ONLY','JET_VERIFY_TEMP_PROBE_ONLY'])delete env[name];
  if(mode==='missing'){env.JET_CANVAS_FIREFOX=path.join(sandbox,'absent-firefox');env.JET_CFFI_CC=path.join(sandbox,'absent-cc');}
  const result=spawnSync('bash',['-c','ulimit -c 0; exec bash Tools/agent/verify-full.sh'],{cwd:sandbox,env,encoding:'utf8',timeout:30000});assert.equal(result.error,undefined);assert.equal(result.signal,null);assert.equal(result.status,mode==='integrity'?35:mode==='disabled-docs'?0:1,`${mode}: ${result.stdout}\n${result.stderr}`);
  const summary=path.join(evidence,'verify-full-summary.tsv'),rows=fs.existsSync(summary)?fs.readFileSync(summary,'utf8').trim().split('\n').slice(1).map(line=>line.split('\t')):[];
  const byScope=Object.fromEntries(rows.map(row=>[row[1],row]));const logged=fs.readFileSync(commands,'utf8');
  if(mode==='ordinary'){
   assert.deepEqual(['compile-1','compile-2','compile-3','compile-4','run-1','run-2','run-3','run-4','doc-4'].map(scope=>[scope,byScope[scope]?.[0]]),[['compile-1','FAIL'],['compile-2','PASS'],['compile-3','PASS'],['compile-4','PASS'],['run-1','BLOCKED'],['run-2','PASS'],['run-3','FAIL'],['run-4','PASS'],['doc-4','PASS']]);
   assert.equal(byScope['compile-1'][2],'17');assert.equal(byScope['run-3'][2],'23');assert.equal(byScope['compile-1'][5],'-p fixture --test alpha');assert.equal(byScope['run-3'][5],'-p fixture --test gamma');assert(!logged.includes('cargo test -p fixture --test alpha --no-fail-fast\n'));assert(logged.indexOf('--test beta --no-run')>logged.indexOf('--test alpha --no-run'));assert(fs.readFileSync(byScope['compile-1'][4],'utf8').includes('first independent compile failure'));assert(fs.readFileSync(byScope['run-3'][4],'utf8').includes('second independent assertion failure'));
  }else if(mode==='preflights'){
   for(const scope of ['agent-doc-flags','shell-parity','comparator-canary','comparator-corpus'])assert.equal(byScope[scope][0],'FAIL');assert.equal(byScope['run-2'][0],'PASS');assert.equal(rows.filter(row=>row[0]==='FAIL').length,4);
  }else if(['unsafe','segv','abort'].includes(mode)){
   assert.equal(byScope['compile-1'][0],'FAIL');assert.equal(byScope['compile-1'][2],String({unsafe:137,segv:139,abort:134}[mode]));for(const scope of ['compile-2','compile-3','compile-4','run-1','run-2','run-3','run-4','doc-4'])assert.equal(byScope[scope][0],'BLOCKED');assert(!logged.includes('cargo test -p fixture --test beta'));
  }else if(['masked-signal','masked-exit'].includes(mode)){
   assert.equal(byScope['run-1'][0],'FAIL');assert.equal(byScope['run-1'][2],'101');assert(byScope['run-1'][3].includes(mode==='masked-signal'?'unsafe Cargo child signal 11':'unsafe Cargo child exit 137'));
   for(const scope of ['run-2','run-3','run-4','doc-4'])assert.equal(byScope[scope][0],'BLOCKED');assert.equal(byScope['compile-2'][0],'PASS');assert(!logged.includes('cargo test -p fixture --test beta --no-fail-fast\n'));
   assert(fs.readFileSync(byScope['run-1'][4],'utf8').includes(mode==='masked-signal'?'signal: 11, SIGSEGV':'exit status: 137'));
  }else if(mode==='ordinary-101'){
   assert.equal(byScope['run-1'][0],'FAIL');assert.equal(byScope['run-1'][2],'101');assert.equal(byScope['run-1'][3],'-');
   for(const scope of ['run-2','run-3','run-4','doc-4'])assert.equal(byScope[scope][0],'PASS');assert(logged.includes('cargo test -p fixture --test beta --no-fail-fast\n'));
  }else if(mode==='integrity'){
   assert(!logged.includes('cargo '));assert.equal(rows.length,0);
  }else if(mode==='lib-failure'){
   assert.equal(byScope['compile-4'][0],'FAIL');assert.equal(byScope['compile-4'][2],'27');assert.equal(byScope['run-4'][0],'BLOCKED');assert.equal(byScope['doc-4'][0],'PASS');assert(logged.includes('cargo test -p fixture --doc --no-fail-fast'));assert.equal(byScope['run-2'][0],'PASS');
  }else if(mode==='authority'){
   assert.equal(byScope['nix-eval-stopline'][0],'FAIL');assert.equal(byScope['nix-eval-stopline'][2],'36');for(const scope of ['compile-1','compile-2','compile-3','compile-4','run-1','run-2','run-3','run-4','doc-4'])assert.equal(byScope[scope][0],'BLOCKED');assert(!logged.split('nix-authority\n')[1].includes('cargo '));
  }else if(mode==='disabled-docs'){
   assert.equal(byScope['compile-4'][0],'PASS');assert.equal(byScope['run-4'][0],'PASS');assert.equal(byScope['doc-4'],undefined);assert(!logged.includes('cargo test -p fixture --doc'));assert.equal(rows.filter(row=>row[0]!=='PASS').length,0);
  }else{
   for(const scope of ['canvas-prerequisites','cffi-prerequisites','run-5','run-6'])assert.equal(byScope[scope][0],'BLOCKED');assert.equal(byScope['run-2'][0],'PASS');assert.equal(byScope['compile-5'][0],'PASS');assert.equal(byScope['compile-6'][0],'PASS');
  }
  if(fs.existsSync(path.join(sandbox,'temporary')))assert.deepEqual(fs.readdirSync(path.join(sandbox,'temporary')),[],'production cleanup must remove temporary work while retaining summary/logs');
  if(output){const dest=path.join(output,mode);fs.mkdirSync(dest,{recursive:true});if(fs.existsSync(evidence))fs.cpSync(evidence,path.join(dest,'evidence'),{recursive:true});fs.copyFileSync(commands,path.join(dest,'commands.log'));fs.writeFileSync(path.join(dest,'stdout.log'),result.stdout);fs.writeFileSync(path.join(dest,'stderr.log'),result.stderr);fs.writeFileSync(path.join(dest,'rc'),`${result.status}\n`);}
  outcomes.push({mode,actualRc:result.status,counts:Object.fromEntries(['PASS','FAIL','BLOCKED'].map(status=>[status,rows.filter(row=>row[0]===status).length])),regression:'PASS'});
 }
 console.log(JSON.stringify({scope:'Actual production shell with real fixture children; NOT compiler/full-suite/Gate proof',sourceSha256:crypto.createHash('sha256').update(source).digest('hex'),outcomes,green:true},null,2));
 console.log(`CHECK OK: durable verify-full collect-all runner (${outcomes.length} scenarios)`);
}finally{fs.rmSync(sandbox,{recursive:true,force:true});}
