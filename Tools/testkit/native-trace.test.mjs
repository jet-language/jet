import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { auditNativeTrace, ffiCaseCachePath, readUserRustAllowlist } from './native-trace.mjs';

const reason = 'user Rust crate via FFI (E0703 class)';
const declared = { base64: '0.22' };
const quote = JSON.stringify;
const allowRow = (name, deps = declared, sources = [name]) => `${name}\t${reason}\t${JSON.stringify(sources)}\t${JSON.stringify(deps)}\n`;
const argv = values => `[${values.map(quote).join(', ')}]`;
const exec = (pid, binary, args = [binary], result = '0', env = '0xabcdef /* 20 vars */') => `${pid} execve(${quote(binary)}, ${argv(args)}, ${env}) = ${result}`;
const fork = (parent, child, flags = 'SIGCHLD') => `${parent} clone(child_stack=NULL, flags=${flags}) = ${child}`;
const chdir = (pid, path) => `${pid} chdir(${quote(path)}) = 0`;

function fixture(t) {
    const home = realpathSync(mkdtempSync(join(tmpdir(), 'jet-native-trace-unit-')));
    t.after(() => rmSync(home, { recursive: true, force: true }));
    const root = join(home, 'source'), cacheRoot = join(home, 'evidence', 'native-ffi-cache');
    const put = (path, contents = '') => { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, contents); return path; };
    mkdirSync(cacheRoot, { recursive: true });
    const wrapper = put(join(root, 'Tools/testkit/native-command.mjs'));
    const peer = put(join(root, 'Tools/ci/compiled-workload-runner.mjs'));
    const perf = put(join(root, 'Tools/perf/nested/workload.mjs'));
    const jet = put(join(home, 'tools/jet'));
    const tools = Object.fromEntries(['cargo', 'rustc', 'rustup', 'ld.lld', 'clang', 'clang++', 'opt', 'llc', 'bwrap'].map(name => [name, put(join(home, 'tools', name))]));
    const name = 'Examples/features/lowlevel/ffi.jet', zero = 'Examples/zero.jet';
    put(join(root, name), 'extern rust "base64@0.22" { fn encode(s: String) -> String = "base64::encode" }\n');
    put(join(root, zero), 'fn run() { print("zero") }\n');
    const allowlistText = allowRow(name);
    const cache = ffiCaseCachePath(cacheRoot, name, declared);
    const contents = '[package]\nname = "jet_ffi_bridge"\nversion = "0.0.0"\nedition = "2021"\n\n[lib]\ncrate-type = ["staticlib"]\npath = "src/lib.rs"\n\n[dependencies]\nbase64 = "0.22"\n\n[profile.release]\nopt-level = 3\n';
    const manifest = put(join(cache, 'helper/Cargo.toml'), contents);
    put(join(dirname(manifest), 'src/lib.rs'), '// synthetic bytes; never compiled\n');
    const mark = (pid = 10, casePath = name, overrides = {}) => {
        const options = { '--case': casePath, '--root': root, '--cwd': root, '--jet': jet, '--cache-root': cacheRoot, '--ffi-cache': '', ...overrides };
        return exec(pid, process.execPath, [process.execPath, wrapper, ...Object.entries(options).flat(), '--', jet, 'run', join(root, casePath)]);
    };
    const jetExec = (pid = 10, command = 'run', extra = []) => exec(pid, jet, [jet, command, join(root, name), ...extra]);
    const cargo = (pid = 20, path = manifest, equals = false) => exec(pid, tools.cargo, [tools.cargo, 'build', ...(equals ? [`--manifest-path=${path}`] : ['--manifest-path', path])]);
    const rustc = (pid = 30) => exec(pid, tools.rustc, [tools.rustc, '--crate-name', 'jet_ffi_bridge', 'src/lib.rs']);
    const branch = () => [chdir(10, root), mark(), jetExec(), fork(10, 20), cargo(), fork(20, 30), rustc()];
    const audit = (lines, overrides = {}) => auditNativeTrace({ text: Array.isArray(lines) ? lines.join('\n') : lines, root, jet, allowlistText, tracePath: join(home, 'outer.strace'), ...overrides });
    return { home, root, cacheRoot, wrapper, peer, perf, jet, tools, name, zero, cache, manifest, contents, put, mark, jetExec, cargo, rustc, branch, audit, allowlistText };
}

function pass(receipt) { assert.equal(receipt.status, 'PASS', JSON.stringify(receipt.violations, null, 2)); assert.deepEqual(receipt.violations, []); }
function fail(receipt, code = 'forbidden-execution') { assert.equal(receipt.status, 'FAIL'); assert.ok(receipt.violations.some(item => item.code === code), JSON.stringify(receipt.violations, null, 2)); }

