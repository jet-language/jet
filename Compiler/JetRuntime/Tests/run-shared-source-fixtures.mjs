#!/usr/bin/env node
// Actual canonical Jet provider acceptance. No seed descriptor implementation.
// Run modes independently so an integrator can collect every failure in one pass.
// Aggregate fixture.o owns the full Jet-only runtime; link platform libs only.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0 } from '../../JetBackend/Tests/jet-front.mjs';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['assemble','lower','link','run'].includes(mode)) {
  throw Error('usage: run-shared-source-fixtures.mjs OUT assemble|lower|link|run');
}
const out = resolve(output), fixture = join(out, 'fixture');
const read = path => readFileSync(join(repo,path),'utf8');
function command(bin,args,options={}) {
  const result = spawnSync(bin,args,{encoding:'utf8',maxBuffer:1<<27,...options});
  if (result.error || result.status !== 0) {
    throw Error(`${bin}: ${result.status ?? result.signal}\n${result.error?.message ?? ''}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  }
  return result;
}
if (mode === 'assemble') {
  mkdirSync(join(fixture,'Source'),{recursive:true});
  const paths = read('Compiler/JetRuntime/sources.list').split('\n').map(s=>s.trim()).filter(s=>s&&!s.startsWith('#'));
  paths.push('Compiler/JetRuntime/Tests/Fragments.fixture/SharedSourceTests.jet');
  const unit = paths.map(read).join('\n');
  const names = [...unit.matchAll(/^#Export\(c\)\s+fn\s+(\w+)\(/gm)].map(m=>m[1]);
  if (new Set(names).size !== names.length) throw Error('duplicate fixture exports');
  const init = new Map([...unit.matchAll(/^\/\/ runtime-init: (\d+)\n#Export\(c\) fn (\w+)/gm)].map(m=>[m[2],m[1]]));
  init.set('jet_rt_alloc_init','0');
  writeFileSync(join(fixture,'Source/Fixture.jet'),unit);
  writeFileSync(join(fixture,'exports.list'),names.sort().map(n=>n==='jet_rt_panic_list_bounds'?`${n} int-index-seam`:init.has(n)?`${n} init ${init.get(n)}`:n).join('\n')+'\n');
  writeFileSync(join(fixture,'package.jet'),read('Compiler/JetRuntime/package.jet'));
} else if (mode === 'lower') {
  const prefix = join(fixture,'fixture');
  const {problem} = runNativeLibrary({jet:candidateJetc0(),file:join(fixture,'Source/Fixture.jet'),prefix,exports:join(fixture,'exports.list'),env:{...process.env,JET_TOOLCHAIN_ROOT:repo,JET_STORE_DIR:join(out,'store')}});
  if (problem) throw Error(problem.detail);
} else if (mode === 'link') {
  command(nativeCCompiler,['-std=c11','-Wall','-Wextra','-Werror','-O2',join(repo,'Compiler/JetRuntime/Tests/shared-source-fixture.c'),join(fixture,'fixture.o'),'-lpthread','-ldl','-lm','-o',join(out,'shared-source-native')]);
} else {
  const result = command(join(out,'shared-source-native'),[],{timeout:120000});
  writeFileSync(join(out,'shared-source.stdout'),result.stdout);
  writeFileSync(join(out,'shared-source.stderr'),result.stderr);
  const expected = 'shared source: hash, kinds, nested ownership, alias, Process refusal, World boundary, reentry: ok\n';
  if (result.stdout !== expected || result.stderr !== '') throw Error('invalid shared Source fixture output');
  console.log(result.stdout.trim());
}
