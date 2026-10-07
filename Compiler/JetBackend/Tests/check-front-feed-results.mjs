#!/usr/bin/env node
// Admission compares complete front feeds, not a hand-picked passing subset.
import {readFileSync,writeFileSync} from 'node:fs';
const [casesFile,baselineFile,rustFile,jetFile,reportFile]=process.argv.slice(2);
if(!reportFile)throw new Error('expected CASES BASELINE RUST_RESULTS JET_RESULTS REPORT');
const lines=file=>readFileSync(file,'utf8').trim().split('\n').filter(Boolean);
const cases=lines(casesFile);
const readRows=file=>lines(file).map(line=>line.split('\t'));
const baseline=readRows(baselineFile);
const defects=[];
if(cases.length!==398||new Set(cases).size!==398) defects.push('acceptance requires exactly 398 unique requested stems');
const modes={};
for(const [mode,file] of [['rust',rustFile],['jet',jetFile]]){
 const rows=readRows(file), map=new Map(rows.map(row=>[row[0],row]));
 const counts={};for(const row of rows)counts[row[1]]=(counts[row[1]]??0)+1;
 const missing=cases.filter(stem=>!map.has(stem));
 const extra=rows.filter(row=>!cases.includes(row[0])).map(row=>row[0]);
 const wrong=rows.filter(row=>row[1]==='wrong').map(row=>row[0]);
 const passesLost=baseline.filter(row=>row[1]==='pass'&&map.get(row[0])?.[1]!=='pass').map(row=>row[0]);
 modes[mode]={file,observed:rows.length,counts,missing,extra,wrong,passesLost};
 if(map.size!==rows.length||missing.length||extra.length||wrong.length||passesLost.length) defects.push(`${mode}: incomplete, duplicate, wrong, or lost baseline passes`);
}
const jet=new Map(readRows(jetFile).map(row=>[row[0],row[1]]));
const lostRustPasses=readRows(rustFile).filter(row=>row[1]==='pass'&&jet.get(row[0])!=='pass').map(row=>row[0]);
if(lostRustPasses.length)defects.push('Jet front lost current Rust-front passes');
if((modes.jet.counts.pass??0)<(modes.rust.counts.pass??0))defects.push('Jet-front pass count is below Rust-front count');
const report={casesFile,baselineFile,requested:cases.length,modes,lostRustPasses,defects,ok:!defects.length};
writeFileSync(reportFile,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));
process.exitCode=report.ok?0:1;
