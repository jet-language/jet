#!/usr/bin/env node
// Matched runtime-object cell: 1M 64B alloc/write/checksum/free operations.
// One identical C driver, Jet runtime O0/O1 and Rust std::alloc, seven rounds.
// Main owns performance admission; this reports numbers without a threshold.
// usage: allocator.mjs OUTPUT O0_JET_OBJECT O1_JET_OBJECT REMAINING_NATIVE_STATICLIB CPU
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, writeFileSync, appendFileSync, existsSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const [outArg, o0Arg, o1Arg, remainingArg, cpu] = process.argv.slice(2);
if (!cpu || !/^\d+(?:[-,]\d+)*$/.test(cpu)) throw Error('usage: allocator.mjs OUTPUT O0_JET_OBJECT O1_JET_OBJECT REMAINING_NATIVE_STATICLIB CPU');
for (const key of ['PQ_JOB', 'FINAL_E2_SHA', 'COMPOSED_SHA']) if (!process.env[key]) throw Error(`missing ${key}; run in the held O1 runtime epoch measure job`);
const source = dirname(fileURLToPath(import.meta.url));
const tree = resolve(source, '../../..');
const out = resolve(outArg), o0 = resolve(o0Arg), o1 = resolve(o1Arg), remaining = resolve(remainingArg);
if (existsSync(join(out, 'result.json')) || existsSync(join(out, 'samples.jsonl'))) throw Error('refusing to overwrite cell evidence');
mkdirSync(out, {recursive: true});
const hash = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const pins = {cell: 'runtime-allocator-64B', iterations: 1000000, words: 8, align: 8, cpu, finalE2: process.env.FINAL_E2_SHA, composed: process.env.COMPOSED_SHA,
    sourceHashes: {driver: hash(join(source, 'allocator.c')), rust: hash(join(source, 'allocator.rs'))},
    inputs: Object.fromEntries(Object.entries({o0, o1, remaining}).map(([arm, file]) => [arm, {file, sha256: hash(file)}]))};
const steps = [];
function run(name, binary, args) {
    const result = spawnSync(binary, args, {cwd: tree, encoding: 'utf8', maxBuffer: 1 << 24});
    writeFileSync(join(out, name + '.log'), `${result.stdout ?? ''}${result.stderr ?? ''}`);
    steps.push({name, rc: result.status, signal: result.signal, error: result.error?.message});
    return result.status === 0 && result.signal === null && !result.error;
}
let ready = true;
ready = run('o0-exports', 'Tools/agent/jet-env', ['jet', 'run', 'Tools/packaging/CheckRuntimeExports.jet', '--', o0, remaining]) && ready;
ready = run('o1-exports', 'Tools/agent/jet-env', ['jet', 'run', 'Tools/packaging/CheckRuntimeExports.jet', '--', o1, remaining]) && ready;
const driver = join(out, 'allocator-driver.o'), rust = join(out, 'allocator-rust.a');
ready = run('driver', 'cc', ['-std=c11', '-O2', '-fno-lto', '-c', join(source, 'allocator.c'), '-o', driver]) && ready;
ready = run('rust', 'Tools/agent/jet-env', ['rustc', '--edition=2024', '-C', 'opt-level=3', '--crate-type=staticlib', join(source, 'allocator.rs'), '-o', rust]) && ready;
const binaries = Object.fromEntries(['O0', 'O1', 'Rust'].map(arm => [arm, join(out, arm)]));
if (ready) {
    for (const [arm, libraries] of Object.entries({O0: [o0, remaining], O1: [o1, remaining], Rust: [rust]})) {
        ready = run('link-' + arm, 'cc', ['-fno-lto', driver, ...libraries, '-lpthread', '-ldl', '-lm', '-o', binaries[arm]]) && ready;
    }
}
writeFileSync(join(out, 'build.json'), JSON.stringify({pins, steps}, null, 2) + '\n');
if (!ready) process.exit(1);
const expected = '4000024000000\n';
const arms = Object.fromEntries(Object.entries(binaries).map(([arm, binary]) => [arm, {binary, sha256: hash(binary), verdict: 'PASS', samples: []}]));
for (let round = 0; round < 7; round++) {
    const order = ['O0', 'O1', 'Rust'];
    // Rotate arm order so startup/cache position is not owned by one arm.
    for (const arm of [...order.slice(round % 3), ...order.slice(0, round % 3)]) {
        const entry = arms[arm];
        const started = process.hrtime.bigint();
        const result = spawnSync('taskset', ['-c', cpu, entry.binary], {encoding: 'utf8', timeout: 300000, maxBuffer: 1 << 20});
        const elapsed = process.hrtime.bigint() - started;
        const ok = result.status === 0 && result.signal === null && result.stdout === expected && result.stderr === '';
        appendFileSync(join(out, 'samples.jsonl'), JSON.stringify({arm, round: round + 1, cpu, wallNs: elapsed.toString(), rc: result.status, signal: result.signal, stdout: result.stdout, stderr: result.stderr, error: result.error?.message, verdict: ok ? 'PASS' : 'WRONG'}) + '\n');
        if (ok) entry.samples.push(Number(elapsed) / 1e6);
        else entry.verdict = 'WRONG';
    }
}
for (const entry of Object.values(arms)) {
    entry.samples.sort((a, b) => a - b);
    if (entry.verdict === 'PASS' && entry.samples.length === 7) {
        entry.medianMs = entry.samples[3]; entry.minMs = entry.samples[0]; entry.maxMs = entry.samples[6];
    }
}
const ratios = {};
for (const arm of ['O0', 'O1']) if (arms[arm].medianMs !== undefined && arms.Rust.medianMs !== undefined) ratios[arm + '/Rust'] = arms[arm].medianMs / arms.Rust.medianMs;
const result = {pins, expected, arms, ratios, performanceAdmission: 'Main-owned; no threshold applied', verdict: Object.values(arms).every(arm => arm.verdict === 'PASS' && arm.samples.length === 7) ? 'CORRECT' : 'WRONG'};
writeFileSync(join(out, 'result.json'), JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result, null, 2));
process.exit(result.verdict === 'CORRECT' ? 0 : 1);
