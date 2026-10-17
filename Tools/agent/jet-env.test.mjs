import {test} from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {mkdtempSync, mkdirSync, writeFileSync, rmSync} from 'node:fs';
import {homedir} from 'node:os';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
function wrappers(overrides = {}) {
    const scratch = join(homedir(), '.cache/jet-dev/scratch');
    mkdirSync(scratch, {recursive: true});
    const temporary = mkdtempSync(join(scratch, 'jet-env-test-'));
    try {
        const bin = join(temporary, 'bin');
        mkdirSync(bin);
        writeFileSync(join(bin, 'direnv'), '#!/bin/sh\nprintf "%s\\n" "$JET_ENV_TEST_IMPORTS"\n', {mode: 0o755});
        writeFileSync(join(bin, 'sccache'), '#!/bin/sh\nexit 99\n', {mode: 0o755});
        const result = spawnSync(join(root, 'Tools/agent/jet-env'), [process.execPath, '-e',
            'console.log(JSON.stringify([process.env.RUSTC_WRAPPER ?? null, process.env.RUSTC_WORKSPACE_WRAPPER ?? null]))'], {
            cwd: root,
            env: {...process.env, PATH: `${bin}:${process.env.PATH}`, JET_SCRATCH_ROOT: temporary,
                CARGO_TARGET_DIR: join(temporary, 'targets/cold'), CARGO_BUILD_JOBS: '4',
                RUSTC_WRAPPER: '', RUSTC_WORKSPACE_WRAPPER: '', JET_NO_SCCACHE: '',
                JET_CARGO_INCREMENTAL: '0', JET_ENV_TEST_IMPORTS: '', ...overrides},
            encoding: 'utf8', timeout: 30000,
        });
        assert.equal(result.status, 0, result.stderr || result.error?.message);
        return JSON.parse(result.stdout);
    } finally {
        rmSync(temporary, {recursive: true, force: true});
    }
}

test('cold targets do not implicitly select available sccache', () => {
    assert.deepEqual(wrappers(), ['', '']);
});

test('caller-selected wrappers remain selected without an opt-out', () => {
    assert.deepEqual(wrappers({RUSTC_WRAPPER: 'caller-wrapper', RUSTC_WORKSPACE_WRAPPER: 'caller-workspace-wrapper'}),
        ['caller-wrapper', 'caller-workspace-wrapper']);
});

test('sccache opt-out clears wrappers after shell imports', () => {
    assert.deepEqual(wrappers({JET_NO_SCCACHE: '1',
        JET_ENV_TEST_IMPORTS: 'export RUSTC_WRAPPER=sccache; export RUSTC_WORKSPACE_WRAPPER=imported-wrapper'}),
        [null, null]);
});
