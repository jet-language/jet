#!/usr/bin/env node
// Execute real C clients linked to the Jet object; the direct jet_panic call
// is the process-mode golden oracle, not a replacement runtime provider.
import { mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';
import { trapLiteral as literal } from './report-literals.mjs';
const [output, tree, event, task] = process.argv.slice(2);
if (!output || !tree || !event || !task || process.argv.length !== 6) {
    throw Error('usage: run-record-variant-panic.mjs OUT DATA_TREE_WITNESS DATA_EVENT_WITNESS TASK_VARIANT_PANIC_WITNESS');
}
const out = resolve(output);
mkdirSync(out, { recursive: true });
const rows = [];
for (const [name, binary] of [['tree', tree], ['event', event]]) {
    const results = new Map();
    for (const mode of ['panic-reference', 'wrong-variant']) {
        const result = spawnSync(resolve(binary), [mode], { timeout: 30000, maxBuffer: 1 << 20 });
        writeFileSync(join(out, `${name}.${mode}.stdout`), result.stdout ?? Buffer.alloc(0));
        writeFileSync(join(out, `${name}.${mode}.stderr`), result.stderr ?? Buffer.alloc(0));
        writeFileSync(join(out, `${name}.${mode}.status.json`), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
        if (result.error || result.signal !== null || result.status !== 70 || result.stdout.length !== 0) {
            throw Error(`${name}/${mode}: expected exit 70, no signal/error/stdout; got ${result.status}/${result.signal}/${result.error?.message ?? ''}`);
        }
        results.set(mode, result.stderr);
    }
    const golden = results.get('panic-reference');
    if (!golden.includes(Buffer.from('record variant index out of range')) || !golden.includes(Buffer.from('<core.prelude>:0'))) {
        throw Error(`${name}: jet_panic reference lost message/location`);
    }
    if (!results.get('wrong-variant').equals(golden)) throw Error(`${name}: wrong-variant stderr differs from jet_panic golden`);
    rows.push(`${name}\tpass\texit=70\tstderr-byte-identical=true`);
}
const taskRun = spawnSync(resolve(task), [], { timeout: 30000, maxBuffer: 1 << 20 });
const survived = 'task variant/panic/numeric/checked-ops joined Panicked(reason); process survived\n';
writeFileSync(join(out, 'task.stdout'), taskRun.stdout ?? Buffer.alloc(0));
writeFileSync(join(out, 'task.stderr'), taskRun.stderr ?? Buffer.alloc(0));
writeFileSync(join(out, 'task.status.json'), JSON.stringify({ status: taskRun.status, signal: taskRun.signal, error: taskRun.error?.message ?? null }) + '\n');
if (taskRun.error || taskRun.signal !== null || taskRun.status !== 0 ||
    taskRun.stdout?.toString() !== survived || taskRun.stderr?.length !== 0) {
    throw Error(`task witness: expected survival and no stderr; got ${taskRun.status}/${taskRun.signal}/${taskRun.error?.message ?? ''}`);
}
const rootResults = [];
for (const mode of ['root-panic', 'root-variant']) {
    const result = spawnSync(resolve(task), [mode], { timeout: 30000, maxBuffer: 1 << 20 });
    writeFileSync(join(out, `task.${mode}.stderr`), result.stderr ?? Buffer.alloc(0));
    writeFileSync(join(out, `task.${mode}.status.json`), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
    if (result.error || result.signal !== null || result.status !== 70 || result.stdout?.length !== 0) {
        throw Error(`task/${mode}: expected unchanged root exit 70 with no stdout`);
    }
    rootResults.push(result.stderr);
}
if (!rootResults[0].equals(rootResults[1])) throw Error('task witness root wrong-variant stderr differs from jet_panic');
// Frozen generated report bytes are the pre-change root golden, not another
// provider. This checks the full header/location/footer, not a substring.
const header = literal('trap_msg_header_numeric'), footer = literal('trap_footer_e3010');
const prelude = literal('trap_msg_prelude_end');
function golden(message) { return Buffer.concat([header, Buffer.from(message), prelude, footer]); }
const overflow = literal('trap_msg_overflow');
const range = literal('trap_msg_native_index_range');
const rootModes = [
    ['root-overflow-export', golden(overflow)], ['root-divide-export', golden(literal('trap_msg_divide_zero'))],
    ['root-int-range', golden(range)],
    ['root-mode-3', golden("the list has 1 items, so position 1 doesn't exist")],
    ['root-mode-4', golden("the list has 9223372036854775807 items, so position -9223372036854775808 doesn't exist")],
    ['root-mode-32', golden("the list has -9223372036854775808 items, so position 9223372036854775807 doesn't exist")],
    ['root-mode-26', golden("the list has 0 items, so position 0 doesn't exist")],
    ['root-mode-27', golden("the list has 0 items, so position 0 doesn't exist")],
    ...Array.from({ length: 13 }, (_, i) => [`root-mode-${i + 5}`, golden(range)]),
    ...Array.from({ length: 8 }, (_, i) => [`root-mode-${i + 18}`, golden(overflow)]),
    ['root-mode-30', golden(overflow)], ['root-mode-31', golden(overflow)],
    ...Array.from({ length: 115 }, (_, test) => {
        const reason = test <= 5 ? 'divided by zero' :
            test === 6 ? 'Negative default Int exponent' :
            test <= 10 ? 'Invalid shift count' : "Value doesn't fit in destination type";
        const expected = test <= 13 ?
            Buffer.concat([header, Buffer.from(reason), literal('trap_msg_message_end'), footer]) :
            golden(test === 15 ? overflow : range);
        return [`root-mode-${200 + test}`, expected];
    }),
];
for (const [mode, expected] of rootModes) {
    const result = spawnSync(resolve(task), [mode], { timeout: 30000, maxBuffer: 1 << 20 });
    writeFileSync(join(out, `task.${mode}.stderr`), result.stderr ?? Buffer.alloc(0));
    writeFileSync(join(out, `task.${mode}.status.json`), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
    if (result.error || result.signal !== null || result.status !== 70 || result.stdout?.length !== 0 ||
        !result.stderr?.equals(expected)) throw Error(`${mode}: changed pre-cutover root report/exit`);
    rows.push(`${mode}\tpass\texit=70\tstderr-byte-identical=true`);
}
for (let kind = 1; kind <= 5; ++kind) {
    const mode = `double-${kind}`;
    const result = spawnSync(resolve(task), [mode], { timeout: 30000, maxBuffer: 1 << 20 });
    writeFileSync(join(out, `task.${mode}.stderr`), result.stderr ?? Buffer.alloc(0));
    writeFileSync(join(out, `task.${mode}.status.json`), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
    if (result.error || result.signal !== 'SIGABRT' || result.stdout?.length !== 0 ||
        result.stderr?.toString() !== 'panic in a destructor during cleanup\n') {
        throw Error(`${mode}: expected seed cleanup-panic message and SIGABRT`);
    }
    rows.push(`${mode}\tpass\tsignal=SIGABRT\tseed=Scheduler.rs:234-244,1022-1024`);
}
// Seed-host behavior is not the registered Jet program stop contract.
for (const [site, seed, defect] of [
    ['bit-set-len', 'Core/Collections.rs:151', 'raw Rust add wraps or host-panics by profile'],
    ['counter-add', 'Core/Collections.rs:3341', 'raw Rust add wraps or host-panics by profile'],
    ['counter-set-count', 'Core/Collections.rs:3382', 'raw Rust subtraction wraps or host-panics by profile'],
    ['counter-total', 'Core/Collections.rs:3386', 'Rust sum wraps or host-panics by profile'],
    ['counter-elements', 'Core/Collections.rs:3393-3399', 'seed clones incrementally without checked aggregate length'],
    ['counter-subtract', 'Core/Collections.rs:3437', 'raw Rust negation wraps or host-panics by profile'],
    ['chain-index', 'Core/Collections.rs:3548', 'raw Vec indexing host-panics rather than a registered Jet stop'],
]) rows.push(`${site}\tSEED-DEFECT\t${seed}\t${defect}; native E3010 root unchanged`);
rows.push('task\tpass\tjoined=Panicked(reason)\tprocess-survived=true\troot-exit=70\troot-stderr-byte-identical=true');
writeFileSync(join(out, 'record-variant-results.tsv'), rows.join('\n') + '\n');
console.log('Dyn/DataEvent/checked-op roots: exit 70 and byte-identical stderr; tasks: joined Panicked(reason), process survives; double panic: SIGABRT');
