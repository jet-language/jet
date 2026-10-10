// Admit the retained, source-bound bootstrap factory before feeding real Jet
// library/native requests. A factory is not an argv CLI or a native candidate.
import { createHash } from 'node:crypto';
import { accessSync, constants, mkdirSync, mkdtempSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { assertPinnedSource } from '../proof-source.mjs';
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
export function admitFactoryReceipt(text, censusText) {
  const rows = new Map(text.trim().split('\n').map(line => {
    const split = line.indexOf('=');
    return [line.slice(0, split), line.slice(split + 1)];
  }));
  for (const [name, value] of Object.entries({ mode: 'factory', complete: 'true', report_count: '0', selected_factory_tier: 'Aot', actual_factory_tier: 'Aot' })) {
    if (rows.get(name) !== value) throw Error(`factory receipt ${name} is not ${value}`);
  }
  const census = censusText.trim().split('\n');
  if (census.length !== 1 || census[0] !== 'phase\titem\tcode\tmessage\tspan') throw Error('native/library factory request has a missing or non-empty all-phases census');
  // emitted_source is legitimately absent for library/native outputs. Do not
  // impose the Rust-source source_bytes>0 predicate on these typed boundaries.
  return Object.fromEntries(rows);
}
export function readStageZeroFactory(recordFile, repo) {
  const record = JSON.parse(readFileSync(recordFile, 'utf8'));
  const source = resolve(repo);
  if (record.schema !== 'jet-sharded-stage-zero/v1' || record.rc !== 0 || record.verdict !== 'GREEN' || record.source !== source || record.factory?.native !== false || record.factory?.protocol !== 'JET_BOOTSTRAP_* factory; NOT an argv CLI') throw Error('source-bound canonical sharded factory receipt is required');
  const manifest = assertPinnedSource(source, record.commit);
  if (manifest.tree !== record.tree || record.factory.identity.repo !== source || record.factory.identity.package !== 'jet_bootstrap_stage_zero' || record.factory.identity.task_roots !== 'false' || !record.factory.identity.id) throw Error('sharded factory source/identity mismatch');
  const binary = record.factory.path;
  accessSync(binary, constants.X_OK);
  if (digest(binary) !== record.factory.sha256 || digest(record.factory.generated) !== record.factory.generated_sha256) throw Error('retained sharded factory binary/source hash mismatch');
  const magic = readFileSync(binary).subarray(0, 4);
  if (!magic.equals(Buffer.from([127, 69, 76, 70]))) throw Error('retained sharded factory is not ELF');
  return record;
}
export function invokeStageZeroFactory({ recordFile, repo, sourceRoot, entry, work, policy }) {
  const record = readStageZeroFactory(recordFile, repo);
  const library = policy.JET_FRONT_LIBRARY_OBJECT_OUTPUT;
  const native = policy.JET_FRONT_NATIVE_OUTPUT;
  if (Boolean(library) === Boolean(native)) throw Error('exactly one real library/native request is required');
  const allowed = new Set(['JET_FRONT_LIBRARY_OBJECT_OUTPUT', 'JET_FRONT_LIBRARY_LIFECYCLE', 'JET_FRONT_RUNTIME_PACK_OUTPUT', 'JET_FRONT_RUNTIME_PLATFORM_INPUTS', 'JET_FRONT_NATIVE_OUTPUT', 'JET_RUNTIME_PACK']);
  for (const key of Object.keys(policy)) if (!allowed.has(key)) throw Error(`unsupported factory policy ${key}`);
  if (library && (!policy.JET_FRONT_LIBRARY_LIFECYCLE || !policy.JET_FRONT_RUNTIME_PACK_OUTPUT || !policy.JET_FRONT_RUNTIME_PLATFORM_INPUTS)) throw Error('real library lifecycle, pack output and platform archives required');
  if (native && !policy.JET_RUNTIME_PACK) throw Error('native factory request requires the proven runtime pack');
  mkdirSync(work, { recursive: true });
  work = mkdtempSync(join(work, 'request-'));
  const receipt = join(work, 'factory.env');
  const census = join(work, 'keep-going.tsv');
  const home = join(work, 'home');
  const store = join(work, 'store');
  mkdirSync(home, { recursive: true });
  mkdirSync(store, { recursive: true });
  const env = { HOME: home, PATH: process.env.PATH, TMPDIR: process.env.TMPDIR ?? '/tmp', LC_ALL: 'C', RUST_MIN_STACK: '268435456', RUST_BACKTRACE: '1', JET_TOOLCHAIN_ROOT: resolve(repo), JET_BOOTSTRAP_MODE: 'factory', JET_BOOTSTRAP_FACTORY_TIER: 'aot', JET_BOOTSTRAP_SOURCE_ROOT: resolve(sourceRoot), JET_BOOTSTRAP_ENTRY: entry, JET_BOOTSTRAP_OUTPUT: join(work, 'unused-rust-output.rs'), JET_BOOTSTRAP_RECEIPT: receipt, JET_STORE_DIR: store, JET_KEEP_GOING: '1', JET_KEEP_GOING_REPORT: census, ...policy };
  const run = spawnSync(record.factory.path, [], { cwd: sourceRoot, env, stdio: 'inherit' });
  if (run.error) throw run.error;
  if (run.status !== 0) throw Error(`bootstrap factory exited ${run.status ?? run.signal}`);
  const admitted = admitFactoryReceipt(readFileSync(receipt, 'utf8'), readFileSync(census, 'utf8'));
  return { record, receipt, census, admitted, compilerProtocol: 'bootstrap-factory', sourceCommit: record.commit };
}