test('allowlist validates canonical source declarations and exact reason/dependencies', t => {
    const f = fixture(t);
    assert.deepEqual(readUserRustAllowlist(f.allowlistText, f.root).get(f.name).dependencies, declared);
    assert.throws(() => readUserRustAllowlist(f.allowlistText.replace(reason, 'compiler implementation'), f.root));
    assert.throws(() => readUserRustAllowlist(f.allowlistText.replace('0.22', '0.23'), f.root));
    assert.throws(() => readUserRustAllowlist(f.allowlistText + f.allowlistText, f.root));
    assert.throws(() => readUserRustAllowlist(f.allowlistText.replace(f.name, `./${f.name}`), f.root));
    assert.throws(() => readUserRustAllowlist(allowRow(f.name, {}), f.root));
    f.put(join(f.root, f.name), '// extern rust "fake@1"\nextern rust "base64@0.22" {}\n');
    assert.deepEqual(readUserRustAllowlist(f.allowlistText, f.root).get(f.name).dependencies, declared);
    f.put(join(f.root, f.name), 'extern rust "base64@0.22" {}\nextern rust "other@1" {}\n');
    assert.throws(() => readUserRustAllowlist(f.allowlistText, f.root));
});

test('case allowlist proves compiled fixture provenance and exact dependency union', t => {
    const f = fixture(t), name = 'tests/cases/cffi/shared.case';
    const sources = ['tests/cases/cffi/shared.fixture/w0/main.jet', 'tests/cases/cffi/shared.fixture/w1/main.jet'];
    f.put(join(f.root, name), 'fixture: shared.fixture\nspawn first: jet build main.jet\n  cwd: w0\nspawn second: jet build main.jet\n  cwd: w1\n');
    for (const source of sources) f.put(join(f.root, source), 'extern rust "base64@0.22" {}\n');
    const row = allowRow(name, declared, sources), declaration = readUserRustAllowlist(row, f.root).get(name);
    assert.deepEqual(declaration.dependencies, declared);
    assert.deepEqual(declaration.provenance, sources.map(path => ({ path, sha256: createHash('sha256').update(readFileSync(join(f.root, path))).digest('hex') })));
    assert.throws(() => readUserRustAllowlist(allowRow(name, declared, [sources[0]]), f.root));
    assert.throws(() => readUserRustAllowlist(allowRow(name, declared, [...sources, f.name]), f.root));
    assert.throws(() => readUserRustAllowlist(allowRow(name, declared, [name]), f.root));
    f.put(join(f.root, sources[1]), 'extern rust "base64@0.23" {}\n');
    assert.throws(() => readUserRustAllowlist(row, f.root));
    for (const source of sources) f.put(join(f.root, source), 'fn run() {}\n');
    assert.throws(() => readUserRustAllowlist(allowRow(name, {}, sources), f.root));
});

test('explicit private cache binds both concurrent branches and rejects a different helper cache', t => {
    const f = fixture(t), cache = join(f.home, 'shared-case', 'ffi-cache');
    const manifest = f.put(join(cache, 'helper/Cargo.toml'), f.contents);
    f.put(join(dirname(manifest), 'src/lib.rs'), '// synthetic\n');
    const branch = pid => [f.mark(pid, f.name, { '--ffi-cache': cache }), f.jetExec(pid), fork(pid, pid + 1), f.cargo(pid + 1, manifest)];
    const receipt = f.audit([...branch(10), ...branch(30)]);
    pass(receipt);
    assert.equal(receipt.allowedFfiBuilds.length, 2);
    for (const build of receipt.allowedFfiBuilds) {
        assert.equal(build.cachePath, cache);
        assert.equal(build.cacheSource, 'explicit');
        assert.equal(build.provenance[0].path, f.name);
    }
    fail(f.audit([f.mark(10, f.name, { '--ffi-cache': cache }), f.jetExec(), fork(10, 20), f.cargo()]));
    fail(f.audit([f.mark(10, f.name, { '--ffi-cache': cache }), f.jetExec(), fork(10, 20), exec(20, f.tools.cargo, [f.tools.cargo, 'build', '--manifest-path', manifest], '0', argv([`JET_FFI_CACHE_DIR=${f.cache}`]))]));
    const query = f.audit([f.mark(10, f.name, { '--ffi-cache': cache }), f.jetExec(), fork(10, 20), exec(20, f.tools.rustc, [f.tools.rustc, '-vV'])]);
    pass(query);
    assert.equal(query.allowedFfiQueries[0].cachePath, cache);
    const relativeCache = f.audit([f.mark(10, f.name, { '--cwd': dirname(cache), '--ffi-cache': 'ffi-cache' }), f.jetExec(), fork(10, 20), exec(20, f.tools.cargo, [f.tools.cargo, 'build', '--manifest-path', manifest], '0', argv(['JET_FFI_CACHE_DIR=ffi-cache']))]);
    pass(relativeCache);
    assert.equal(relativeCache.allowedFfiBuilds[0].cachePath, cache);
});

