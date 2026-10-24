import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { nativeCommandProblem, nativeTransportEnvironment, runNativeLibrary, runNativeProgram } from './compiler-native-transport.mjs';

const temporary = (t) => {
    const dir = mkdtempSync(join(tmpdir(), 'jet-native-transport-'));
    t.after(() => rmSync(dir, { recursive: true, force: true }));
    return dir;
};
const completionOnly = (_command, _args, options) => spawnSync(process.execPath, ['-e', 'process.stdout.write("command completed\\n")'], options);

test('native command outcomes never accept interruption or unsuccessful exits', () => {
    assert.equal(nativeCommandProblem({ status: 0 }), null);
    for (const run of [{ status: 1 }, { status: null, signal: 'SIGKILL' }, { status: null, error: Error('spawn refused') }]) assert.equal(nativeCommandProblem(run).verdict, 'no-artifact');
});

test('native transport clears inherited bootstrap and observation hooks', () => {
    assert.deepEqual(nativeTransportEnvironment({ JET_TOOLCHAIN_ROOT: '/pinned', JET_RUNTIME_PACK: '/pack', JET_BOOTSTRAP_MODE: 'factory', JET_FRONT_LIBRARY_OBJECT_OUTPUT: '/stale' }), { JET_TOOLCHAIN_ROOT: '/pinned', JET_RUNTIME_PACK: '/pack' });
});

test('library uses check FILE and rejects completion without fresh artifacts', (t) => {
    const root = temporary(t), file = join(root, 'unit.jet'), prefix = join(root, 'fixture'), lifecycle = join(root, 'lifecycle.list');
    writeFileSync(file, 'fn run() {}\n');
    writeFileSync(lifecycle, 'explicit lifecycle fixture\n');
    for (const suffix of ['.o', '.exports', '.abi.json', '.issues']) writeFileSync(prefix + suffix, 'stale artifact');
    const result = runNativeLibrary({ jet: '/pinned/jet', file, prefix, lifecycle, env: { JET_BOOTSTRAP_MODE: 'factory', JET_TOOLCHAIN_ROOT: root }, execute(command, args, options) {
        assert.equal(command, '/pinned/jet'); assert.deepEqual(args, ['check', file]);
        assert.equal(options.env.JET_BOOTSTRAP_MODE, undefined);
        assert.equal(options.env.JET_FRONT_LIBRARY_OBJECT_OUTPUT, prefix);
        assert.equal(options.env.JET_FRONT_LIBRARY_LIFECYCLE, lifecycle);
        return completionOnly(command, args, options);
    } });
    assert.equal(result.run.status, 0);
    assert.equal(result.problem.detail, 'native library hook omitted .o');
    assert.equal(readFileSync(lifecycle, 'utf8'), 'explicit lifecycle fixture\n');
    for (const suffix of ['.o', '.exports', '.abi.json', '.issues']) assert.equal(existsSync(prefix + suffix), false);
    const receipt = JSON.parse(readFileSync(prefix + '.front.run.json', 'utf8'));
    assert.deepEqual(receipt.args, ['check', file]); assert.equal(receipt.status, 0);
});

test('program build completion without an executable is not a native feed', (t) => {
    const root = temporary(t), file = join(root, 'unit.jet'), prefix = join(root, 'case');
    writeFileSync(file, 'fn run() {}\n'); writeFileSync(prefix + '.exe', 'stale executable');
    const result = runNativeProgram({ jet: '/pinned/jet', file, root, prefix, execute(command, args, options) {
        assert.deepEqual(args, ['build', file]); assert.equal(options.cwd, root);
        return completionOnly(command, args, options);
    } });
    assert.match(result.problem.detail, /a fresh executable/);
    assert.equal(existsSync(prefix + '.exe'), false);
});
