import test from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { planTransport, prepareTransport } from './compiler-registry-transports.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const options = { repo, jet: '/native/jet', runtimePack: '/native/jet-runtime/lib/jet_runtime.pack.o', runtimeDir: '/native/jet-runtime' };

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
