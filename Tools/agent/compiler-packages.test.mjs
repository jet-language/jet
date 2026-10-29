import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compilerFixtureUnit, compilerInputRoots, compilerInputs, compilerPackages, compilerSourceIdentity, compilerSources, isCompilerInput } from './compiler-packages.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

function tree(files) {
    const root = mkdtempSync(join(tmpdir(), 'compiler-packages-'));
    for (const [path, text] of Object.entries(files)) {
        mkdirSync(dirname(join(root, path)), { recursive: true });
        writeFileSync(join(root, path), text);
    }
    return root;
}

const manifest = (name, deps = {}) => `name: "${name}"\nversion: "0.0.1"\n${Object.keys(deps).length ? `deps: {\n${Object.entries(deps).map(([dep, path]) => `    ${dep}: ${path},`).join('\n')}\n}\n` : ''}`;

test('the closure follows path dependencies and lists every package after its dependencies', t => {
    const root = tree({
        'Compiler/JetCli/package.jet': manifest('jet_cli', { jet_sema: '../JetSema', jet_foundation: '../JetFoundation', shared: '../../Jetpack/Shared' }),
        'Compiler/JetSema/package.jet': manifest('jet_sema', { jet_foundation: '../JetFoundation' }),
        'Compiler/JetFoundation/package.jet': manifest('jet_foundation'),
        'Jetpack/Shared/package.jet': manifest('shared', { jet_foundation: '../../Compiler/JetFoundation' }),
        'Compiler/JetLSP/package.jet': manifest('jet_lsp', { jet_cli: '../JetCli' }),
    });
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const names = [...compilerPackages(root).keys()];
    assert.deepEqual(names, ['jet_foundation', 'jet_sema', 'shared', 'jet_cli'], 'each package follows its dependencies; jet_lsp is not a CLI dependency');
});