test('wrapper preserves an explicit cache verbatim and enables retention only for user Rust', t => {
    const f = fixture(t), cacheRoot = realpathSync(mkdtempSync(join(homedir(), '.jet-native-ffi-unit-')));
    t.after(() => rmSync(cacheRoot, { recursive: true, force: true }));
    const cwd = join(cacheRoot, 'case-work'), override = 'ffi-cache';
    mkdirSync(cwd);
    f.put(join(f.root, 'Tools/testkit/user-rust-ffi.allow'), f.allowlistText);
    const probe = 'console.log(JSON.stringify({cache:process.env.JET_FFI_CACHE_DIR,retain:process.env.JET_FFI_TRACE_WORKSPACE??null}));';
    const run = name => spawnSync(process.execPath, [
        new URL('./native-command.mjs', import.meta.url).pathname,
        '--case', name, '--root', f.root, '--cwd', cwd, '--jet', process.execPath,
        '--cache-root', cacheRoot, '--ffi-cache', override, '--', process.execPath, '-e', probe,
    ], { env: { ...process.env, JET_FFI_CACHE_DIR: override, JET_FFI_TRACE_WORKSPACE: '1' }, encoding: 'utf8', timeout: 10000 });
    const rust = run(f.name);
    assert.equal(rust.status, 0, rust.stderr);
    assert.deepEqual(JSON.parse(rust.stdout), { cache: override, retain: '1' });
    const pure = run(f.zero);
    assert.equal(pure.status, 0, pure.stderr);
    assert.deepEqual(JSON.parse(pure.stdout), { cache: override, retain: null });
});

test('metadata admits only exact format-version 1 for the generated helper', t => {
    const f = fixture(t);
    const branch = args => [f.mark(), f.jetExec(), fork(10, 20), exec(20, f.tools.cargo, [f.tools.cargo, ...args])];
    const receipt = f.audit(branch(['metadata', '--format-version', '1', '--manifest-path', f.manifest]));
    pass(receipt);
    assert.equal(receipt.allowedFfiBuilds[0].command, 'metadata');
    pass(f.audit(branch(['metadata', '--format-version', '1', `--manifest-path=${f.manifest}`])));
    for (const args of [
        ['metadata', '--format-version', '2', '--manifest-path', f.manifest],
        ['metadata', '--format-version', '1', '--manifest-path', f.manifest, '--no-deps'],
        ['metadata', '--format-version', '1'],
        ['metadata', '--format-version', '1', '--manifest-path', f.manifest, '--config', 'foo=1'],
    ]) fail(f.audit(branch(args)));
});

test('retained sandbox binds exact readonly helper and writable output for build and metadata', t => {
    const f = fixture(t), workspace = join(f.cache, 'a'.repeat(64), '.trace-workspaces', '20-0');
    const source = join(workspace, 'source'), output = join(workspace, 'output');
    const manifest = f.put(join(source, 'Cargo.toml'), f.contents);
    f.put(join(source, 'src/lib.rs'), '// synthetic\n');
    const metadata = f.put(join(output, 'metadata-source/Cargo.toml'), f.contents);
    f.put(join(output, 'metadata-source/src/lib.rs'), '// synthetic\n');
    const bwrap = (changes = {}) => exec(20, f.tools.bwrap, [f.tools.bwrap, '--clearenv',
        changes.sourceMode ?? '--ro-bind', changes.source ?? source, '/work/source',
        '--bind', changes.output ?? output, '/work/output', '--chdir', '/work/source',
        ...(changes.extra ?? []), '--', f.tools.cargo]);
    const branch = (command = 'build', changes = {}) => [f.mark(), f.jetExec(), fork(10, 20), bwrap(changes), chdir(20, '/work/source'), exec(20, f.tools.cargo,
        [f.tools.cargo, command, ...(command === 'metadata' ? ['--format-version', '1'] : []), '--manifest-path', command === 'metadata' ? '/work/output/metadata-source/Cargo.toml' : '/work/source/Cargo.toml']), fork(20, 30), f.rustc()];
    const build = f.audit(branch());
    pass(build);
    assert.equal(build.allowedFfiBuilds[0].manifestPath, manifest);
    assert.equal(build.allowedFfiBuilds[0].sandbox.source, source);
    const inspection = f.audit(branch('metadata'));
    pass(inspection);
    assert.equal(inspection.allowedFfiBuilds[0].manifestPath, metadata);
    assert.equal(inspection.allowedFfiBuilds[0].manifestSha256, createHash('sha256').update(f.contents).digest('hex'));
    for (const changes of [
        { sourceMode: '--bind' }, { source: join(f.home, 'outside') },
        { output: source }, { output: join(workspace, 'other') },
        { extra: ['--bind', output, '/work/source/src'] },
        { extra: ['--tmpfs', '/work/output/metadata-source'] },
    ]) fail(f.audit(branch('build', changes)), 'invalid-foreign-sandbox');
    f.put(manifest, f.contents.replace('0.22', '0.23'));
    fail(f.audit(branch()));
});

