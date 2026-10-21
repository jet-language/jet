#!/usr/bin/env node
// Full composed Jet runtime, real kernel descriptors/processes, C ABI carriers.
// The held proof supplies the FINAL E2 producer; fixture.o owns the full runtime.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0 } from '../../JetBackend/Tests/jet-front.mjs';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
import { parseRuntimeExports, runtimeLifecycleList } from '../../../Tools/agent/runtime-route-abi.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['assemble', 'lower', 'audit', 'compile', 'link', 'run'].includes(mode)) throw Error('usage: run-host-fixtures.mjs OUT assemble|lower|audit|compile|link|run');
const out = resolve(output), dir = join(out, 'unit');
const fixtures = [['files', 'HostFilesTests.c', 'files raw: ok\n'], ['process', 'HostSpawnTests.c', 'process raw: ok\n'], ['root-stdio', 'RootStdioWitness.c', 'root stdio: ok\n'], ['tcp-routes', 'TCPRoutesWitness.c', 'tcp routes: ok\n'], ['build-http', 'HostBuildHTTPWitness.c', 'build http: ok\n']];
function command(bin, args, options = {}) {
  const result = spawnSync(bin, args, { encoding: 'utf8', maxBuffer: 1 << 27, cwd: root, ...options });
  if (result.error || result.status !== 0) throw Error(`${bin}: ${result.error?.message ?? result.status}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  return result;
}
if (mode === 'assemble') {
  command(process.execPath, [join(root, 'Compiler/JetRuntime/Tests/check-runtime.mjs'), dir, '--assemble-only']);
  mkdirSync(join(dir, 'Source'), { recursive: true });
  const fixture = readFileSync(join(root, 'Compiler/JetRuntime/Tests/Fragments.fixture/HostSpawnTests.jet'), 'utf8');
  const source = readFileSync(join(dir, 'unit.jet'), 'utf8') + '\n' + fixture;
  writeFileSync(join(dir, 'Source/Fixture.jet'), source);
  writeFileSync(join(dir, 'lifecycle.list'), runtimeLifecycleList(parseRuntimeExports(source, 'Source/Fixture.jet')));
} else if (mode === 'lower') {
  const prefix = join(dir, 'fixture');
  const { problem } = runNativeLibrary({ jet: candidateJetc0(), file: join(dir, 'Source/Fixture.jet'), prefix, lifecycle: join(dir, 'lifecycle.list'), timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 0), env: { ...process.env, JET_TOOLCHAIN_ROOT: root, JET_STORE_DIR: join(out, 'store') } });
  if (problem) throw Error(problem.detail);
} else if (mode === 'audit') {
  const object = join(dir, 'fixture.o');
  const imports = command('nm', ['-u', '--format=posix', object]).stdout;
  writeFileSync(join(out, 'imports.nm'), imports);
  const allowed = new Set(readFileSync(join(root, 'Compiler/JetRuntime/imports.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#')));
  for (const line of imports.trim().split('\n').filter(Boolean)) if (!allowed.has(line.split(/\s+/)[0])) throw Error(`unledgered host fixture import ${line}`);
  const symbols = command('nm', ['-g', '--defined-only', '--format=posix', object]).stdout;
  writeFileSync(join(out, 'exports.nm'), symbols);
  for (const file of ['HostFiles.jet', 'HostSpawn.jet']) {
    const source = readFileSync(join(root, 'Compiler/JetRuntime/Source', file), 'utf8');
    for (const match of source.matchAll(/^#Export\(c\)\s+fn\s+(\w+)\(/gm)) if (!new RegExp(`^${match[1]} T `, 'm').test(symbols)) throw Error(`missing real host leaf ${match[1]}`);
  }
  writeFileSync(join(out, 'disassembly.txt'), command('objdump', ['-dr', object]).stdout);
} else if (mode === 'compile') {
  const collections = readFileSync(join(root, 'Compiler/JetRuntime/Source/Collections.jet'), 'utf8');
  const stride = /RT_HOST_U8_STRIDE\s*::\s*prep\s*\{\s*I64\{(1|8)\}\s*\}/.exec(collections)?.[1];
  if (!stride) throw Error('TCP witness requires the canonical native U8 epoch');
  for (const [name, source] of fixtures) command(join(root, 'Tools/agent/jet-env'), [nativeCCompiler, '-std=c11', '-O1', '-fno-stack-protector', '-fno-PIE', '-fno-builtin', '-Wall', '-Wextra', '-Werror', ...(name === 'tcp-routes' ? [`-DJET_TCP_U8_STRIDE=${stride}`] : name === 'build-http' ? [`-DJET_U8_STRIDE=${stride}`] : []), '-c', join(root, 'Compiler/JetRuntime/Tests', source), '-o', join(out, `${name}.o`)]);
} else if (mode === 'link') {
  const musl = process.env.JET_PACK_MUSL_LIB, compiler = process.env.JET;
  if (!musl || !compiler) throw Error('JET_PACK_MUSL_LIB and matching FINAL E2 JET are required; fixture.o supplies the Jet-only runtime');
  command(process.execPath, [join(root, 'Compiler/JetBackend/Tests/run-link.mjs'), join(out, 'link')]);
  for (const [name] of fixtures) {
    const result = command(compiler, ['run', join(out, 'link/unit.jet'), '--', join(out, name), join(musl, 'crt1.o'), join(musl, 'crti.o'), join(out, `${name}.o`), join(dir, 'fixture.o'), join(musl, 'libc.a'), join(musl, 'crtn.o')], { cwd: join(out, 'link') });
    writeFileSync(join(out, `${name}-link.log`), result.stdout + result.stderr);
  }
} else {
  const failures = [];
  for (const [name, , expected] of fixtures) {
    const result = spawnSync(join(out, name), [], { encoding: 'utf8', timeout: 0 });
    writeFileSync(join(out, `${name}.receipt.json`), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null, stdout: result.stdout, stderr: result.stderr }, null, 2) + '\n');
    if (result.error || result.status !== 0 || result.stdout !== expected || result.stderr !== '') failures.push(`${name}: ${result.error?.message ?? result.status}, stderr=${JSON.stringify(result.stderr)}`);
  }
  if (failures.length) throw Error(failures.join('\n'));
}
console.log(`host fixtures ${mode}: ok`);
