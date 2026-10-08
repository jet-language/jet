#!/usr/bin/env node
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const [output,binary,oracle]=process.argv.slice(2);
if(!output||!binary||!oracle||process.argv.length!==5)throw Error('usage: run-ui-key-witness.mjs OUT UI_KEY_WITNESS FROZEN_CHAR_ORACLE');
const here=dirname(fileURLToPath(import.meta.url)),out=resolve(output);
mkdirSync(out,{recursive:true});
for(const [name,args,status,golden] of [['ownership',[resolve(oracle)],0,'UiKeyWitness.stdout'],['key-tag',['key-tag'],70,'DataPlotStops-getter-variant.stderr']]){
    const result=spawnSync(resolve(binary),args,{timeout:300000,maxBuffer:1<<20});
    writeFileSync(join(out,name+'.stdout'),result.stdout??Buffer.alloc(0));writeFileSync(join(out,name+'.stderr'),result.stderr??Buffer.alloc(0));
    writeFileSync(join(out,name+'.status.json'),JSON.stringify({status:result.status,signal:result.signal,error:result.error?.message??null})+'\n');
    if(result.error||result.signal!==null||result.status!==status)throw Error(`${name}: expected exit ${status}, got ${result.status}/${result.signal}/${result.error?.message??''}`);
    const expected=readFileSync(join(here,golden));
    if(!result.stdout.equals(status?Buffer.alloc(0):expected)||!result.stderr.equals(status?expected:Buffer.alloc(0)))throw Error(name+': byte-exact output mismatch');
}
console.log('UI Key: exhaustive real-provider oracle and record-variant stop byte-identical');
