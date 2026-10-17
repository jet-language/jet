#!/usr/bin/env node
// A test-only library object, not runtime telemetry or an exported pack API.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { appendFileSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0 } from '../../JetBackend/Tests/jet-front.mjs';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
import { parseRuntimeExports, runtimeLifecycleList } from '../../../Tools/agent/runtime-route-abi.mjs';
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
    renameSync(join(out, 'unit.jet'), join(out, 'Source/Fixture.jet'));
    // Instrument only this fixture's iterator body. The wrappers delegate the
    // real free/policy call exactly once, then stop via the real budget rail.
    const unit = join(out, 'Source/Fixture.jet');
    const assembled = readFileSync(unit, 'utf8');
    const iterStart = assembled.indexOf('// [unit source: Compiler/JetRuntime/Source/Iter.jet]');
    const iterEnd = assembled.indexOf('// [unit source:', iterStart + 1);
    if (iterStart < 0 || iterEnd < 0) throw Error('missing canonical iterator source boundary');
    const iter = assembled.slice(iterStart, iterEnd).replaceAll('jet_rt_free(', 'coll_stop_free(')
        .replace('= "jet_rt_progress_state_advance"', '= "coll_stop_progress_advance"');
    writeFileSync(unit, assembled.slice(0, iterStart) + iter + assembled.slice(iterEnd));
    appendFileSync(join(out, 'Source/Fixture.jet'), '\n' + readFileSync(join(repo, 'Compiler/JetRuntime/Tests/CollectionsStopTests.jet'), 'utf8'));
    writeFileSync(join(out, 'lifecycle.list'), runtimeLifecycleList(parseRuntimeExports(readFileSync(unit, 'utf8'), 'Source/Fixture.jet')));
} else if (mode === '--lower') {
    const prefix = join(out, 'fixture');
    const { problem } = runNativeLibrary({ jet: candidateJetc0(), file: join(out, 'Source/Fixture.jet'), prefix, lifecycle: join(out, 'lifecycle.list'), timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 120000), env: { ...process.env, JET_TOOLCHAIN_ROOT: repo, JET_STORE_DIR: join(out, 'store'), RUST_MIN_STACK: '8388608' } });
    if (problem) throw Error(problem.detail);
} else {
    const executable = join(out, 'caller');
    const build = run(nativeCCompiler, ['-Wall', '-Wextra', '-Werror', join(repo, 'Compiler/JetRuntime/Tests/collections-stop.c'), join(out, 'fixture.o'), '-lpthread', '-ldl', '-lm', '-o', executable]);
    writeFileSync(join(out, 'build.log'), `${build.stdout}${build.stderr}`);
    const result = run(executable, [], { timeout: 30000 });
    writeFileSync(join(out, 'run.stdout'), result.stdout);
    writeFileSync(join(out, 'run.stderr'), result.stderr);
    const expected = Array.from({ length: 13 }, (_, family) => `family ${family}: live payloads restored\n`).join('') +
        Array.from({ length: 11 }, (_, family) => `review ${family}: live payloads restored\n`).join('');
    if (result.stdout !== expected || result.stderr) throw Error('stopped-result witness output changed');
    console.log('13 stopped-result families + 11 focused review edges: allocator live totals restored');
}
