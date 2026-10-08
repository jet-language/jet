#!/usr/bin/env node
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const [output,binary]=process.argv.slice(2);
if(!output||!binary||process.argv.length!==4)throw Error('usage: run-ui-clipboard-witness.mjs OUT UI_CLIPBOARD_WITNESS');
const here=dirname(fileURLToPath(import.meta.url)),out=resolve(output);mkdirSync(out,{recursive:true});
const result=spawnSync(resolve(binary),[],{timeout:30000,maxBuffer:1<<20});
writeFileSync(join(out,'ownership.stdout'),result.stdout??Buffer.alloc(0));writeFileSync(join(out,'ownership.stderr'),result.stderr??Buffer.alloc(0));
writeFileSync(join(out,'ownership.status.json'),JSON.stringify({status:result.status,signal:result.signal,error:result.error?.message??null})+'\n');
if(result.error||result.signal!==null||result.status!==0)throw Error(`ownership: expected exit0, got ${result.status}/${result.signal}/${result.error?.message??''}`);
if(!result.stdout.equals(readFileSync(join(here,'UiClipboardWitness.stdout')))||result.stderr.length)throw Error('ownership: byte-exact output mismatch');
console.log('UI Clipboard: real-provider ownership byte-identical');
