// Registry transports use the existing assemblers and their real native/C verdicts.
// One counted test is one complete source-registry verdict, not each internal assert.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseRuntimeExports, runtimeLifecycleList } from './runtime-route-abi.mjs';

const adapter = fileURLToPath(import.meta.url);
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const rt = 'Compiler/JetRuntime/Tests/';
const be = 'Compiler/JetBackend/Tests/';
const packageSource = 'name: "compiler_registry_transport"\nversion: "0.0.1"\nedition: "2028"\nauthority: { holds: { allow: [Env, Exec, Exec.Args, FS, IO, Mem.Alloc, Time.Wait, FFI] } }\n';
const mirTransportSource = `
fn run() {
    args :: cli_process.args()
    if args.len() != 2 { panic("expected source and checked MIR output") }
    compiled :: jet_cli_compile_file(args[0], false, false, None, JetDriverCompileMode.Program)
    if compiled == {
        .Refused(refusal) -> panic("registry MIR request refused: {refusal.stderr}")
        .Compiled(result, _) -> {
            if !result.complete || result.internal_problem != None { panic("registry MIR compile did not complete") }
            program :: result.mir ?? panic("registry MIR compile omitted MIR")
            entry :: result.entry_function ?? MIRFunctionID{value: 0}
            stream := [String]{"{entry.value}"}
            mirw_mir_program(program, &stream)
            cli_files.write(args[1], stream.join("\\n")) ?? panic("cannot write checked registry MIR")
            print("checked registry MIR: pass")
        }
    }
}
`;
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
        if (suites[name]) return { type: 'suite', runner: rt + suites[name][0], modes: suites[name][1], name };
        if (['EnvironmentLeaves', 'FaultSchedule', 'IterCharge', 'ProgressState', 'HostFault', 'FloatBits', 'TerminalColor', 'TrapReport', 'VaultHooks', 'VaultProvider', 'Thread', 'HostSpawn', 'CompilerEnv', 'CompilerRoutes'].includes(name)) return { type: 'c', name, runner: be + 'run-runtime.mjs' };
        if (name === 'GenericRouteSchema') return { type: 'direct', runner: 'Compiler/JetRuntime/Tests/GenericRouteSchema.jet', expected: '' };
        return { blocked: `No registered transport for ${id}; the source must not be counted without an exact native verdict` };
    }
    const name = id.slice('JetBackend/Tests/'.length);
    if (name === 'CallImages' || name === 'SessionHostFixtures') return { type: 'calls', runner: be + 'run-call-images.mjs', session: name === 'SessionHostFixtures' };
    if (name === 'LinkIndexes') return { type: 'indices', runner: be + 'run-link.mjs' };
    if (name === 'LinkPackRoots') return { type: 'roots', runner: be + 'run-runtime-pack-roots.mjs' };
    if (name === 'LinkRuntime') return { type: 'link', runner: be + 'run-link.mjs' };
    if (name === 'LowerFixtures' || name === 'LoadImages') return { type: 'lower', runner: be + 'run-lower-fixtures.mjs', loader: name === 'LoadImages' };
    if (name === 'RuntimeLower') return { type: 'runtime-lower', runner: be + 'run-runtime.mjs' };
    if (name === 'GoldenLower') return { type: 'golden-lower', runner: be + 'run-runtime.mjs' };
    if (name === 'ExpectProducer') return { type: 'producer', runner: be + 'run-expect-producer.mjs' };
    if (name === 'SelfHost/SelfHostLower') return { type: 'selfhost', runner: be + 'SelfHost/assemble-lower.mjs' };
    return null;
}
export function prepareTransport(row, { repo, out }) {
    const id = row.source.replace(/^Compiler\//, '').replace(/\.jet$/, '');
    const spec = specification(id);
    if (!spec) return null;
    const entry = { id, package: row.package, mode: 'run', kind: 'transport', path: adapter,
        cwd: join(out, id), sha256: digest(readFileSync(adapter)), testCount: 1,
        stdout: `JET_COMPILER_REGISTRY PASS ${id} tests=1\n`, sources: [row.source], sourceManifests: ['Compiler/Bootstrap/sources.list', 'Compiler/JetRuntime/sources.list'] };
    const witness = row.source.replace(/\.jet$/, '.c');
    if (spec.type === 'c' && existsSync(join(repo, witness))) entry.sources.push(witness);
    if (spec.blocked) return { ...entry, status: 'BLOCKED', reason: spec.blocked };
    return { ...entry, transport: { ...spec, runnerSha256: digest(readFileSync(join(repo, spec.runner))) } };
}

// Plans expose the executable boundary without invoking a compiler or C linker.
// Internal transformations are named explicitly and performed only by runTransport.
export function planTransport(entry, { jet, repo, runtimePack }) {
    const spec = specification(entry.id), out = entry.cwd;
    if (!spec || spec.blocked) return { id: entry.id, status: 'BLOCKED', reason: spec?.blocked ?? 'unknown registry' };
    const steps = [], node = (file, ...args) => steps.push({ command: process.execPath, args: [join(repo, file), ...args], cwd: out });
    const build = file => steps.push({ command: jet, args: ['build', file], cwd: dirname(file) });
    const run = (command, args = [], expected = null, env = {}) => steps.push({ command, args, cwd: out, env, ...(expected ? { expected } : {}) });
    const checkLibrary = (file, prefix, lifecycle, exports = null) => run(jet, ['check', file], null, { JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix, JET_FRONT_LIBRARY_LIFECYCLE: lifecycle, ...(exports ? { JET_FRONT_LIBRARY_EXPORTS: exports } : {}) });
    const cc = (source, object, binary) => run(process.env.CC ?? 'cc', ['-std=c11', '-Wall', '-Wextra', '-Werror', source, object, '-lpthread', '-ldl', '-lm', '-o', binary]);
    const mir = (file, output) => {
        steps.push({ action: 'assemble canonical compiler plus explicit checked-MIR Jet transport entry', source: join(out, 'mir-transport/transport.jet') });
        build(join(out, 'mir-transport/transport.jet')); run(join(out, 'mir-transport/transport.exe'), [file, output], { code: 0, stdout: 'checked registry MIR: pass\n', stderr: '' });
    };
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
        }
    } else if (spec.type === 'direct') {
        steps.push({ action: 'copy real source to isolated project', source: join(repo, spec.runner) }); build(join(out, 'unit.jet')); run(join(out, 'registry-native.exe'), [], { code: 0, stdout: spec.expected, stderr: '' });
    } else if (spec.type === 'runtime-lower' || spec.type === 'golden-lower') {
        node(spec.runner, out, '--until', 'assemble');
        mir(spec.type === 'runtime-lower' ? join(out, 'library/Source/Runtime.jet') : join(out, 'golden-input/unit.jet'), join(out, spec.type === 'runtime-lower' ? 'runtime.mird' : 'golden.mird'));
        steps.push({ action: spec.type === 'runtime-lower' ? 'remove entry-id wire header for library driver' : 'assemble actual GoldenLower entry with existing runtime assembler and real decoder' });
        build(join(out, 'unit.jet')); run(join(out, 'registry-native.exe'));
        if (spec.type === 'runtime-lower') { node(rt + 'check-object.mjs', join(out, 'jet_runtime_jet.o'), join(out, 'object-check')); node(rt + 'run-cabi-driver.mjs', join(out, 'cabi'), join(out, 'jet_runtime_jet.o')); }
        else run(join(out, 'golden.exe'), [], { code: 0, stdout: 'registry golden\n', stderr: '' });
    } else if (spec.type === 'producer') {
        steps.push({ action: 'assemble canonical compiler in external assembly-home', output: join(out, 'assembly-home/.cache/jet-luna/compiler-bootstrap/project/src/compiler.jet') });
        node(spec.runner, out, 'assemble', join(out, 'assembly-home/.cache/jet-luna/compiler-bootstrap/project/src/compiler.jet'));
        build(join(out, 'unit.jet')); node(spec.runner, out, 'produce', join(out, 'producer-unit.exe')); node(spec.runner, out, 'link'); node(spec.runner, out, 'run');
    } else if (spec.type === 'selfhost') {
        steps.push({ action: 'assemble full canonical compiler with production CLI entry', source: join(out, 'compiler/unit.jet') });
        mir(join(out, 'compiler/unit.jet'), join(out, 'self.mird')); node(spec.runner, out, repo);
        build(join(out, 'unit.jet')); run(join(out, 'self-lower.exe'), [], null, { SELFHOST_MIRD: join(out, 'self.mird'), SELFHOST_OUT: join(out, 'self'), SELFHOST_PACK: runtimePack });
        run(jet, ['--version']); run(join(out, 'self.exe'), ['--version']);
    } else if (spec.type === 'roots' || spec.type === 'link') {
        node(be + 'run-runtime-pack-roots.mjs', out, '--prepare-only');
        if (spec.type === 'link') node(spec.runner, out);
        build(join(out, 'unit.jet')); run(join(out, spec.type === 'link' ? 'link-native.exe' : 'registry-native.exe'), spec.type === 'link' ? [join(out, 'consumer'), join(out, 'consumer.o'), join(out, 'runtime.o'), join(out, 'runtime.a')] : []);
        run(join(out, 'consumer'), [], { code: 0, stdout: '', stderr: '' });
    } else {
        node(spec.runner, out, ...(spec.type === 'indices' ? ['--indices'] : spec.type === 'calls' && spec.session ? ['--session'] : []));
        const file = join(out, spec.type === 'calls' ? 'calls.jet' : 'unit.jet'); build(file); run(join(out, spec.type === 'lower' ? 'lower-native.exe' : 'registry-native.exe'));
        if (spec.type === 'lower' && !spec.loader) node(be + 'check-native-fixtures.mjs', out);
        if (spec.type === 'calls' && spec.session || spec.type === 'lower' && spec.loader) {
            run(process.env.CC ?? 'cc', ['-shared', join(dirname(runtimePack), 'jet_runtime_jet.o'), '-lpthread', '-ldl', '-lm', '-o', join(out, 'libjet_runtime.so')]);
            if (spec.loader) { build(join(out, 'load/load.jet')); run(join(out, 'load/registry-native.exe'), ['hello', join(out, 'libjet_runtime.so')], { code: 0, stdout: 'hello, world\n', stderr: '' }); }
            else steps.at(-2).args = [join(out, 'libjet_runtime.so')];
            if (spec.session) {
                const shared = steps.pop(); steps.splice(steps.length - 1, 0, shared);
            }
        }
    }
    return { id: entry.id, testCount: 1, inputs: { jet, runtimePack, runtimeObject: join(dirname(runtimePack), 'jet_runtime_jet.o'), provider: 'Jet-only; system C compiler permitted' }, steps };
}
function stripImports(text) {
    return text.replace(/^use jet_\w+\.\[[^\]]*\]\s*$/gm, '').replace(/^use jet_\w+\.[^\n]*$/gm, '');
}
function withoutDeclaration(text, name) {
    const lines = text.split('\n'), start = lines.findIndex(line => new RegExp(`^(?:pub )?(?:fn|struct) ${name}\\b`).test(line));
    if (start < 0) throw Error(`missing declaration ${name}`);
    const end = lines[start].trimEnd().endsWith('}') ? start : lines.findIndex((line, index) => index > start && line === '}');
    if (end < start) throw Error(`unterminated declaration ${name}`);
    return [...lines.slice(0, start), ...lines.slice(end + 1)].join('\n');
}
function cSource(name, repo) {
    const head = '#define _GNU_SOURCE\n#include <stdint.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n';
    const exports = { FaultSchedule: 'jet_rt_fault_schedule_fixture', IterCharge: 'jet_rt_iter_charge_fixture', ProgressState: 'jet_rt_progress_state_fixture' };
    if (exports[name]) return head + `extern int64_t ${exports[name]}(void);\nint main(void) { return (int)${exports[name]}(); }\n`;
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
    const toolchain = join(logDir, 'toolchain');
    if (!existsSync(toolchain)) {
        mkdirSync(toolchain, { recursive: true });
        copyFileSync(jet, join(toolchain, 'jet')); chmodSync(join(toolchain, 'jet'), 0o755);
        cpSync(runtimeDir, join(toolchain, 'jet-runtime'), { recursive: true, dereference: true });
    } else if (digest(readFileSync(join(toolchain, 'jet'))) !== digest(readFileSync(jet)) ||
        digest(readFileSync(join(toolchain, 'jet-runtime', basename(dirname(runtimePack)), 'jet_runtime.pack.o'))) !== digest(readFileSync(runtimePack))) throw Error('staged transport toolchain changed');
    const nativeJet = join(toolchain, 'jet');
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
    async function compile(file, root, prefix, mir = null) {
        if (!existsSync(join(root, 'package.jet'))) writeFileSync(join(root, 'package.jet'), packageSource);
        if (mir) {
            // A real Jet transport driver obtains checked MIR in-process through
            // the same authorized CLI request; no Tests.rs factory protocol.
            const transportRoot = join(out, 'mir-transport'); mkdirSync(transportRoot, { recursive: true });
            const source = join(transportRoot, 'transport.jet');
            if (!existsSync(source)) {
                writeFileSync(source, readFileSync(await aggregate(), 'utf8') + '\n' + mirTransportSource);
                writeFileSync(join(transportRoot, 'package.jet'), packageSource);
            }
            const driver = await compile(source, transportRoot, join(transportRoot, 'transport'));
            exact(execute(driver, [file, mir]), 'checked registry MIR: pass\n');
            return mir;
        }
        const built = join(root, '.jet/build', basename(file, '.jet'));
        rmSync(built, { force: true }); rmSync(built + '.o', { force: true });
        execute(nativeJet, ['build', file], { cwd: root });
        if (!existsSync(built) || !existsSync(built + '.o')) throw Error('native build did not emit fresh executable and object');
        copyFileSync(built, prefix + '.exe'); chmodSync(prefix + '.exe', 0o755);
        copyFileSync(built + '.o', prefix + '.o');
        return prefix + '.exe';
    }
    function library(file, prefix, lifecycle, exports = null) {
        const hookEnv = { ...env, JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix, JET_FRONT_LIBRARY_LIFECYCLE: lifecycle };
        if (!existsSync(lifecycle)) writeFileSync(lifecycle, runtimeLifecycleList(parseRuntimeExports(readFileSync(file, 'utf8'), file)));
        if (exports) hookEnv.JET_FRONT_LIBRARY_EXPORTS = exports;
        for (const suffix of ['.o', '.exports', '.abi.json', '.issues']) rmSync(prefix + suffix, { force: true });
        execute(nativeJet, ['check', file], { env: hookEnv, cwd: dirname(file) });
        if (existsSync(prefix + '.issues')) throw Error(readFileSync(prefix + '.issues', 'utf8'));
        for (const suffix of ['.o', '.exports', '.abi.json']) if (!existsSync(prefix + suffix)) throw Error(`native library hook omitted ${suffix}`);
    }
    async function program(file, args = [], expected = '', options = {}) {
        const binary = await compile(file, dirname(file), join(dirname(file), 'registry-native'));
        const result = execute(binary, args, options); exact(result, expected); return result;
    }
    function link(source, object, binary, extra = []) {
        execute(process.env.CC ?? 'cc', ['-std=c11', '-Wall', '-Wextra', '-Werror', source, object, ...extra, '-lpthread', '-ldl', '-lm', '-o', binary]);
    }
    async function aggregate() {
        const home = join(out, 'assembly-home'); mkdirSync(home, { recursive: true });
        execute(process.execPath, [join(repo, 'Compiler/Bootstrap/assemble.mjs')], { cwd: repo, env: { ...env, HOME: home, JET_BOOTSTRAP_SOURCE_ROOT: repo } });
        return join(home, '.cache/jet-luna/compiler-bootstrap/project/src/compiler.jet');
    }
    try {
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
            }
        } else if (spec.type === 'calls') {
            node(spec.runner, out, ...(spec.session ? ['--session'] : []));
            let args = [];
            if (spec.session) {
                const shared = join(out, 'libjet_runtime.so'); execute(process.env.CC ?? 'cc', ['-shared', runtimeObject, '-lpthread', '-ldl', '-lm', '-o', shared]); args = [shared];
            }
            await program(join(out, 'calls.jet'), args, spec.session ? 'one child: binary image transport, shared turns/comptime, recoverable fuel, typed refusal, fatal reset: pass\n' : 'native call images: pass\n');
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
        } else if (spec.type === 'lower') {
            node(spec.runner, out); const binary = await compile(join(out, 'unit.jet'), out, join(out, 'lower-native')); execute(binary);
            if (!spec.loader) {
                const checkerEnv = { ...env }; delete checkerEnv.JET_RUNTIME_C_LIB; delete checkerEnv.JET_RUN;
                nodeResultChecker(execute(process.execPath, [join(repo, be + 'check-native-fixtures.mjs'), out], { env: checkerEnv }));
            } else {
                const shared = join(out, 'libjet_runtime.so'); execute(process.env.CC ?? 'cc', ['-shared', runtimeObject, '-lpthread', '-ldl', '-lm', '-o', shared]);
                await program(join(out, 'load/load.jet'), ['hello', shared], 'hello, world\n', { cwd: out });
            }
        } else if (spec.type === 'runtime-lower' || spec.type === 'golden-lower') {
            node(spec.runner, out, '--until', 'assemble');
            const file = join(out, 'unit.jet');
            if (spec.type === 'runtime-lower') {
                // Execute RuntimeLower itself, not run-runtime's production hook bypass.
                await compile(join(out, 'library/Source/Runtime.jet'), join(out, 'library'), join(out, 'runtime-feed'), join(out, 'runtime.mird'));
                const stream = readFileSync(join(out, 'runtime.mird'), 'utf8');
                writeFileSync(join(out, 'runtime.mird'), stream.slice(stream.indexOf('\n') + 1));
                await program(file);
                node(rt + 'check-object.mjs', join(out, 'jet_runtime_jet.o'), join(out, 'object-check'));
                node(rt + 'run-cabi-driver.mjs', join(out, 'cabi'), join(out, 'jet_runtime_jet.o'));
            } else {
                let unit = readFileSync(file, 'utf8');
                for (const name of ['run', 'MIRDReader', 'mird_next', 'mird_int', 'mird_u64', 'mird_bool', 'mird_float', 'mird_hex', 'mird_string']) unit = withoutDeclaration(unit, name);
                unit += '\n' + stripImports(readFileSync(join(repo, be + 'GoldenLower.jet'), 'utf8'));
                for (const name of ['Archive', 'StaticLink', 'RuntimePack']) unit += '\n' + stripImports(readFileSync(join(repo, 'Compiler/JetBackend/Source/Image/' + name + '.jet'), 'utf8'));
                unit += '\n' + stripImports(readFileSync(join(repo, 'Compiler/JetBackend/Source/LIR/Sanitize.jet'), 'utf8')) + '\n' + stripImports(readFileSync(join(repo, be + 'TestPack.jet'), 'utf8'));
                const imports = new Set();
                unit = unit.replace(/^use core\.[^\n]*$/gm, line => { if (imports.has(line)) return ''; imports.add(line); return line; });
                writeFileSync(file, unit);
                const seed = join(out, 'golden-input'); mkdirSync(seed, { recursive: true }); const seedFile = join(seed, 'unit.jet'); writeFileSync(seedFile, 'fn run() { print("registry golden") }\n');
                await compile(seedFile, seed, join(out, 'golden-feed'), join(out, 'golden.mird'));
                writeFileSync(join(out, 'cases.txt'), 'golden\n'); writeFileSync(join(out, 'runtime-pack.txt'), runtimePack + '\n');
                await program(file, [], 'golden\tpack\n'); exact(execute(join(out, 'golden.exe')), 'registry golden\n');
            }
        } else if (spec.type === 'producer') {
            node(spec.runner, out, 'assemble', await aggregate());
            const binary = await compile(join(out, 'unit.jet'), out, join(out, 'producer-unit'));
            node(spec.runner, out, 'produce', binary); node(spec.runner, out, 'link');
            const result = node(spec.runner, out, 'run'); exact(result, 'compiled native expectation producer: CHECK OK\n');
        } else if (spec.type === 'selfhost') {
            const unitRoot = join(out, 'compiler'); mkdirSync(unitRoot, { recursive: true }); const file = join(unitRoot, 'unit.jet');
            writeFileSync(file, readFileSync(await aggregate(), 'utf8') + '\nuse core.process as selfhost_process\nfn run() { selfhost_process.exit(jet_cli_main(selfhost_process.args())) }\n');
            await compile(file, unitRoot, join(out, 'self-feed'), join(out, 'self.mird'));
            node(spec.runner, out, repo);
            const binary = await compile(join(out, 'unit.jet'), out, join(out, 'self-lower'));
            exact(execute(binary, [], { env: { ...env, SELFHOST_MIRD: join(out, 'self.mird'), SELFHOST_OUT: join(out, 'self'), SELFHOST_PACK: runtimePack } }));
            for (const suffix of ['.issues', '.prepare-issues', '.object-issues', '.link-issues']) if (existsSync(join(out, 'self' + suffix)) && readFileSync(join(out, 'self' + suffix), 'utf8').trim()) throw Error(`self-host verdict has ${suffix}`);
            const reference = execute(nativeJet, ['--version']); exact(execute(join(out, 'self.exe'), ['--version']), reference.stdout, reference.stderr);
        } else throw Error(`unimplemented transport ${spec.type}`);
        return { code: 0, signal: null, stdout: entry.stdout, stderr: '', testCount: 1, evidence };
    } catch (error) {
        return { code: 1, signal: null, stdout: '', stderr: error.message + '\n', testCount: 1, evidence };
    }
}
function nodeResultChecker(result) {
    const rows = result.stdout.split('\n').filter(line => /: ok \(exit /.test(line));
    if (!rows.length || /FAIL|MISSING/.test(result.stdout) || !result.stdout.endsWith('all native fixtures match\n')) throw Error('native fixture checker lacks its exact complete verdict');
}
