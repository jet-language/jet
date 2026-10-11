#!/usr/bin/env node
// Discovery only: observe Entry.native through object emission while the real
// pack is built independently. No absent link prerequisite is called a pass.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { observeStageZeroNativeLowering } from './native-factory.mjs';
const [recordFile, repoArg, outArg, mode] = process.argv.slice(2);
if (!recordFile || !repoArg || !outArg || (mode && mode !== '--dry-run')) throw Error('native-lowering-census.mjs FACTORY_RECORD PINNED_SOURCE FRESH_OUT [--dry-run]');
const repo = resolve(repoArg), out = resolve(outArg), producer = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
if (existsSync(out)) throw Error('fresh lowering census output required');
mkdirSync(out, { recursive: true });
const assembly = join(out, 'assembly');
const assemble = spawnSync(process.execPath, [join(producer, 'Tools/agent/native-compiler.mjs'), '--assemble-only', '--repo', repo, '--out', assembly], { stdio: 'inherit' });
if (assemble.error || assemble.status !== 0) throw assemble.error ?? Error('canonical compiler CLI assembly failed');
const unit = JSON.parse(readFileSync(join(assembly, 'plan.json'), 'utf8'));
const prefix = join(out, 'native'), plan = { schema: 'jet-native-lowering-discovery-plan/v1', sourceRoot: repo, source: unit.source, sourceSha256: unit.sourceSha256, entry: unit.entry, factoryRecord: resolve(recordFile), outputPrefix: prefix, pack: 'not requested', link: 'BLOCKED: deliberate discovery-only absence of the runtime pack', qualifiesFactory: false, qualifiesCli: false };
writeFileSync(join(out, 'plan.json'), JSON.stringify(plan, null, 2) + '\n');
if (mode === '--dry-run') { console.log(JSON.stringify({ ...plan, dryRun: true }, null, 2)); process.exit(0); }
const start = Date.now(); let receipt;
try {
  const observed = observeStageZeroNativeLowering({ recordFile: resolve(recordFile), repo, sourceRoot: unit.project, entry: 'selfhost.jet', work: join(out, 'factory'), policy: { JET_FRONT_NATIVE_OUTPUT: prefix } });
  const censusText = readFileSync(observed.census, 'utf8'), rows = censusText.trim().split('\n');
  if (rows[0] !== 'phase\titem\tcode\tmessage\tspan') throw Error('missing all-phases report header');
  const diagnostics = rows.slice(1).filter(Boolean).map(row => { const [phase, item, code, message, span] = row.split('\t'); return { phase, item, code, message, span }; });
  const backend = existsSync(prefix + '.issues') ? readFileSync(prefix + '.issues', 'utf8').split('\n').filter(Boolean) : [];
  const object = prefix + '.o', bytes = existsSync(object) ? readFileSync(object) : null;
  const objectValid = bytes?.subarray(0, 4).toString('hex') === '7f454c46' && bytes.readUInt16LE(16) === 1;
  const rawFactoryReceipt = readFileSync(observed.receipt, 'utf8');
  const clean = observed.rc === 0 && diagnostics.length === 0 && backend.length === 0 && objectValid;
  receipt = { schema: 'jet-native-lowering-discovery/v1', ...plan, commit: observed.record.commit, actualFactoryRc: observed.rc, actualFactoryReceipt: observed.receipt, rawFactoryReceipt, keepGoingReport: observed.census, diagnostics, backendFindings: backend, object: objectValid ? { path: object, sha256: createHash('sha256').update(bytes).digest('hex') } : null, seconds: (Date.now() - start) / 1000, frontendAndObjectStatus: clean ? 'PASS' : 'FAIL', qualifiesFactory: false, qualifiesCli: false, link: observed.link, rc: clean ? 0 : 1 };
} catch (error) { receipt = { schema: 'jet-native-lowering-discovery/v1', ...plan, seconds: (Date.now() - start) / 1000, rc: 1, error: error.stack, frontendAndObjectStatus: 'UNKNOWN OR FAILED', qualifiesFactory: false, qualifiesCli: false }; }
writeFileSync(join(out, 'receipt.json'), JSON.stringify(receipt, null, 2) + '\n');
writeFileSync(join(out, 'driver.rc'), String(receipt.rc) + '\n');
console.log(JSON.stringify(receipt, null, 2)); process.exitCode = receipt.rc;
