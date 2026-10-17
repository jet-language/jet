#!/usr/bin/env node
// Jet source-body comparison against saved formatter oracle bytes.
// Preparation preserves every case without invoking a live Rust reference.
// This is not native selector lowering or runtime-pack admission proof.
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [mode, output, checker, frozenExpected] = process.argv.slice(2);
if (mode === 'reference') {
  console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
  process.exit(0);
}
if (!['prepare', 'compare'].includes(mode) || !output || (mode === 'compare' && !checker))
  throw Error('usage: run-fmt-parity.mjs prepare OUT [FROZEN_EXPECTED] | compare OUT CHECKER [FROZEN_EXPECTED]');
const out = resolve(output);
mkdirSync(out, { recursive: true });
const i = value => ({ kind: 'int', value: String(value) });
const f = value => ({ kind: 'float', value: String(value) });
const s = value => ({ kind: 'text', value });
const cases = [];
const add = (kernel, label, ...args) => cases.push({ id: `${kernel}-${label}`, kernel, args });
const limits = ['-9223372036854775808', '-1001', '-1', '0', '1', '1001', '9223372036854775807'];
for (const kernel of ['bin', 'oct', 'number', 'ordinal', 'bytes', 'duration'])
  limits.forEach((value, at) => add(kernel, `integer${at}`, i(value)));
for (const value of limits) for (const width of [-1, 0, 4, 24]) add('hex', `${value}-width${width}`, i(value), i(width));
for (const kernel of ['decimal_int', 'grouped_int'])
  for (const value of limits) for (const precision of [-1, 0, 3]) add(kernel, `${value}-precision${precision}`, i(value), i(precision));
for (const kernel of ['bin_decimal', 'oct_decimal', 'hex_decimal']) {
  for (const [at, value] of ['0', '-0', '+0000', '-9223372036854775808', '9223372036854775807', '184467440737095516160000', '  -00123  ', '', '+', '-x', '12.5', '１２', '1_000'].entries()) {
    const args = [s(value)];
    if (kernel === 'hex_decimal') args.push(i(8));
    add(kernel, `spelling${at}`, ...args);
  }
}
for (const kernel of ['decimal', 'grouped', 'percent', 'sci'])
  for (const [at, value] of ['-12345.625', '-2.5', '-0.0', '0.0', '0.125', '2.5', '999.999', '12345.625'].entries())
    for (const precision of [-1, 0, 2]) add(kernel, `float${at}-precision${precision}`, f(value), i(precision));
