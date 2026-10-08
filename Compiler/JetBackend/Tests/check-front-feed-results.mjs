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
const feeds={};
for(const [mode,file] of [['rust',rustFile],['jet',jetFile]]){
 const rows=readRows(file), map=new Map(rows.map(row=>[row[0],row]));
 feeds[mode]=map;
 const counts={};for(const row of rows)counts[row[1]]=(counts[row[1]]??0)+1;
 const missing=cases.filter(stem=>!map.has(stem));
 const extra=rows.filter(row=>!cases.includes(row[0])).map(row=>row[0]);
 const wrong=rows.filter(row=>row[1]==='wrong').map(row=>row[0]);
 const passesLost=baseline.filter(row=>row[1]==='pass'&&map.get(row[0])?.[1]!=='pass').map(row=>row[0]);
 modes[mode]={file,observed:rows.length,counts,missing,extra,wrong,passesLost};
 if(map.size!==rows.length||missing.length||extra.length||wrong.length||passesLost.length) defects.push(`${mode}: incomplete, duplicate, wrong, or lost baseline passes`);
}
const lostRustPasses=[...feeds.rust.values()].filter(row=>row[1]==='pass'&&feeds.jet.get(row[0])?.[1]!=='pass').map(row=>row[0]);
if(lostRustPasses.length)defects.push('Jet front lost current Rust-front passes');
if((modes.jet.counts.pass??0)<(modes.rust.counts.pass??0))defects.push('Jet-front pass count is below Rust-front count');
const differences=[];
const roots=new Map();
for(const stem of cases){
 const rust=feeds.rust.get(stem),jet=feeds.jet.get(stem);
 if(rust?.[1]===jet?.[1]&&rust?.slice(2).join('\t')===jet?.slice(2).join('\t'))continue;
 const row={stem,rust:{verdict:rust?.[1]??'missing',detail:rust?.slice(2).join('\t')??''},jet:{verdict:jet?.[1]??'missing',detail:jet?.slice(2).join('\t')??''}};
 differences.push(row);
 // Keep diagnostic text intact in each row; normalize IDs only for grouping.
 const detail=row.jet.verdict==='pass'?row.rust.detail:row.jet.detail||row.rust.detail;
 const root=`${row.rust.verdict} -> ${row.jet.verdict}: ${detail}`.replace(/-?\d{2,}/g,'N').replace(/v\d+/g,'vN');
 if(!roots.has(root))roots.set(root,[]);
 roots.get(root).push(stem);
}
const rankedRoots=[...roots].map(([root,stems])=>({root,affected:stems.length,stems})).sort((a,b)=>b.affected-a.affected||a.root.localeCompare(b.root));
const metric={requested:cases.length,rustPass:modes.rust.counts.pass??0,jetPass:modes.jet.counts.pass??0,rustWrong:modes.rust.wrong.length,jetWrong:modes.jet.wrong.length};
const report={casesFile,baselineFile,requested:cases.length,metric,modes,lostRustPasses,differences,rankedRoots,defects,ok:!defects.length};
writeFileSync(reportFile,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));
process.exitCode=report.ok?0:1;
