#!/usr/bin/env node
// Use the ladder's canonical, unrounded hierarchical phase rows, not report text.
import {readFileSync,existsSync} from 'node:fs';
import {join,resolve} from 'node:path';
function env(file) {
  return Object.fromEntries(readFileSync(file,'utf8').split('\n').filter(s=>s.includes('=')).map(s=>{const at=s.indexOf('=');return [s.slice(0,at),s.slice(at+1)];}));
}
try {
  if(process.argv.length!==3) throw new Error('usage: growth-gate.sh <ladder-run-dir>');
  const {readTrace,phases,fitExponent}=await import('../ladder.mjs');
  const root=resolve(process.argv[2]),lines=readFileSync(join(root,'rungs.tsv'),'utf8').trim().split('\n');
  const headers=lines.shift().split('\t'),runs=[],invalid=[],recorded=new Set();
  const metadata=existsSync(join(root,'ladder.env'))?env(join(root,'ladder.env')):{};
  const requested=new Set((metadata.selected||'').split(',').filter(Boolean));
  for(const line of lines) {
    const rung=Object.fromEntries(headers.map((h,i)=>[h,line.split('\t')[i]])),dir=join(root,rung.rung);
    if(!existsSync(join(dir,'result.env'))) continue;
    recorded.add(rung.rung);
    const result=env(join(dir,'result.env'));
    if(result.status!=='ok') invalid.push(`${rung.rung}: failed compilation (rc=${result.rc})`);
    try {
      const file=join(dir,'trace.json'),balances=new Map();
      // The human reader tolerates malformed/truncated streaming input. A HARD
      // gate records that as unavailable instead of silently dropping events.
      for(const raw of readFileSync(file,'utf8').split('\n')) {
        const line=raw.trim().replace(/,$/,'');
        if(!line || line==='[' || line===']') continue;
        const e=JSON.parse(line);
        if(!['B','E'].includes(e.ph)) continue;
        if(typeof e.cat!=='string' || !Number.isFinite(e.ts)) throw new Error('span has invalid category/time');
        const key=`${e.tid}:${e.cat}`,depth=balances.get(key)||0;
        if(e.ph==='E' && !depth) throw new Error(`unpaired span end ${key}`);
        balances.set(key,depth+(e.ph==='B'?1:-1));
      }
      const trace=readTrace(file),wall=Number(result.wall_ms)/1000,bytes=Number(rung.unit_bytes);
      if(!Number.isFinite(wall) || wall<=0 || !Number.isFinite(bytes) || bytes<=0) throw new Error('invalid wall/unit size');
      const rows=phases(trace,wall);
      for(const [name,row] of rows) if(row.open) {invalid.push(`${rung.rung}: unfinished ${name} excluded from fit`);rows.delete(name);}
      runs.push({bytes,rows});
    } catch(e) {invalid.push(`${rung.rung}: ${e.message}`);}
  }
  for(const name of requested) if(!recorded.has(name)) invalid.push(`${name}: requested rung has no result`);
  const names=new Set(runs.flatMap(r=>[...r.rows.keys()]));let fitted=0,rssFitted=0,offenders=0;
  for(const name of [...names].sort()) for(const [metric,property] of [['seconds','secs'],['rss','rss'],['peak','peak']]) {
    const observed=runs.filter(r=>r.rows.has(name)).map(r=>[r.bytes,r.rows.get(name)[property]]);
    const points=observed.filter(([,value])=>Number.isFinite(value) && value>0);
    const k=fitExponent(points);
    if(k===null) {
      if(metric==='seconds' && observed.length>=2 && new Set(observed.map(([b])=>b)).size>=2 && observed.every(([,v])=>v===0)) {
        console.log(`CONSTANT ${name} seconds=0 n=${observed.length}`);
      } else if(metric==='seconds') invalid.push(`${name}: insufficient positive-duration rung evidence`);
      continue;
    }
    ++fitted;if(metric!=='seconds') ++rssFitted;
    const verdict=k>1.3?'SUPERLINEAR':'LINEAR';if(k>1.3) ++offenders;
    console.log(`${verdict} ${name} ${metric} k=${k.toFixed(6)} n=${points.length}`);
  }
  if(!fitted) invalid.push('fewer than two distinct completed rung sizes: no fitted growth evidence');
  if(!rssFitted) invalid.push('no fitted RSS growth evidence across distinct rung sizes');
  for(const reason of invalid) console.error('UNAVAILABLE '+reason);
  if(offenders) {console.log(`FAIL: ${offenders} superlinear phase/metric fits`);process.exitCode=1;}
  else if(invalid.length) process.exitCode=2;
  else {console.log(`PASS: ${fitted} phase/metric fits <= 1.3`);process.exitCode=0;}
} catch(e) {console.error('UNAVAILABLE: '+e.message);process.exitCode=2;}