for (const [at, value] of ['-10000000000000000.0', '-0.0', '0.0', '1.0', '1.25', '0.00001', '10000000000000000.0'].entries()) add('quantity', `float${at}`, f(value));
for (const [at, value] of limits.entries()) add('plural', `integer${at}`, i(value), s('café 🐈'), s('cafés 🐈🐈'));
for (const kernel of ['pad', 'pad_left', 'pad_right', 'pad_center']) {
  for (const [at, text, fill] of [[0, '', ''], [1, 'ab', '.'], [2, 'é🐈', '界🙂'], [3, 'e\u0301', 'ab'], [4, 'quoted "x"', '']])
    for (const width of [-1, 0, 1, 5, 12]) add(kernel, `text${at}-width${width}`, s(text), i(width), s(fill));
}
for (const [at, text] of ['0', '  scalar  ', '[]', '[:]', '[redacted]', '[1, 2, 3]', 'Point {x: 1, y: [2, 3]}', 'Ok(Point {x: "{[,]}", y: (1, 2)})', '"quoted [1, 2]"', '[1, 2', '[1] trailing'].entries()) add('pretty', `structure${at}`, s(text));
const members = { decimal_int: 'fixed_int', grouped_int: 'grouped_int_fixed' };
const registered = Object.keys({ bin: 1, bin_decimal: 1, bytes: 1, decimal: 1, decimal_int: 1, duration: 1, grouped: 1, grouped_int: 1, hex: 1, hex_decimal: 1, number: 1, oct: 1, oct_decimal: 1, ordinal: 1, pad: 1, pad_center: 1, pad_left: 1, pad_right: 1, percent: 1, plural: 1, pretty: 1, quantity: 1, sci: 1 });
const kernels = [...new Set(cases.map(row => row.kernel))].sort();
if (registered.length !== 23 || registered.some(kernel => !kernels.includes(kernel)) || kernels.some(kernel => !registered.includes(kernel))) throw Error('parity corpus does not cover the canonical 23 formatter routes');
if (new Set(cases.map(row => row.id)).size !== cases.length) throw Error('duplicate case id');
const quote = text => JSON.stringify(text);
const jetArg = arg => arg.kind === 'text' ? quote(arg.value.replace(/\{/g, '{{').replace(/\}/g, '}}')) : arg.value;
const jet = `use fmt_source as fmt\nfn run() {\n${cases.map(row => `    print("CASE ${row.id}")\n    print(fmt.${members[row.kernel] ?? row.kernel}(${row.args.map(jetArg).join(', ')}))\n    print("END ${row.id}")`).join('\n')}\n}\n`;
function run(binary, args, name, cwd = out) {
  const result = spawnSync(binary, args, { cwd, encoding: 'utf8', maxBuffer: 1 << 27, timeout: 0 });
  writeFileSync(join(out, `${name}.stdout`), result.stdout ?? '');
  writeFileSync(join(out, `${name}.stderr`), `${result.stderr ?? ''}${result.error ? `\n${result.error.message}\n` : ''}`);
  if (result.status !== 0) throw Error(`${name} exited ${result.status} signal=${result.signal}; see ${name}.stderr`);
  return result.stdout;
}
if (mode === 'prepare') {
  writeFileSync(join(out, 'cases.json'), JSON.stringify(cases, null, 2) + '\n');
  writeFileSync(join(out, 'source.jet'), jet);
  writeFileSync(join(out, 'fmt_source.jet'), readFileSync(join(root, 'Core/text/fmt.jet')));
  writeFileSync(join(out, 'package.jet'), 'name: "fmt_source_parity"\nversion: "0.0.1"\nedition: "2028"\n');
  console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
  if (checker) writeFileSync(join(out, 'expected.out'), readFileSync(resolve(checker)));
  console.log(`fmt parity prepared: kernels=${kernels.length} cases=${cases.length}; expected bytes must be supplied frozen; source/native comparison NOT RUN`);
} else {
  // Refresh the source body after Core parity fixes without regenerating or
  // weakening the saved Rust oracle bytes.
  writeFileSync(join(out, 'fmt_source.jet'), readFileSync(join(root, 'Core/text/fmt.jet')));
  writeFileSync(join(out, 'source.jet'), jet);
  const checkerPath = realpathSync(resolve(checker));
  const checkerSha256 = createHash('sha256').update(readFileSync(checkerPath)).digest('hex');
  const actual = run(resolve(checker), ['run', '--release', join(out, 'source.jet')], 'source-run');
  const expected = readFileSync(frozenExpected ? resolve(frozenExpected) : join(out, 'expected.out'), 'utf8');
  function blocks(text) {
    const result = new Map();
    for (const row of cases) {
      const begin = `CASE ${row.id}\n`, end = `END ${row.id}\n`;
      const start = text.indexOf(begin), stop = start < 0 ? -1 : text.indexOf(end, start + begin.length);
      if (start < 0 || stop < 0 || text.indexOf(begin, start + begin.length) >= 0) throw Error(`missing/duplicate framed output ${row.id}`);
      result.set(row.id, text.slice(start + begin.length, stop));
    }
    return result;
  }
  const wanted = blocks(expected), got = blocks(actual);
  const sourceLines = readFileSync(join(out, 'fmt_source.jet'), 'utf8').split('\n');
  const rows = cases.map(row => ({ id: row.id, kernel: row.kernel, input: row.args,
    source: `Core/text/fmt.jet:${sourceLines.findIndex(line => new RegExp(`^(?:pub )?fn ${members[row.kernel] ?? row.kernel}\\(`).test(line)) + 1}`,
    byte_equal: wanted.get(row.id) === got.get(row.id), expected: wanted.get(row.id), actual: got.get(row.id) }));
  const failures = rows.filter(row => !row.byte_equal);
  writeFileSync(join(out, 'parity.json'), JSON.stringify({ label: 'Rust-seed source-body byte parity; NOT native selector migration parity', checker: { path: checkerPath, sha256: checkerSha256 }, expected_sha256: createHash('sha256').update(expected).digest('hex'), kernels: kernels.length, cases: cases.length, failures: failures.length, stdout_equal: actual === expected, rows }, null, 2) + '\n');
  console.log(`fmt source-body parity: kernels=${kernels.length} cases=${cases.length} failures=${failures.length} stdout_equal=${actual === expected}; native parity UNPROVEN`);
  process.exitCode = failures.length === 0 && actual === expected ? 0 : 1;
}
