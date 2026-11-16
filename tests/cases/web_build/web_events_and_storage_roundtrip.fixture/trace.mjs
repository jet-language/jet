// Public traceMap projection equivalent to BrowserTrace.rs400-510; original unwrap success is retained.
import assert from 'node:assert/strict';import{readFileSync}from'node:fs';import{createHash}from'node:crypto';
const unhex=s=>{assert.equal(s.length%2,0);assert.match(s,/^(?:[0-9a-fA-F]{2})*$/);return new TextDecoder('utf-8',{fatal:true}).decode(Buffer.from(s,'hex'));};
const manifest=readFileSync('web.manifest.json','utf8'),m=manifest.match(/"traceMap": "([^"]*)"/);assert.ok(m);const sources=[],symbols=[];
for(const line of unhex(m[1]).split('\n')){const row=line.split('\t');if(row[0]==='source'){assert.equal(row.length,3);assert.match(row[2],/^[0-9a-f]{64}$/);sources.push({path:unhex(row[1]),sha256:row[2],symbols:[]});}else{assert.equal(row[0],'symbol');assert.equal(row.length,4);const name=unhex(row[2]);assert.ok(name.length);assert.ok(['fn','handler'].includes(row[3]));symbols.push([unhex(row[1]),name,row[3]]);}}
assert.ok(sources.length);for(const[path,name,kind]of symbols){assert.ok(!sources.some(s=>s.symbols.some(([n])=>n===name)));const s=sources.find(s=>s.path===path);assert.ok(s);s.symbols.push([name,kind]);}
const optional=manifest.match(/"sourceMap": "([^"]*)"/);if(optional)unhex(optional[1]);
const source=sources.find(s=>s.path==="tests/fixtures/web_api.jet");assert.ok(source);assert.equal(source.sha256,createHash('sha256').update(readFileSync('../../tests/fixtures/web_api.jet')).digest('hex'));for(const want of [['init','fn'],['init$handler0','handler']])assert.ok(source.symbols.some(s=>s[0]===want[0]&&s[1]===want[1]));
