#!/usr/bin/env node
// Conservative structural gate, not a typechecker or proof of asymptotic cost.
import fs from 'node:fs';
import path from 'node:path';

const argv = process.argv.slice(2);
if (!argv.length) { console.error('usage: superlinear-static.mjs <tree> [paths...]'); process.exit(2); }
const root = fs.realpathSync(argv.shift());
const selected = argv.length ? argv : ['Compiler', 'Core'];
const rules = new Set(['nested-scan', 'linear-lookup', 'prefix-concat', 'join-in-loop', 'aggregate-copy', 'copy-push', 'repeated-scan']);
function files(p, result = []) {
  const st = fs.lstatSync(p);
  if (st.isSymbolicLink()) throw new Error(`source symlink requires explicit real tree: ${p}`);
  if (st.isDirectory()) for (const e of fs.readdirSync(p).sort()) files(path.join(p,e),result);
  else if (p.endsWith('.jet')) result.push(p);
  return result;
}
function tokens(source) {
  const out = []; let i = 0, line = 1;
  const lexeme=/(?:[A-Za-z_][A-Za-z_0-9]*|[0-9]+(?:\.[0-9]+)?|\.\.<|\.\.|::|:=|->|==|!=|<=|>=|\+=|&&|\|\||[^\s])/y;
  function advance(end) { for (; i < end; ++i) if (source[i] === '\n') ++line; }
  function quoted(at) {
    const quote = source[at]; let j = at + 1, depth = 0;
    while (j < source.length) {
      if (source[j] === '\\') { j += 2; continue; }
      if (!depth && (source.startsWith('{{',j) || source.startsWith('}}',j))) { j += 2; continue; }
      if (quote === '"' && source[j] === '{') { ++depth; ++j; continue; }
      if (depth && source[j] === '}') { --depth; ++j; continue; }
      if (depth && (source[j] === '"' || source[j] === "'")) { j = quoted(j); continue; }
      if (!depth && source[j] === quote) return j + 1;
      ++j;
    }
    throw new Error(`unterminated string at line ${line}`);
  }
  while (i < source.length) {
    if (/\s/.test(source[i])) { advance(i+1); continue; }
    if (source.startsWith('//',i)) { const end = source.indexOf('\n',i); advance(end < 0 ? source.length : end); continue; }
    if (source.startsWith('/*',i)) {
      let j = i+2, depth = 1;
      while (j < source.length && depth) {
        if (source.startsWith('/*',j)) { ++depth; j+=2; }
        else if (source.startsWith('*/',j)) { --depth; j+=2; } else ++j;
      }
      if (depth) throw new Error(`unterminated comment at line ${line}`);
      advance(j); continue;
    }
    const start = i, n = line;
    if (source[i] === '"' || source[i] === "'") { const end = quoted(i); out.push({v:source.slice(i,end),line:n,start,kind:'string'}); advance(end); continue; }
    lexeme.lastIndex=i; const match=lexeme.exec(source);
    out.push({v:match[0],line:n,start,kind:'code'}); advance(i+match[0].length);
  }
  return out;
}
function callOpener(t,index) {
  let open=index+1;
  if(t[open]?.v==='<') {
    let depth=1; ++open;
    while(open<t.length && depth && !['{','}',';'].includes(t[open].v)) {
      if(t[open].v==='<') ++depth;
      if(t[open].v==='>') --depth;
      ++open;
    }
    if(depth) return null;
  }
  return t[open]?.v==='(' ? open : null;
}
function analyse(file) {
  const source = fs.readFileSync(file,'utf8'); let t;
  try { t = tokens(source); } catch(e) { throw new Error(`${file}: ${e.message}`); }
  const pairs = new Map(), stack=[];
  for (let i=0;i<t.length;++i) {
    if (['{','(','['].includes(t[i].v)) stack.push(i);
    if (['}',')',']'].includes(t[i].v)) {
      const a=stack.pop();
      if (a === undefined || {'{':'}','(':')','[':']'}[t[a].v] !== t[i].v) throw new Error(`${file}:${t[i].line}: unmatched delimiter`);
      pairs.set(a,i);
    }
  }
  if (stack.length) throw new Error(`${file}: unclosed delimiter`);
  const text=(a,b)=>t.slice(a,b).map(x=>x.v).join(' ');
  const functions=[];
  for(let i=0;i<t.length;++i) if(t[i].v==='fn' && /^[A-Za-z_]\w*$/.test(t[i+1]?.v||'')) {
    const open=callOpener(t,i+1); if(open===null) continue;
    const paramsEnd=pairs.get(open); if(paramsEnd===undefined) continue;
    let b=paramsEnd+1; while(b<t.length && t[b].v!=='{' && t[b].v!=='fn' && t[b].v!==';') ++b;
    if(t[b]?.v!== '{') continue;
    functions.push({name:t[i+1].v,a:b,b:pairs.get(b),params:text(open+1,paramsEnd), loops:[], scans:new Set(), types:new Map()});
  }
  for(const f of functions) {
    for(const m of f.params.matchAll(/(?:^|,)\s*(&?\w+)\s*:\s*([^,]+)/g)) f.types.set(m[1].replace(/^&/,''),m[2].trim());
    for(let i=f.a+1;i<f.b;++i) {
      if(['::',':='].includes(t[i].v) && /^[A-Za-z_]\w*$/.test(t[i-1]?.v)) {
        const rhs=t[i+1];
        if(rhs?.kind==='string') f.types.set(t[i-1].v,'String');
        else if(rhs?.v==='[') f.types.set(t[i-1].v,text(i+1,pairs.get(i+1)+1));
        else if(rhs && /^[A-Z]/.test(rhs.v) && t[i+2]?.v==='{') f.types.set(t[i-1].v,rhs.v);
      }
      if(t[i].v!=='loop') continue;
      let b=i+1;
      while(b<f.b && !['{','->'].includes(t[b].v)) {
        if(['(','['].includes(t[b].v)) b=pairs.get(b)+1; else ++b;
      }
      if(b>=f.b) throw new Error(`${file}:${t[i].line}: loop without body`);
      let end;
      if(t[b].v==='{') end=pairs.get(b);
      else { end=b+1; while(end<f.b && t[end].line===t[b].line) ++end; --end; }
      const header=text(i+1,b);
      const ids=new Set([...header.matchAll(/\b[A-Za-z_]\w*(?:\s*\.\s*[A-Za-z_]\w*)*/g)].map(m=>m[0].replace(/\s/g,'')).filter(v=>!['in','len','bytes','true','false'].includes(v)));
      const literal=/\bin\s+\d+\s*\.\.<\s*\d+\s*$/.test(header);
      f.loops.push({a:i,body:b,b:end,header,ids,literal});
    }
    // Loop bindings inherit list element types (struct/string copies need review).
    for(const l of f.loops) {
      const m=/^(\w+)\s+in\s+(\w+)\s*$/.exec(l.header);
      if(m && f.types.get(m[2])?.startsWith('[')) f.types.set(m[1],f.types.get(m[2]).slice(1,-1));
      for(const id of l.ids) {
        const base=id.split('.')[0];
        if(f.types.has(base) && (/\bin\b/.test(l.header) || id.endsWith('.len'))) f.scans.add(base);
      }
    }
  }
  return {file,source,lines:source.split('\n'),t,functions,pairs,text};
}
function aggregate(type) { type=type?.trim(); return type && (type.startsWith('[') || (!/^(?:Int|U\d+|I\d+|Bool|Float|F\d+|Unit|String)\b/.test(type))); }
try {
  const all=[...new Set(selected.flatMap(p=>files(path.resolve(root,p))))];
  if(!all.length) throw new Error('no Jet source files selected');
  const units=all.map(analyse), byName=new Map();
  function argumentsOf(u,open) {
    const args=[], end=u.pairs.get(open); let start=open+1, i=start;
    while(i<end) {
      if(['(','[','{'].includes(u.t[i].v)) {i=u.pairs.get(i)+1;continue;}
      if(u.t[i].v===',') {args.push(u.text(start,i).trim());start=i+1;}
      ++i;
    }
    if(start<end) args.push(u.text(start,end).trim());
    return args;
  }
  const fields=new Map();
  for(const u of units) for(let i=0;i<u.t.length;++i) if(u.t[i].v==='struct' && u.t[i+2]?.v==='{') {
    const end=u.pairs.get(i+2), name=u.t[i+1].v;
    for(let j=i+3;j<end;++j) {
      if(u.t[j+1]?.v!==':') continue;
      let k=j+2;
      while(k<end && u.t[k].line===u.t[j].line && !['{','pub','}'].includes(u.t[k].v) && !(u.t[k+1]?.v===':' && k>j+2)) {
        if(['[','('].includes(u.t[k].v)) k=u.pairs.get(k)+1; else ++k;
      }
      const key=`${name}.${u.t[j].v}`, type=u.text(j+2,k).trim();
      if(!fields.has(key)) fields.set(key,type);
      else if(fields.get(key)!==type) fields.set(key,null);
      j=k-1;
    }
  }
  function expressionType(u,f,start) {
    let type=f.types.get(u.t[start]?.v), end=start+1;
    while(type && end<u.t.length) {
      if(u.t[end].v==='.' && u.t[end+2]?.v!=='(') {
        type=fields.get(`${type.trim().replace(/\s*\?$/,'')}.${u.t[end+1]?.v}`); end+=2;
      } else if(u.t[end].v==='[' && type.trim().startsWith('[')) {
        type=type.trim().slice(1,-1).trim(); end=u.pairs.get(end)+1;
        if(type.includes(':')) type=type.slice(type.lastIndexOf(':')+1).trim();
      } else break;
    }
    return u.t[end]?.v==='(' ? null : {type,end};
  }
  for(const u of units) for(const f of u.functions) for(let i=f.a+1;i<f.b;++i) {
    if(['::',':='].includes(u.t[i].v)) {
      const expression=expressionType(u,f,i+1);
      if(expression?.type) f.types.set(u.t[i-1].v,expression.type);
    }
  }
  for(const u of units) for(const f of u.functions) {
    if(!byName.has(f.name)) byName.set(f.name,[]);
    byName.get(f.name).push(f);
  }
  // Propagate parameter scans through helpers to a fixed point. Only exact
  // identifiers propagate; field/alias uncertainty is separately surfaced.
  let changed=true;
  while(changed) {
    changed=false;
    for(const u of units) for(const f of u.functions) for(let i=f.a+1;i<f.b;++i) {
      const callees=byName.get(u.t[i].v), open=callOpener(u.t,i);
      if(!callees || open===null || u.t[i-1]?.v==='.') continue;
      const args=argumentsOf(u,open).map(s=>s.replace(/^[&~]\s*/,''));
      for(const c of callees) {
        const params=[...c.params.matchAll(/(?:^|,)\s*&?(\w+)\s*:/g)].map(m=>m[1]);
        for(let n=0;n<params.length;++n) if(c.scans.has(params[n]) && f.types.has(args[n]) && !f.scans.has(args[n])) {f.scans.add(args[n]);changed=true;}
      }
    }
  }
  const hits=[],seen=new Set();
  function hit(u,f,i,rule,detail) {
    const rel=path.relative(root,u.file), key=`${rel}:${u.t[i].line}:${rule}`;
    if(seen.has(key)) return; seen.add(key);
    hits.push({key,file:rel,line:u.t[i].line,rule,function:f.name,detail,source:u.lines[u.t[i].line-1].trim(),hot:0});
  }
  for(const u of units) for(const f of u.functions) {
    for(const l of f.loops) {
      const parent=f.loops.find(p=>p.a<l.a && p.b>=l.b);
      if(parent && !l.literal) {
        const shared=[...l.ids].filter(id=>parent.ids.has(id));
        hit(u,f,l.a,'nested-scan',shared.length ? `nested loops share bound/collection ${shared.join(', ')}` : `nested variable-sized traversal (${parent.header}) / (${l.header}); disjoint partitions require justification`);
      }
      for(let i=l.body+1;i<=l.b;++i) {
        const tok=u.t[i], prev=u.t[i-1]?.v, next=u.t[i+1]?.v;
        if(['find','contains','index_of','remove','position','count','filter','fold','sum','sort','unique','dedup'].includes(tok.v) && next==='(') hit(u,f,i,'linear-lookup',`${tok.v} within loop; establish receiver bound/index or remove repeated scan`);
        if(tok.v==='join' && next==='(') hit(u,f,i,'join-in-loop','whole list join inside loop');
        if(tok.kind==='string' && /\.(?:join|contains|find|index_of)\s*\(/.test(tok.v)) hit(u,f,i,'repeated-scan','scan/join in string interpolation inside loop');
        if(tok.v==='=' && /^[A-Za-z_]\w*$/.test(prev||'')) {
          const rhs=u.t[i+1];
          if(rhs?.kind==='string' && rhs.v.includes(`{${prev}`)) hit(u,f,i,'prefix-concat',`interpolation copies growing ${prev} prefix`);
          let end=i+1; while(end<=l.b && u.t[end].line===tok.line) ++end;
          const body=u.text(i+1,end);
          if(f.types.get(prev)?.match(/^(String|\[)/) && new RegExp(`\\b${prev}\\b`).test(body) && body.includes('+')) hit(u,f,i,'prefix-concat',`+ concatenation of ${prev} inside loop`);
          if(rhs && aggregate(f.types.get(rhs.v)) && !['~','&'].includes(rhs.v) && !['[','.'].includes(u.t[i+2]?.v)) hit(u,f,i,'aggregate-copy',`by-value assignment of aggregate ${rhs.v}`);
        }
        if(tok.v==='+=' && f.types.get(prev)?.match(/^(String|\[)/)) hit(u,f,i,'prefix-concat',`+= of ${prev} inside loop`);
        if(tok.v==='+' && (u.t[i-1]?.kind==='string' || u.t[i+1]?.kind==='string' || f.types.get(prev)?.trim().match(/^(String|\[)/) || f.types.get(next)?.trim().match(/^(String|\[)/))) hit(u,f,i,'prefix-concat','String/list + inside loop; establish bounded/disjoint operands or use one final join');
        if(['clone','copy','to_list','to_vec','to_owned','collect'].includes(tok.v) && next==='(') hit(u,f,i,'aggregate-copy',`${tok.v} materializes a collection each iteration`);
        if(['=','::',':=','~'].includes(tok.v)) {
          const expression=expressionType(u,f,i+1);
          if(expression && aggregate(expression.type)) hit(u,f,i,'aggregate-copy',`by-value aggregate expression ${u.text(i+1,expression.end)} inside loop (${expression.type})`);
        }
        if(tok.v==='~' && (aggregate(f.types.get(next)) || f.types.get(next)?.trim()==='String') && !['[','.'].includes(u.t[i+2]?.v)) hit(u,f,i,'aggregate-copy',`explicit Jet copy of whole ${next} each iteration`);
        if(['::',':='].includes(tok.v) && aggregate(f.types.get(u.t[i+1]?.v)) && !['~','&'].includes(u.t[i+1]?.v) && !['[','.'].includes(u.t[i+2]?.v)) hit(u,f,i,'aggregate-copy',`by-value binding of aggregate ${u.t[i+1].v}`);
        if(tok.v==='push' && next==='(') {
          let argIndex=i+2;
          if(u.t[argIndex]?.v==='~') ++argIndex;
          const arg=u.t[argIndex];
          if(arg?.kind==='string' && /\{/.test(arg.v)) hit(u,f,i,'copy-push','push of interpolated copy; establish non-growing/disjoint input');
          if(arg && aggregate(f.types.get(arg.v)) && !['[','.'].includes(u.t[argIndex+1]?.v)) hit(u,f,i,'copy-push',`push copies aggregate ${arg.v}`);
        }
        const callees=byName.get(tok.v);
        const open=callOpener(u.t,i);
        if(callees && open!==null && prev!=='.') {
          const args=argumentsOf(u,open);
          for(const c of callees) {
            const params=[...c.params.matchAll(/(?:^|,)\s*(&?\w+)\s*:\s*([^,]+)/g)];
            for(let n=0;n<params.length;++n) {
              const arg=args[n], param=params[n][1].replace(/^&/,''); if(!arg) continue;
              if(c.scans.has(param)) hit(u,f,i,'repeated-scan',`${tok.v} scans ${param} for every loop iteration (argument ${arg}); disjoint child inputs require justification`);
              if(aggregate(params[n][2].trim()) && /^[A-Za-z_]\w*(?:\s*\.\s*\w+)*$/.test(arg) && !params[n][1].startsWith('&')) hit(u,f,i,'aggregate-copy',`${tok.v} receives aggregate ${arg} by value; emitted borrowing/move must be established`);
            }
          }
        }
      }
    }
  }
  const allowPath=process.env.STATIC_ALLOWLIST || path.join(root,'.superlinear-allowlist.json');
  const allow=new Map();
  if(fs.existsSync(allowPath)) {
    const entries=JSON.parse(fs.readFileSync(allowPath,'utf8'));
    if(!Array.isArray(entries)) throw new Error('allowlist must be an array');
    for(const e of entries) {
      if(typeof e.key!=='string' || typeof e.justification!=='string' || !e.justification.trim()) throw new Error('every allowlist entry requires key and non-empty justification');
      const rule=e.key.split(':').at(-1);
      if(!rules.has(rule) || allow.has(e.key)) throw new Error(`invalid/duplicate allowlist key ${e.key}`);
      allow.set(e.key,e.justification.trim());
    }
    for(const key of allow.keys()) if(!seen.has(key)) throw new Error(`stale allowlist entry ${key}`);
  }
  const profile=process.env.STATIC_PROFILE;
  if(profile) {
    const p=JSON.parse(fs.readFileSync(profile,'utf8'));
    const scores=new Map();
    for(const [phase,c] of Object.entries(p.phases)) for(const [name,count] of c.jet_inclusive||[]) {
      const decoded=name.replaceAll('_u','_');
      const old=scores.get(decoded); if(!old || count>old.count) scores.set(decoded,{count,phase,share:100*count/c.samples});
    }
    for(const h of hits) {const score=scores.get(h.function);if(score) {h.hot=score.count;h.profile=`${score.phase}: ${score.share.toFixed(2)}% inclusive (${score.count} samples)`;}}
  }
  hits.sort((a,b)=>b.hot-a.hot || a.file.localeCompare(b.file) || a.line-b.line || a.rule.localeCompare(b.rule));
  for(const h of hits) {h.justification=allow.get(h.key);console.log(`${h.justification?'ALLOW':'FAIL'} ${h.key} ${h.function} ${h.profile||'unprofiled'} — ${h.detail}${h.justification ? ' — '+h.justification : ''}`);}
  if(process.env.STATIC_REPORT) {
    const rows=hits.map((h,i)=>`| ${i+1} | \`${h.key}\` | \`${h.function}\` | ${h.profile||'unprofiled'} | ${h.justification?'ALLOW: '+h.justification:'FAIL'} | ${h.detail.replaceAll('|','\\|')} |`);
    fs.writeFileSync(process.env.STATIC_REPORT,`# Static superlinearity gate\n\nTree: \`${root}\`. Profile: \`${profile||'none'}\`. Generated ${new Date().toISOString()}.\n\nConservative review-required structural findings, not independently proven asymptotic bugs. Profile shares are phase-inclusive; unprofiled does not mean cold. Failed compilation profiles are partial, not throughput acceptance. Rules include variable-bound nested scans (partitioned/bounded loops require justification), linear searches, growing prefix construction, joins, explicit/implicit aggregate copies and transitively repeated helper scans. Types are syntactically inferred: emitted borrow/move and collection aliasing require human review. This checker cannot prove the absence of superlinearity in generated/native code; dynamic growth/profile gates remain mandatory.\n\n| Rank | Location / rule | Function | Latest L5 hotness | Verdict | Evidence |\n|---|---|---|---|---|---|\n${rows.join('\n')}\n\n${hits.length} hits, ${hits.filter(h=>!h.justification).length} unallowlisted.\n`);
  }
  console.log(`static gate: ${all.length} files, ${hits.length} hits, ${hits.filter(h=>!h.justification).length} unallowlisted`);
  process.exitCode=hits.some(h=>!h.justification)?1:0;
} catch(e) {console.error(`UNAVAILABLE: ${e.message}`);process.exitCode=2;}
