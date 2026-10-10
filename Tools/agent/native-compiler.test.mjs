import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../..');
const scratch = process.env.JET_TEST_SCRATCH_DIR ?? '/mnt/jetscratch/tests';
function dryRun(extra, inspect) {
    mkdirSync(scratch, { recursive: true });
    const root = mkdtempSync(join(scratch, 'native-compiler-'));
    const out = join(root, 'output');
    try {
        const result = spawnSync(process.execPath, [join(here, 'native-compiler.mjs'), '--jet', join(root, 'unavailable-seed'),
            '--repo', repo, '--out', out, '--dry-run', ...extra(root)], { encoding: 'utf8' });
        assert.equal(result.status, 0, result.stderr);
        const source = readFileSync(join(out, 'compiler/selfhost.jet'), 'utf8');
        assert.equal((source.match(/^fn run\(/gm) ?? []).length, 1);
        assert.match(source, /process\.exit\(jet_cli_main\(process\.args\(\)\)\)/);
        assert.equal((source.match(/^use core\.process as process$/gm) ?? []).length, 1);
        const manifest = readFileSync(join(out, 'compiler/package.jet'), 'utf8');
        assert.doesNotMatch(manifest, /^deps:/m);
        const budget = manifest.match(/holds:\s*\{\s*allow:\s*\[([^\]]*)\]/)?.[1];
        assert.ok(budget, 'the standalone compiler retains its declared authority budget');
        const effects = budget.split(',').map(effect => effect.trim());
        assert.ok(effects.includes('FFI.C'), 'the native child uses raw C transport');
        assert.ok(effects.includes('FFI.Asm'), 'the native child uses raw assembly transport');
        for (const effect of ['FFI', 'Browser', 'DB', 'GPU', 'Log', 'Secret']) {
            assert.equal(effects.includes(effect), false, `native transport must not grant ${effect}`);
        }
        const executeRow = '-[Exec, IO, FFI.C, FFI.Asm, Time.Wait]>';
        assert.equal(source.split(`fn execute(self, request: X64ExecutionRequest) ${executeRow} X64ExecutionResponse`).length - 1, 2,
            'the execution-host trait and implementation retain the same explicit transport bound');
        assert.equal(JSON.parse(readFileSync(join(out, 'compiler.map.json'))).source_segments.at(-1).source_file, 'Compiler/JetCli/run.jet');
        assert.equal(existsSync(join(out, 'seed/jet')), false, 'dry-run must not manufacture a compiler');
        inspect(JSON.parse(readFileSync(join(out, 'plan.json'))));
    } finally { rmSync(root, { recursive: true, force: true }); }
}

test('native assembly selects real CLI and three native O0 builds at the same path', () => {
    dryRun(root => ['--runtime-dir', join(root, 'unavailable-runtime')], plan => {
        assert.equal(plan.backend, 'jet-native');
        assert.equal(plan.optimization, 'O0');
        assert.equal(plan.lowering, 'lir_lower_entry');
        assert.equal(plan.linker, 'x64_link_executable');
        assert.equal(plan.stages.length, 3);
        for (const stage of plan.stages) {
            assert.deepEqual(stage.command.slice(-2), ['build', plan.source]);
            assert.equal(stage.command.includes('--release'), false);
            assert.match(stage.object, /selfhost\.o$/);
        }
        assert.equal(plan.comparisons.length, 2);
    });
});

test('bootstrap seed exception emits production CLI, never private Tests.rs main', () => {
    dryRun(() => ['--seed-build'], plan => {
        assert.equal(plan.bootstrap, 'rust-emission');
        assert.deepEqual(plan.command.slice(-3), ['build', '--release', plan.source]);
        assert.equal(plan.entry, 'Compiler/JetCli/run.jet');
        assert.match(plan.jet, /seed\/jet$/);
    });
});
