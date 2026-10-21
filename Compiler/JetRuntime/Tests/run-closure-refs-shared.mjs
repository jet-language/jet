#!/usr/bin/env node
// Actual Jet Shared-held closure with generated clone/drop glue. No C stand-in
// closure, Rust runtime replacement, serial fallback, or simulated free count.
// usage: run-closure-refs-shared.mjs OUT assemble|lower|link|run [JET_ONLY_RUNTIME_OBJECT_OR_ARCHIVE]
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {existsSync, mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode, objectArg] = process.argv.slice(2);
if (!output || !['assemble', 'lower', 'link', 'run'].includes(mode)) throw Error('usage: run-closure-refs-shared.mjs OUT assemble|lower|link|run [JET_ONLY_RUNTIME_OBJECT_OR_ARCHIVE]');
const out = resolve(output), prefix = join(out, 'fixture'), binary = join(out, 'driver');
const hash = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const read = path => readFileSync(join(repo, path), 'utf8');
function command(name, args, options = {}) {
    const result = spawnSync(join(repo, 'Tools/agent/jet-env'), args, {cwd: repo, encoding: 'utf8', maxBuffer: 1 << 27, ...options});
    writeFileSync(join(out, name + '.stdout'), result.stdout ?? '');
    writeFileSync(join(out, name + '.stderr'), result.stderr ?? '');
    writeFileSync(join(out, name + '.status.json'), JSON.stringify({status: result.status, signal: result.signal, error: result.error?.message ?? null}) + '\n');
    if (result.error || result.signal || result.status !== 0) throw Error(`${name}: ${result.error?.message ?? result.signal ?? result.status}; see retained tool output`);
    return result;
}
if (mode === 'assemble') {
    mkdirSync(join(out, 'Source'), {recursive: true});
    writeFileSync(join(out, 'Source/Fixture.jet'), read('Compiler/JetRuntime/Tests/Fragments.fixture/ClosureRefsShared.jet'));
    writeFileSync(join(out, 'package.jet'), 'name: "closure_refs_shared"\nversion: "0.0.1"\nedition: "2028"\noutputs: { runtime: .Library{ name: "closure_refs_shared", native: true } }\n');
    writeFileSync(join(out, 'exports.list'), 'jet_closure_ref_test_run\n');
} else {
    if (!process.env.PQ_JOB || !/^[a-f0-9]{40}$/.test(process.env.COMPOSED_SHA ?? '')) throw Error('held proofq job and immutable composed source pin required');
    if (mode === 'lower') {
        if (!process.env.JET_FRONT_JETC0) throw Error('fresh composed Jet candidate required; no published/historical fallback');
        const candidate = resolve(process.env.JET_FRONT_JETC0);
        if (existsSync(prefix + '.o')) throw Error('refusing to overwrite lowering evidence');
        const { problem } = runNativeLibrary({ jet: candidate, file: join(out, 'Source/Fixture.jet'), prefix, exports: join(out, 'exports.list'), timeout: 600000, env: {...process.env, JET_TOOLCHAIN_ROOT: repo, JET_STORE_DIR: join(out, 'store')} });
        if (problem) throw Error(problem.detail);
        writeFileSync(join(out, 'lower.json'), JSON.stringify({sha: process.env.COMPOSED_SHA, candidate, candidateSha256: hash(candidate), sourceSha256: hash(join(out, 'Source/Fixture.jet')), objectSha256: hash(prefix + '.o')}, null, 2) + '\n');
    } else if (mode === 'link') {
        // This fixture contains only the closure witness, not an aggregate runtime.
        if (!objectArg) throw Error('fresh Jet-only runtime object/archive required (KEY/jet_runtime_jet.o, not jet_runtime.pack.o)');
        const object = resolve(objectArg);
        const lower = JSON.parse(readFileSync(join(out, 'lower.json'), 'utf8'));
        if (lower.sha !== process.env.COMPOSED_SHA || lower.objectSha256 !== hash(prefix + '.o')) throw Error('lowering pin changed');
        command('link', [nativeCCompiler, '-std=c11', '-O2', join(repo, 'Compiler/JetRuntime/Tests/closure-refs-shared.c'), prefix + '.o', object,
            '-Wl,--wrap=jet_rt_alloc', '-Wl,--wrap=jet_rt_free', '-lpthread', '-ldl', '-lm', '-o', binary]);
        writeFileSync(join(out, 'link.json'), JSON.stringify({sha: process.env.COMPOSED_SHA, lower, object, objectSha256: hash(object), driverSha256: hash(join(repo, 'Compiler/JetRuntime/Tests/closure-refs-shared.c')), binarySha256: hash(binary)}, null, 2) + '\n');
    } else {
        const linked = JSON.parse(readFileSync(join(out, 'link.json'), 'utf8'));
        if (linked.sha !== process.env.COMPOSED_SHA || linked.binarySha256 !== hash(binary)) throw Error('linked witness pin changed');
        const result = command('run', [binary], {timeout: 300000});
        const observed = JSON.parse(result.stdout);
        if (result.stderr !== '' || observed.checksum !== 54720000 || observed.workers !== 32 || observed.threads < 2 || observed.threads > 32 || observed.closure_allocs !== 1 || observed.closure_frees !== 1 || observed.closure_live !== 0) throw Error('wrong output or unbalanced closure allocator');
        const receipt = {sha: process.env.COMPOSED_SHA, linked, observed, verdict: 'PASS', allocationCoverage: 'actual 168B eighteen-I64-capture closure box, observed through linker-wrapped Jet alloc/free; not the #4754 strict native instrumentation receipt'};
        writeFileSync(join(out, 'result.json'), JSON.stringify(receipt, null, 2) + '\n');
        console.log(JSON.stringify(receipt));
    }
}