test('std-only fixed companions require their own marker and empty-dependency helper', t => {
    const f = fixture(t), companion = 'tests/ui/ffi_bad_type.fixed.jet';
    const source = 'extern rust "std" { fn identity(s: String) -> String = "std::convert::identity" }\n';
    f.put(join(f.root, f.name), source);
    f.put(join(f.root, companion), source);
    const allowlistText = allowRow(companion, {});
    assert.deepEqual(readUserRustAllowlist(allowlistText, f.root).get(companion).dependencies, {});
    assert.throws(() => readUserRustAllowlist(allowRow(f.zero, {}), f.root));
    f.put(join(f.root, companion), `${source}extern rust "base64@0.22" {}\n`);
    assert.throws(() => readUserRustAllowlist(allowlistText, f.root));
    assert.deepEqual(readUserRustAllowlist(allowRow(companion), f.root).get(companion).dependencies, declared);
    f.put(join(f.root, companion), source);
    const cache = ffiCaseCachePath(f.cacheRoot, companion, {});
    const contents = f.contents.replace('base64 = "0.22"\n', '');
    const manifest = f.put(join(cache, 'helper/Cargo.toml'), contents);
    f.put(join(dirname(manifest), 'src/lib.rs'), '// synthetic bytes; never compiled\n');
    const branch = marker => [
        f.mark(10, marker),
        exec(10, f.jet, [f.jet, 'run', join(f.root, companion)]),
        fork(10, 20), f.cargo(20, manifest), fork(20, 30), f.rustc(),
    ];
    const receipt = f.audit(branch(companion), { allowlistText });
    pass(receipt);
    assert.deepEqual(receipt.allowedFfiBuilds[0].dependencies, {});
    fail(f.audit(branch(f.name), { allowlistText }));
    f.put(manifest, f.contents);
    fail(f.audit(branch(companion), { allowlistText }));
});

test('missing-Cargo marker clears only its private warmed helper and preserves PATH', t => {
    const f = fixture(t), name = 'tests/ui/ffi_no_cargo.jet';
    const cacheRoot = realpathSync(mkdtempSync(join(homedir(), '.jet-native-ffi-unit-')));
    t.after(() => rmSync(cacheRoot, { recursive: true, force: true }));
    f.put(join(f.root, name), 'extern rust "base64@0.22" { fn encode(s: String) -> String = "base64::encode" }\n');
    f.put(join(f.root, 'Tools/testkit/user-rust-ffi.allow'), allowRow(name));
    const cache = ffiCaseCachePath(cacheRoot, name, declared);
    const warmed = f.put(join(cache, 'helper/Cargo.toml'), f.contents);
    const otherCache = ffiCaseCachePath(cacheRoot, f.name, declared);
    const other = f.put(join(otherCache, 'helper/Cargo.toml'), f.contents);
    const path = join(f.home, 'empty-path');
    mkdirSync(path);
    const probe = 'const fs=require("node:fs"); console.log(JSON.stringify({path:process.env.PATH,cache:process.env.JET_FFI_CACHE_DIR,warm:fs.existsSync(process.argv[1]),other:fs.existsSync(process.argv[2])}));';
    const result = spawnSync(process.execPath, [
        new URL('./native-command.mjs', import.meta.url).pathname,
        '--case', name, '--root', f.root, '--cwd', f.root, '--jet', process.execPath,
        '--cache-root', cacheRoot, '--ffi-cache', '', '--', process.execPath, '-e', probe, warmed, other,
    ], { env: { ...process.env, PATH: path, JET_FFI_CACHE_DIR: '' }, encoding: 'utf8', timeout: 10000 });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), { path, cache, warm: false, other: true });
});

test('cache hash includes the canonical case and sorted dependency entries', t => {
    const f = fixture(t), deps = { zed: '2', alpha: '1' };
    const digest = createHash('sha256').update(`${f.name}\n${JSON.stringify([['alpha', '1'], ['zed', '2']])}`).digest('hex');
    assert.equal(ffiCaseCachePath(f.cacheRoot, f.name, deps), join(f.cacheRoot, 'native-ffi', digest));
    assert.equal(ffiCaseCachePath(f.cacheRoot, f.name, deps), ffiCaseCachePath(f.cacheRoot, f.name, { alpha: '1', zed: '2' }));
    assert.notEqual(ffiCaseCachePath(f.cacheRoot, f.zero, deps), ffiCaseCachePath(f.cacheRoot, f.name, deps));
});

