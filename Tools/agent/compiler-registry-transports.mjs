// Registry transports use the existing native fixture runners and their real native/C verdicts.
// One counted test is one complete source-registry verdict, not each internal assert.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { runNativeLibrary, runNativeProgram, stageNativeToolchain } from './compiler-native-transport.mjs';

const adapter = fileURLToPath(import.meta.url);
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const rt = 'Compiler/JetRuntime/Tests/';
const be = 'Compiler/JetBackend/Tests/';
function suiteLibrary(spec, out) {
    if (spec.name === 'SharedThread') return { file: join(out, 'shared/Source/Fixture.jet'), prefix: join(out, 'shared/fixture'), lifecycle: join(out, 'shared/lifecycle.list') };
    const folder = spec.name === 'CollectionsStop' ? out : join(out, spec.runner.endsWith('run-channel-fixtures.mjs') || spec.name === 'SharedSource' ? 'fixture' : 'unit');
    return { file: join(folder, 'Source/Fixture.jet'), prefix: join(folder, 'fixture'), lifecycle: join(folder, 'lifecycle.list'), exports: spec.crypto ? join(folder, 'exports.list') : null };
}
const crypto = new Set(['AgeCodec', 'AgeV1', 'CryptoAead', 'CryptoArgon', 'CryptoBlake3', 'CryptoChaCha', 'CryptoEd25519', 'CryptoEd25519Signature', 'CryptoEntropy', 'CryptoHash', 'CryptoHkdf', 'CryptoOsEntropy', 'CryptoPbkdf2', 'CryptoSha3', 'CryptoX25519', 'CryptoZeroize', 'Sha256']);
const plain = { Alloc: ['alloc_tests', 'alloc tests: ok\n'], Collections: ['collections_tests', ''], Scan: ['scan_tests', ''], Text: ['text_tests', ''], Unicode: ['unicode_tests', ''], Secret: [null, 'secret tests: ok\n'] };
const suites = {
    AllocationPolicy: ['run-world-sentry-fixtures.mjs', ['assemble', 'lower', 'link', 'run']],
    World: ['run-world-sentry-fixtures.mjs', ['assemble', 'lower', 'link', 'run']],
    Sentry: ['run-world-sentry-fixtures.mjs', ['assemble', 'lower', 'link', 'run']],
    SentryInline: ['run-world-sentry-fixtures.mjs', ['assemble', 'lower', 'link', 'run']],
    TestScope: ['run-world-sentry-fixtures.mjs', ['assemble', 'lower', 'link', 'run']],
    Channel: ['run-channel-fixtures.mjs', ['assemble', 'lower', 'link', 'run']],
    Stm: ['run-channel-fixtures.mjs', ['assemble-stm', 'lower', 'link-stm', 'run-stm']],
    Sync: ['run-channel-fixtures.mjs', ['assemble-sync', 'lower', 'link-sync', 'run-sync']],
    Observe: ['run-channel-fixtures.mjs', ['assemble-observe', 'lower', 'link-observe', 'run-observe']],
    SharedPublication: ['run-channel-fixtures.mjs', ['assemble-publication', 'lower', 'link-publication', 'run-publication']],
    SharedSource: ['run-shared-source-fixtures.mjs', ['assemble', 'lower', 'link', 'run']],
    SharedThread: ['run-thread-fixtures.mjs', ['assemble', 'lower', 'shared-link', 'shared-run']],
    Parallel: ['run-parallel-fixtures.mjs', ['assemble', 'lower', 'link', 'run']],
    Scheduler: ['run-scheduler-fixtures.mjs', ['assemble', 'lower', 'link', 'run']],
    TestSuite: ['run-testsuite-fixtures.mjs', ['assemble', 'lower', 'link', 'run']],
    CollectionsStop: ['run-collections-stop.mjs', ['--assemble', '--lower', '--run']],
};
function specification(id) {
    if (id.startsWith('JetRuntime/Tests/')) {
        const name = id.slice('JetRuntime/Tests/'.length).replace(/Tests$/, '');
        if (crypto.has(name)) return { type: 'suite', runner: rt + 'run-crypto-fixtures.mjs', modes: ['assemble', 'lower', 'link', 'run'], crypto: true };
        if (plain[name]) return { type: 'plain', name, call: plain[name][0], expected: plain[name][1], runner: rt + 'check-runtime.mjs' };
        if (['CodecCSV', 'CodecEnv', 'DynCodec', 'CompilerFileLeaves'].includes(name)) return { type: 'codec-fixture', name, runner: rt + 'run-native-codec-file-leaves.mjs' };
        if (['ProcessExecution', 'TerminalProcess'].includes(name)) return { type: 'process-fixture', name, runner: rt + 'run-process-witnesses.mjs', witness: rt + name + 'Witness.c', expectedFile: rt + name + 'Witness.stdout' };
        if (['FilesWalk', 'HostParallelism'].includes(name)) return { type: 'native-suite', name, runner: rt + (name === 'FilesWalk' ? 'run-files-walk-fixtures.mjs' : 'run-host-parallelism-fixtures.mjs'), witness: rt + name + 'Witness.c', modes: ['assemble', 'lower', 'audit', 'link', 'run'] };
        if (suites[name]) return { type: 'suite', runner: rt + suites[name][0], modes: suites[name][1], name };
        if (['EnvironmentLeaves', 'FaultSchedule', 'IterCharge', 'ProgressState', 'HostFault', 'FloatBits', 'TerminalColor', 'TrapReport', 'VaultHooks', 'VaultProvider', 'Thread', 'HostSpawn', 'CompilerEnv', 'CompilerRoutes', 'CompilerNeededJSON', 'CompilerNeededURL', 'CompilerNeededMathText', 'DynNative'].includes(name)) return { type: 'c', name, runner: be + 'run-runtime.mjs' };
        return { blocked: `No registered transport for ${id}; the source must not be counted without an exact native verdict` };
    }
    if (!id.startsWith('JetBackend/Tests/')) return null;
    const name = id.slice('JetBackend/Tests/'.length);
    if (name === 'LinkIndexes') return { type: 'indices', runner: be + 'run-link.mjs' };
    if (name === 'LinkPackRoots') return { type: 'roots', runner: be + 'run-runtime-pack-roots.mjs' };
    if (name === 'LinkRuntime') return { type: 'link', runner: be + 'run-link.mjs' };
    return null;
}
export function prepareTransport(row, { repo, out }) {
    // A registry fragment lives in its Tests package's fixture directory; its identity and C witness are
    // named from the Tests package itself.
    const home = row.source.replace(/\/[A-Za-z0-9]+\.fixture\//, '/');
    const id = home.replace(/^Compiler\//, '').replace(/\.jet$/, '');
    const spec = specification(id);
    if (!spec) return null;
    const entry = { id, package: row.package, mode: 'run', kind: 'transport', path: adapter,
        cwd: join(out, id), sha256: digest(readFileSync(adapter)), testCount: 1,
        stdout: `JET_COMPILER_REGISTRY PASS ${id} tests=1\n`, sources: [row.source], sourceManifests: ['Compiler/JetRuntime/sources.list'] };
    const witness = home.replace(/\.jet$/, '.c');
    if (spec.witness) entry.sources.push(spec.witness);
    if (spec.expectedFile) entry.sources.push(spec.expectedFile);
    if (spec.type === 'c' && existsSync(join(repo, witness))) entry.sources.push(witness);
    if (spec.blocked) return { ...entry, status: 'BLOCKED', reason: spec.blocked };
    return { ...entry, transport: { ...spec, runnerSha256: digest(readFileSync(join(repo, spec.runner))) } };
}

// Plans expose the executable boundary without invoking a compiler or C linker.
// Internal transformations are named explicitly and performed only by runTransport.
export function planTransport(entry, { jet, repo, runtimePack, runtimeDir = dirname(dirname(runtimePack)) }) {
    const spec = specification(entry.id), out = entry.cwd;
    if (!spec || spec.blocked) return { id: entry.id, status: 'BLOCKED', reason: spec?.blocked ?? 'unknown registry' };
    const steps = [], node = (file, ...args) => steps.push({ command: process.execPath, args: [join(repo, file), ...args], cwd: out });
    const build = file => steps.push({ command: jet, args: ['build', file], cwd: dirname(file) });
    const run = (command, args = [], expected = null, env = {}) => steps.push({ command, args, cwd: out, env, ...(expected ? { expected } : {}) });
    const checkLibrary = (file, prefix, lifecycle, exports = null) => run(jet, ['check', file], null, { JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix, JET_FRONT_LIBRARY_LIFECYCLE: lifecycle, ...(exports ? { JET_FRONT_LIBRARY_EXPORTS: exports } : {}) });
    const cc = (source, object, binary) => run(process.env.CC ?? 'cc', ['-std=c11', '-Wall', '-Wextra', '-Werror', source, object, '-lpthread', '-ldl', '-lm', '-o', binary]);
    if (spec.type === 'codec-fixture') {
        node(spec.runner, join(out, 'codec'), jet, runtimeDir, '--test', spec.name);
    } else if (spec.type === 'process-fixture') {
        node(spec.runner, join(out, 'process'), jet, runtimeDir, '--test', spec.name);
    } else if (spec.type === 'native-suite') {
        for (const mode of spec.modes) run(process.execPath, [join(repo, spec.runner), out, mode], null, { JET_FRONT_JETC0: jet });
    } else
    if (spec.type === 'suite') {
        node(spec.runner, out, spec.modes[0]);
        const library = suiteLibrary(spec, out); checkLibrary(library.file, library.prefix, library.lifecycle, library.exports);
        for (const mode of spec.modes.slice(2)) node(spec.runner, out, mode);
    } else if (spec.type === 'plain') {
        node(spec.runner, join(out, 'raw'), '--assemble-only', '--test', spec.name);
        steps.push({ action: 'copy assembled runtime to isolated native project and append existing witness entry', call: spec.call });
        build(join(out, 'native/unit.jet')); run(join(out, 'native/registry-native.exe'), [], { code: 0, stdout: spec.expected, stderr: '' });
    } else if (spec.type === 'c') {
        node(spec.runner, out, '--until', 'assemble', '--test', spec.name);
        checkLibrary(join(out, 'library/Source/Runtime.jet'), join(out, 'jet_runtime_jet'), join(repo, 'Compiler/JetRuntime/lifecycle.list'));
        cc(['VaultHooks', 'VaultProvider'].includes(spec.name) ? join(repo, rt + spec.name + 'Witness.c') : spec.name === 'Thread' ? join(repo, rt + 'thread-fixture.c') : ['HostSpawn', 'CompilerEnv', 'CompilerRoutes'].includes(spec.name) ? join(repo, rt + spec.name + 'Tests.c') : join(out, 'driver.c'), join(out, 'jet_runtime_jet.o'), join(out, 'driver'));
        if (spec.name === 'TrapReport') {
            for (let kind = 0; kind < 5; kind++) for (const mode of ['direct', 'captured']) {
                run(join(out, 'driver'), [String(kind), mode], { code: 70, stdout: '' });
                steps.at(-1).comparison = 'committed simple-report literal and complete direct/captured byte equality';
            }
        } else {
            run(join(out, 'driver'), spec.name === 'CompilerRoutes' ? [out] : []); if (spec.name === 'HostFault') run(join(out, 'driver'), ['unreachable']);
            if (spec.name === 'CompilerEnv') run(join(out, 'driver'), ['empty']);
            if (spec.name === 'DynNative') run(join(out, 'driver'), ['datetime'], { code: 70, stdout: '' });
        }
    } else if (spec.type === 'direct') {
        steps.push({ action: 'copy real source to isolated project', source: join(repo, spec.runner) }); build(join(out, 'unit.jet')); run(join(out, 'registry-native.exe'), [], { code: 0, stdout: spec.expected, stderr: '' });
    } else if (spec.type === 'roots' || spec.type === 'link') {
        node(be + 'run-runtime-pack-roots.mjs', out, '--prepare-only');
        if (spec.type === 'link') node(spec.runner, out);
        build(join(out, 'unit.jet')); run(join(out, spec.type === 'link' ? 'link-native.exe' : 'registry-native.exe'), spec.type === 'link' ? [join(out, 'consumer'), join(out, 'consumer.o'), join(out, 'runtime.o'), join(out, 'runtime.a')] : []);
        run(join(out, 'consumer'), [], { code: 0, stdout: '', stderr: '' });
    } else {
        node(spec.runner, out, '--indices');
        build(join(out, 'unit.jet')); run(join(out, 'registry-native.exe'));
    }
    return { id: entry.id, testCount: 1, inputs: { jet, runtimePack, runtimeObject: join(dirname(runtimePack), 'jet_runtime_jet.o'), provider: 'Jet-only; system C compiler permitted' }, steps };
}
function cSource(name, repo) {
    const head = '#define _GNU_SOURCE\n#include <stdint.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n';
    const exports = { FaultSchedule: 'jet_rt_fault_schedule_fixture', IterCharge: 'jet_rt_iter_charge_fixture', ProgressState: 'jet_rt_progress_state_fixture', CompilerNeededJSON: 'jet_rt_compiler_needed_json_fixture', CompilerNeededURL: 'jet_rt_compiler_needed_url_fixture', CompilerNeededMathText: 'jet_rt_compiler_math_text_fixture' };
    if (exports[name]) return head + `extern char **environ;\nextern void jet_rt_host_arguments(int64_t,char **,char **);\nextern int64_t ${exports[name]}(void);\nint main(int argc,char **argv) { jet_rt_host_arguments(argc,argv,environ); return (int)${exports[name]}(); }\n`;
    if (name === 'DynNative') return head + 'extern char **environ;\nextern void jet_rt_host_arguments(int64_t,char **,char **);\nextern int64_t jet_rt_dyn_native_fixture(void);\nextern void jet_rt_dyn_native_datetime_trap_fixture(void);\nint main(int argc,char **argv) { jet_rt_host_arguments(argc,argv,environ); if(argc==2 && !strcmp(argv[1],"datetime")) { jet_rt_dyn_native_datetime_trap_fixture(); return 91; } return (int)jet_rt_dyn_native_fixture(); }\n';
    if (name === 'EnvironmentLeaves') return head + 'extern char **environ;\nextern int64_t jet_rt_environment_leaves_fixture(int64_t,int64_t,int64_t);\nint main(int argc,char **argv) { char invalid[]="JET_LEAF_INVALID=\\xff"; if (putenv(invalid)||setenv("JET_LEAF_EMPTY","",1)||unsetenv("JET_LEAF_MISSING")) return 100; return (int)jet_rt_environment_leaves_fixture(argc,(intptr_t)argv,(intptr_t)environ); }\n';
    if (name === 'HostFault') return head + 'extern int64_t jet_rt_host_fault_fixture(int64_t,_Bool);\nint main(int argc,char **argv) { (void)argv; return (int)jet_rt_host_fault_fixture((intptr_t)"A",argc>1); }\n';
    if (name === 'FloatBits') return head + 'extern int64_t jet_rt_test_f64_bits(double);\nextern double jet_rt_test_f64_from_bits(int64_t);\nint main(void) { const uint64_t words[]={0,UINT64_C(0x8000000000000000),1,UINT64_C(0x7ff0000000000000),UINT64_C(0xfff0000000000000),UINT64_C(0x7ff8000000000001),UINT64_C(0x3ff0000000000000),UINT64_C(0xffffffffffffffff)}; for(unsigned i=0;i<sizeof(words)/sizeof(*words);i++) { double value; memcpy(&value,&words[i],8); if((uint64_t)jet_rt_test_f64_bits(value)!=words[i]) return 1; value=jet_rt_test_f64_from_bits((int64_t)words[i]); uint64_t back; memcpy(&back,&value,8); if(back!=words[i]) return 2; } return 0; }\n';
    if (name === 'TerminalColor') return head + '#include <pthread.h>\nextern void jet_rt_thread_attach(void),jet_rt_thread_detach(void);\nextern int64_t jet_rt_terminal_color_fixture(void);\nstatic void *worker(void *unused) { (void)unused; jet_rt_thread_attach(); int64_t code=jet_rt_terminal_color_fixture(); jet_rt_thread_detach(); return (void *)(intptr_t)code; }\nint main(void) { if(jet_rt_terminal_color_fixture()) return 1; pthread_t thread; void *result; if(pthread_create(&thread,NULL,worker,NULL)||pthread_join(thread,&result)) return 2; return result!=NULL; }\n';
    if (name === 'TrapReport') return `#define main registry_cabi_main\n#include ${JSON.stringify(join(repo, rt + 'CAbiDriver.c'))}\n#undef main\nextern int64_t jet_rt_trap_report_fixture(int64_t,int64_t,_Bool);\nextern void jet_rt_panic_message(const void *,int64_t);\nstatic int64_t callback(int64_t kind) { if(kind==0) jet_rt_panic_message("registry boom",13); else if(kind==1) numeric_stop(); else if(kind==2) require(); else if(kind==3) require_eq(); else panic_case(); return 0; }\nint main(int argc,char **argv) { if(argc!=3) return 2; return (int)jet_rt_trap_report_fixture((intptr_t)callback,strtol(argv[1],NULL,10),!strcmp(argv[2],"captured")); }\n`;
    throw Error(`no C source ${name}`);
}

export async function runTransport(entry, { jet, repo, logDir, runtimePack, runtimeDir = dirname(dirname(runtimePack)), runtimeObject = join(dirname(runtimePack), 'jet_runtime_jet.o') }) {
    const spec = specification(entry.id), out = entry.cwd;
    if (!spec || spec.blocked) throw Error(spec?.blocked ?? `unknown transport ${entry.id}`);
    if (entry.testCount !== 1 || entry.sha256 !== digest(readFileSync(adapter)) || entry.transport?.runnerSha256 !== digest(readFileSync(join(repo, spec.runner)))) throw Error(`transport source/count changed: ${entry.id}`);
    mkdirSync(out, { recursive: true }); mkdirSync(logDir, { recursive: true });
    const evidence = [];
    // Production CLI resolves its runtime beside its executable. Stage exactly
    // the explicitly supplied pack, never use a previous installed runtime.
    const nativeJet = stageNativeToolchain({ jet, runtimePack, runtimeDir, out: join(logDir, 'toolchain') });
    const env = { ...process.env, JET_TOOLCHAIN_ROOT: repo, JET_KEEP_GOING: '1', JET: nativeJet, JET_RUN: nativeJet, JET_RUNTIME_PACK: runtimePack, JET_RUNTIME_JET_OBJECT: runtimeObject, JET_RUNTIME_OBJECT: runtimeObject, JET_RUNTIME_C_LIB: runtimeObject, NO_COLOR: '1', TERM: 'dumb' };
    for (const name of Object.keys(env)) if (/^JET_(?:BOOTSTRAP_|FRONT_)/.test(name) || name === 'JET_DUMP_MIR') delete env[name];
    for (const name of ['JET_TEST_FILTER', 'JET_NATIVE_TEST_ID', 'FX_ONLY', 'FX_HEAD', 'FX_EXCLUDE', 'SELFHOST_SKIP', 'SELFHOST_ROOTS']) delete env[name];
    function execute(command, args = [], options = {}) {
        const result = spawnSync(command, args, { cwd: out, env, encoding: 'utf8', maxBuffer: 128 << 20, timeout: 600000, ...options });
        const prefix = join(logDir, `${entry.id.replaceAll('/', '_')}-${evidence.length}`);
        writeFileSync(prefix + '.stdout', result.stdout ?? ''); writeFileSync(prefix + '.stderr', result.stderr ?? '');
        evidence.push({ command, args, cwd: options.cwd ?? out, code: result.status, signal: result.signal, error: result.error?.message ?? null, stdoutPath: prefix + '.stdout', stderrPath: prefix + '.stderr' });
        if (result.error || result.signal || result.status !== (options.expectedCode ?? 0)) throw Error(`${command}: ${result.error?.message ?? result.signal ?? result.status}; see ${prefix}`);
        return result;
    }
    const node = (runner, ...args) => execute(process.execPath, [join(repo, runner), ...args]);
    function exact(result, stdout = '', stderr = '') {
        if (result.stdout !== stdout || result.stderr !== stderr) throw Error(`exact native verdict differs: ${JSON.stringify({ stdout: result.stdout, stderr: result.stderr, expectedStdout: stdout, expectedStderr: stderr })}`);
    }
    async function compile(file, root, prefix) {
        const result = runNativeProgram({ jet: nativeJet, file, root, prefix, env, execute });
        if (result.problem) throw Error(result.problem.detail);
        return result.binary;
    }
    function library(file, prefix, lifecycle, exports = null) {
        const result = runNativeLibrary({ jet: nativeJet, file, prefix, lifecycle, exports, env, execute });
        if (result.problem) throw Error(result.problem.detail);
    }
    async function program(file, args = [], expected = '', options = {}) {
        const binary = await compile(file, dirname(file), join(dirname(file), 'registry-native'));
        const result = execute(binary, args, options); exact(result, expected); return result;
    }
    function link(source, object, binary, extra = []) {
        execute(process.env.CC ?? 'cc', ['-std=c11', '-Wall', '-Wextra', '-Werror', source, object, ...extra, '-lpthread', '-ldl', '-lm', '-o', binary]);
    }
    try {
        if (spec.type === 'codec-fixture') {
            const fixtureOut = join(out, 'codec');
            node(spec.runner, fixtureOut, nativeJet, runtimeDir, '--test', spec.name);
            const result = JSON.parse(readFileSync(join(fixtureOut, 'fixture-result.json'), 'utf8'));
            if (result.status !== 0 || result.test !== spec.name || result.stdout !== '' || result.stderr !== '') throw Error('named native codec/file fixture receipt differs');
        } else if (spec.type === 'process-fixture') {
            const fixtureOut = join(out, 'process');
            node(spec.runner, fixtureOut, nativeJet, runtimeDir, '--test', spec.name);
            const result = JSON.parse(readFileSync(join(fixtureOut, 'fixture-result.json'), 'utf8'));
            if (result.status !== 0 || result.test !== spec.name || result.witness !== spec.name + 'Witness') throw Error('named native process fixture receipt differs');
        } else if (spec.type === 'native-suite') {
            for (const mode of spec.modes) execute(process.execPath, [join(repo, spec.runner), out, mode], { env: { ...env, JET_FRONT_JETC0: nativeJet } });
            const result = JSON.parse(readFileSync(join(out, 'run.receipt.json'), 'utf8'));
            if (result.status !== 0 || result.signal || result.error || result.stdout !== '' || result.stderr !== '') throw Error('native files/host fixture receipt differs');
        } else
        if (spec.type === 'suite') {
            node(spec.runner, out, spec.modes[0]);
            const selected = suiteLibrary(spec, out);
            library(selected.file, selected.prefix, selected.lifecycle, selected.exports);
            for (const mode of spec.modes.slice(2)) node(spec.runner, out, mode);
            // The existing final run modes execute the actual C driver and check
            // literal output/stops/ledgers. Never admit assembly-only or reference.
            if (spec.crypto) {
                const name = entry.id.split('/').at(-1).replace(/Tests$/, '');
                const frozen = JSON.parse(readFileSync(join(repo, rt + 'AgeV1Fixtures.json'), 'utf8'));
                const receipts = name === 'AgeV1' ? [...frozen.cases, ...frozen.seals].map(test => join(out, `run-${test.name}.receipt.json`)) : [join(out, `run-${name}.receipt.json`)];
                for (const path of receipts) {
                    const receipt = JSON.parse(readFileSync(path, 'utf8'));
                    if (receipt.status !== 0 || receipt.signal || receipt.error || receipt.stderr !== '') throw Error('selected crypto witness execution differs');
                    if (name === 'AgeV1' && receipt.stdout !== '') throw Error('AgeV1 vector output differs');
                    if (name !== 'AgeV1') {
                        const stdoutFile = join(repo, rt + name + 'Witness.stdout');
                        const puts = [...readFileSync(join(repo, rt + name + 'Witness.c'), 'utf8').matchAll(/puts\(("(?:\\.|[^"\\])*")\);/g)];
                        const expected = existsSync(stdoutFile) ? readFileSync(stdoutFile, 'utf8') : puts.map(match => JSON.parse(match[1]) + '\n').join('');
                        if (!expected || receipt.stdout !== expected) throw Error('selected crypto stdout differs from committed C witness literal');
                    }
                }
            }
        } else if (spec.type === 'plain') {
            const raw = join(out, 'raw'), native = join(out, 'native'); mkdirSync(native, { recursive: true });
            node(spec.runner, raw, '--assemble-only', '--test', spec.name);
            writeFileSync(join(native, 'unit.jet'), readFileSync(join(raw, 'unit.jet'), 'utf8') + (spec.call ? `\nfn run() { ${spec.call}() }\n` : ''));
            await program(join(native, 'unit.jet'), [], spec.expected);
        } else if (spec.type === 'direct') {
            const file = join(out, 'unit.jet'); writeFileSync(file, readFileSync(join(repo, spec.runner)));
            await program(file, [], spec.expected);
        } else if (spec.type === 'c') {
            node(spec.runner, out, '--until', 'assemble', '--test', spec.name);
            library(join(out, 'library/Source/Runtime.jet'), join(out, 'jet_runtime_jet'), join(repo, 'Compiler/JetRuntime/lifecycle.list'));
            const source = ['VaultHooks', 'VaultProvider'].includes(spec.name) ? join(repo, rt + spec.name + 'Witness.c') : spec.name === 'Thread' ? join(repo, rt + 'thread-fixture.c') : ['HostSpawn', 'CompilerEnv', 'CompilerRoutes'].includes(spec.name) ? join(repo, rt + spec.name + 'Tests.c') : join(out, 'driver.c');
            if (source === join(out, 'driver.c')) writeFileSync(source, cSource(spec.name, repo));
            const binary = join(out, 'driver'); link(source, join(out, 'jet_runtime_jet.o'), binary);
            if (spec.name === 'HostFault') {
                const { trapLiteral } = await import(pathToFileURL(join(repo, rt + 'report-literals.mjs')));
                exact(execute(binary, [], { expectedCode: 101 }), '', 'A\n');
                exact(execute(binary, ['unreachable'], { expectedCode: 101 }), '', trapLiteral('trap_msg_unreachable').toString() + 'A\n');
            } else if (spec.name === 'TrapReport') {
                const { trapLiteral } = await import(pathToFileURL(join(repo, rt + 'report-literals.mjs')));
                const simple = Buffer.concat([trapLiteral('trap_msg_header_panic'), Buffer.from('registry boom'), trapLiteral('trap_msg_prelude_end'), trapLiteral('trap_footer_e3001')]).toString();
                for (let kind = 0; kind < 5; kind++) {
                    const direct = execute(binary, [String(kind), 'direct'], { expectedCode: 70 });
                    const captured = execute(binary, [String(kind), 'captured'], { expectedCode: 70 });
                    if (direct.stdout !== '' || !direct.stderr.startsWith(kind === 1 ? 'Stop [E3010]:' : 'Stop [E3001]:')) throw Error('trap report process-edge verdict differs');
                    exact(captured, '', direct.stderr);
                    if (kind === 0) exact(direct, '', simple);
                }
            } else {
                const expected = { VaultHooks: 'vault hooks witness OK (TLS ownership, exact stops, 8 root-filtered two-phase barriers)\n', VaultProvider: 'vault witness OK (13 errors, legacy+JVLT2 codec, imported origin/default entropy, native Secret redaction/wipe, long-origin hash, duplicate priority)\n', Thread: 'tls ok\n', HostSpawn: 'process raw: ok\n', CompilerEnv: 'compiler environment routes: Jet-only (Rust frozen)\n', CompilerRoutes: 'compiler routes: Jet-only (Rust frozen)\n' }[spec.name] ?? '';
                exact(execute(binary, spec.name === 'CompilerRoutes' ? [out] : []), expected);
                if (spec.name === 'CompilerEnv') exact(execute(binary, ['empty']), expected);
                if (spec.name === 'DynNative') {
                    const { trapLiteral } = await import(pathToFileURL(join(repo, rt + 'report-literals.mjs')));
                    const stopped = execute(binary, ['datetime'], { expectedCode: 70 });
                    const header = trapLiteral('trap_msg_header_panic').toString() + 'Dyn DateTime is not yet bridged natively';
                    if (stopped.stdout !== '' || !stopped.stderr.startsWith(header) || !stopped.stderr.endsWith(trapLiteral('trap_footer_e3001').toString())) throw Error('Dyn DateTime must publish the exact registered E3001 message/footer');
                }
            }
        } else if (spec.type === 'indices') {
            node(spec.runner, out, '--indices'); await program(join(out, 'unit.jet'), [], 'link section indexes: pass\n');
        } else if (spec.type === 'roots' || spec.type === 'link') {
            node(be + 'run-runtime-pack-roots.mjs', out, '--prepare-only');
            if (spec.type === 'roots') await program(join(out, 'unit.jet'), [], 'runtime pack object roots: pass\n');
            else {
                node(spec.runner, out);
                const binary = await compile(join(out, 'unit.jet'), out, join(out, 'link-native'));
                const result = execute(binary, [join(out, 'consumer'), join(out, 'consumer.o'), join(out, 'runtime.o'), join(out, 'runtime.a')]);
                if (!/^linked .+: [0-9]+ bytes; read [0-9]+ ms, link [0-9]+ ms, write [0-9]+ ms\n$/.test(result.stdout) || result.stderr !== '') throw Error('linker artifact receipt differs');
            }
            exact(execute(join(out, 'consumer')));
        } else throw Error(`unimplemented transport ${spec.type}`);
        return { code: 0, signal: null, stdout: entry.stdout, stderr: '', testCount: 1, evidence };
    } catch (error) {
        return { code: 1, signal: null, stdout: '', stderr: error.message + '\n', testCount: 1, evidence };
    }
}
