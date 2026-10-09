#!/usr/bin/env node
// Historical oracle recipe retained; frozen fixtures remain native inputs.
console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
process.exit(0);
// ONE deterministic seed render/exit capture. Never execute or capture OS draws.
import {readFileSync, writeFileSync, mkdirSync, existsSync} from 'node:fs';
import {resolve, dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const root=resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const scratch=process.argv[2];
if(!scratch) throw Error('usage: freeze-os-entropy-fixtures.mjs SCRATCH');
const tests=join(root,'Compiler/JetRuntime/Tests');
const target=join(tests,'CryptoOsEntropyWorld.stderr');
if(existsSync(target)) throw Error('refusing to recapture frozen World rejection');
mkdirSync(scratch,{recursive:true});
const renderer=join(root,'crates/jet-foundation/src/RuntimeDiagnosticCore.rs');
const scheduler=join(root,'crates/jet-codegen/src/Prelude/Scheduler.rs');
const registry=join(root,'crates/jet-codegen/src/Prelude/Diagnostics.jet');
const text=readFileSync(scheduler,'utf8');
const guard=text.slice(text.indexOf('pub fn jet_scheduler_world_reject_uncontrolled('),text.indexOf('pub fn jet_scheduler_world_rng_next('));
const match=guard.match(/"entropy" => "([^"]+)"/u);
if(!match) throw Error('seed World guard message not found');
const row=readFileSync(registry,'utf8').split('\n').find(line=>line.startsWith('diagnostic\tE3001\t')).split('\t');
if(row[5]!=='active'||row[1]!=='E3001') throw Error('E3001 registry row is not active');
const source=join(scratch,'seed-render.rs'), binary=join(scratch,'seed-render');
writeFileSync(source,`#![allow(dead_code)]
include!(${JSON.stringify(renderer)});
fn main(){
let row=Some(JetRuntimeDiagnosticRow{code:"E3001",what:${JSON.stringify(row[7])},why:${JSON.stringify(row[8])},fix:${JSON.stringify(row[9])},template_holes:&["msg"]});
let context=JetRuntimeStopContext{file:"",line:0,function:"",source_line:"",column:1,caret_len:1,expected_type:""};
let mut output=String::new();
jet_write_runtime_stop(&mut output,row,"E3001",context,&${JSON.stringify(match[1])},None).unwrap();
eprint!("{output}");
std::process::exit(jet_runtime_stop_status(row));
}
`);
function run(name,args){const r=spawnSync(name,args,{encoding:'utf8',timeout:60000});if(r.error||r.status!==0)throw Error(`${name} exit=${r.status} ${r.error??''}\n${r.stderr}`);return r.stdout;}
const rustc=run('rustc',['--version']).trim();
run('rustc',['--edition=2024',source,'-o',binary]);
const capture=spawnSync(binary,[],{encoding:'utf8',timeout:60000});
if(capture.error||capture.status!==70||capture.signal||capture.stdout!=='') throw Error('World render/exit seed capture did not exit 70 with only stderr');
writeFileSync(target,capture.stderr);
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const metadata={stdoutSha256:hash(readFileSync(join(tests,'CryptoOsEntropyWitness.stdout'))),expectationProvenance:'Handwritten public ABI contract witness line; NOT captured native stdout or random seed bytes',nativeExecution:'UNEXERCISED, owed at composed lower/audit/link/run',world:{message:match[1],exitCode:capture.status,stderrSha256:hash(capture.stderr),renderer:'crates/jet-foundation/src/RuntimeDiagnosticCore.rs',rendererSha256:hash(readFileSync(renderer)),scheduler:'crates/jet-codegen/src/Prelude/Scheduler.rs',schedulerSha256:hash(text),registry:'crates/jet-codegen/src/Prelude/Diagnostics.jet',registrySha256:hash(readFileSync(registry)),referenceSha256:hash(readFileSync(source)),rustc,guardExecution:'UNEXERCISED; capture executes the real shared renderer/status only, using the source-derived guard message, code E3001 and empty owner context'},abi:{symbol:'jet_random_os_entropy',bounds:[0,1048576],some:'tag1 writes payload pointer/count; empty ptr0/count0',none:'tag0 leaves pointer/count outputs untouched',stride:'RT_HOST_U8_STRIDE',provider:'existing rt_crypto_entropy_fill ONLY'},cases:['zero and out-of-bounds do not invoke provider','partial + interrupted suffix wipe through same provider','backward in-place widening: high byte values, 256 elements, 1MiB maximum','provider failure leaves output slots unchanged','World rejects counts 0/-1/1048577/7 before provider']};
writeFileSync(join(tests,'CryptoOsEntropyFixtures.json'),JSON.stringify(metadata,null,2)+'\n');
console.log(`OS_ENTROPY_WORLD_SEED_FROZEN exit=${capture.status} stderr_sha256=${hash(capture.stderr)} renderer_sha256=${metadata.world.rendererSha256} no_random_capture=true`);