test('zero-Rust cases are explicit and arbitrary argument text does not classify a tool', t => {
    const f = fixture(t);
    const receipt = f.audit([f.mark(10, f.zero), exec(10, f.jet, [f.jet, 'run', join(f.root, f.zero)]), fork(10, 20), exec(20, '/bin/echo', ['echo', 'cargo', 'rustc', '/usr/bin/clang'])]);
    pass(receipt);
    assert.equal(receipt.schema, 'jet-native-case-trace/v1');
    assert.deepEqual(receipt.zeroRustCases, [f.zero]);
    assert.deepEqual(receipt.allowedFfiBuilds, []);
    assert.deepEqual(receipt.excludedPerformancePeers, []);
    assert.equal(receipt.cases[0].case, f.zero);
    assert.equal(receipt.events, 4);
});

test('compiler Rust is denied without markers and without an allowlisted case', t => {
    const f = fixture(t);
    fail(f.audit([f.jetExec(), fork(10, 20), f.cargo()]));
    fail(f.audit([f.mark(10, f.zero), f.jetExec(), fork(10, 20), f.cargo()]));
    fail(f.audit([f.mark(), f.jetExec(), fork(10, 20), f.cargo()], { allowlistText: '' }));
    fail(f.audit([f.mark(), f.cargo(10)]));
    fail(f.audit([f.mark(), f.jetExec(), f.rustc(10)]));
    fail(f.audit([f.mark(), f.jetExec(), fork(10, 30), f.rustc()]));
});

test('only exact allowlisted foreign rustc identity query is admitted', t => {
    const f = fixture(t);
    const query = exec(20, f.tools.rustc, [f.tools.rustc, '-vV']);
    const receipt = f.audit([f.mark(), f.jetExec(), fork(10, 20), query]);
    pass(receipt);
    assert.equal(receipt.allowedFfiQueries.length, 1);
    assert.equal(receipt.allowedFfiQueries[0].cachePath, f.cache);
    assert.deepEqual(receipt.zeroRustCases, []);
    fail(f.audit([f.mark(10, f.zero), f.jetExec(), fork(10, 20), query]));
    fail(f.audit([f.mark(), f.jetExec(), fork(10, 20), exec(20, f.tools.rustc, [f.tools.rustc, '-vV', 'program.rs'])]));
    fail(f.audit([f.mark(), f.jetExec(), fork(10, 20), exec(20, f.tools.rustc, [f.tools.rustc, '--print', 'sysroot'])]));
    fail(f.audit([f.mark(), f.jetExec(10, 'self'), fork(10, 20), query]));
});

test('approved generated helper records bytes, digest, dependencies, PIDs and ancestry', t => {
    const f = fixture(t), receipt = f.audit(f.branch());
    pass(receipt);
    assert.deepEqual(receipt.zeroRustCases, []);
    assert.equal(receipt.allowedFfiBuilds.length, 1);
    const build = receipt.allowedFfiBuilds[0];
    assert.equal(build.manifestPath, realpathSync(f.manifest));
    assert.equal(build.manifestContents, readFileSync(f.manifest, 'utf8'));
    assert.equal(build.manifestSha256, createHash('sha256').update(f.contents).digest('hex'));
    assert.deepEqual(build.dependencies, declared);
    assert.deepEqual(build.pids, [20, 30]);
    assert.equal(build.jetPid, 10);
    assert.equal(build.cargoPid, 20);
    assert.ok(build.executions.at(-1).ancestry.some(item => item.executable === f.tools.cargo));
    pass(f.audit([f.mark(), f.jetExec(), fork(10, 20), f.cargo(20, f.manifest, true), fork(20, 30), f.rustc()]));
    f.put(f.manifest, f.contents.replace('base64 = "0.22"', 'base64 = { version = "0.22" }'));
    pass(f.audit(f.branch()));
});

test('expanded environment must match cache; approved Cargo cannot admit sibling compiler Rust', t => {
    const f = fixture(t);
    const cargoWithEnv = env => exec(20, f.tools.cargo, [f.tools.cargo, 'build', '--manifest-path', f.manifest], '0', argv(env));
    pass(f.audit([f.mark(), f.jetExec(), fork(10, 20), cargoWithEnv([`JET_FFI_CACHE_DIR=${f.cache}`]), fork(20, 30), f.rustc()]));
    fail(f.audit([f.mark(), f.jetExec(), fork(10, 20), cargoWithEnv([`JET_FFI_CACHE_DIR=${f.cacheRoot}`])]));
    fail(f.audit([f.mark(), f.jetExec(), fork(10, 20), cargoWithEnv([])]));
    fail(f.audit([...f.branch(), fork(10, 40), f.rustc(40)]));
    fail(f.audit([...f.branch(), fork(30, 40), f.jetExec(40), f.rustc(40)]));
    fail(f.audit([f.mark(), f.jetExec(), fork(10, 20), f.mark(20), f.cargo()]));
    pass(f.audit([f.mark(), f.jetExec(), fork(10, 20), f.mark(20), f.jetExec(20), f.cargo()]));
});

