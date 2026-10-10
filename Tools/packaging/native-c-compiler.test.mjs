import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { nativeCCompiler, nativeCEnvironment, nativeCFlags } from './native-c-compiler.mjs';
import { runtimeExportSignatures } from '../agent/runtime-route-abi.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const fixtureNames = ['ProcessRoutesWitness', 'ProcessExecutionWitness', 'TerminalProcessWitness', 'ProcessSpecWitness', 'TCPRoutesWitness'];
const forbidden = name => /_chk(?:_|$)/u.test(name) || /^__.*_2$/u.test(name);
function command(bin, args, options = {}) {
  const result = spawnSync(bin, args, { encoding: 'utf8', maxBuffer: 128 * 1024 * 1024, ...options });
  assert.ifError(result.error);
  assert.equal(result.signal, null, `${bin}: ${result.signal}`);
  assert.equal(result.status, 0, `${bin} ${args.join(' ')}:\n${result.stderr}`);
  return result.stdout;
}

test('native C policy keeps other hardening and never changes the parent environment', () => {
  assert.deepEqual(nativeCFlags, ['-U_FORTIFY_SOURCE', '-D_FORTIFY_SOURCE=0']);
  assert.ok(Object.isFrozen(nativeCFlags));
  const parent = { NIX_HARDENING_ENABLE: 'bindnow fortify format fortify3 stackprotector', KEEP: 'unchanged' };
  assert.deepEqual(nativeCEnvironment(parent), { NIX_HARDENING_ENABLE: 'bindnow format stackprotector', KEEP: 'unchanged' });
  assert.equal(parent.NIX_HARDENING_ENABLE, 'bindnow fortify format fortify3 stackprotector');
  assert.deepEqual(nativeCEnvironment({ KEEP: 'unchanged' }), { KEEP: 'unchanged' });
});

test('the Jet CLI embeds the same single native C policy rows', () => {
  const result = spawnSync(process.execPath, [join(root, 'Compiler/Bootstrap/generate-native-c-policy.mjs'), '--check'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  const embedded = readFileSync(join(root, 'Compiler/JetCli/Source/Cli/NativeCPolicy.jet'), 'utf8');
  assert.match(embedded, /JET_CLI_NATIVE_C_FLAGS :: prep \{ \[String\]\{"-U_FORTIFY_SOURCE", "-D_FORTIFY_SOURCE=0"\} \}/u);
  assert.match(embedded, /JET_CLI_NATIVE_C_HARDENING_REMOVED :: prep \{ \[String\]\{"fortify", "fortify3"\} \}/u);
});

test('native fixture and packaging drivers have no independent C compiler policy', () => {
  for (const directory of ['Compiler/JetRuntime/Tests', 'Compiler/JetBackend/Tests', 'Tools/packaging']) {
    for (const name of readdirSync(join(root, directory))) {
      if (!name.endsWith('.mjs') || name === 'native-c-compiler.mjs' || name === 'native-c-compiler.test.mjs') continue;
      const source = readFileSync(join(root, directory, name), 'utf8');
      assert.doesNotMatch(source, /process\.env\.CC\s*\?\?/u, `${directory}/${name}: use the shared native C driver`);
      assert.doesNotMatch(source, /\b(?:spawnSync|execFileSync|run|command)\([^\n]*(['"])(?:cc|gcc|clang)\1(?=\s*[,\]])/u,
        `${directory}/${name}: hard-coded C compiler bypasses the shared policy`);
    }
  }
});

test('real native C witnesses have no glibc fortified imports and resolve against the admitted platform archives', t => {
  const musl = process.env.JET_PACK_MUSL_LIB;
  assert.ok(musl, 'JET_PACK_MUSL_LIB must name the native pack static musl directory');
  const scratch = process.env.JET_TEST_SCRATCH_DIR ?? tmpdir();
  mkdirSync(scratch, { recursive: true });
  const out = mkdtempSync(join(scratch, 'native-c-platform-census-'));
  t.after(() => rmSync(out, { recursive: true, force: true }));
  const archives = [resolve(musl, 'libc.a'), ...['libgcc.a', 'libgcc_eh.a'].map(name =>
    command(nativeCCompiler, [`-print-file-name=${name}`]).trim())];
  const platformSymbols = new Set();
  for (const path of archives) {
    for (const line of command('nm', ['--defined-only', '--extern-only', '--format=posix', path]).split('\n')) {
      const [symbol, type] = line.split(/\s+/u);
      if (symbol && type && /^[A-Za-z]$/u.test(type)) platformSymbols.add(symbol);
    }
  }
  const providerSymbols = runtimeExportSignatures(root);
  const collections = readFileSync(join(root, 'Compiler/JetRuntime/Source/Collections.jet'), 'utf8');
  const stride = /RT_HOST_U8_STRIDE\s*::\s*prep\s*\{\s*I64\{(1|8)\}\s*\}/u.exec(collections)?.[1];
  assert.ok(stride, 'the TCP fixture must use the canonical U8 carrier epoch');
  const census = [];
  for (const name of fixtureNames) {
    const object = join(out, name + '.o');
    command(nativeCCompiler, ['-std=c11', '-O1', '-fno-stack-protector', '-fno-PIE', '-fno-builtin', '-Wall', '-Wextra', '-Werror',
      ...(name === 'TCPRoutesWitness' ? [`-DJET_TCP_U8_STRIDE=${stride}`] : []),
      '-c', join(root, 'Compiler/JetRuntime/Tests', name + '.c'), '-o', object]);
    const imports = command('nm', ['-u', '--format=posix', object]).split('\n').filter(Boolean).map(line => line.split(/\s+/u)[0]);
    const forbiddenImports = imports.filter(forbidden);
    const missingImports = imports.filter(symbol => !platformSymbols.has(symbol) && !providerSymbols.has(symbol) && symbol !== '__real_jet_rt_free');
    census.push({ name, imports, forbiddenImports, missingImports });
  }
  writeFileSync(join(out, 'census.json'), JSON.stringify({ archives, census }, null, 2) + '\n');
  // Compile every independent witness before returning the complete failure list.
  assert.deepEqual(census.filter(row => row.forbiddenImports.length || row.missingImports.length), [], JSON.stringify(census, null, 2));
});
