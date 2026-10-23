#!/usr/bin/env node
// First-RC witness: reuse the admitted standalone Jet object, never rebake a
// runtime or link the startup-bearing pack into a C program with its own main.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { nativePackHash, readNativeRuntimePack } from '../../../Tools/packaging/runtime-pack-cache.mjs';
import { fileObject } from '../../../Tools/packaging/publish-native-runtime-pack.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, compiler, runtime, ...extra] = process.argv.slice(2);
if (!output || !compiler || !runtime || extra.length) throw Error('usage: run-native-tcp-routes.mjs OUT STAGE2_JET RC_RUNTIME_DIR');
const musl = process.env.JET_PACK_MUSL_LIB;
if (!musl) throw Error('JET_PACK_MUSL_LIB must name the admitted static musl directory');
const out = resolve(output), jet = resolve(compiler), runtimeDirectory = resolve(runtime);
// A fresh directory prevents any stale executable or C object from satisfying
// a failed current compile/link step.
mkdirSync(out);
function receipt(name, value) {
  writeFileSync(join(out, name), JSON.stringify(value, null, 2) + '\n', { flag: 'wx' });
}
function command(name, bin, args, cwd = root) {
  const result = spawnSync(bin, args, { cwd, env: process.env, encoding: 'utf8', maxBuffer: 1 << 27, timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 0) });
  receipt(`${name}.receipt.json`, { bin, args, cwd, status: result.status, signal: result.signal, error: result.error?.message ?? null, stdout: result.stdout, stderr: result.stderr });
  if (result.error || result.status !== 0) throw Error(`${name}: ${result.error?.message ?? result.signal ?? result.status}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  return result;
}
const admitted = readNativeRuntimePack(runtimeDirectory);
const publishedObject = join(runtimeDirectory, admitted.key, 'jet_runtime_jet.o');
const object = join(out, 'jet_runtime_jet.o');
// Link an exact snapshot of the bytes held and verified by the existing pack
// admission reader, so a pathname change after admission cannot substitute code.
writeFileSync(object, admitted.files.get('jet_runtime_jet.o'), { flag: 'wx' });
const objectHandle = fileObject(object);
if (objectHandle.split(':')[0] !== admitted.provenance.objectSha256) throw Error('standalone object snapshot differs from admitted provenance');
const archives = admitted.provenance.cPlatformArchives;
if (!Array.isArray(archives) || !archives.length || archives.some(row => row.language !== 'C' || row.role !== 'platform')) throw Error('runtime provenance lacks canonical C platform inputs');
const libc = resolve(musl, 'libc.a');
const libcHandle = fileObject(libc);
if (!archives.some(row => basename(row.path) === 'libc.a' && row.sha256 === libcHandle.split(':')[0])) throw Error('musl libc does not match runtime provenance');
const archiveInputs = [];
for (const row of archives) {
  const path = basename(row.path) === 'libc.a' ? libc : resolve(row.path);
  const handle = fileObject(path);
  if (handle.split(':')[0] !== row.sha256) throw Error(`C platform archive differs from runtime provenance: ${path}`);
  archiveInputs.push({ path, handle });
}
const source = join(root, 'Compiler/JetRuntime/Tests/TCPRoutesWitness.c');
const collectionsPath = 'Compiler/JetRuntime/Source/Collections.jet';
const collections = readFileSync(join(root, collectionsPath));
const collectionsInput = admitted.provenance.source.files.find(row => row.path === collectionsPath);
if (!collectionsInput || nativePackHash(collections) !== collectionsInput.sha256) throw Error('canonical U8 carrier source differs from the RC runtime');
const stride = /RT_HOST_U8_STRIDE\s*::\s*comptime\s*\{\s*I64\{(1|8)\}\s*\}/.exec(collections.toString('utf8'))?.[1];
if (!stride) throw Error('canonical U8 carrier source has no admitted epoch');
const sourceHandle = fileObject(source), compilerHandle = fileObject(jet);
const crt = ['crt1.o', 'crti.o', 'crtn.o'].map(name => ({ path: resolve(musl, name), handle: fileObject(resolve(musl, name)) }));
receipt('admission.json', { runtimeDirectory, runtimeKey: admitted.key, provenance: admitted.provenance, publishedObject, linkedObject: object, object: objectHandle, compiler: { path: jet, object: compilerHandle }, witness: { path: source, object: sourceHandle }, u8Stride: Number(stride), archives: archiveInputs, crt });
const cObject = join(out, 'tcp-routes.o');
const link = join(out, 'link');
command('compile', join(root, 'Tools/agent/jet-env'), [nativeCCompiler, '-std=c11', '-O1', '-fno-stack-protector', '-fno-PIE', '-fno-builtin', '-Wall', '-Wextra', '-Werror', `-DJET_TCP_U8_STRIDE=${stride}`, '-c', source, '-o', cObject]);
command('assemble-linker', process.execPath, [join(root, 'Compiler/JetBackend/Tests/run-link.mjs'), link]);
const executable = join(out, 'tcp-routes');
command('link', jet, ['run', join(link, 'unit.jet'), '--', executable, crt[0].path, crt[1].path, cObject, object, ...archiveInputs.map(row => row.path), crt[2].path], link);
for (const row of [...archiveInputs, ...crt]) if (fileObject(row.path) !== row.handle) throw Error(`C platform input changed during link: ${row.path}`);
if (fileObject(source) !== sourceHandle || fileObject(jet) !== compilerHandle || fileObject(object) !== objectHandle) throw Error('witness, compiler, or admitted object changed during the proof');
const result = command('run', executable, []);
if (result.stdout !== 'tcp routes: ok\n' || result.stderr !== '') throw Error('TCP runtime witness output differs from its exact success receipt');
receipt('result.json', { status: 0, runtimeKey: admitted.key, object: objectHandle, compiler: compilerHandle, witness: sourceHandle, cObject: fileObject(cObject), executable: fileObject(executable), stdout: result.stdout, stderr: result.stderr });
console.log(`native TCP routes: ok (${admitted.key})`);