test('case digest directory may not redirect to a different cache namespace', t => {
    const f = fixture(t);
    const moved = join(f.cacheRoot, 'native-ffi', 'different-digest');
    f.put(join(moved, 'helper/Cargo.toml'), f.contents);
    f.put(join(moved, 'helper/src/lib.rs'), '// synthetic');
    rmSync(f.cache, { recursive: true });
    symlinkSync(moved, f.cache);
    fail(f.audit(f.branch()));
});

test('wrong manifest, dependencies, case cache, digest and symlink escapes fail closed', t => {
    const f = fixture(t);
    const foreign = f.put(join(f.home, 'outside/Cargo.toml'), f.contents);
    fail(f.audit([f.mark(), f.jetExec(), fork(10, 20), f.cargo(20, foreign)]));
    const wrongDigest = f.put(join(f.cacheRoot, 'native-ffi/wrong/helper/Cargo.toml'), f.contents);
    fail(f.audit([f.mark(), f.jetExec(), fork(10, 20), f.cargo(20, wrongDigest)]));
    fail(f.audit([f.mark(), f.jetExec(), fork(10, 20), f.cargo(20, join(f.cache, 'missing/Cargo.toml'))]));
    const escape = join(f.cache, 'escape'); symlinkSync(dirname(foreign), escape);
    fail(f.audit([f.mark(), f.jetExec(), fork(10, 20), f.cargo(20, join(escape, 'Cargo.toml'))]));
    for (const replacement of ['base64 = "0.23"', 'base64 = { version = "0.22", path = "../outside" }', 'base64 = { git = "https://example.invalid" }', 'base64 = "0.22"\nother = "1"']) {
        f.put(f.manifest, f.contents.replace('base64 = "0.22"', replacement)); fail(f.audit(f.branch()));
    }
    f.put(f.manifest, f.contents);
    const other = 'Examples/other-ffi.jet'; f.put(join(f.root, other), 'extern rust "base64@0.22" {}');
    fail(f.audit([f.mark(10, other), f.jetExec(), fork(10, 20), f.cargo()], { allowlistText: f.allowlistText + allowRow(other) }));
});

test('manifest workspace, build-script, library path and package overrides are rejected', t => {
    const f = fixture(t);
    for (const suffix of ['\n[workspace]\nmembers = ["../escape"]\n', '\n[package.metadata.jet]\nbuild = "build.rs"\n', '\n[build-dependencies]\ncc = "1"\n', '\n[patch.crates-io]\nbase64 = "0.22"\n', '\n[[bin]]\nname = "compiler"\n']) {
        f.put(f.manifest, f.contents + suffix); fail(f.audit(f.branch()));
    }
    f.put(f.manifest, f.contents + '\n[workspace]\n\n[package.metadata.jet]\nnative-helper-artifact-mode = "jet-native-rust-staticlib-v1"\n'); pass(f.audit(f.branch()));
    f.put(f.manifest, f.contents.replace('edition = "2021"', 'edition = "2021"\nbuild = "build.rs"')); fail(f.audit(f.branch()));
    const outside = f.put(join(f.home, 'outside.rs'), '// outside');
    f.put(f.manifest, f.contents.replace('path = "src/lib.rs"', `path = ${quote(outside)}`)); fail(f.audit(f.branch()));
    f.put(f.manifest, f.contents); f.put(join(dirname(f.manifest), 'build.rs'), '// implicit'); fail(f.audit(f.branch()));
});

test('trusted marker validates root, jet, source case, wrapper and external namespace', t => {
    const f = fixture(t);
    for (const overrides of [{ '--root': f.home }, { '--jet': f.tools.rustc }, { '--cache-root': f.root }, { '--case': '../outside.jet' }, { '--case': './Examples/zero.jet' }]) {
        fail(f.audit([f.mark(10, f.name, overrides), f.jetExec(), fork(10, 20), f.cargo()]), 'invalid-marker-or-launcher');
    }
    const counterfeit = f.put(join(f.home, 'fake/native-command.mjs'));
    fail(f.audit([f.mark().replace(quote(f.wrapper), quote(counterfeit)), f.jetExec(), fork(10, 20), f.cargo()]), 'invalid-marker-or-launcher');
    fail(f.audit([exec(10, process.execPath, [process.execPath, f.wrapper, '--case', f.name]), f.jetExec(), f.cargo(10)]), 'invalid-marker-or-launcher');
});

