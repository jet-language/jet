#!/usr/bin/env node
// Execute real C clients linked to the Jet object; the direct jet_panic call
// is the process-mode golden oracle, not a replacement runtime provider.
import { mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';
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
const survived = 'task variant/panic/numeric joined Panicked(reason); process survived\n';
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
rows.push('task\tpass\tjoined=Panicked(reason)\tprocess-survived=true\troot-exit=70\troot-stderr-byte-identical=true');
writeFileSync(join(out, 'record-variant-results.tsv'), rows.join('\n') + '\n');
console.log('DataTree/DataEvent roots: exit 70 and byte-identical stderr; task variant/panic/numeric: joined Panicked(reason) and process survives');
