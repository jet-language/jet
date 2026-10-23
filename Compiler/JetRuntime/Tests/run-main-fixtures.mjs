#!/usr/bin/env node
// Link/run only against an explicit fresh Jet-only runtime object/archive
// (KEY/jet_runtime_jet.o, not startup-bearing jet_runtime.pack.o or a Rust provider).
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, openSync, closeSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode, pack] = process.argv.slice(2);
if (!output || !['link', 'run'].includes(mode) || !pack || !existsSync(pack)) throw Error('usage: run-main-fixtures.mjs OUT link|run JET_ONLY_RUNTIME_OBJECT_OR_ARCHIVE');
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
  for (const symbol of ['jet_rt_main','jet_rt_process_exit','jet_rt_process_exit_begin','jet_rt_process_exit_finish','jet_rt_logical_env_seed','jet_rt_observe_runtime_start_from_env','jet_rt_observe_parked_tasks_report']) {
    const count = nm.stdout.split('\n').filter(line => line.startsWith(`${symbol} T `)).length;
    if (count !== 1) throw Error(`${symbol}: expected one strong native definition, got ${count}`);
  }
  checked(nativeCCompiler, ['-O2',join(root,'Compiler/JetRuntime/Tests/main-fixture.c'),resolve(pack),'-lpthread','-ldl','-lm','-o',executable]);
} else {
  // Byte goldens derive from the reference entry/atexit ordering and the
  // registered E3013 wire fragments (ObserveMessages/Channel.text(0)).
  const parked = (label, site) => `Stop [E3013]: Parked tasks remain at process exit: \n  ${label} (spawn site ${site})\n    state: blocked\n    wait target: channel receive\n Why: The scheduler reached process exit while one or more tasks were blocked on a wait target.\n Fix: Join each task or make its wait reachable before process exit.\nMore: jet-lang.dev/e/E3013\n`;
  const numeric = "Stop [E3010]: `divided by zero`\n  --> <core.prelude>:0\n Why: The operands or position do not produce a valid result, so the safe runtime stops instead of returning corrupted data — with Jet file and line.\n Fix: Check the operands or bounds before the operation, or use a checked operation that returns an outcome.\nMore: jet-lang.dev/e/E3010\n";
  const normal = 'entryexit2exit1', restart = parked('restart-child',41);
  const cases = [
    ['normal',0,normal], ['twice',0,normal + normal],
    ['flush',0,'entryerrorexit2exit1'], ['explicit',73,normal],
    ['parked',70,normal + parked('main-child',23)],
    ['worker-fatal',70,normal + numeric],
    ['second-fatal',70,normal + normal + numeric],
    ['observe-enabled',0,'entry' + restart + restart + 'exit2exit1'],
    ['observe-disabled',0,'entry' + restart + 'exit2exit1'],
    ['concurrent-fatal',null,null],
  ];
  for (const [name,status,golden] of cases) {
    const capture = join(out,`${name}.bytes`), fd = openSync(capture,'w');
    let result;
    const env = {...process.env,JET_OBSERVE:name === 'observe-enabled' ? '1' : '0',JET_SCHEDULER_THREADS:'2',TMPDIR:out,
      JET_DEVTOOLS_RELAY_SESSION_ID:'main-session',JET_DEVTOOLS_RELAY_SOURCE_ID:'main-source',
      JET_DEVTOOLS_RELAY_BUILD_ID:'main-build',JET_DEVTOOLS_RELAY_REVISION:'main-revision',JET_DEVTOOLS_RELAY_WORLD_ID:'main-world'};
    try { result = spawnSync(executable,[name],{cwd:root,stdio:['ignore',fd,fd],timeout:30000,env}); }
    finally { closeSync(fd); }
    writeFileSync(join(out,`${name}.receipt.json`),JSON.stringify({status:result.status,signal:result.signal,error:result.error?.message ?? null,expectedStatus:status,capture},null,2)+'\n');
    const bytes = readFileSync(capture);
    const matches = name === 'concurrent-fatal'
      ? (result.status === 70 && bytes.equals(Buffer.from('entrywinner70'))) || (result.status === 71 && bytes.equals(Buffer.from('entrywinner71')))
      : result.status === status && bytes.equals(Buffer.from(golden));
    if (result.error || result.signal || !matches) throw Error(`${name}: exit/merged stdout-stderr bytes differ from the entry golden`);
  }
  console.log('CHECK OK — native process boundary: ten entry/exit/flush/restart cases; busy-worker fatal has no hang; concurrent fatal has one report and its winning code');
}
