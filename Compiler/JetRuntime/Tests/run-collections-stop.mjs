#!/usr/bin/env node
// A test-only library object, not runtime telemetry or an exported pack API.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0, frontReceipt } from '../../JetBackend/Tests/jet-front.mjs';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['--assemble', '--lower', '--run'].includes(mode) || process.argv.length !== 4) {
    throw Error('usage: run-collections-stop.mjs OUT --assemble|--lower|--run');
}
const out = resolve(output);
mkdirSync(join(out, 'Source'), { recursive: true });
function run(bin, args, options = {}) {
    const result = spawnSync(bin, args, { cwd: repo, encoding: 'utf8', maxBuffer: 1 << 27, ...options });
    if (result.error || result.status !== 0) throw Error(`${bin}: rc=${result.status}, signal=${result.signal}: ${result.error?.message ?? ''}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
    return result;
}
if (mode === '--assemble') {
    run(process.execPath, [join(repo, 'Compiler/JetRuntime/Tests/check-runtime.mjs'), out, '--assemble-only', '--test', 'Collections']);
    writeFileSync(join(out, 'Source/Fixture.jet'), readFileSync(join(out, 'unit.jet'), 'utf8') + '\n' + readFileSync(join(repo, 'Compiler/JetRuntime/Tests/CollectionsStopTests.jet'), 'utf8'));
    writeFileSync(join(out, 'exports.list'), readFileSync(join(repo, 'Compiler/JetRuntime/exports.list'), 'utf8') + 'collections_stop_test\n');
} else if (mode === '--lower') {
    const receipt = join(out, 'front.receipt'), prefix = join(out, 'fixture');
    const result = spawnSync(candidateJetc0(), [], {
        cwd: repo, encoding: 'utf8', timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 120000), maxBuffer: 1 << 27,
        env: { ...process.env, JET_BOOTSTRAP_MODE: 'factory', JET_BOOTSTRAP_FACTORY_TIER: 'aot', JET_BOOTSTRAP_SOURCE_ROOT: out,
            JET_BOOTSTRAP_ENTRY: 'Source/Fixture.jet', JET_BOOTSTRAP_OUTPUT: join(out, 'unused.output'), JET_BOOTSTRAP_RECEIPT: receipt,
            JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix, JET_FRONT_LIBRARY_EXPORTS: join(out, 'exports.list'), JET_STORE_DIR: join(out, 'store'), RUST_MIN_STACK: '8388608' },
    });
    writeFileSync(join(out, 'front.log'), `${result.stdout ?? ''}${result.stderr ?? ''}`);
    const problem = frontReceipt(existsSync(receipt) ? readFileSync(receipt, 'utf8') : '', result);
    if (problem) throw Error(problem.detail);
    if (existsSync(prefix + '.issues')) throw Error(readFileSync(prefix + '.issues', 'utf8'));
    if (!existsSync(prefix + '.o')) throw Error('missing stopped-result fixture object');
} else {
    const library = process.env.JET_RUNTIME_C_LIB;
    if (!library || !existsSync(library)) throw Error('JET_RUNTIME_C_LIB must name the freshly built remaining native staticlib');
    const executable = join(out, 'caller');
    const build = run(process.env.CC ?? 'cc', ['-Wall', '-Wextra', '-Werror', join(repo, 'Compiler/JetRuntime/Tests/collections-stop.c'), join(out, 'fixture.o'), library, '-lpthread', '-ldl', '-lm', '-o', executable]);
    writeFileSync(join(out, 'build.log'), `${build.stdout}${build.stderr}`);
    const result = run(executable, [], { timeout: 30000 });
    writeFileSync(join(out, 'run.stdout'), result.stdout);
    writeFileSync(join(out, 'run.stderr'), result.stderr);
    const expected = Array.from({ length: 13 }, (_, family) => `family ${family}: live payloads restored\n`).join('');
    if (result.stdout !== expected || result.stderr) throw Error('stopped-result witness output changed');
    console.log('13 stopped-result families: allocator live totals restored');
}
