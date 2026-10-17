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
        const groupedBindings = new Set();
        for (const [, module, members] of source.matchAll(/^use (core(?:\.[A-Za-z_][A-Za-z0-9_]*)*)\.\[([^\]]*)\]/gm)) {
            for (const member of members.split(',')) {
                if (!member.trim()) continue;
                const binding = member.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)(?:\s+as\s+([A-Za-z_][A-Za-z0-9_]*))?\s*$/);
                assert.ok(binding, `unexpected grouped Core import member: ${member}`);
                const key = `${module}.${binding[1]} as ${binding[2] ?? binding[1]}`;
                assert.equal(groupedBindings.has(key), false, `duplicate assembled Core binding: ${key}`);
                groupedBindings.add(key);
            }
        }
        assert.ok(groupedBindings.has('core.build.BuildGraphNode as BuildGraphNode'));
        assert.doesNotMatch(source, /^use core\.build\.BuildGraphNode$/m,
            'a Core build type must use the member-import route, not a fictitious module path');
        const sourceMap = JSON.parse(readFileSync(join(out, 'compiler.map.json')));
        assert.equal(sourceMap.source_segments.at(-1).source_file, 'Compiler/JetCli/run.jet');
        const assembledBytes = Buffer.from(source);
        for (const segment of sourceMap.source_segments) {
            const original = readFileSync(join(repo, segment.source_file), 'utf8');
            for (const match of original.matchAll(/^use core(?:\.[A-Za-z_][A-Za-z0-9_]*)*\.\[[^\]]*\]/gm)) {
                const originalBytes = Buffer.from(match[0]);
                const start = segment.generated.start + Buffer.byteLength(original.slice(0, match.index));
                const generatedBytes = assembledBytes.subarray(start, start + originalBytes.length);
                assert.equal(generatedBytes.length, originalBytes.length);
                for (let index = 0; index < originalBytes.length; index++) {
                    assert.ok(generatedBytes[index] === originalBytes[index] ||
                        (generatedBytes[index] === 32 && originalBytes[index] !== 10),
                        `${segment.source_file}: grouped import token or source-map offset shifted at byte ${index}`);
                }
            }
        }
        assert.equal(existsSync(join(out, 'seed/jet')), false, 'dry-run must not manufacture a compiler');
        inspect(JSON.parse(readFileSync(join(out, 'plan.json'))));
    } finally { rmSync(root, { recursive: true, force: true }); }
}

test('native assembly selects real CLI and two native O1 builds at the same path', () => {
    dryRun(root => ['--runtime-dir', join(root, 'unavailable-runtime')], plan => {
        assert.equal(plan.backend, 'jet-native');
        assert.equal(plan.optimization, 'O1');
        assert.equal(plan.lowering, 'lir_lower_entry');
        assert.equal(plan.linker, 'x64_link_executable');
        assert.equal(plan.stages.length, 2);
        for (const stage of plan.stages) {
            assert.deepEqual(stage.command.slice(-2), ['build', plan.source]);
            assert.equal(stage.command.includes('--release'), false);
            assert.match(stage.object, /selfhost\.o$/);
            assert.match(stage.keepGoingReport, new RegExp(`stage${stage.stage}-keep-going\\.tsv$`));
        }
        assert.deepEqual(plan.comparisons, ['stage1/jet.o == stage2/jet.o', 'stage1/jet == stage2/jet']);
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
