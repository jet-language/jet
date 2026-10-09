import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

const hash = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const hex = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
export async function readSharedBuild({file, head, source, target}) {
  const root = path.dirname(source), manifest = JSON.parse(fs.readFileSync(path.join(root, 'source.manifest.json'), 'utf8'));
  for (const phase of ['host', 'candidate']) {
    const directory = path.join(root, 'regression', `Main-DevGreen-${phase}-r1`);
    if (fs.readFileSync(path.join(directory, 'driver.rc'), 'utf8').trim() !== '0') throw Error(`shared ${phase} driver did not pass`);
    const verdict = JSON.parse(fs.readFileSync(path.join(directory, 'verdict.json'), 'utf8'));
    if (verdict.commit !== head || verdict.rc !== 0 || verdict.verdict !== 'GREEN' || !Array.isArray(verdict.steps) || verdict.steps.some(step => step.rc !== 0)) throw Error(`shared ${phase} verdict is not a same-pin GREEN`);
  }
  const receipt = JSON.parse(fs.readFileSync(file, 'utf8'));
  if (receipt.schema !== 'jet-regression-artifacts/v1' || receipt.commit !== head || receipt.tree !== manifest.tree || path.resolve(receipt.source) !== source || path.resolve(receipt.target_dir) !== path.resolve(target)) throw Error('shared artifacts do not match the pinned source/tree/target');
  for (const name of ['bootstrap_host', 'release_jet', 'candidate_jetc0']) {
    const artifact = receipt.artifacts?.[name];
    if (!artifact || !path.isAbsolute(artifact.path) || !hex(artifact.sha256)) throw Error(`missing/invalid ${name} publication`);
    const stat = fs.statSync(artifact.path);
    if (!stat.isFile() || !(stat.mode & 0o111)) throw Error(`${name} is not executable`);
    const digest = crypto.createHash('sha256');
    for await (const bytes of fs.createReadStream(artifact.path)) digest.update(bytes);
    if (digest.digest('hex') !== artifact.sha256) throw Error(`${name} sha256 differs from publication`);
  }
  return receipt;
}
export function readSharedPack({directory, head, source}) {
  if (fs.readFileSync(path.join(directory, 'driver.rc'), 'utf8').trim() !== '0') throw Error('shared Jet-only pack real driver did not pass');
  const receipt = JSON.parse(fs.readFileSync(path.join(directory, 'receipt.json'), 'utf8'));
  const runtimeDir = path.join(directory, 'jet-runtime'), key = fs.readFileSync(path.join(runtimeDir, 'native.key'), 'utf8').trim();
  const manifest = JSON.parse(fs.readFileSync(path.join(path.dirname(source), 'source.manifest.json'), 'utf8'));
  if (receipt.schema !== 'jet-native-runtime-proof/v1' || receipt.commit !== head || receipt.tree !== manifest.tree || path.resolve(receipt.source) !== source || path.resolve(receipt.runtime_dir) !== runtimeDir || receipt.rustObjects !== 0 || receipt.llvmObjects !== 0) throw Error('shared pack receipt does not match the source pin');
  if (!hex(key) || receipt.runtimeKey !== key) throw Error('shared pack receipt runtimeKey differs from native.key');
  const pack = path.join(runtimeDir, key, 'jet_runtime.pack.o'), jet = path.join(directory, 'jet');
  if (!hex(receipt.packSha256) || receipt.packSha256 !== hash(pack)) throw Error('shared pack receipt digest differs from selected pack');
  if (!hex(receipt.candidate?.sha256) || receipt.candidate.sha256 !== hash(jet)) throw Error('shared native CLI digest differs from pack receipt');
  return {receipt, pack, jet, runtimeDir, key};
}
