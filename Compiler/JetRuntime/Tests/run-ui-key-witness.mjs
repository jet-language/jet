#!/usr/bin/env node
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const [output,binary]=process.argv.slice(2);
if(!output||!binary||process.argv.length!==4)throw Error('usage: run-ui-key-witness.mjs OUT UI_KEY_WITNESS');
const here=dirname(fileURLToPath(import.meta.url)),out=resolve(output);
mkdirSync(out,{recursive:true});
// The char Debug goldens for all 1,112,064 scalars, rebuilt from the tracked
// CharDebugPlain.ranges: a scalar listed there renders as itself in quotes,
// \0 \t \n \r \' \\ use their short escapes, and every other scalar renders
// as \u{hex}. Each record is u32le scalar, u8 length, then the bytes; the
// sha256 pins the exact frozen goldens.
const plain=readFileSync(join(here,'CharDebugPlain.ranges'),'utf8').trim().split('\n').map(row=>row.split('..').map(cp=>Number.parseInt(cp,16)));
const short={0:"'\\0'",9:"'\\t'",10:"'\\n'",13:"'\\r'",39:"'\\''",92:"'\\\\'"};
const records=[];
let range=0;
for(let cp=0;cp<=0x10ffff;++cp){
    if(cp>=0xd800&&cp<=0xdfff)continue;
    while(range<plain.length&&(plain[range][1]??plain[range][0])<cp)++range;
    const listed=range<plain.length&&plain[range][0]<=cp;
    const text=Buffer.from(short[cp]??(listed?`'${String.fromCodePoint(cp)}'`:`'\\u{${cp.toString(16)}}'`),'utf8');
    const head=Buffer.alloc(5);head.writeUInt32LE(cp,0);head[4]=text.length;
    records.push(head,text);
}
const goldens=Buffer.concat(records),oracle=join(out,'char-debug-goldens.bin');
if(createHash('sha256').update(goldens).digest('hex')!=='72a498085e0dc43fc6f9b0bc6f29482738c01ab7a35c11d72a1cfaf2504807a5')throw Error('char Debug goldens differ from the frozen bytes');
writeFileSync(oracle,goldens);
for(const [name,args,status,golden] of [['ownership',[oracle],0,'UiKeyWitness.stdout'],['key-tag',['key-tag'],70,'DataPlotStops-getter-variant.stderr']]){
    const result=spawnSync(resolve(binary),args,{timeout:300000,maxBuffer:1<<20});
    writeFileSync(join(out,name+'.stdout'),result.stdout??Buffer.alloc(0));writeFileSync(join(out,name+'.stderr'),result.stderr??Buffer.alloc(0));
    writeFileSync(join(out,name+'.status.json'),JSON.stringify({status:result.status,signal:result.signal,error:result.error?.message??null})+'\n');
    if(result.error||result.signal!==null||result.status!==status)throw Error(`${name}: expected exit ${status}, got ${result.status}/${result.signal}/${result.error?.message??''}`);
    const expected=readFileSync(join(here,golden));
    if(!result.stdout.equals(status?Buffer.alloc(0):expected)||!result.stderr.equals(status?expected:Buffer.alloc(0)))throw Error(name+': byte-exact output mismatch');
}
console.log('UI Key: exhaustive real-provider oracle and record-variant stop byte-identical');
