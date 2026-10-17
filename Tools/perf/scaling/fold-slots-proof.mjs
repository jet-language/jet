#!/usr/bin/env node
// Execute the actual Jet folding helpers from two revisions on identical text.
// Only namespace identifiers change; neither side is reimplemented in JavaScript.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const sourcePath = 'Compiler/JetCodegen/Source/Emit/Functions.jet';
const args = process.argv.slice(2);
const flags = Object.fromEntries(args.reduce((pairs, arg, i) => i % 2 ? pairs : [...pairs, [arg, args[i + 1]]], []));
if (!flags['--reference'] || !flags['--jet'] || !flags['--root']) throw new Error('Usage: fold-slots-proof.mjs --reference REV --jet BINARY --root FRESH_DIR [--rows 512] [--timeout 120]');
const root = path.resolve(flags['--root']);
if (fs.existsSync(root)) throw new Error(`Proof directory already exists: ${root}`);
fs.mkdirSync(root, { recursive: true });
const git = spawnSync('git', ['show', `${flags['--reference']}:${sourcePath}`], { cwd: repo, encoding: 'utf8', maxBuffer: 4 * 1024 * 1024 });
if (git.status !== 0) throw new Error(git.stderr);
const before = git.stdout;
const after = fs.readFileSync(path.join(repo, sourcePath), 'utf8');
function section(text, start, end) {
  const from = text.indexOf(start), to = text.indexOf(end, from);
  if (from < 0 || to < 0) throw new Error(`Missing production boundary: ${start} / ${end}`);
  return text.slice(from, to);
}
function helpers(text, prefix) {
  const selected = section(text, 'fn jet_rust_emit_identifier_byte(', '// The emitted function lines after slot localization') +
    section(text, 'fn jet_rust_emit_fold_strip_parens(', '// A Rust type whose values copy') +
    section(text, 'fn jet_rust_emit_bytes_at(', '// MIRRust unwrap_value_slots:');
  return selected.replaceAll('jet_rust_emit_', `${prefix}_`).replaceAll('JetRustEmit', `${prefix[0].toUpperCase()}${prefix.slice(1)}Type`);
}
const literal = text => JSON.stringify(text).replaceAll('{', '{{').replaceAll('}', '}}');
const int = n => `jet_foundation::Numeric::JetInt::from_i64(${n})`;
const definition = (id, payload = int(id)) => `    let mut __jet_v_${id}: Option<jet_foundation::Numeric::JetInt> = Some(${payload});`;
const read = (id, kind = 0) => [`(__jet_v_${id}.as_ref().expect("MIR value")).clone()`, `(*__jet_v_${id}.as_ref().expect("MIR value"))`, `__jet_v_${id}.take().expect("MIR value")`][kind];
const consume = expr => `    __jet_v_999999 = Some(${expr});`;
const cases = [];
const add = (name, lines, expected = null, bodyStart = 0) => cases.push({ name, lines, expected, bodyStart });
const rows = Number(flags['--rows'] ?? 512);
if (!Number.isSafeInteger(rows) || rows < 2 || rows > 4096) throw new Error('--rows must be 2..4096');
add('table', [...Array.from({ length: rows }, (_, i) => definition(i + 1)), consume(`vec![${Array.from({ length: rows }, (_, i) => read(i + 1, i % 3)).join(', ')}]`)], [consume(`vec![${Array.from({ length: rows }, (_, i) => `(${int(i + 1)})`).join(', ')}]`)]);
add('reverse-order', [definition(1), definition(2), definition(3), consume(`vec![${read(3)}, ${read(1, 1)}, ${read(2, 2)}]`)], [consume(`vec![(${int(3)}), (${int(1)}), (${int(2)})]`)]);
add('attributed', [definition(1), '    #[cfg(not(jet_release))]', consume(`vec![${read(1)}]`)]);
add('pure-boundary', [definition(1), definition(2, read(1)), consume(`vec![${read(2)}]`)]);
add('mixed-dependent', [definition(1), definition(2, '__jet_l_5.as_ref().expect("MIR local").clone()'), consume(`vec![${read(1)}, ${read(2)}, __jet_l_5]`)]);
add('mixed-eager-flush', [definition(1), definition(2, '__jet_l_5.as_ref().expect("MIR local").clone()'), consume(`vec![${read(1)}, ${read(2)}]`)]);
add('closure', [definition(1), consume(`make(|| ${read(1)})`)]);
add('quoted-read', [definition(1), consume(`vec!["${read(1)}"]`)]);
add('quoted-duplicate', [definition(1), consume(`vec![${read(1)}, "${read(1)}"]`)]);
add('joined-take', [definition(1), consume(`vec![${read(1, 2)}, joined${read(1, 2)}]`)]);
add('field-take', [definition(1), consume(`vec![object.${read(1, 2)}]`)]);
const stringPayload = '{ let mut __jet_string = String::new(); __jet_string.push_str("name"); __jet_string }';
add('inserted-identifiers', [definition(1, stringPayload), consume(`vec![${read(1)}]`)], [consume(`vec![(${stringPayload})]`)]);
const slotPayload = '{ let mut __jet_string = String::new(); __jet_string.push_str("__jet_v_99"); __jet_string }';
const slotLines = [definition(1, slotPayload), definition(99), consume(`vec![${read(1)}, ${read(99)}]`)];
add('literal-slot-spelling', slotLines, slotLines);
const pipePayload = '{ let mut __jet_string = String::new(); __jet_string.push_str("a|b\\\"c"); __jet_string }';
add('literal-pipe', [definition(1, pipePayload), consume(`vec![${read(1)}]`)], [consume(`vec![(${pipePayload})]`)]);
add('hoisted', ['    let mut __jet_v_1: Option<jet_foundation::Numeric::JetInt> = None;', `    __jet_v_1 = Some(${int(1)});`, consume(`vec![${read(1)}]`)], [consume(`vec![(${int(1)})]`)], 1);
add('barrier', [definition(1), '    side_effect();', consume(`vec![${read(1)}]`)]);
add('different-indent', [definition(1), `        __jet_v_999999 = Some(vec![${read(1)}]);`]);
const code = [helpers(before, 'before'), helpers(after, 'after'), 'fn run() {'];
for (const c of cases) {
  code.push(`    if true {`, `        input :: [String]{${c.lines.map(literal).join(', ')}}`,
    `        old :: before_fold_value_slots(input, 0, ${c.bodyStart}).join("\\n")`,
    `        new :: after_fold_value_slots(input, 0, ${c.bodyStart}).join("\\n")`,
    `        if old != new -> panic(${literal(`fold mismatch: ${c.name}`)})`);
  if (c.expected) code.push(`        if new != ${literal(c.expected.join('\n'))} -> panic(${literal(`unexpected fold: ${c.name}`)})`);
  code.push(`        print("case ${c.name}: {new}")`, '    }');
}
code.push('}', '');
const source = code.join('\n');
fs.writeFileSync(path.join(root, 'main.jet'), source);
fs.writeFileSync(path.join(root, 'inputs.json'), JSON.stringify({ reference: flags['--reference'], sourcePath, before_sha256: createHash('sha256').update(before).digest('hex'), after_sha256: createHash('sha256').update(after).digest('hex'), fixture_sha256: createHash('sha256').update(source).digest('hex'), rows, cases }, null, 2) + '\n');
const result = spawnSync(path.resolve(flags['--jet']), ['run', '--interpret', '--color=never', path.join(root, 'main.jet')], {
  cwd: root, env: { ...process.env, JET_STORE_DIR: path.join(root, 'store'), JET_RECEIPT_BYPASS: '1', JET_NO_SCCACHE: '1' }, encoding: 'utf8', timeout: Number(flags['--timeout'] ?? 120) * 1000, maxBuffer: 16 * 1024 * 1024,
});
fs.writeFileSync(path.join(root, 'stdout.log'), result.stdout ?? '');
fs.writeFileSync(path.join(root, 'stderr.log'), result.stderr ?? '');
fs.writeFileSync(path.join(root, 'result.json'), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message, cases: cases.length, rows, output_sha256: createHash('sha256').update(result.stdout ?? '').digest('hex'), protocol: 'actual Jet helpers; interpreted semantic differential, not performance evidence' }, null, 2) + '\n');
console.log(`Proof: ${root}; ${cases.length} cases, ${rows} elements, rc=${result.status}`);
process.exitCode = result.status ?? 2;
