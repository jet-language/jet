#!/usr/bin/env node
// Light seed-only transport proof: execute the actual Jet text generator twice,
// then exercise all three build-splice cases using its emitted text and the
// actual Native.rs splice helper. No cargo, runtime library or native claim.
import {spawnSync} from 'node:child_process';
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {resolve, dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const out = process.argv[2];
if (!out || !process.env.JET) throw new Error('usage: JET=<trusted compiler> node test-core-parent-seed.mjs <disk-backed scratch dir>');
mkdirSync(out, {recursive:true});
const seedPath = join(root, 'Compiler/JetCodegen/Source/Emit/CoreParentSeed.jet');
const fixturePath = join(root, 'Compiler/JetBackend/Tests/CoreParentSeedFixtures.jet');
const unitPath = resolve(out, 'seed-fixture.jet');
writeFileSync(unitPath, readFileSync(seedPath, 'utf8') + '\n' + readFileSync(fixturePath, 'utf8'));
function run(command, args) {
  const result = spawnSync(command, args, {cwd:resolve(out), encoding:'utf8', maxBuffer:8*1024*1024});
  if (result.error) throw result.error;
  if (result.status !== 0) {
    process.stdout.write(result.stdout ?? '');
    process.stderr.write(result.stderr ?? '');
    throw new Error(`${command}: actual exit ${result.status ?? result.signal}`);
  }
  return result;
}
const generated = run(process.env.JET, ['run', '--threads', '1', unitPath]);
const summary = generated.stdout.split('\n').find(line => line.startsWith('Core parent seed determinism: PASS;'));
if (!summary) throw new Error('Jet fixture returned no deterministic generator receipt');
const ingressPath = resolve(out, 'seed-ingress.txt');
writeFileSync(ingressPath, generated.stdout);
console.log(summary);

const nativePath = join(root, 'Compiler/Bootstrap/Host/Native.rs');
const native = readFileSync(nativePath, 'utf8');
const start = native.lastIndexOf('fn bootstrap_core_seed_splice(');
if (start < 0) throw new Error('actual seed splice helper is absent');
const helper = native.slice(start).trim();
const testSource = helper + `\nfn main() {
    let ingress = include_str!(${JSON.stringify(ingressPath)});
    let stage0 = include_str!(${JSON.stringify(seedPath)});
    assert_eq!(bootstrap_core_seed_splice(ingress, true), Ok("__jet_bootstrap_core_ingress(&__jet_request, __jet_runtime_admission)?;"));
    assert_eq!(bootstrap_core_seed_splice(stage0, true), Err("CoreSharedSeedIngressMissing"));
    assert_eq!(bootstrap_core_seed_splice(stage0, false), Ok(""));
    println!("Core parent seed splice: PASS; ingress present, missing-requested refusal, stage0 unchanged (3/3)");
}\n`;
const rustPath = resolve(out, 'splice-fixture.rs');
const executable = resolve(out, 'splice-fixture');
writeFileSync(rustPath, testSource);
run('rustc', ['--edition=2021', rustPath, '-o', executable]);
const cases = run(executable, []);
process.stdout.write(cases.stdout);
