#!/usr/bin/env node
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const [output,binary]=process.argv.slice(2);
if(!output||!binary||process.argv.length!==4)throw Error('usage: run-ui-drag-witness.mjs OUT UI_DRAG_WITNESS');
const here=dirname(fileURLToPath(import.meta.url)),out=resolve(output);mkdirSync(out,{recursive:true});
const modes=['phase','operation','item'].flatMap(kind=>['clone-tag','drop-tag','eq-tag'].map(op=>kind+'-'+op));
for(const [mode,status,golden] of [['',0,'UiDragWitness.stdout'],...modes.map(mode=>[mode,70,'DataPlotStops-getter-variant.stderr'])]){
    const name=mode||'ownership',result=spawnSync(resolve(binary),mode?[mode]:[],{timeout:30000,maxBuffer:1<<20});
    writeFileSync(join(out,name+'.stdout'),result.stdout??Buffer.alloc(0));writeFileSync(join(out,name+'.stderr'),result.stderr??Buffer.alloc(0));
    writeFileSync(join(out,name+'.status.json'),JSON.stringify({status:result.status,signal:result.signal,error:result.error?.message??null})+'\n');
    if(result.error||result.signal!==null||result.status!==status)throw Error(`${name}: expected exit${status}, got ${result.status}/${result.signal}/${result.error?.message??''}`);
    const expected=readFileSync(join(here,golden));
    if(!result.stdout.equals(mode?Buffer.alloc(0):expected)||!result.stderr.equals(mode?expected:Buffer.alloc(0)))throw Error(name+': byte-exact output mismatch');
}
console.log('UI Drag: seed ownership and shared variant stops byte-identical');
