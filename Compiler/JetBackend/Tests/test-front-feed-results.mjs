#!/usr/bin/env node
// Admission-data tests only: no candidate or native execution is simulated.
import assert from 'node:assert/strict';
import {mkdtempSync,readFileSync,rmSync,writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
const dir=mkdtempSync(join(tmpdir(),'jet-front-results-'));
const checker=join(dirname(fileURLToPath(import.meta.url)),'check-front-feed-results.mjs');
const cases=Array.from({length:398},(_,i)=>`case/${i}`);
const passing=cases.map(stem=>[stem,'pass','pack']);
const rows=value=>value.map(row=>row.join('\t')).join('\n')+'\n';
let checks=0;
function check({requested=cases,baseline=passing,rust=passing,jet=passing,ok=false},inspect=()=>{}){
 for(const [name,text] of [['cases',requested.join('\n')+'\n'],['baseline',rows(baseline)],['rust',rows(rust)],['jet',rows(jet)]])writeFileSync(join(dir,name),text);
 const run=spawnSync(process.execPath,[checker,...['cases','baseline','rust','jet','report'].map(name=>join(dir,name))],{encoding:'utf8'});
 assert.equal(run.status,ok?0:1,run.stderr);
 const report=JSON.parse(readFileSync(join(dir,'report'),'utf8'));
 assert.equal(report.ok,ok);inspect(report);checks++;
}
try{
 check({ok:true},report=>{assert.deepEqual(report.metric,{requested:398,rustPass:398,jetPass:398,rustWrong:0,jetWrong:0});assert.deepEqual(report.differences,[]);});
 check({requested:cases.slice(1)});
 check({requested:[cases[0],...cases.slice(0,-1)]});
 check({jet:passing.slice(1)},report=>assert.deepEqual(report.modes.jet.missing,[cases[0]]));
 check({rust:[...passing,passing[0]]});
 check({jet:[...passing,['extra','pass','pack']]});
 check({jet:[['case/0','wrong','stdout differs'],...passing.slice(1)]},report=>assert.equal(report.metric.jetWrong,1));
 check({rust:[['case/0','wrong','stdout differs'],...passing.slice(1)]},report=>assert.equal(report.metric.rustWrong,1));
 const baseline=passing.slice(2),rust=[['case/0','unsupported','function 100: missing route'],['case/1','unsupported','function 101: missing route'],...passing.slice(2)];
 check({baseline,rust,jet:passing,ok:true},report=>{assert.equal(report.metric.rustPass,396);assert.equal(report.metric.jetPass,398);assert.equal(report.differences.length,2);assert.equal(report.rankedRoots.length,1);assert.equal(report.rankedRoots[0].affected,2);assert.equal(report.differences[0].rust.detail,'function 100: missing route');});
 check({baseline,rust:passing,jet:rust},report=>assert.deepEqual(report.lostRustPasses,['case/0','case/1']));
 check({baseline:passing,rust,jet:rust},report=>assert.deepEqual(report.modes.rust.passesLost,['case/0','case/1']));
 check({rust:passing,jet:[['case/0','pass','static'],...passing.slice(1)],ok:true},report=>assert.equal(report.differences.length,1));
 console.log(`front-feed results: ${checks} admission-data checks passed (no native execution claim)`);
}finally{rmSync(dir,{recursive:true,force:true});}
