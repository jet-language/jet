#!/usr/bin/env node
// One aggregate Jet object supplies actual comptime, numeric and U32 kernels.
// Link fixture.o only plus platform libraries, never another runtime pack.
// Each mode can run under cheap.sh + jet-env.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0 } from '../../JetBackend/Tests/jet-front.mjs';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['assemble', 'lower', 'link', 'run'].includes(mode)) {
  throw Error('usage: run-residue-fixtures.mjs OUT assemble|lower|link|run');
}
const out = resolve(output), fixture = join(out, 'fixture');
const read = path => readFileSync(join(repo, path), 'utf8');
function command(bin, args) {
  const result = spawnSync(bin, args, {cwd:repo,encoding:'utf8',maxBuffer:1<<27,timeout:600000});
  if (result.error || result.status !== 0) throw Error(`${bin}: ${result.status ?? result.signal}\n${result.error?.message ?? ''}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  return result;
}
const cases = [
  ['comptime', 'Compiler/JetRuntime/Tests/comptime-fixture.c'],
  ['numeric', 'Compiler/JetRuntime/Tests/numeric-fixture.c'],
  ['int-parse', 'Compiler/JetRuntime/Tests/int-parse-fixture.c'],
  ['float-parse', 'Compiler/JetRuntime/Tests/float-parse-fixture.c'],
  ['u32', 'Compiler/JetBackend/Tests/u32-import.c'],
];
if (mode === 'assemble') {
  mkdirSync(join(fixture, 'Source'), {recursive:true});
  const paths = read('Compiler/JetRuntime/sources.list').split('\n').map(s=>s.trim()).filter(s=>s&&!s.startsWith('#'));
  paths.push('Compiler/JetBackend/Tests/Fixtures/U32Import.jet');
  const unit = paths.map(read).join('\n');
  const names = [...unit.matchAll(/^#Export\(c\)\s+fn\s+(\w+)\(/gm)].map(m=>m[1]);
  if (new Set(names).size !== names.length) throw Error('duplicate fixture export');
  const init = new Map([...unit.matchAll(/^\/\/ runtime-init: (\d+)\n#Export\(c\) fn (\w+)/gm)].map(m=>[m[2],m[1]]));
  init.set('jet_rt_alloc_init','0');
  writeFileSync(join(fixture,'Source/Fixture.jet'),unit);
  writeFileSync(join(fixture,'exports.list'),names.sort().map(n=>n==='jet_rt_panic_list_bounds'?`${n} int-index-seam`:init.has(n)?`${n} init ${init.get(n)}`:n).join('\n')+'\n');
  writeFileSync(join(fixture,'package.jet'),read('Compiler/JetRuntime/package.jet'));
} else if (mode === 'lower') {
  const prefix=join(fixture,'fixture');
  const {problem}=runNativeLibrary({jet:candidateJetc0(),file:join(fixture,'Source/Fixture.jet'),prefix,exports:join(fixture,'exports.list'),timeout:600000,env:{...process.env,JET_TOOLCHAIN_ROOT:repo,JET_STORE_DIR:join(out,'store')}});
  if(problem)throw Error(problem.detail);
} else if (mode === 'link') {
  for (const [name, source] of cases) {
    const provider=name==='u32'?[]:['-DU32_KERNELS_ONLY',join(repo,'Compiler/JetBackend/Tests/u32-import.c')];
    command(nativeCCompiler,['-O2',...provider,join(repo,source),join(fixture,'fixture.o'),'-lpthread','-ldl','-lm','-o',join(out,name)]);
  }
} else {
  for (const [name] of cases) {
    const args=name==='float-parse'?[join(repo,'Compiler/JetRuntime/Tests/float-parse.seed.golden')]:[];
    const result=command(join(out,name),args);
    writeFileSync(join(out,`${name}.stdout`),result.stdout);
    writeFileSync(join(out,`${name}.stderr`),result.stderr);
    if (!result.stdout.startsWith('CHECK OK — native ') || result.stderr !== '') throw Error(`${name}: unexpected witness output`);
    console.log(result.stdout.trimEnd());
  }
  console.log(`CHECK OK — composed native residue fixtures: ${cases.map(([name])=>name).join(', ')}`);
}
