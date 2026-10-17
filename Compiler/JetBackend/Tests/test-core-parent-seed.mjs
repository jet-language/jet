#!/usr/bin/env node
// Light seed-only transport proof: execute the actual Jet text generator, then
// check the assembled aggregate's canonical source offsets. The Rust Native.rs
// build-splice witness is retired: it reports UNCOVERED and exits 2. No cargo,
// rustc, runtime library or native claim.
import {spawnSync} from 'node:child_process';
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {resolve, dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const out = process.argv[2];
if (!out || !process.env.JET) throw new Error('usage: JET=<trusted compiler> node test-core-parent-seed.mjs <disk-backed scratch dir>');
mkdirSync(out, {recursive:true});
const seedPath = join(root, 'Compiler/JetCodegen/Source/Emit/CoreParentSeed.jet');
const fixturePath = join(root, 'Compiler/JetBackend/Tests/CoreParentSeedFixtures.jet');
const unitPath = resolve(out, 'seed-fixture.jet');
writeFileSync(unitPath, readFileSync(seedPath, 'utf8') + '\n' + readFileSync(fixturePath, 'utf8'));
function run(command, args, env = process.env) {
  const result = spawnSync(command, args, {cwd:resolve(out), env, encoding:'utf8', maxBuffer:8*1024*1024});
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
const provenance = generated.stdout.split('\n').find(line => line.startsWith('Core parent private provenance: PASS;'));
if (!provenance) throw new Error('Jet fixture returned no private provenance receipt');
console.log(provenance);

// Run the real assembler in an isolated disk-backed HOME. The exact prefix
// must shift every mapped range while preserving original diagnostic locations.
const assemblyHome = resolve(out, 'assembly-home');
run(process.execPath, [join(root, 'Compiler/Bootstrap/assemble.mjs')], {
  ...process.env, HOME:assemblyHome, JET_BOOTSTRAP_SOURCE_ROOT:root,
});
const assemblyDir = join(assemblyHome, '.cache/jet-luna/compiler-bootstrap');
const aggregate = readFileSync(join(assemblyDir, 'project/src/compiler.jet'));
const sourceMap = JSON.parse(readFileSync(join(assemblyDir, 'compiler.map.json'), 'utf8'));
const header = "// [jet-bootstrap private host: Compiler/Bootstrap/Host/Entry.jet]\n";
assert.equal(aggregate.subarray(0, Buffer.byteLength(header)).toString('utf8'), header);
let byte = Buffer.byteLength(header);
let line = 2;
for (const row of sourceMap.files) {
  const marker = `// [jet-bootstrap source: ${row.path}]\n`;
  assert.equal(aggregate.subarray(byte, byte + Buffer.byteLength(marker)).toString('utf8'), marker);
  byte += Buffer.byteLength(marker);
  line += 1;
  assert.equal(row.generated_start_byte, byte);
  assert.equal(row.generated_start_line, line);
  const original = readFileSync(join(root, row.path));
  assert.equal(row.generated_end_byte_exclusive, byte + original.length);
  const newlines = original.toString('utf8').split('\n').length - 1;
  byte += original.length;
  line += newlines;
  if (original.at(-1) !== 10) { byte += 1; line += 1; }
}
assert.equal(byte, aggregate.length);
const diagnosticPath = 'Compiler/JetFoundation/Source/Diagnostics/Diagnostic.jet';
const diagnostic = sourceMap.source_segments.find(row => row.source_file === diagnosticPath);
assert.ok(diagnostic);
const token = Buffer.from('pub struct Span {');
const originalOffset = readFileSync(join(root, diagnosticPath)).indexOf(token);
const generatedOffset = diagnostic.generated.start + originalOffset;
assert.equal(aggregate.subarray(generatedOffset, generatedOffset + token.length).toString('utf8'), token.toString('utf8'));
assert.equal(diagnostic.line_starts[3], originalOffset);
console.log(`Core parent aggregate offsets: PASS; ${sourceMap.files.length} canonical ranges; mapped diagnostic ${diagnosticPath}:4:1 unchanged`);
console.log(JSON.stringify({status: 'UNCOVERED', cardRequired: true, reason: 'Core parent build-splice witness (Native.rs bootstrap_core_seed_splice compiled and run by rustc) retired (owner #38/#59; Tower #4923): ingress-present, missing-requested refusal and stage0-unchanged cases are not exercised; no rustc invoked'}));
process.exit(2);
