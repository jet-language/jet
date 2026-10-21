#!/usr/bin/env node
// L8a-4 actual-runtime slot witnesses and paired 1M-message benchmark.
// Each mode is independently reportable by the one held keep-going exit job.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0 } from '../../JetBackend/Tests/jet-front.mjs';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
import { parseRuntimeExports, runtimeLifecycleList } from '../../../Tools/agent/runtime-route-abi.mjs';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode, source] = process.argv.slice(2);
const parkedModes = ['prepare-native', 'prepare-seed', 'link-seed', 'run-seed', 'cross', 'fatal-reference', 'perf-reference'];
const modes = ['assemble', 'assemble-stm', 'assemble-sync', 'assemble-observe', 'assemble-publication', 'lower', 'link', 'link-stm', 'run-stm', 'link-sync', 'run-sync', 'link-observe', 'run-observe', 'link-publication', 'run-publication', 'link-weak', 'run-weak', 'run', 'fatal', 'perf'];
if (parkedModes.includes(mode)) {
  console.log(`${mode}: parked: Rust reference retired (owner #38/#59); Tower #4923`);
  process.exit(0);
}
if (!output || !modes.includes(mode)) throw Error(`usage: run-channel-fixtures.mjs OUT ${modes.join('|')}`);
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
#[no_mangle] pub extern "C" fn jet_fixture_rust_select_wake_handoff() -> bool {
    let early = JetSchedulerChannel::<u64>::new();
    let late = JetSchedulerChannel::<u64>::new();
    let early_sender = early.sender();
    let late_sender = late.sender();
    let plain = ParkSlot::new();
    let master = ParkSlot::new();
    late.inner.register_select_waiter(plain.clone());
    let inners = vec![early.inner.clone(), late.inner.clone()];
    let registration = SelectRegistration::new(&inners, master, None);
    assert!(early_sender.send(10));
    assert!(late_sender.send(20));
    assert!(matches!(jet_scheduler_try_select(inners, vec![]),
                    Some(JetSelectOutcome::Recv { arm: 0, value: 10 })));
    drop(registration);
    let queued = late.inner.state.lock().unwrap().queue.len();
    queued == 1 && plain.notified.load(std::sync::atomic::Ordering::Acquire)
}
`;
if (mode === 'assemble' || mode.startsWith('assemble-')) {
  mkdirSync(join(fixture, 'Source'), {recursive:true});
  const paths = read('Compiler/JetRuntime/sources.list').split('\n').map(s=>s.trim()).filter(s=>s&&!s.startsWith('#'));
  paths.push('Compiler/JetRuntime/Tests/Fragments.fixture/ChannelTests.jet');
  function include(path) { if (!paths.includes(path)) paths.push(path); }
  if (mode === 'assemble-publication') include('Compiler/JetRuntime/Tests/Fragments.fixture/SharedPublicationTests.jet');
  if (mode !== 'assemble' && mode !== 'assemble-publication') {
    include('Compiler/JetRuntime/Source/SharedTransactions.jet');
    include('Compiler/JetRuntime/Tests/Fragments.fixture/StmTests.jet');
  }
  if (mode === 'assemble-sync' || mode === 'assemble-observe') {
    include('Compiler/JetRuntime/Source/Condition.jet');
    include('Compiler/JetRuntime/Tests/Fragments.fixture/SyncTests.jet');
  }
  if (mode === 'assemble-observe') {
    include('Compiler/JetRuntime/Source/ObserveMessages.jet');
    include('Compiler/JetRuntime/Source/Observe.jet');
    include('Compiler/JetRuntime/Source/ObserveLive.jet');
    include('Compiler/JetRuntime/Tests/Fragments.fixture/ObserveTests.jet');
  }
  const unit = paths.map(read).join('\n');
  const names = [...unit.matchAll(/^#Export\(c\)\s+fn\s+(\w+)\(/gm)].map(m=>m[1]);
  if (new Set(names).size !== names.length) throw Error('duplicate fixture exports');
  writeFileSync(join(fixture,'Source/Fixture.jet'),unit);
  writeFileSync(join(fixture,'lifecycle.list'),runtimeLifecycleList(parseRuntimeExports(unit,'Source/Fixture.jet')));
  writeFileSync(join(fixture,'package.jet'),read('Compiler/JetRuntime/package.jet'));
} else if (mode === 'lower') {
  const prefix=join(fixture,'fixture');
  const {problem}=runNativeLibrary({jet:candidateJetc0(),file:join(fixture,'Source/Fixture.jet'),prefix,lifecycle:join(fixture,'lifecycle.list'),env:{...process.env,JET_TOOLCHAIN_ROOT:repo,JET_STORE_DIR:join(out,'store'),RUST_MIN_STACK:'8388608'}});
  if(problem)throw Error(problem.detail);
} else if (mode === 'prepare-native' || mode === 'prepare-seed') {
  if (!source || !existsSync(source)) throw Error('preparation requires fresh all-parts runtime source from this composed tree');
  const destination=join(out,mode==='prepare-native'?'native.rs':'seed.rs');
  const prepared=join(out,mode==='prepare-native'?'prepared-native.rs':'prepared-seed.rs');
  command('node',[join(repo,'Compiler/JetBackend/Tests/prepare-call-runtime.mjs'),resolve(source),prepared,'--allparts'],{cwd:repo});
  writeFileSync(destination,readFileSync(prepared,'utf8')+bridge);
} else if (mode === 'link' || mode === 'link-seed') {
  command(nativeCCompiler,['-O2','-DJET_CHANNEL_INTERNAL',join(repo,'Compiler/JetRuntime/Tests/channel-fixture.c'),join(fixture,'fixture.o'),'-lpthread','-ldl','-lm','-o',join(out,'channel-native')]);
} else if (mode === 'link-publication') {
  command(nativeCCompiler, ['-std=c11', '-Wall', '-Wextra', '-Werror', join(repo, 'Compiler/JetRuntime/Tests/shared-publication-fixture.c'), join(fixture, 'fixture.o'), '-lpthread', '-ldl', '-lm', '-o', join(out, 'shared-publication-native')]);
} else if (mode === 'run-publication') {
  const result = command(join(out, 'shared-publication-native'), [], {timeout:120000});
  writeFileSync(join(out, `${mode}.txt`), result.stdout + result.stderr);
  const expected = 'Shared World race/loser once/register once/reset owner only ok\nShared Process race/loser once/no World registration ok\n';
  if (result.stdout !== expected || result.stderr !== '') throw Error('invalid Shared publication fixture result');
} else if (mode === 'link-weak') {
  command(nativeCCompiler, ['-std=c11', '-Wall', '-Wextra', '-Werror', join(repo, 'Compiler/JetRuntime/Tests/shared-weak-fixture.c'), join(fixture, 'fixture.o'), '-lpthread', '-ldl', '-lm', '-o', join(out, 'shared-weak-native')]);
} else if (mode === 'run-weak') {
  const result = command(join(out, 'shared-weak-native'), [], {timeout:120000});
  writeFileSync(join(out, `${mode}.txt`), result.stdout + result.stderr);
  const expected = 'Shared weak no resurrection/payload once/last weak frees Cell96 ok\nShared weak upgrade versus final strong drop 256 races ok\nShared physical clean/dirty/abort/resume/alias once/logical versus pin lifetime ok\n';
  if (result.stdout !== expected || result.stderr !== '') throw Error('invalid Shared weak fixture result');
} else if (['link-stm', 'link-sync', 'link-observe'].includes(mode)) {
  const kind = mode.slice(5);
  const flags = kind === 'stm' ? [] : ['-DJET_CONDITION_FIXTURE', ...(kind === 'observe' ? ['-DJET_OBSERVE_FIXTURE'] : [])];
  command(nativeCCompiler, ['-std=c11', '-Wall', '-Wextra', '-Werror', ...flags, join(repo, 'Compiler/JetRuntime/Tests/sync-fixture.c'), join(fixture, 'fixture.o'), '-lpthread', '-ldl', '-lm', '-o', join(out, `${kind}-native`)]);
} else if (['run-stm', 'run-sync', 'run-observe'].includes(mode)) {
  const kind = mode.slice(4);
  const result = command(join(out, `${kind}-native`), [], {timeout:120000});
  writeFileSync(join(out, `${mode}.txt`), result.stdout + result.stderr);
  const expected = [
    'STM nested commit transfer/rollback/clone parity ok',
    'STM two-thread body gate serialization ok',
    'STM prepare rejection/no publication/drop/revision stop ok',
    'STM dropped uncommitted root releases second-thread gate ok',
    ...(kind !== 'stm' ? ['Condition FIFO/drain/pending/wait/cancel/unheld access+drop ok'] : []),
    ...(kind === 'observe' ? ['Observe empty/parked exact report/owned once ok'] : []),
  ].join('\n') + '\n';
  if (result.stdout !== expected || result.stderr !== '') throw Error(`invalid ${kind} fixture result`);
} else if (mode === 'fatal') {
  const failures = [];
  for (const name of ['select-closed', 'select-empty']) {
    const results = {};
    for (const tier of ['native']) {
      const result = spawnSync(join(out, `channel-${tier}`), [name], {encoding:'utf8',timeout:120000,maxBuffer:1<<27});
      results[tier] = result;
      writeFileSync(join(out, `${name}-${tier}.txt`), result.stdout ?? '');
      writeFileSync(join(out, `${name}-${tier}.stderr.txt`), result.stderr ?? '');
      writeFileSync(join(out, `${name}-${tier}.status.json`), JSON.stringify({status:result.status,signal:result.signal,error:result.error?.message})+'\n');
      if (result.error || result.status !== 1 || result.stdout !== '') failures.push(`${name}/${tier}: invalid fatal exit`);
    }
    console.log('fatal-reference: parked: Rust reference retired (owner #38/#59); Tower #4923');
  }
  if (failures.length) throw Error(failures.join('\n'));
} else if (mode === 'perf') {
  const rows=[];
  // Keep all five native benchmark rounds; the Rust comparison is frozen.
  for(let round=0;round<5;round++)for(const tier of ['native']){
    const result=command(join(out,`channel-${tier}`),['perf'],{timeout:120000});
    writeFileSync(join(out,`perf-${round}-${tier}.txt`),result.stdout+result.stderr);
    const m=/^ping rounds=1000000 checksum=500000500000 ns=(\d+)\n$/.exec(result.stdout);
    if(!m||result.stderr!=='')throw Error(`invalid ${tier} performance result`);
    rows.push({round,tier,ns:Number(m[1])});
  }
  const median=t=>rows.filter(r=>r.tier===t).map(r=>r.ns).sort((a,b)=>a-b)[2];
  const jet=median('native');
  writeFileSync(join(out,'perf.tsv'),'round\ttier\tns\n'+rows.map(r=>`${r.round}\t${r.tier}\t${r.ns}`).join('\n')+'\n');
  writeFileSync(join(out,'perf-summary.json'),JSON.stringify({messages:1000000,jet_median_ns:jet,provider:'Jet-only',rust_reference:'parked; Tower #4923'},null,2)+'\n');
  console.log('perf-reference: parked: Rust reference retired (owner #38/#59); Tower #4923');
} else {
  const seed=mode==='run-seed',binary=join(out,seed?'channel-seed':'channel-native');
  const result=command(binary,[],{timeout:120000});
  writeFileSync(join(out,`${mode}.txt`),result.stdout+result.stderr);
  if(result.stderr!=='')throw Error('fixture wrote stderr');
  for(const text of ['queue/close/owners ok','select source-order/probe/timers ok','shared concurrent edits/drop ok','select wake handoff parity ok'])if(!result.stdout.includes(text+'\n'))throw Error(`missing ${text}`);
  if(!seed)for(const text of ['reservations/stopped output/permit cleanup ok','shared concurrent readers/writer priority ok','shared guards concurrent readers/writer exclusion/priority ok','shared guard retain/drop/read-only commit/overflow stop ok'])if(!result.stdout.includes(text+'\n'))throw Error(`missing ${text}`);
  if(mode==='cross'&&!result.stdout.includes('Closed Rust/Jet ownership cross ok\n'))throw Error('cross ownership fixture did not execute');
}
console.log(`channel fixtures ${mode}: ok`);
