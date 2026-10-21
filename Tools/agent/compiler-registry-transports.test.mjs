import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { collectGoldenEntries } from './compiler-diff.mjs';
import { planTransport, prepareTransport } from './compiler-registry-transports.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const features = join(repo, 'Examples/features');
const cases = collectGoldenEntries(features).filter(entry => ['.out', '.err.out'].some(suffix => existsSync(join(features, 'expected', entry.stem + suffix))));
const options = { repo, jet: '/native/jet', runtimePack: '/native/jet-runtime/lib/jet_runtime.pack.o', runtimeDir: '/native/jet-runtime' };

for (const [source, packageName, mode] of [
    ['Compiler/JetBackend/Tests/Fragments.fixture/GoldenLower.jet', 'JetBackend', 'golden-lower'],
    ['Compiler/JetOptimizer/Tests/Fragments.fixture/LintCorpus.jet', 'JetOptimizer', 'corpus'],
]) test(`${packageName} native corpus plan includes every committed golden, with no manufactured cases`, () => {
    const entry = prepareTransport({ source, package: packageName }, { repo, out: '/native/registries' });
    assert.equal(entry.transport.type, mode);
    const plan = planTransport(entry, options);
    assert.ok(cases.length > 1);
    assert.equal(plan.corpus.caseCount, cases.length);
    assert.deepEqual(plan.corpus.cases.map(c => c.stem), cases.map(c => c.stem));
    assert.equal(plan.testCount, 1, 'one registry verdict, not one counted test per corpus case');
    const mirRuns = plan.steps.filter(step => step.command?.endsWith('/mir-transport/transport.exe'));
    assert.equal(mirRuns.length, cases.length);
    assert.deepEqual(mirRuns.map(step => step.args[1]), plan.corpus.cases.map(c => c.mird));
    assert.equal(plan.steps.filter(step => step.command === options.jet && step.args.includes(join(entry.cwd, 'mir-transport/run.jet'))).length, 1);
    assert.doesNotMatch(JSON.stringify(plan), /registry golden|golden-input|Tests\.rs|safe-jet|cargo|rustc|--front|--case-list/);
    if (mode === 'golden-lower') {
        assert.ok(plan.steps.some(step => step.env?.JET_LOWER_ASSEMBLE_ONLY === '1'));
        assert.ok(plan.steps.some(step => step.args?.includes('--stage') && step.args.includes('run')));
    } else {
        assert.ok(plan.steps.some(step => step.env?.JET_LINT_ASSEMBLE_ONLY === '1'));
        const verdict = plan.steps.find(step => step.command?.endsWith('/lint/registry-native.exe'));
        assert.equal(verdict.expected.stdout, cases.map(c => `../lower/${c.stem.replace(/[^A-Za-z0-9_-]+/gu, '_')}\t0\n`).join(''));
    }
});

test('native corpus assemblers retain the real registry and complete MIR decoder without a front end', () => {
    const scratch = process.env.JET_TEST_SCRATCH_DIR ?? '/mnt/jetscratch/tests';
    mkdirSync(scratch, { recursive: true });
    const out = mkdtempSync(join(scratch, 'registry-corpus-'));
    try {
        for (const [runner, args, variable, folder, verdict] of [
            ['Compiler/JetBackend/Tests/run-goldens.mjs', ['--stage', 'lower', '--until', 'lower'], 'JET_LOWER_ASSEMBLE_ONLY', 'lower', 'gl_case'],
            ['Compiler/JetOptimizer/Tests/run-lint-corpus.mjs', [], 'JET_LINT_ASSEMBLE_ONLY', 'lint', 'lc_case'],
        ]) {
            const result = spawnSync(process.execPath, [join(repo, runner), out, ...args], {
                encoding: 'utf8', env: { ...process.env, JET: options.jet, JET_RUNTIME_PACK: options.runtimePack, JET_GOLDEN_FEATURES: features, JET_GOLDEN_NATIVE_MIR: '1', [variable]: '1', JET_LOWER_RESUME: '0' },
            });
            assert.equal(result.status, 0, result.stderr);
            const unit = readFileSync(join(out, folder, 'unit.jet'), 'utf8');
            for (const name of ['param_zone_name', 'mird_u64', 'mird_mir_program', verdict, 'run']) {
                assert.equal((unit.match(new RegExp(`^(?:pub )?fn ${name}\\(`, 'gm')) ?? []).length, 1, name);
            }
            assert.ok(existsSync(join(out, folder, 'package.jet')));
            assert.equal(existsSync(join(out, folder, '.jet/build/unit')), false);
        }
    } finally { rmSync(out, { recursive: true, force: true }); }
});

