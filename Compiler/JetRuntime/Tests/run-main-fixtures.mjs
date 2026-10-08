#!/usr/bin/env node
// Link/run only against an explicitly supplied, freshly admitted native pack.
// No Rust entry wrapper or replacement observer is accepted for these witnesses.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, openSync, closeSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode, pack] = process.argv.slice(2);
if (!output || !['link', 'run'].includes(mode) || !pack || !existsSync(pack)) throw Error('usage: run-main-fixtures.mjs OUT link|run FRESH_NATIVE_PACK');
const out = resolve(output), executable = join(out, 'main-fixture');
mkdirSync(out, {recursive:true});
function checked(binary, args) {
  const run = spawnSync(binary, args, {cwd:root,encoding:'utf8',timeout:30000,maxBuffer:1<<26});
  if (run.error || run.signal || run.status !== 0) throw Error(`${binary}: rc=${run.status}, signal=${run.signal}\n${run.error?.message ?? ''}${run.stdout ?? ''}${run.stderr ?? ''}`);
  return run;
}
if (mode === 'link') {
  const nm = checked('nm', ['-g','--defined-only','--format=posix',resolve(pack)]);
  writeFileSync(join(out,'pack-symbols.txt'),nm.stdout);
  for (const symbol of ['jet_rt_main','jet_rt_logical_env_seed','jet_rt_observe_runtime_start_from_env','jet_rt_observe_parked_tasks_report']) {
    const count = nm.stdout.split('\n').filter(line => line.startsWith(`${symbol} T `)).length;
    if (count !== 1) throw Error(`${symbol}: expected one strong native definition, got ${count}`);
  }
  checked(process.env.CC ?? 'cc', ['-O2',join(root,'Compiler/JetRuntime/Tests/main-fixture.c'),resolve(pack),'-lpthread','-ldl','-lm','-o',executable]);
} else {
  // Byte goldens derive from the reference entry/atexit ordering and the
  // registered E3013 wire fragments (ObserveMessages/Channel.text(0)).
  const report = 'Stop [E3013]: Parked tasks remain at process exit: {\n  main-child (spawn site 23)\n    state: blocked\n    wait target: channel receive}\n Why: The scheduler reached process exit while one or more tasks were blocked on a wait target.\n Fix: Join each task or make its wait reachable before process exit.\nMore: jet-lang.dev/e/E3013\n';
  for (const [name,status,golden] of [['normal',0,'entryexit2exit1'],['flush',0,'entryerrorexit2exit1'],['parked',70,`entryexit2exit1${report}`]]) {
    const capture = join(out,`${name}.bytes`), fd = openSync(capture,'w');
    let result;
    try { result = spawnSync(executable,[name],{cwd:root,stdio:['ignore',fd,fd],timeout:30000,env:{...process.env,JET_OBSERVE:'0',JET_SCHEDULER_THREADS:'2'}}); }
    finally { closeSync(fd); }
    writeFileSync(join(out,`${name}.receipt.json`),JSON.stringify({status:result.status,signal:result.signal,error:result.error?.message ?? null,expectedStatus:status,capture},null,2)+'\n');
    if (result.error || result.signal || result.status !== status || !readFileSync(capture).equals(Buffer.from(golden))) throw Error(`${name}: exit/merged stdout-stderr bytes differ from the entry golden`);
  }
  console.log('CHECK OK — native main: normal exit0, reverse atexit, stdout/stderr flush ordering, real parked child exit70');
}
