#!/usr/bin/env node
// --freeze is a ONE-TIME authorized offline seed oracle; never a runtime path.
// --native accepts only a freshly composed Jet-only runtime object.
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {resolve,dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const here=join(root,'Compiler/JetRuntime/Tests');
const [mode,outArg,object]=process.argv.slice(2);
if(!['--freeze','--native'].includes(mode)||!outArg||(mode==='--native'&&!object)) throw Error('usage: run-native-layout.mjs --freeze OUT | --native OUT fresh-runtime.o');
const out=resolve(outArg); mkdirSync(out,{recursive:true});
const hash=b=>createHash('sha256').update(b).digest('hex');
const vectors=readFileSync(join(here,'LayoutVectors.txt'),'utf8');
function run(cmd,args,label){
  const r=spawnSync(cmd,args,{cwd:root,encoding:'utf8',maxBuffer:1<<24});
  writeFileSync(join(out,label+'.log'),`exit=${r.status}\n${r.stdout??''}${r.stderr??''}`);
  process.stdout.write(r.stdout??''); process.stderr.write(r.stderr??'');
  console.log(`${label}: tool exit=${r.status}`);
  if(r.error||r.status!==0) throw Error(`${label}: ${r.error?.message??r.status}`);
  return r.stdout;
}
function literals(){
  // Verify every reused draft byte and declared length, including UTF-8 em dash.
  const source=readFileSync(join(root,'Compiler/JetRuntime/Source/NativeLayoutText.jet'),'utf8');
  const buffer=Buffer.alloc(432);
  for(const m of source.matchAll(/layout_put\(root \+ I64\{(\d+)\}, I64\{(\d+)\}\)/gu)) buffer.writeBigInt64LE(BigInt(m[2]),Number(m[1]));
  const expected=['layout constraint expression has more than 16 distinct terms','constraint#',' (<=)',' (>=)',' (==)','suggest(',')','(layout has no feasible solution)','layout "','" has no feasible solution — conflicting constraints: ', ', ', '.'];
  const offsets=[192,256,272,280,288,296,304,312,352,360,416,424];
  for(let i=0;i<expected.length;i++) {
    const length=Number(buffer.readBigInt64LE(i*16+8));
    if(buffer.subarray(offsets[i],offsets[i]+length).toString('utf8')!==expected[i]) throw Error(`Layout literal ${i} bytes/length mismatch`);
  }
  console.log('Layout reused draft literals: all 12 exact');
}
literals();
if(mode==='--freeze') {
  const fixture=join(here,'LayoutSeed.json');
  if(existsSync(fixture)||existsSync(join(out,'oracle-execute.log'))) throw Error('ONE-TIME oracle already captured; do not rerun');
  const seed=readFileSync(join(root,'crates/jet-codegen/src/Prelude/Layout.rs'),'utf8');
  const end=seed.lastIndexOf('}');
  const helper=`
    pub fn oracle_feasible(c: &Constraint) -> bool {
        let mut i=c.handle.borrow_mut(); if i.dirty { solve(&mut i); i.dirty=false; } i.infeasible.is_none()
    }
    pub fn oracle_conflict(c: &Constraint) -> Vec<String> {
        let mut i=c.handle.borrow_mut(); if i.dirty { solve(&mut i); i.dirty=false; } i.infeasible.clone().unwrap_or_default()
    }
`;
  let body='let mut e: Vec<Option<jet_layout::LinExpr>>=vec![None;64]; let mut c: Vec<Option<jet_layout::Constraint>>=vec![None;64]; let mut h: Option<jet_layout::Handle>=None;\n';
  let name='';
  for(const line of vectors.trim().split('\n')) {
    const [op,a,b,d,r]=line.split(' '), q=JSON.stringify;
    const expr=x=>`e[${x}].unwrap()`;
    const ch=x=>`c[${x}].as_ref().unwrap()`;
    const handle='h.as_ref().unwrap()';
    switch(op) {
      case 'case': name=a; body+=`h=Some(jet_layout::Handle::new(${q(a)}));\n`; break;
      case 'var': body+=`e[${a}]=Some(${handle}.h(${q('v'+a)},"width"));\n`; break;
      case 'const': body+=`e[${a}]=Some(jet_layout::LinExpr::from_const(${Number(b).toFixed(9)}));\n`; break;
      case 'add': case 'sub': body+=`e[${a}]=Some(jet_layout::${op}(${expr(b)},${expr(d)}));\n`; break;
      case 'cmp': body+=`c[${a}]=Some(jet_layout::${['le','ge','eq_'][Number(r)]}(${expr(b)},${expr(d)}));\n`; break;
      case 'strength': body+=`c[${a}]=Some(c[${a}].take().unwrap().${['required','strong','medium','weak'][Number(b)]}());\n`; break;
      case 'suggest': body+=`${handle}.suggest(${expr(a)},${Number(b).toFixed(9)});\n`; break;
      case 'value': body+=`println!("v ${name} ${a} {:.9}",${handle}.value(${expr(a)}));\n`; break;
      case 'feasible': body+=`println!("f ${name} {}",${handle}.is_feasible() as u8);\n`; break;
      case 'conflict': body+=`print_conflict(${q(name)},${handle}.conflict());\n`; break;
      case 'constraint_feasible': body+=`println!("f ${name} {}",jet_layout::oracle_feasible(${ch(a)}) as u8);\n`; break;
      case 'constraint_conflict': body+=`print_conflict(${q(name)},jet_layout::oracle_conflict(${ch(a)}));\n`; break;
      case 'end': body+='h=None; for v in e.iter_mut(){*v=None;} for v in c.iter_mut(){*v=None;}\n'; break;
      default: throw Error('unknown vector op '+op);
    }
  }
  const fatal=`
fn panic_text(f: impl FnOnce() + std::panic::UnwindSafe) -> String {
    let p=std::panic::catch_unwind(f).expect_err("seed fatal must panic");
    p.downcast_ref::<String>().cloned().or_else(||p.downcast_ref::<&str>().map(|s|s.to_string())).unwrap()
}
fn fatal_vectors() {
    std::panic::set_hook(Box::new(|_|{}));
    for compare in [false,true] {
        let h=jet_layout::Handle::new("fatal"); let mut sum=h.h("v0","width");
        for i in 1..16 {sum=jet_layout::add(sum,h.h(format!("v{}",i),"width"));}
        let last=h.h("v16","width");
        let message=panic_text(move || {if compare {let _=jet_layout::eq_(sum,last);} else {let _=jet_layout::add(sum,last);}});
        println!("p {}|{}",if compare {"overflow-compare"} else {"overflow-add"},message);
    }
    let message=panic_text(|| {let h=jet_layout::Handle::new("fatal"); let x=h.h("v0","width"); let _=jet_layout::ge(x,jet_layout::LinExpr::from_const(10.0)); let _=jet_layout::le(x,jet_layout::LinExpr::from_const(5.0)); let _=h.value(x);});
    println!("p infeasible-value|{}",message);
}
`;
  const reference=join(out,'reference'); mkdirSync(join(reference,'src'),{recursive:true});
  writeFileSync(join(reference,'Cargo.toml'),'[package]\nname="layout-seed-oracle"\nversion="0.0.0"\nedition="2021"\n[workspace]\n');
  const source=seed.slice(0,end)+helper+seed.slice(end)+fatal+'\nfn print_conflict(name:&str, labels:Vec<String>) {print!("c {} {}",name,labels.len()); for l in labels {print!("|{}",l);} println!();}\nfn main(){\n'+body+'fatal_vectors();}\n';
  writeFileSync(join(reference,'src/main.rs'),source);
  run('cargo',['build','--offline','--release','--jobs','1','--manifest-path',join(reference,'Cargo.toml'),'--target-dir',join(out,'target')],'oracle-build');
  const stdout=run(join(out,'target/release/layout-seed-oracle'),[],'oracle-execute');
  const rustc=run('rustc',['--version'],'oracle-version').trim();
  const data={seed:'crates/jet-codegen/src/Prelude/Layout.rs',seed_sha256:hash(seed),vectors_sha256:hash(vectors),recipe_sha256:hash(readFileSync(fileURLToPath(import.meta.url))),oracle_source_sha256:hash(source),cargo_lock_sha256:hash(readFileSync(join(reference,'Cargo.lock'))),rustc,stdout_sha256:hash(stdout),stdout};
  writeFileSync(fixture,JSON.stringify(data,null,2)+'\n');
  console.log(`Layout seed frozen: ${hash(readFileSync(fixture))}; ${stdout.trim().split('\n').length} observations`);
} else {
  const frozen=JSON.parse(readFileSync(join(here,'LayoutSeed.json'),'utf8'));
  if(hash(vectors)!==frozen.vectors_sha256||hash(frozen.stdout)!==frozen.stdout_sha256) throw Error('frozen oracle/vector checksum mismatch');
  run('cc',['-std=c11','-Wall','-Wextra','-Werror','-pedantic',join(here,'NativeLayoutWitness.c'),resolve(object),'-Wl,--wrap=jet_rt_free','-lm','-pthread','-o',join(out,'native-layout')],'native-link');
  const actual=run(join(out,'native-layout'),[join(here,'LayoutVectors.txt')],'native-execute').trim().split('\n');
  if(actual.pop()!=='Layout6 ownership/registry/clone/drop OK') throw Error('ownership witness did not complete');
  const expected=frozen.stdout.trim().split('\n').filter(s=>!s.startsWith('p '));
  if(expected.length!==actual.length) throw Error('native observation count mismatch');
  for(let i=0;i<expected.length;i++) {
    if(expected[i].startsWith('v ')) {
      const a=actual[i].split(' '),e=expected[i].split(' ');
      if(a.slice(0,3).join(' ')!==e.slice(0,3).join(' ')||!Number.isFinite(Number(a[3]))||Math.abs(Number(a[3])-Number(e[3]))>1e-7) throw Error(`value mismatch ${i}: ${actual[i]} != ${expected[i]}`);
    } else if(actual[i]!==expected[i]) throw Error(`observation mismatch ${i}: ${actual[i]} != ${expected[i]}`);
  }
  for(const row of frozen.stdout.trim().split('\n').filter(s=>s.startsWith('p '))) {
    const [tag,...parts]=row.slice(2).split('|'),message=parts.join('|');
    const r=spawnSync(join(out,'native-layout'),['--fatal',tag],{cwd:root,encoding:'utf8'});
    writeFileSync(join(out,tag+'.log'),`exit=${r.status}\n${r.stdout??''}${r.stderr??''}`);
    if(r.error||r.status!==70||!(r.stderr??'').includes(message)) throw Error(`fatal ${tag} did not preserve seed message/exit70`);
    console.log(`Layout fatal ${tag}: tool exit=70; seed message exact`);
  }
  console.log(`Layout6 native/seed ${expected.length} observations + 3 fatal modes + ownership OK`);
}
