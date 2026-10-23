// Production Jet CLI transport shared by registry and native fixture runners.
// Receipts record real command outcomes; artifacts, not a bootstrap completion
// string, establish whether the requested native feed was produced.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, relative, resolve } from 'node:path';
import { compilerPackages } from './compiler-packages.mjs';
import { parseRuntimeExports, runtimeLifecycleList } from './runtime-route-abi.mjs';
import { jetWriter } from '../../Compiler/JetBackend/Tests/front-feed-codec.mjs';
import { loadMirSchema } from '../../Compiler/JetBackend/Tests/mir-debug.mjs';

export const nativeTransportPackage = 'name: "compiler_registry_transport"\nversion: "0.0.1"\nedition: "2028"\nauthority: { holds: { allow: [Env, Exec, Exec.Args, FS, IO, Mem.Alloc, Time.Wait, FFI] } }\n';
// The checked-MIR driver's entry: the production front end compiles one source,
// and the schema's generated observation writer (front-feed-codec.mjs) streams
// the checked MIR.
const checkedMirDriverEntry = `
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
// Write the checked-MIR driver package into `out`: package.jet (the compiler
// packages as path dependencies, JetCli's authority) and run.jet.
export function writeCheckedMirDriver({ repo, out }) {
    mkdirSync(out, { recursive: true });
    const packages = compilerPackages(repo);
    const authority = /^authority:\s*\{[\s\S]*?^\}/m.exec(readFileSync(join(repo, 'Compiler/JetCli/package.jet'), 'utf8'))?.[0];
    if (!authority) throw Error('Compiler/JetCli/package.jet declares no authority');
    writeFileSync(join(out, 'package.jet'), `name: "checked_mir_driver"\nversion: "0.0.1"\nedition: "2026"\ndeps: {\n${[...packages].map(([name, directory]) => `    ${name}: ${relative(out, directory)},`).join('\n')}\n}\n${authority}\n`);
    const schema = loadMirSchema(repo), writer = jetWriter(schema);
    const types = [...new Set(['MIRFunctionID', ...[...schema.keys()].filter(type => new RegExp(`\\b${type}\\b`).test(writer))])];
    const source = join(out, 'run.jet');
    writeFileSync(source, `use jet_cli.[jet_cli_compile_file]\nuse jet_driver.[JetDriverCompileMode]\nuse jet_foundation.[${types.join(', ')}]\nuse core.files as cli_files\nuse core.math as math\nuse core.process as cli_process\n${checkedMirDriverEntry}\n${writer}`);
    return source;
}
const digest = path => createHash('sha256').update(readFileSync(path)).digest('hex');

export function nativeCommandProblem(run) {
    if (run.error || run.signal || run.status !== 0) return { verdict: 'no-mir', detail: `native Jet exit ${run.status ?? run.signal}: ${run.error?.message ?? run.stderr ?? ''}` };
    return null;
}

export function nativeTransportEnvironment(env = process.env) {
    const clean = { ...env };
    for (const name of Object.keys(clean)) if (/^JET_(?:BOOTSTRAP_|FRONT_)/.test(name) || name === 'JET_DUMP_MIR') delete clean[name];
    return clean;
}

function command(jet, args, { prefix, cwd, env, execute = spawnSync, timeout = Number(process.env.JET_FRONT_TIMEOUT_MS ?? 600000) }) {
    mkdirSync(dirname(prefix), { recursive: true });
    const run = execute(jet, args, { cwd, env, encoding: 'utf8', timeout, maxBuffer: 128 << 20 });
    writeFileSync(prefix + '.front.log', `${run.stdout ?? ''}${run.stderr ?? ''}`);
    writeFileSync(prefix + '.front.run.json', JSON.stringify({ command: jet, args, cwd, status: run.status, signal: run.signal ?? null, error: run.error?.message ?? null, stdout: run.stdout ?? '', stderr: run.stderr ?? '' }, null, 2) + '\n');
    return run;
}

export function runNativeLibrary({ jet, file, prefix, lifecycle = prefix + '.lifecycle.list', exports: exportsFile = null, env = process.env, cwd = dirname(file), execute, timeout }) {
    mkdirSync(dirname(lifecycle), { recursive: true });
    if (!existsSync(lifecycle)) writeFileSync(lifecycle, runtimeLifecycleList(parseRuntimeExports(readFileSync(file, 'utf8'), file)));
    for (const suffix of ['.o', '.exports', '.abi.json', '.issues']) rmSync(prefix + suffix, { force: true });
    const hookEnv = { ...nativeTransportEnvironment(env), JET_FRONT_LIBRARY_OBJECT_OUTPUT: resolve(prefix), JET_FRONT_LIBRARY_LIFECYCLE: resolve(lifecycle) };
    if (exportsFile) hookEnv.JET_FRONT_LIBRARY_EXPORTS = resolve(exportsFile);
    const run = command(jet, ['check', resolve(file)], { prefix, cwd, env: hookEnv, execute, timeout });
    let problem = nativeCommandProblem(run);
    if (existsSync(prefix + '.issues')) problem = { verdict: 'no-mir', detail: readFileSync(prefix + '.issues', 'utf8') };
    if (!problem) for (const suffix of ['.o', '.exports', '.abi.json']) if (!existsSync(prefix + suffix)) {
        problem = { verdict: 'no-mir', detail: `native library hook omitted ${suffix}` };
        break;
    }
    return { run, problem };
}

export function stageNativeToolchain({ jet, runtimePack, runtimeDir = dirname(dirname(runtimePack)), out }) {
    const stagedJet = join(out, 'jet');
    const stagedPack = join(out, 'jet-runtime', basename(dirname(runtimePack)), 'jet_runtime.pack.o');
    if (!existsSync(out)) {
        mkdirSync(out, { recursive: true });
        copyFileSync(jet, stagedJet); chmodSync(stagedJet, 0o755);
        cpSync(runtimeDir, join(out, 'jet-runtime'), { recursive: true, dereference: true });
    } else if (!existsSync(stagedJet) || !existsSync(stagedPack) || digest(stagedJet) !== digest(jet) || digest(stagedPack) !== digest(runtimePack)) throw Error('staged transport toolchain changed');
    return stagedJet;
}

export function runNativeProgram({ jet, file, root = dirname(file), prefix, env = process.env, execute, timeout }) {
    if (!existsSync(join(root, 'package.jet'))) writeFileSync(join(root, 'package.jet'), nativeTransportPackage);
    const built = join(root, '.jet/build', basename(file, '.jet'));
    rmSync(built, { force: true });
    for (const suffix of ['.exe', '.issues', '.pack-issues', '.lir']) rmSync(prefix + suffix, { force: true });
    const run = command(jet, ['build', resolve(file)], { prefix, cwd: root, env: nativeTransportEnvironment(env), execute, timeout });
    const problem = nativeCommandProblem(run);
    if (problem) return { run, problem };
    if (!existsSync(built)) return { run, problem: { verdict: 'no-mir', detail: 'native build did not emit a fresh executable' } };
    copyFileSync(built, prefix + '.exe'); chmodSync(prefix + '.exe', 0o755);
    return { run, problem: null, binary: prefix + '.exe' };
}

export function buildNativeCheckedMirDriver({ jet, repo, out, env = process.env, execute, timeout }) {
    const source = writeCheckedMirDriver({ repo, out });
    const result = runNativeProgram({ jet, file: source, root: out, prefix: join(out, 'transport'), env, execute, timeout });
    if (result.problem) throw Error(result.problem.detail);
    return result.binary;
}

export function runNativeCheckedMir({ driver, file, mir, prefix, cwd = dirname(file), env = process.env, execute, timeout }) {
    rmSync(mir, { force: true });
    const run = command(driver, [resolve(file), resolve(mir)], { prefix, cwd, env: nativeTransportEnvironment(env), execute, timeout });
    let problem = nativeCommandProblem(run);
    if (!problem && (run.stdout !== 'checked registry MIR: pass\n' || run.stderr !== '')) problem = { verdict: 'no-mir', detail: 'checked MIR driver verdict differs' };
    if (!problem && !existsSync(mir)) problem = { verdict: 'no-mir', detail: 'native checked-MIR transport omitted MIR' };
    return { run, problem };
}
