#!/usr/bin/env node
// --freeze is retired: the Rust seed oracle is reported UNCOVERED (exit 2) and never run.
// --native accepts only a freshly composed Jet-only runtime object.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {resolve,dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const here=join(root,'Compiler/JetRuntime/Tests');
const [mode,outArg,object]=process.argv.slice(2);
if (mode === '--freeze') {
  console.log(JSON.stringify({status: 'UNCOVERED', cardRequired: true, reason: '--freeze Rust seed oracle (cargo build of the Layout.rs reference) retired (owner #38/#59; Tower #4923): LayoutSeed.json is not regenerated; no cargo/rustc invoked'}));
  process.exit(2);
}
if(mode!=='--native'||!outArg||!object) throw Error('usage: run-native-layout.mjs --native OUT fresh-runtime.o');
const out=resolve(outArg); mkdirSync(out,{recursive:true});
const hash=b=>createHash('sha256').update(b).digest('hex');
const vectors=readFileSync(join(here,'LayoutVectors.txt'),'utf8');
function run(cmd,args,label){
  const r=spawnSync(cmd,args,{cwd:root,encoding:'utf8',maxBuffer:1<<24});
  writeFileSync(join(out,label+'.log'),`exit=${r.status}\n${r.stdout??''}${r.stderr??''}`);
  process.stdout.write(r.stdout??''); process.stderr.write(r.stderr??'');
  console.log(`${label}: tool exit=${r.status}`);
  if(r.error||r.status!==0) throw Error(`${label}: ${r.error?.message??r.status}`);
  return r.stdout;
}
function literals(){
  // Verify every reused draft byte and declared length, including UTF-8 em dash.
  const source=readFileSync(join(root,'Compiler/JetRuntime/Source/NativeLayoutText.jet'),'utf8');
  const buffer=Buffer.alloc(432);
  for(const m of source.matchAll(/layout_put\(root \+ I64\{(\d+)\}, I64\{(\d+)\}\)/gu)) buffer.writeBigInt64LE(BigInt(m[2]),Number(m[1]));
  const expected=['layout constraint expression has more than 16 distinct terms','constraint#',' (<=)',' (>=)',' (==)','suggest(',')','(layout has no feasible solution)','layout "','" has no feasible solution — conflicting constraints: ', ', ', '.'];
  const offsets=[192,256,272,280,288,296,304,312,352,360,416,424];
  for(let i=0;i<expected.length;i++) {
    const length=Number(buffer.readBigInt64LE(i*16+8));
    if(buffer.subarray(offsets[i],offsets[i]+length).toString('utf8')!==expected[i]) throw Error(`Layout literal ${i} bytes/length mismatch`);
  }
  console.log('Layout reused draft literals: all 12 exact');
}
literals();
if(mode==='--native') {
  const frozen=JSON.parse(readFileSync(join(here,'LayoutSeed.json'),'utf8'));
  if(hash(vectors)!==frozen.vectors_sha256||hash(frozen.stdout)!==frozen.stdout_sha256) throw Error('frozen oracle/vector checksum mismatch');
  run(nativeCCompiler,['-std=c11','-Wall','-Wextra','-Werror','-pedantic',join(here,'NativeLayoutWitness.c'),resolve(object),'-Wl,--wrap=jet_rt_free','-lm','-pthread','-o',join(out,'native-layout')],'native-link');
  const actual=run(join(out,'native-layout'),[join(here,'LayoutVectors.txt')],'native-execute').trim().split('\n');
  if(actual.pop()!=='Layout6 ownership/registry/clone/drop OK') throw Error('ownership witness did not complete');
  const expected=frozen.stdout.trim().split('\n').filter(s=>!s.startsWith('p '));
  if(expected.length!==actual.length) throw Error('native observation count mismatch');
  for(let i=0;i<expected.length;i++) {
    if(expected[i].startsWith('v ')) {
      const a=actual[i].split(' '),e=expected[i].split(' ');
      if(a.slice(0,3).join(' ')!==e.slice(0,3).join(' ')||!Number.isFinite(Number(a[3]))||Math.abs(Number(a[3])-Number(e[3]))>1e-7) throw Error(`value mismatch ${i}: ${actual[i]} != ${expected[i]}`);
    } else if(actual[i]!==expected[i]) throw Error(`observation mismatch ${i}: ${actual[i]} != ${expected[i]}`);
  }
  for(const row of frozen.stdout.trim().split('\n').filter(s=>s.startsWith('p '))) {
    const [tag,...parts]=row.slice(2).split('|'),message=parts.join('|');
    const r=spawnSync(join(out,'native-layout'),['--fatal',tag],{cwd:root,encoding:'utf8'});
    writeFileSync(join(out,tag+'.log'),`exit=${r.status}\n${r.stdout??''}${r.stderr??''}`);
    if(r.error||r.status!==70||!(r.stderr??'').includes(message)) throw Error(`fatal ${tag} did not preserve seed message/exit70`);
    console.log(`Layout fatal ${tag}: tool exit=70; seed message exact`);
  }
  console.log(`Layout6 native/seed ${expected.length} observations + 3 fatal modes + ownership OK`);
}
