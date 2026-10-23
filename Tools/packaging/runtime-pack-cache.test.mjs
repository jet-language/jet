import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { nativePackHash as hash, nativePackRecipe, readNativeRuntimePack } from './runtime-pack-cache.mjs';

// Receipt-boundary unit fixtures, not executable objects or latency evidence.
function receipt(t) {
  const root = mkdtempSync(join(tmpdir(), 'jet-native-pack-receipt-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const object = Buffer.from('object bytes'), pack = Buffer.from('pack bytes'), image = Buffer.from('image bytes'), ir = Buffer.from('ir bytes');
  const abi = JSON.stringify([{ symbol: 'jet_rt_alloc_init', parameters: [], result: null }]);
  const source = hash('runtime'), backend = hash('backend'), lifecycle = hash('lifecycle');
  const platform = [hash('platform')], compiler = hash('compiler');
  const recipe = nativePackRecipe(source, backend, lifecycle, platform, compiler);
  const key = hash(JSON.stringify({ format: 'jet-native-runtime/v1', source, abi: hash(abi), backend, lifecycle, platform }));
  const directory = join(root, key);
  mkdirSync(directory);
  const provenance = { backend: 'jet-native', runtimeKey: key, rustObjects: 0, llvmObjects: 0,
    source: { sha256: source }, backendInputs: { sha256: backend }, lifecycle: { sha256: lifecycle },
    cPlatformArchives: platform.map(sha256 => ({ sha256 })), compilerSha256: compiler,
    recipeSha256: recipe, objectSha256: hash(object), irSha256: hash(ir), packSha256: hash(pack), imageSha256: hash(image), abiSha256: hash(abi) };
  const publish = () => writeFileSync(join(directory, 'provenance.json'), JSON.stringify(provenance));
  publish();
  writeFileSync(join(root, 'native.key'), key + '\n');
  for (const [name, bytes] of [['jet_runtime_jet.o', object], ['runtime.ir', ir], ['jet_runtime.pack.o', pack], ['jet_runtime.image', image], ['runtime.abi.json', abi], ['runtime.exports', 'jet_rt_alloc_init\n']]) writeFileSync(join(directory, name), bytes);
  return { root, directory, provenance, recipe, key, pack, publish };
}

test('toolchain recipe changes for every semantic/ABI producer input', () => {
  const inputs = ['source', 'backend', 'lifecycle', ['archive'], 'compiler'];
  const expected = nativePackRecipe(...inputs);
  assert.equal(nativePackRecipe(...inputs), expected);
  for (let index = 0; index < inputs.length; index++) {
    const changed = [...inputs]; changed[index] = index === 3 ? ['other archive'] : 'changed';
    assert.notEqual(nativePackRecipe(...changed), expected);
  }
});
test('matching cached receipt admits held artifact bytes', t => {
  const f = receipt(t), admitted = readNativeRuntimePack(f.root, f.recipe);
  assert.equal(admitted.key, f.key);
  assert.deepEqual(admitted.pack, f.pack);
  assert.equal(admitted.files.size, 7);
});
test('changed recipe and every damaged artifact refuse reuse', t => {
  const f = receipt(t);
  assert.throws(() => readNativeRuntimePack(f.root, hash('new recipe')), /recipe/);
  for (const name of ['jet_runtime_jet.o', 'runtime.ir', 'jet_runtime.pack.o', 'jet_runtime.image', 'runtime.abi.json']) {
    const admitted = readNativeRuntimePack(f.root);
    writeFileSync(join(f.directory, name), 'damaged');
    assert.throws(() => readNativeRuntimePack(f.root), /digest mismatch/);
    writeFileSync(join(f.directory, name), admitted.files.get(name));
  }
});
test('key traversal, symbol inventory drift and inconsistent identity refuse reuse', t => {
  const f = receipt(t);
  writeFileSync(join(f.root, 'native.key'), '../elsewhere');
  assert.throws(() => readNativeRuntimePack(f.root), /content-addressed/);
  writeFileSync(join(f.root, 'native.key'), f.key);
  writeFileSync(join(f.directory, 'runtime.exports'), 'another_export\n');
  assert.throws(() => readNativeRuntimePack(f.root), /exports disagree/);
  writeFileSync(join(f.directory, 'runtime.exports'), 'jet_rt_alloc_init\n');
  f.provenance.source.sha256 = hash('different source'); f.publish();
  assert.throws(() => readNativeRuntimePack(f.root), /identity/);
});
test('Rust or LLVM provenance and substituted symlink artifacts refuse reuse', t => {
  const f = receipt(t);
  for (const field of ['rustObjects', 'llvmObjects']) {
    f.provenance[field] = 1; f.publish();
    assert.throws(() => readNativeRuntimePack(f.root), /zero-Rust/);
    f.provenance[field] = 0;
  }
  f.publish();
  const pack = join(f.directory, 'jet_runtime.pack.o');
  rmSync(pack); symlinkSync(join(f.directory, 'jet_runtime_jet.o'), pack);
  assert.throws(() => readNativeRuntimePack(f.root), /ELOOP/);
});
