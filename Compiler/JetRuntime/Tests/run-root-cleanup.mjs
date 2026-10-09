#!/usr/bin/env node
// Real Root ABI/lifetime witness only; MIR lexical Close has its own fixture.
// An optional single mode is a rehearsal, never a full-suite receipt.
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const [output, binary, selected, ...extra] = process.argv.slice(2);
const success = ['normal', 'exit0', 'exact-report', 'nested', 'fifo', 'bare-cancel', 'task-origin', 'central-drop', 'deadline', 'budget3', 'budget4', 'raw-publish', 'panic-message'];
const host = ['host-context-operation', 'host-error-context', 'host-limit-tag', 'host-limit-range'];
success.push(...host);
success.push('range-zero-step', 'range-exhausted');
const defects = ['root-cancel-publisher', 'root-interrupt-producer', 'corrupt-root-poll', 'corrupt-root-take'];
const aborts = ['double-panic-shield', 'nested-inherited-stop'];
const cold = ['cold-gs0-unreachable', 'cold-gs0-layout', 'cold-probe-error', 'cold-set-error'];
const modes = [...success, ...defects, ...aborts, ...cold];
if (!output || !binary || extra.length || (selected && !modes.includes(selected))) throw Error('usage: run-root-cleanup.mjs OUT ROOT_CLEANUP_WITNESS [MODE]');
const out = resolve(output);
mkdirSync(out, { recursive: true });
const literals = readFileSync(join(dirname(fileURLToPath(import.meta.url)), '../Source/TrapMessages.jet'), 'utf8');
function literal(name) {
  const body = literals.match(new RegExp(`^fn ${name}\\(\\) \\{\\n([\\s\\S]*?)^\\}`, 'm'))?.[1];
  if (!body) throw Error(`missing frozen report literal ${name}`);
  const rows = [...body.matchAll(/trap_write_word\(I64\{(-?\d+)\}, I64\{(\d+)\}\)/g)];
  if (!rows.length || body.replace(/\s*trap_write_word\(I64\{-?\d+\}, I64\{\d+\}\)\s*/g, '').trim()) throw Error(`unsupported frozen report literal ${name}`);
  return Buffer.concat(rows.map(([, value, length]) => {
    const word = Buffer.alloc(8);
    word.writeBigInt64LE(BigInt(value));
    return word.subarray(0, Number(length));
  }));
}
const panic = Buffer.concat([literal('trap_msg_header_panic'), Buffer.from('root-reason'), literal('trap_msg_prelude_end'), literal('trap_footer_e3001')]);
const hostReport = Buffer.concat([literal('trap_msg_header_panic'), Buffer.from('record variant index out of range'), literal('trap_msg_prelude_end'), literal('trap_footer_e3001')]);
const defect = Buffer.concat([literal('trap_msg_unreachable'), literal('trap_msg_newline')]);
const layout = Buffer.concat([literal('trap_invalid_layout_head'), literal('trap_footer_e3001')]);
const ok = Buffer.from('ROOT CLEANUP OK\n');
const records = [];
for (const mode of selected ? [selected] : modes) {
  // Descriptor3 is the actual comptime channel, distinct from stdout/stderr.
  const result = spawnSync(resolve(binary), [mode], { timeout: 30000, maxBuffer: 1 << 20, stdio: ['ignore', 'pipe', 'pipe', 'pipe'] });
  const stdout = result.stdout ?? Buffer.alloc(0), stderr = result.stderr ?? Buffer.alloc(0), channel = result.output?.[3] ?? Buffer.alloc(0);
  for (const [name, bytes] of [['stdout', stdout], ['stderr', stderr], ['channel3', channel]]) writeFileSync(join(out, `${mode}.${name}`), bytes);
  const record = { mode, status: result.status, signal: result.signal, error: result.error?.message ?? null, stdoutBytes: stdout.length, stderrBytes: stderr.length, channelBytes: channel.length };
  writeFileSync(join(out, `${mode}.status.json`), JSON.stringify(record) + '\n');
  if (result.error) throw Error(`${mode}: ${result.error.message}`);
  if (cold.includes(mode)) {
    const expectedCode = mode === 'cold-gs0-layout' ? 70 : 101;
    const expectedOutput = Buffer.from('cold-buffered');
    const expectedReport = mode === 'cold-gs0-layout' ? layout : defect;
    if (result.status !== expectedCode || result.signal !== null || !stdout.equals(expectedOutput) || !stderr.equals(expectedReport)) throw Error(`${mode}: cold base/error/report/flush contract differs`);
  } else if (defects.includes(mode)) {
    if (result.status !== 101 || result.signal !== null || stdout.length || !stderr.equals(defect)) throw Error(`${mode}: expected exact host defect101, not a Root report/success`);
  } else if (aborts.includes(mode)) {
    if (result.signal !== 'SIGABRT' || stdout.length || !stderr.equals(Buffer.from('panic in a destructor during cleanup\n'))) throw Error(`${mode}: expected unmasked cleanup double panic and SIGABRT`);
  } else {
    const report = host.includes(mode) ? hostReport : mode === 'raw-publish' || mode === 'panic-message' ? panic : null;
    const expected = report ? Buffer.concat([report, ok]) : ok;
    if (result.status !== 0 || result.signal !== null || stderr.length || !stdout.equals(expected)) throw Error(`${mode}: Root witness output/status differs`);
  }
  if (mode === 'budget3' || mode === 'budget4') {
    const status = mode === 'budget3' ? 2 : 3, kind = status === 2 ? 'BudgetFuel' : 'BudgetDepth';
    const body = Buffer.from(`{"kind":"${kind}","jct_status":${status},"owner":18446744073709551615,"start":9223372036854775808,"end":9223372036854775807,"message":""}`);
    const expected = Buffer.concat([Buffer.from(`${body.length}\n`), body]);
    if (!channel.equals(expected)) throw Error(`${mode}: actual kind/metadata/private frame bytes differ`);
  } else if (channel.length) throw Error(`${mode}: unexpected private comptime frame`);
  records.push(record);
}
writeFileSync(join(out, 'root-cleanup-results.json'), JSON.stringify({ scope: selected ? 'single-mode-rehearsal' : 'full-ABI-lifetime-suite', binary: resolve(binary), modes: records }, null, 2) + '\n');
console.log(`${selected ? 'Root rehearsal' : 'Root ABI/lifetime suite'}: ${records.length} passed; no MIR Close claim`);
