#!/usr/bin/env node
// One selected host corpus, three existing reference tiers and the real Jet pack.
// Reuse discovery/staging/diagnostic rules rather than a second golden registry.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { collectGoldenEntries, loadExampleStdin } from '../../../Tools/agent/compiler-diff.mjs';
import { copyFeatureProject, featureProjectRoot, withoutWarningDiagnostics } from '../../../Tools/agent/run-feature-examples.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
import {readGoldenResults} from './golden-results.mjs';
const [output,mode,resultsFlag,sharedResults]=process.argv.slice(2);
if (mode === 'reference') {
  console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
  process.exit(0);
}
if(!output||!['prepare','reference','native'].includes(mode)||(resultsFlag&&(mode!=='native'||resultsFlag!=='--results'||!sharedResults))||process.argv.length>(sharedResults?6:4))throw Error('usage: run-host-goldens.mjs OUT prepare|native [--results fresh-results.tsv]');
const out=resolve(output),features=join(root,'Examples/features'),list=join(out,'cases.list');
const filters=['io/process_','io/env_overlay','io/os_','io/sys_','io/files','io/path','io/terminal_source'];
mkdirSync(out,{recursive:true});
if(mode==='prepare') {
  const entries=collectGoldenEntries(features).filter(entry=>filters.some(filter=>entry.stem.includes(filter))&&(existsSync(join(features,'expected',`${entry.stem}.out`))||existsSync(join(features,'expected',`${entry.stem}.err.out`))));
  if(!entries.length)throw Error('host golden selection is empty');
  writeFileSync(list,entries.map(entry=>entry.stem).join('\n')+'\n');
  writeFileSync(join(out,'selection.json'),JSON.stringify({filters,stems:entries.map(entry=>entry.stem)},null,2)+'\n');
} else if(mode==='native') {
  const result=sharedResults?null:spawnSync(process.execPath,[join(root,'Compiler/JetBackend/Tests/run-goldens.mjs'),join(out,'native'),'--front','jet','--case-list',list],{cwd:root,stdio:'inherit'});
  if(result&&(result.error||result.status!==0))throw Error(`native host goldens: ${result.error?.message ?? result.status}`);
  const rows=readGoldenResults(sharedResults?resolve(sharedResults):join(out,'native/results.tsv'));
  const expected=readFileSync(list,'utf8').trim().split('\n');
  const failures=expected.filter(stem=>rows.get(stem)!=='pass');
  if(failures.length||(!sharedResults&&rows.size!==expected.length))throw Error(`native host corpus did not pass every selected case: ${JSON.stringify(failures)}`);
} else {
  const compiler=process.env.JET;if(!compiler)throw Error('JET must name the published matching compiler');
  const selected=new Set(readFileSync(list,'utf8').trim().split('\n'));
  const entries=collectGoldenEntries(features).filter(entry=>selected.has(entry.stem));
  if(!entries.length||entries.length!==selected.size)throw Error('host case manifest changed');
  const stdin=loadExampleStdin(),results=[];
  for(const [tier,flags] of [['interpret',['--interpret']],['dev',[]],['release',['--release']]])for(const entry of entries) {
    const dir=join(out,'reference',tier,entry.stem.replace(/[^A-Za-z0-9_-]/g,'_'));
    mkdirSync(dir,{recursive:true});
    const project=featureProjectRoot(entry);
    if(project)copyFeatureProject(project,join(dir,relative(root,project)));
    else if(basename(entry.path)==='run.jet')copyFeatureProject(dirname(entry.path),join(dir,relative(root,dirname(entry.path))));
    else {const destination=join(dir,relative(root,entry.path));mkdirSync(dirname(destination),{recursive:true});writeFileSync(destination,readFileSync(entry.path));}
    const args=['run',...flags,entry.shown];
    const result=spawnSync(join(root,'Tools/agent/jet-env'),[compiler,...args],{cwd:dir,encoding:'utf8',maxBuffer:1<<27,input:stdin.get(entry.stem)??null,timeout:0});
    const errorPath=join(features,'expected',`${entry.stem}.err.out`),failure=existsSync(errorPath);
    const expectedOut=failure?'':readFileSync(join(features,'expected',`${entry.stem}.out`),'utf8');
    const stderrPath=join(features,'expected',`${entry.stem}.stderr.out`);
    const expectedErr=failure?readFileSync(errorPath,'utf8'):existsSync(stderrPath)?readFileSync(stderrPath,'utf8'):'';
    const actualErr=withoutWarningDiagnostics(result.stderr??'');
    const passed=!result.error&&(failure?[1,70].includes(result.status):result.status===0)&&(failure||result.stdout===expectedOut)&&actualErr===expectedErr;
    const receipt={tier,stem:entry.stem,command:[compiler,...args],status:result.status,signal:result.signal,error:result.error?.message??null,stdout:result.stdout,stderr:result.stderr,expected_stdout:expectedOut,expected_stderr:expectedErr,passed};
    writeFileSync(join(dir,'receipt.json'),JSON.stringify(receipt,null,2)+'\n');results.push(receipt);
  }
  writeFileSync(join(out,'reference-summary.json'),JSON.stringify({cases:entries.length,executions:results.length,passed:results.filter(row=>row.passed).length,results},null,2)+'\n');
  if(results.length!==entries.length*3||results.some(row=>!row.passed))throw Error(`host reference tier failures: ${results.filter(row=>!row.passed).map(row=>`${row.tier}:${row.stem}`).join(', ')}`);
}
console.log(`host goldens ${mode}: ok`);
