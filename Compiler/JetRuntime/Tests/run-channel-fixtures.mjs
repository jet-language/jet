#!/usr/bin/env node
// L8a-4 actual-runtime slot witnesses and paired 1M-message benchmark.
// Each mode is independently reportable by the one held keep-going exit job.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0, frontReceipt } from '../../JetBackend/Tests/jet-front.mjs';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode, source] = process.argv.slice(2);
const modes = ['assemble', 'lower', 'prepare-native', 'prepare-seed', 'link', 'link-seed', 'run', 'run-seed', 'cross', 'perf'];
if (!output || !modes.includes(mode)) throw Error(`usage: run-channel-fixtures.mjs OUT ${modes.join('|')} [FRESH_RUNTIME_RS]`);
const out = resolve(output), fixture = join(out, 'fixture');
const read = p => readFileSync(join(repo, p), 'utf8');
function command(bin, args, options = {}) {
  const result = spawnSync(bin, args, {encoding:'utf8',maxBuffer:1<<27,...options});
  if (result.error || result.status !== 0) throw Error(`${bin}: ${result.status ?? result.signal}\n${result.error?.message ?? ''}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  return result;
}
// This seed-only ABI bridge invokes the actual Prelude Closed enum and traits;
// it never reimplements a runtime type. Both directions of the carrier crossing
// use the same linked Jet E4 text allocator. It dies with Rust verification.
const bridge = `
#[no_mangle] pub extern "C" fn jet_fixture_rust_closed_new() -> usize {
    Box::into_raw(Box::new(jet_std::Closed::Closed)) as usize
}
#[no_mangle] pub unsafe extern "C" fn jet_fixture_rust_closed_drop(value: usize) {
    if value != 0 { drop(Box::from_raw(value as *mut jet_std::Closed)); }
}
#[no_mangle] pub unsafe extern "C" fn jet_fixture_rust_closed_clone(value: usize) -> usize {
    Box::into_raw(Box::new((&*(value as *mut jet_std::Closed)).clone())) as usize
}
#[no_mangle] pub unsafe extern "C" fn jet_fixture_rust_closed_show(value: usize) -> usize {
    jet_c_abi::handle(JetShow::jet_show(&*(value as *mut jet_std::Closed))) as usize
}
#[no_mangle] pub unsafe extern "C" fn jet_fixture_rust_closed_eq(a: usize, b: usize) -> bool {
    &*(a as *mut jet_std::Closed) == &*(b as *mut jet_std::Closed)
}
`;
if (mode === 'assemble') {
  mkdirSync(join(fixture, 'Source'), {recursive:true});
  const paths = read('Compiler/JetRuntime/sources.list').split('\n').map(s=>s.trim()).filter(s=>s&&!s.startsWith('#'));
  paths.push('Compiler/JetRuntime/Tests/ChannelTests.jet');
  const unit = paths.map(read).join('\n');
  const names = [...unit.matchAll(/^#Export\(c\)\s+fn\s+(\w+)\(/gm)].map(m=>m[1]);
  if (new Set(names).size !== names.length) throw Error('duplicate fixture exports');
  const init = new Map([...unit.matchAll(/^\/\/ runtime-init: (\d+)\n#Export\(c\) fn (\w+)/gm)].map(m=>[m[2],m[1]]));
  init.set('jet_rt_alloc_init','0');
  writeFileSync(join(fixture,'Source/Fixture.jet'),unit);
  writeFileSync(join(fixture,'exports.list'),names.sort().map(n=>n==='jet_rt_panic_list_bounds'?`${n} int-index-seam`:init.has(n)?`${n} init ${init.get(n)}`:n).join('\n')+'\n');
  writeFileSync(join(fixture,'package.jet'),read('Compiler/JetRuntime/package.jet'));
} else if (mode === 'lower') {
  const prefix=join(fixture,'fixture'),receipt=join(fixture,'front.receipt');
  const result=spawnSync(candidateJetc0(),[],{cwd:repo,encoding:'utf8',maxBuffer:1<<27,timeout:Number(process.env.JET_FRONT_TIMEOUT_MS??600000),env:{...process.env,JET_BOOTSTRAP_MODE:'factory',JET_BOOTSTRAP_FACTORY_TIER:'aot',JET_BOOTSTRAP_SOURCE_ROOT:fixture,JET_BOOTSTRAP_ENTRY:'Source/Fixture.jet',JET_BOOTSTRAP_OUTPUT:join(fixture,'unused.output'),JET_BOOTSTRAP_RECEIPT:receipt,JET_FRONT_LIBRARY_OBJECT_OUTPUT:prefix,JET_FRONT_LIBRARY_EXPORTS:join(fixture,'exports.list'),JET_STORE_DIR:join(out,'store'),RUST_MIN_STACK:'8388608'}});
  writeFileSync(join(fixture,'front.log'),`${result.stdout??''}${result.stderr??''}`);
  const problem=frontReceipt(existsSync(receipt)?readFileSync(receipt,'utf8'):'',result);
  if(problem)throw Error(problem.detail);
  if(existsSync(prefix+'.issues'))throw Error(readFileSync(prefix+'.issues','utf8'));
  if(!existsSync(prefix+'.o'))throw Error('candidate lacks runtime-object hook');
} else if (mode === 'prepare-native' || mode === 'prepare-seed') {
  if (!source || !existsSync(source)) throw Error('preparation requires fresh all-parts runtime source from this composed tree');
  const destination=join(out,mode==='prepare-native'?'native.rs':'seed.rs');
  const prepared=join(out,mode==='prepare-native'?'prepared-native.rs':'prepared-seed.rs');
  command('node',[join(repo,'Compiler/JetBackend/Tests/prepare-call-runtime.mjs'),resolve(source),prepared,'--allparts'],{cwd:repo});
  writeFileSync(destination,readFileSync(prepared,'utf8')+bridge);
} else if (mode === 'link' || mode === 'link-seed') {
  const seed=mode==='link-seed';
  const lib=seed?process.env.JET_CHANNEL_SEED_C_LIB:process.env.JET_CHANNEL_NATIVE_C_LIB;
  if(!lib||!existsSync(lib))throw Error('missing freshly compiled fixture library');
  const flags=seed?['-DJET_SEED','-Djet_rt_handle_clone_Closed=jet_fixture_rust_closed_clone','-Djet_rt_handle_drop_Closed=jet_fixture_rust_closed_drop','-Djet_rt_handle_eq_Closed=jet_fixture_rust_closed_eq','-Djet_rt_handle_show_Closed=jet_fixture_rust_closed_show']:['-DJET_CHANNEL_INTERNAL','-DJET_CLOSED_CROSS'];
  const objects=seed?[]:[join(fixture,'fixture.o')];
  command(process.env.CC??'cc',['-O2',...flags,join(repo,'Compiler/JetRuntime/Tests/channel-fixture.c'),...objects,lib,'-lpthread','-ldl','-lm','-o',join(out,seed?'channel-seed':'channel-native')]);
} else if (mode === 'perf') {
  const rows=[];
  // Alternate order to avoid comparing a cold program with a warm competitor.
  for(let round=0;round<5;round++)for(const tier of round%2?['seed','native']:['native','seed']){
    const result=command(join(out,`channel-${tier}`),['perf'],{timeout:120000});
    writeFileSync(join(out,`perf-${round}-${tier}.txt`),result.stdout+result.stderr);
    const m=/^ping rounds=1000000 checksum=500000500000 ns=(\d+)\n$/.exec(result.stdout);
    if(!m||result.stderr!=='')throw Error(`invalid ${tier} performance result`);
    rows.push({round,tier,ns:Number(m[1])});
  }
  const median=t=>rows.filter(r=>r.tier===t).map(r=>r.ns).sort((a,b)=>a-b)[2];
  const jet=median('native'),rust=median('seed');
  writeFileSync(join(out,'perf.tsv'),'round\ttier\tns\n'+rows.map(r=>`${r.round}\t${r.tier}\t${r.ns}`).join('\n')+'\n');
  writeFileSync(join(out,'perf-summary.json'),JSON.stringify({messages:1000000,jet_median_ns:jet,rust_median_ns:rust,ratio:jet/rust,strict_win:jet/rust<1/1.05},null,2)+'\n');
  console.log(`1M-message ping-pong Jet/Rust=${jet/rust}`);
} else {
  const seed=mode==='run-seed',binary=join(out,seed?'channel-seed':'channel-native');
  const result=command(binary,[],{timeout:120000});
  writeFileSync(join(out,`${mode}.txt`),result.stdout+result.stderr);
  if(result.stderr!=='')throw Error('fixture wrote stderr');
  for(const text of ['queue/close/owners ok','select source-order/probe/timers ok','shared concurrent edits/drop ok'])if(!result.stdout.includes(text+'\n'))throw Error(`missing ${text}`);
  if(mode==='cross'&&!result.stdout.includes('Closed Rust/Jet ownership cross ok\n'))throw Error('cross ownership fixture did not execute');
}
console.log(`channel fixtures ${mode}: ok`);
