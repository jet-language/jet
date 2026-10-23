// The static native pack's single packaging/admission boundary. Recipe inputs
// describe a toolchain, never a consumer program or its optimization profile.
import { createHash } from 'node:crypto';
import { constants, openSync, closeSync, fstatSync, lstatSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

export const nativePackHash = bytes => createHash('sha256').update(bytes).digest('hex');
export function nativePackRecipe(source, backend, lifecycle, platform, compiler) {
  return nativePackHash(JSON.stringify({ format: 'jet-native-runtime-recipe/v1',
    source, backend, lifecycle, platform, compiler }));
}
function heldFile(path) {
  const fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const before = fstatSync(fd, { bigint: true });
    if (!before.isFile()) throw Error(`runtime input is not a regular file: ${path}`);
    const bytes = readFileSync(fd);
    const after = fstatSync(fd, { bigint: true });
    const named = lstatSync(path, { bigint: true });
    for (const stat of [after, named]) {
      if (!stat.isFile() || stat.dev !== before.dev || stat.ino !== before.ino ||
          stat.size !== before.size || stat.mtimeNs !== before.mtimeNs || stat.ctimeNs !== before.ctimeNs) {
        throw Error(`runtime input changed while reading: ${path}`);
      }
    }
    return bytes;
  } finally { closeSync(fd); }
}
export function readNativeRuntimePack(directory, recipe = null) {
  const key = heldFile(join(directory, 'native.key')).toString('utf8').trim();
  if (!/^[a-f0-9]{64}$/.test(key)) throw Error('native.key must name one content-addressed runtime directory');
  const root = join(directory, key);
  const provenanceBytes = heldFile(join(root, 'provenance.json'));
  const provenance = JSON.parse(provenanceBytes);
  if (provenance.backend !== 'jet-native' || provenance.rustObjects !== 0 || provenance.llvmObjects !== 0 || provenance.runtimeKey !== key) {
    throw Error('runtime pack must have matching Jet-native, zero-Rust/LLVM provenance');
  }
  if (recipe !== null && provenance.recipeSha256 !== recipe) throw Error('runtime pack recipe does not match this toolchain');
  const files = new Map([['provenance.json', provenanceBytes]]);
  for (const [name, expected] of [['jet_runtime.pack.o', provenance.packSha256],
    ['jet_runtime_jet.o', provenance.objectSha256], ['runtime.ir', provenance.irSha256], ['runtime.abi.json', provenance.abiSha256]]) {
    const bytes = heldFile(join(root, name));
    if (!/^[a-f0-9]{64}$/.test(expected ?? '') || nativePackHash(bytes) !== expected) throw Error(`runtime pack digest mismatch: ${name}`);
    files.set(name, bytes);
  }
  const abi = JSON.parse(files.get('runtime.abi.json'));
  const exports = heldFile(join(root, 'runtime.exports'));
  const names = exports.toString('utf8').trim().split('\n').sort();
  if (!Array.isArray(abi) || !abi.length || JSON.stringify(abi.map(row => row.symbol).sort()) !== JSON.stringify(names)) {
    throw Error('runtime ABI and checked exports disagree');
  }
  const identity = { format: 'jet-native-runtime/v1', source: provenance.source.sha256,
    abi: provenance.abiSha256, backend: provenance.backendInputs.sha256,
    lifecycle: provenance.lifecycle.sha256, platform: provenance.cPlatformArchives.map(row => row.sha256) };
  if (nativePackHash(JSON.stringify(identity)) !== key) throw Error('runtime pack identity does not match native.key');
  files.set('runtime.exports', exports);
  return { key, provenance, provenanceBytes, pack: files.get('jet_runtime.pack.o'), files };
}