test('maintenance and Rust emission Jet ancestry cannot admit Cargo', t => {
    const f = fixture(t);
    for (const command of ['emit', 'fmt', 'cache', 'bootstrap']) fail(f.audit([f.mark(), f.jetExec(10, command), fork(10, 20), f.cargo()]));
    for (const extra of [['--emit=rust'], ['--emit', 'rust'], ['--backend=rust'], ['--codegen', 'rust']]) fail(f.audit([f.mark(), f.jetExec(10, 'build', extra), fork(10, 20), f.cargo()]));
});

test('ld.lld only below admitted FFI Rustc; other LLVM tools and rustup always denied', t => {
    const f = fixture(t);
    const allowed = f.audit([...f.branch(), fork(30, 40), exec(40, f.tools['ld.lld'])]);
    pass(allowed); assert.deepEqual(allowed.allowedFfiBuilds[0].pids, [20, 30, 40]);
    fail(f.audit([f.mark(), f.jetExec(), fork(10, 20), f.cargo(), fork(20, 40), exec(40, f.tools['ld.lld'])]));
    for (const name of ['rustup', 'clang', 'clang++', 'llc', 'opt']) fail(f.audit([...f.branch(), fork(30, 40), exec(40, f.tools[name])]));
    for (const name of ['cargo-1.90', 'rustc-1.90.0', 'clang-19', 'clang++-19', 'llc-19', 'opt-19', 'ld.lld-19', 'rustup-1']) fail(f.audit([exec(10, join(f.home, 'tools', name))]));
    const disguised = join(f.home, 'tools/innocent'); symlinkSync(f.tools.rustc, disguised); fail(f.audit([exec(10, disguised)]));
});

test('only direct source-root performance Rustc peers are excluded, not their Jet Rust or Cargo', t => {
    const f = fixture(t);
    for (const launcher of [f.peer, f.perf]) {
        const start = exec(10, process.execPath, [process.execPath, launcher]);
        const receipt = f.audit([start, fork(10, 30), f.rustc()]); pass(receipt); assert.equal(receipt.excludedPerformancePeers.length, 1);
        fail(f.audit([start, fork(10, 20), f.jetExec(20), fork(20, 30), f.rustc()]));
        fail(f.audit([start, fork(10, 20), f.cargo()]));
        fail(f.audit([start, fork(10, 20), exec(20, '/bin/sh', ['sh', '-c', 'rustc']), fork(20, 30), f.rustc()]));
    }
    const fake = f.put(join(f.home, 'Tools/perf/peer.mjs'));
    fail(f.audit([exec(10, process.execPath, [process.execPath, fake]), fork(10, 30), f.rustc()]));
    fail(f.audit([exec(10, process.execPath, [process.execPath, '-e', quote(f.peer)]), fork(10, 30), f.rustc()]));
});

test('unfinished/resumed fork and child-before-parent completed fork preserve ancestry', t => {
    const f = fixture(t);
    const receipt = f.audit([
        f.mark(), f.jetExec(),
        '10 12:34:56.000001 clone(child_stack=NULL, flags=SIGCHLD <unfinished ...>',
        `[pid 20] 12:34:56.000002 ${f.cargo().replace(/^20 /, '')}`,
        '10 12:34:56.000003 <... clone resumed>) = 20',
        f.rustc(), fork(20, 30),
    ]);
    pass(receipt); assert.equal(receipt.allowedFfiBuilds[0].jetPid, 10);
    pass(f.audit([f.mark(), f.jetExec(), f.cargo(), fork(10, 20), f.rustc(), fork(20, 30)]));
    const split = exec(10, f.jet, [f.jet, 'run', join(f.root, f.name)]).split(', ');
    pass(f.audit([f.mark(), `${split[0]}, <unfinished ...>`, `10 <... execve resumed>${split.slice(1).join(', ')}`, fork(10, 20), f.cargo()]));
});