test('the source identity covers compiler inputs only', t => {
    const root = tree({
        'Compiler/package.jet': 'name: "compiler"\n',
        'Compiler/JetCli/package.jet': manifest('jet_cli'),
        'Compiler/JetCli/run.jet': 'fn run() {}\n',
        'Compiler/JetCli/Source/Cli/Registry.jet': 'JET_CLI_BINARY :: "jet"\nJET_CLI_VERSION :: "0.0.1"\n',
        'Compiler/JetCli/Tests/package.jet': manifest('jet_cli_tests'),
        'Compiler/JetCli/Tests/T.jet': 'fn t() {}\n',
        'Core/Text.jet': 'fn text() {}\n',
        'Docs/README.md': 'docs\n',
    });
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const before = compilerSourceIdentity(root);
    assert.match(before, /^jet@0\.0\.1#[0-9a-f]{64}$/);
    writeFileSync(join(root, 'Compiler/JetCli/Tests/T.jet'), 'fn t2() {}\n');
    writeFileSync(join(root, 'Docs/README.md'), 'other\n');
    assert.equal(compilerSourceIdentity(root), before, 'test packages and other trees are not compiler inputs');
    writeFileSync(join(root, 'Core/Text.jet'), 'fn text2() {}\n');
    const core = compilerSourceIdentity(root);
    assert.notEqual(core, before);
    writeFileSync(join(root, 'Compiler/package.jet'), 'name: "compiler2"\n');
    assert.notEqual(compilerSourceIdentity(root), core);
});

test('package members follow the driver walk', t => {
    const root = tree({
        'Compiler/JetCli/package.jet': manifest('jet_cli'),
        'Compiler/JetCli/run.jet': 'fn run() {}\n',
        'Compiler/JetCli/Source/A.jet': 'fn a() {}\n',
        'Compiler/JetCli/Source/Deep/B.jet': 'fn b() {}\n',
        'Compiler/JetCli/Source/notes.md': 'data\n',
        'Compiler/JetCli/Source/script.jet': '#!/usr/bin/env jet\nfn run() {}\n',
        'Compiler/JetCli/Source/config.jet': 'x :: 1\n',
        'Compiler/JetCli/.hidden/C.jet': 'fn c() {}\n',
        'Compiler/JetCli/build/D.jet': 'fn d() {}\n',
        'Compiler/JetCli/target/E.jet': 'fn e() {}\n',
        'Compiler/JetCli/Tests/package.jet': manifest('jet_cli_tests'),
        'Compiler/JetCli/Tests/T.jet': 'fn t() {}\n',
        'Compiler/JetCli/Locked/.jet/lock': '',
        'Compiler/JetCli/Locked/L.jet': 'fn l() {}\n',
    });
    t.after(() => rmSync(root, { recursive: true, force: true }));
    symlinkSync(join(root, 'Compiler/JetCli/Source'), join(root, 'Compiler/JetCli/Linked'));
    assert.deepEqual(compilerSources(root), [{ name: 'jet_cli', directory: 'Compiler/JetCli', files: ['Compiler/JetCli/Source/A.jet', 'Compiler/JetCli/Source/Deep/B.jet', 'Compiler/JetCli/run.jet'] }]);
    const inputs = compilerInputs(root);
    assert.ok(inputs.includes('Compiler/JetCli/Source/notes.md'), 'identity covers data files of a package');
    assert.ok(inputs.includes('Compiler/JetCli/package.jet'));
    assert.ok(!inputs.some(path => path.includes('/Tests/') || path.includes('/.hidden/') || path.includes('/Locked/')));
    assert.deepEqual(compilerInputRoots(root), ['Compiler/package.jet', 'Compiler/build.jet', 'Compiler/JetCli', 'Core']);
    assert.ok(isCompilerInput(root, 'Compiler/JetCli/Source/Removed.jet'), 'a deleted member path is still an input path');
    assert.ok(isCompilerInput(root, 'Core/Text/Text.jet'));
    assert.ok(isCompilerInput(root, 'Compiler/build.jet'));
    assert.ok(!isCompilerInput(root, 'Compiler/JetCli/Tests/T.jet'));
    assert.ok(!isCompilerInput(root, 'Compiler/JetCli/build/D.jet'));
    assert.ok(!isCompilerInput(root, 'Compiler/JetLSP/X.jet'));
});

test('a fixture unit blanks package imports and repeated Core imports in place and leaves out package entries', t => {
    const root = tree({
        'Compiler/JetCli/package.jet': manifest('jet_cli', { jet_foundation: '../JetFoundation' }),
        'Compiler/JetCli/run.jet': 'fn run() {}\n',
        'Compiler/JetCli/Source/Cli.jet': 'use jet_foundation.[found]\nuse core.files as files\nfn cli() { found() }\n',
        'Compiler/JetFoundation/package.jet': manifest('jet_foundation'),
        'Compiler/JetFoundation/Source/Found.jet': 'use core.files as files\nuse core.text.[Text, Rune]\nfn found() {}',
        'Compiler/JetFoundation/Source/More.jet': 'use core.text.[Rune, Glyph]\nfn more() {}\n',
    });
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const unit = compilerFixtureUnit(root);
    assert.ok(!unit.includes('fn run()'));
    assert.ok(!unit.includes('use jet_foundation'));
    assert.equal(unit.match(/use core\.files as files/g).length, 1);
    assert.ok(unit.includes('use core.text.[Text, Rune]'));
    assert.ok(unit.includes('use core.text.[      Glyph]'), 'a repeated grouped Core member is blanked in place');
    assert.ok(unit.indexOf('fn found()') < unit.indexOf('fn cli()'), 'dependencies come first');
    assert.equal(compilerFixtureUnit(root, ['Compiler/JetFoundation']).includes('fn cli()'), false);
});

test('the repository compiler closure is the CLI and its dependencies', () => {
    const packages = compilerPackages(repo);
    assert.equal([...packages.keys()].at(-1), 'jet_cli');
    assert.ok(packages.has('jet_foundation') && packages.has('jet_backend') && packages.has('jet_pkg_model'));
    const sources = compilerSources(repo, packages).flatMap(entry => entry.files);
    assert.ok(sources.includes('Compiler/JetCli/run.jet'));
    assert.ok(!sources.some(path => path.includes('/Tests/')), 'test packages are their own projects');
});
