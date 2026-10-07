#!/usr/bin/env node
// E2: real source .Global exports and allocator startup in executable/library.
// OUT --assemble only prepares units. --lower uses the composed Jet candidate;
// --global/--initializer/--allocator execute separately so a proof keeps going.
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0, frontReceipt } from '../../JetBackend/Tests/jet-front.mjs';
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
    writeFileSync(join(out, 'global/Source/Fixture.jet'), global.slice(0, entry) + '\n' + read('Compiler/JetRuntime/Tests/FloatAbi.jet'));
    writeFileSync(join(out, 'global/exports.list'), 'native_global_store\nnative_global_load\nruntime_float_identity\n');
    execFileSync(process.execPath, [join(repo, 'Compiler/JetRuntime/Tests/check-runtime.mjs'), join(out, 'init'), '--assemble-only', '--tests'], { stdio: 'inherit' });
    let unit = readFileSync(join(out, 'init/unit.jet'), 'utf8');
    const runHead = 'fn run() {\n';
    if (unit.split(runHead).length !== 2) throw Error('allocator witness must have one entry');
    // Full tests may call init twice, but startup must already have initialized.
    unit = unit.replace(runHead, `${runHead}    alloc_test_require(alloc_root != I64{0})\n`);
    unit += '\n#Export(c)\nfn runtime_fixture_root() -> I64 Never! { alloc_root }\n\n#Export(c)\nfn runtime_fixture_tests() { run() }\n';
    writeFileSync(join(out, 'init/Source/Fixture.jet'), unit);
    writeFileSync(join(out, 'init/exports.list'), read('Compiler/JetRuntime/exports.list') + 'runtime_fixture_root\nruntime_fixture_tests\n');
    for (const name of ['global', 'init']) writeFileSync(join(out, name, 'package.jet'), read('Compiler/JetRuntime/package.jet'));
} else if (mode === '--lower') {
    const failures = [];
    for (const name of ['global', 'init']) {
        try {
            const dir = join(out, name), prefix = join(dir, 'fixture'), receipt = join(dir, 'front.receipt');
            const result = spawnSync(candidateJetc0(), [], {
                cwd: repo, encoding: 'utf8', timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 120000), maxBuffer: 1 << 27,
                env: { ...process.env, JET_BOOTSTRAP_MODE: 'factory', JET_BOOTSTRAP_FACTORY_TIER: 'aot', JET_BOOTSTRAP_SOURCE_ROOT: dir,
                    JET_BOOTSTRAP_ENTRY: 'Source/Fixture.jet', JET_BOOTSTRAP_OUTPUT: join(dir, 'unused.output'), JET_BOOTSTRAP_RECEIPT: receipt,
                    JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix, JET_FRONT_LIBRARY_EXPORTS: join(dir, 'exports.list'), JET_STORE_DIR: join(out, 'store'), RUST_MIN_STACK: '8388608' },
            });
            writeFileSync(join(dir, 'front.log'), `${result.stdout ?? ''}${result.stderr ?? ''}`);
            const problem = frontReceipt(existsSync(receipt) ? readFileSync(receipt, 'utf8') : '', result);
            if (problem) throw Error(problem.detail);
            if (existsSync(prefix + '.issues')) throw Error(readFileSync(prefix + '.issues', 'utf8'));
            if (!existsSync(prefix + '.o')) throw Error('missing library object');
        } catch (error) { failures.push(`${name}: ${error.message}`); }
    }
    if (failures.length) throw Error(failures.join('\n'));
} else if (mode === '--global') {
    const object = join(out, 'global/fixture.o');
    const exports = run('nm', ['-g', '--defined-only', '--format=posix', object]).stdout;
    for (const name of ['native_global_store', 'native_global_load', 'runtime_float_identity']) if (!exports.includes(`${name} T `)) throw Error(`missing export ${name}`);
    if (/native_global_word\s/.test(exports)) throw Error('private storage escaped as a global symbol');
    run(process.env.CC ?? 'cc', [join(repo, 'Compiler/JetRuntime/Tests/global-word.c'), object, '-o', join(out, 'global/caller')]);
    run(join(out, 'global/caller'), []);
} else {
    const library = process.env.JET_RUNTIME_C_LIB;
    if (!library) throw Error('JET_RUNTIME_C_LIB must name the freshly built remaining Rust staticlib');
    const object = join(out, 'init/fixture.o');
    const source = join(repo, 'Compiler/JetRuntime/Tests/initializer.c');
    const executable = join(out, 'init/caller');
    if (mode === '--allocator') {
        const result = run(executable, ['allocator']);
        if (result.stdout !== 'alloc tests: ok\n' || result.stderr !== '') throw Error('allocator stdout/stderr changed');
    } else {
        const cc = process.env.CC ?? 'cc';
        run(cc, [source, object, library, '-lpthread', '-ldl', '-lm', '-o', executable]);
        run(executable, []);
        const shared = join(out, 'init/entryless.so');
        run(cc, ['-shared', '-Wl,-Bsymbolic', '-Wl,-z,defs', object, library, '-lpthread', '-ldl', '-lm', '-o', shared]);
        const sections = run('readelf', ['-SW', shared]).stdout;
        if (!sections.includes('.init_array')) throw Error('entryless library lost its constructor array');
        if (/^main\s/m.test(run('nm', ['-g', '--defined-only', '--format=posix', shared]).stdout)) throw Error('library unexpectedly has main');
        const loader = join(out, 'init/loader');
        run(cc, ['-DJET_ENTRYLESS_LIBRARY', source, '-ldl', '-o', loader]);
        run(loader, [shared]);
    }
}