test('cwd is inherited, CLONE_FS changes propagate, and exec unshares cwd', t => {
    const f = fixture(t);
    const directory = dirname(f.manifest);
    const cargoByCwd = exec(10, f.tools.cargo, [f.tools.cargo, 'build']);
    pass(f.audit([chdir(10, f.root), f.mark(), f.jetExec(), fork(10, 20, 'CLONE_VM|CLONE_FS|CLONE_THREAD'), chdir(20, directory), cargoByCwd, fork(10, 30), f.rustc()]));
    pass(f.audit([chdir(10, directory), f.mark(10, f.name, { '--cwd': directory }), f.jetExec(), fork(10, 20), exec(20, f.tools.cargo, [f.tools.cargo, 'build']), fork(20, 30), f.rustc()]));
    pass(f.audit([f.mark(), f.jetExec(), `10 fchdir(7<${directory}>) = 0`, cargoByCwd]));
    // A sibling exec detaches CLONE_FS before chdir; the parent stays at root.
    fail(f.audit([chdir(10, f.root), f.mark(), f.jetExec(), fork(10, 20, 'CLONE_FS|SIGCHLD'), exec(20, '/bin/echo'), chdir(20, directory), cargoByCwd]));
    fail(f.audit([f.mark(), f.jetExec(), cargoByCwd]));
    fail(f.audit([f.mark(), f.jetExec(), '10 fchdir(7) = 0', f.cargo(10)]), 'ambiguous-cwd');
});

test('overlapping shared cwd mutations and executions are ambiguous, never admitted', t => {
    const f = fixture(t), directory = dirname(f.manifest);
    fail(f.audit([
        f.mark(), f.jetExec(), fork(10, 20, 'CLONE_FS|SIGCHLD'),
        `20 chdir(${quote(directory)} <unfinished ...>`,
        f.cargo(10),
        '20 <... chdir resumed>) = 0',
    ]), 'ambiguous-shared-cwd');
    const cargoLine = f.cargo(10).replace(/ = 0$/, ' <unfinished ...>');
    fail(f.audit([
        f.mark(), f.jetExec(), fork(10, 20, 'CLONE_FS|SIGCHLD'),
        cargoLine, chdir(20, directory),
        '10 <... execve resumed> = 0',
    ]), 'ambiguous-shared-cwd');
});

test('pid-less and epoch timestamp syscall forms are accepted', t => {
    const f = fixture(t);
    pass(f.audit([f.mark(10, f.zero), exec(10, f.jet, [f.jet, 'run', join(f.root, f.zero)])].map(line => line.replace(/^10 /, '1730000000.123456 '))));
    pass(f.audit([f.mark(10, f.zero), exec(10, f.jet, [f.jet, 'run', join(f.root, f.zero)])].map(line => line.replace(/^10 /, ''))));
});

test('execveat paths, C escapes and failed exec attempts are reconstructed safely', t => {
    const f = fixture(t);
    const first = f.audit([f.mark(10, f.zero), `10 execveat(AT_FDCWD, ${quote(f.jet)}, ${argv([f.jet, 'run', join(f.root, f.zero)])}, 0xabc /* 2 vars */, 0) = 0`]);
    pass(first);
    fail(f.audit([`10 execveat(7<${dirname(f.tools.rustc)}>, "rustc", ["rustc"], 0xabc /* 2 vars */, 0) = 0`]));
    fail(f.audit([`10 execveat(7<${f.tools.rustc}>, "", ["rustc"], 0xabc /* 2 vars */, AT_EMPTY_PATH) = 0`]));
    const escaped = quote(f.tools.rustc).replace('rustc', '\\162ustc'); fail(f.audit([`10 execve(${escaped}, ["rustc"], 0xabc /* 2 vars */) = 0`]));
    const receipt = f.audit([f.mark(10, f.zero), exec(10, f.tools.rustc, ['rustc'], '-1 ENOENT (No such file or directory)'), exec(10, f.jet, [f.jet, 'run', join(f.root, f.zero)])]);
    pass(receipt); assert.equal(receipt.failedExecAttempts.length, 1); assert.deepEqual(receipt.zeroRustCases, [f.zero]);
    fail(f.audit([`10 execve("relative/rustc", ["rustc"], 0xabc /* 2 vars */) = 0`]), 'ambiguous-execution');
});

test('truncation, malformed/unfinished calls and ambiguous results cannot silently pass', t => {
    const f = fixture(t);
    fail(f.audit(''), 'empty-trace');
    fail(f.audit([f.mark(), '10 execve("/usr/bin/rustc", ["rustc", ...], 0xabc /* 2 vars */) = 0']), 'ambiguous-execution');
    fail(f.audit([f.mark(), '10 execve("/usr/bin/rustc"..., ["rustc"], 0xabc /* 2 vars */) = 0']), 'ambiguous-execution');
    fail(f.audit([f.mark(), '10 execve("/usr/bin/rustc", ["rustc"], 0xabc /* 2 vars */ <unfinished ...>']), 'incomplete-syscall');
    fail(f.audit([f.mark(), '10 <... execve resumed>) = 0']), 'unmatched-resumed');
    fail(f.audit([f.mark(), exec(10, f.tools.rustc, ['rustc'], '?')]), 'ambiguous-exec-result');
    fail(f.audit([f.mark(), 'strace: Process 10 detached']), 'trace-diagnostic');
});
