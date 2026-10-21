#!/usr/bin/env node
// E2: real source .Global exports and allocator startup in executable/library.
// OUT --assemble only prepares units. --lower uses the composed Jet candidate;
// --global/--initializer/--allocator execute separately so a proof keeps going.
// Aggregate init/fixture.o supplies the Jet-only runtime; no additional pack.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0 } from '../../JetBackend/Tests/jet-front.mjs';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
import { parseRuntimeExports, runtimeLifecycleList } from '../../../Tools/agent/runtime-route-abi.mjs';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['--assemble', '--lower', '--global', '--initializer', '--allocator'].includes(mode)) {
    throw Error('usage: run-object-fixtures.mjs OUT --assemble|--lower|--global|--initializer|--allocator');
}
const out = resolve(output);
const read = path => readFileSync(join(repo, path), 'utf8');
function run(bin, args, options = {}) {
    const result = spawnSync(bin, args, { encoding: 'utf8', maxBuffer: 1 << 27, ...options });
    if (result.error || result.status !== 0) throw Error(`${bin} exit ${result.status ?? result.signal}: ${result.error?.message ?? ''}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
    return result;
}
if (mode === '--assemble') {
    for (const name of ['global', 'init']) mkdirSync(join(out, name, 'Source'), { recursive: true });
    const global = read('tests/fixtures/native_global_word.jet');
    const entry = global.indexOf('fn run()');
    if (entry < 0) throw Error('Global source witness lost its program entry');
    const globalUnit = global.slice(0, entry) + '\n' + read('Compiler/JetRuntime/Tests/Fragments.fixture/FloatAbi.jet');
    writeFileSync(join(out, 'global/Source/Fixture.jet'), globalUnit);
    writeFileSync(join(out, 'global/lifecycle.list'), runtimeLifecycleList(parseRuntimeExports(globalUnit, 'Source/Fixture.jet')));
    execFileSync(process.execPath, [join(repo, 'Compiler/JetRuntime/Tests/check-runtime.mjs'), join(out, 'init'), '--assemble-only', '--tests'], { stdio: 'inherit' });
    let unit = readFileSync(join(out, 'init/unit.jet'), 'utf8');
    const testHead = 'fn alloc_tests() {\n';
    if (unit.split(testHead).length !== 2) throw Error('allocator witness must have one test entry');
    // Full tests may call init twice, but startup must already have initialized.
    unit = unit.replace(testHead, `${testHead}    alloc_test_require(alloc_root != I64{0})\n`);
    unit += '\n#Export(c)\nfn runtime_fixture_root() -> I64 { alloc_root }\n\n#Export(c)\nfn runtime_fixture_tests() { alloc_tests() }\n';
    writeFileSync(join(out, 'init/Source/Fixture.jet'), unit);
    rmSync(join(out, 'init/unit.jet'));
    writeFileSync(join(out, 'init/lifecycle.list'), runtimeLifecycleList(parseRuntimeExports(unit, 'Source/Fixture.jet')));
    for (const name of ['global', 'init']) writeFileSync(join(out, name, 'package.jet'), read('Compiler/JetRuntime/package.jet'));
} else if (mode === '--lower') {
    const failures = [];
    for (const name of ['global', 'init']) {
        try {
            const dir = join(out, name), prefix = join(dir, 'fixture');
            const { problem } = runNativeLibrary({ jet: candidateJetc0(), file: join(dir, 'Source/Fixture.jet'), prefix, lifecycle: join(dir, 'lifecycle.list'), timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 120000), env: { ...process.env, JET_TOOLCHAIN_ROOT: repo, JET_STORE_DIR: join(out, 'store') } });
            if (problem) throw Error(problem.detail);
        } catch (error) { failures.push(`${name}: ${error.message}`); }
    }
    if (failures.length) throw Error(failures.join('\n'));
} else if (mode === '--global') {
    const object = join(out, 'global/fixture.o');
    const exports = run('nm', ['-g', '--defined-only', '--format=posix', object]).stdout;
    for (const name of ['native_global_store', 'native_global_load', 'runtime_float_identity']) if (!exports.includes(`${name} T `)) throw Error(`missing export ${name}`);
    if (/native_global_word\s/.test(exports)) throw Error('private storage escaped as a global symbol');
    run(nativeCCompiler, [join(repo, 'Compiler/JetRuntime/Tests/global-word.c'), object, '-o', join(out, 'global/caller')]);
    run(join(out, 'global/caller'), []);
} else {
    const object = join(out, 'init/fixture.o');
    const source = join(repo, 'Compiler/JetRuntime/Tests/initializer.c');
    const executable = join(out, 'init/caller');
    if (mode === '--allocator') {
        const result = run(executable, ['allocator']);
        if (result.stdout !== 'alloc tests: ok\n' || result.stderr !== '') throw Error('allocator stdout/stderr changed');
    } else {
        const cc = nativeCCompiler;
        run(cc, [source, object, '-lpthread', '-ldl', '-lm', '-o', executable]);
        run(executable, []);
        const shared = join(out, 'init/entryless.so');
        run(cc, ['-shared', '-Wl,-Bsymbolic', '-Wl,-z,defs', object, '-lpthread', '-ldl', '-lm', '-o', shared]);
        const sections = run('readelf', ['-SW', shared]).stdout;
        if (!sections.includes('.init_array')) throw Error('entryless library lost its constructor array');
        if (/^main\s/m.test(run('nm', ['-g', '--defined-only', '--format=posix', shared]).stdout)) throw Error('library unexpectedly has main');
        const loader = join(out, 'init/loader');
        run(cc, ['-DJET_ENTRYLESS_LIBRARY', source, '-ldl', '-o', loader]);
        run(loader, [shared]);
    }
}