test('compiler runtime route plans run the complete C witness variants', () => {
    for (const name of ['CompilerEnv', 'CompilerRoutes', 'CompilerNeededJSON', 'CompilerNeededURL', 'CompilerNeededMathText', 'DynNative']) {
        const source = `Compiler/JetRuntime/Tests/Fragments.fixture/${name}Tests.jet`;
        const entry = prepareTransport({ source, package: 'JetRuntime' }, { repo, out: '/native/registries' });
        assert.equal(entry.transport.type, 'c');
        assert.equal(entry.transport.name, name);
        assert.equal(entry.testCount, 1);
        const plan = planTransport(entry, options);
        assert.ok(plan.steps.some(step => step.args?.includes('--test') && step.args.at(-1) === name));
        const runs = plan.steps.filter(step => step.command === join(entry.cwd, 'driver'));
        assert.deepEqual(runs.map(step => step.args), name === 'CompilerEnv' ? [[], ['empty']] : name === 'CompilerRoutes' ? [[entry.cwd]] : name === 'DynNative' ? [[], ['datetime']] : [[]]);
        if (name === 'DynNative') assert.equal(runs[1].expected.code, 70);
        if (name === 'CompilerEnv' || name === 'CompilerRoutes') assert.ok(entry.sources.includes(`Compiler/JetRuntime/Tests/${name}Tests.c`));
    }
});

test('codec and compiler-file plans execute the selected real Jet export', () => {
    for (const name of ['CodecCSV', 'CodecEnv', 'DynCodec', 'CompilerFileLeaves']) {
        const source = `Compiler/JetRuntime/Tests/Fragments.fixture/${name}Tests.jet`;
        const entry = prepareTransport({ source, package: 'JetRuntime' }, { repo, out: '/native/registries' });
        assert.equal(entry.transport.type, 'codec-fixture');
        assert.equal(entry.testCount, 1);
        const plan = planTransport(entry, options);
        assert.equal(plan.steps.length, 1);
        assert.deepEqual(plan.steps[0].args, [join(repo, 'Compiler/JetRuntime/Tests/run-native-codec-file-leaves.mjs'), join(entry.cwd, 'codec'), options.jet, options.runtimeDir, '--test', name]);
    }
});

test('files-walk and host-parallelism plans include lowering, ABI audit and actual witness execution', () => {
    for (const name of ['FilesWalk', 'HostParallelism']) {
        const source = `Compiler/JetRuntime/Tests/Fragments.fixture/${name}Tests.jet`;
        const entry = prepareTransport({ source, package: 'JetRuntime' }, { repo, out: '/native/registries' });
        assert.equal(entry.transport.type, 'native-suite');
        assert.ok(entry.sources.includes(`Compiler/JetRuntime/Tests/${name}Witness.c`));
        const plan = planTransport(entry, options);
        assert.deepEqual(plan.steps.map(step => step.args.at(-1)), ['assemble', 'lower', 'audit', 'link', 'run']);
        assert.ok(plan.steps.every(step => step.env.JET_FRONT_JETC0 === options.jet));
    }
});

test('process plans call exported Jet fixtures instead of witness-local replacements', () => {
    for (const name of ['ProcessExecution', 'TerminalProcess']) {
        const source = `Compiler/JetRuntime/Tests/Fragments.fixture/${name}Tests.jet`;
        const entry = prepareTransport({ source, package: 'JetRuntime' }, { repo, out: '/native/registries' });
        assert.equal(entry.transport.type, 'process-fixture');
        assert.ok(entry.sources.includes(`Compiler/JetRuntime/Tests/${name}Witness.c`));
        assert.ok(entry.sources.includes(`Compiler/JetRuntime/Tests/${name}Witness.stdout`));
        const plan = planTransport(entry, options);
        assert.deepEqual(plan.steps[0].args, [join(repo, 'Compiler/JetRuntime/Tests/run-process-witnesses.mjs'), join(entry.cwd, 'process'), options.jet, options.runtimeDir, '--test', name]);
    }
});
