#!/usr/bin/env node
// Build the production compiler, not Bootstrap/Tests.rs's private factory.
// The seed must already execute JetCli's Jet code (a Rust-bootstrapped Jet-built
// seed is allowed); the Rust reference CLI is NOT a seed for this driver.
// `build` without --release selects Host.jet -> lir_lower_entry -> the O1 LIR passes ->
// x64_relocatable_object -> Image/StaticLink.x64_link_executable. Every concrete
// function reachable from the full CLI dispatch is lowered; generic templates
// are instantiated by the backend, not independently emitted as dead roots.
// The native path uses no rustc, Rust emitter, system linker, or probe skips.
//
// Usage (run through Tools/agent/jet-env):
// node Tools/agent/native-compiler.mjs --jet /path/to/jetc0 \
//   --runtime-dir /path/to/jet-runtime --out /disk/fresh-output [--repo DIR]
// --dry-run assembles the real unit and records commands without executing jet.
// --assemble-only writes the canonical CLI unit/map for the source-bound
// factory/native-feed path, without inventing a hosted Rust build plan.
// --stages 1 produces one successor; default 2 proves stage1 == stage2, for
// both object and executable, using the same source path and runtime each time.
// --seed-build is the explicit bootstrap exception: --jet names the Rust host
// from job (a), which builds this same production entry with --release. Job
// (b) publishes the resulting Jet-built CLI seed; no Tests.rs main is spliced.
// This mode does not need --runtime-dir, and never claims a native fixed point.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { accessSync, chmodSync, constants, copyFileSync, cpSync, existsSync,
    mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readNativeRuntimePack } from '../packaging/runtime-pack-cache.mjs';
import { hostedSeedCensus } from './hosted-seed-proof.mjs';

const usage = 'native-compiler.mjs --jet CLI_SEED --runtime-dir DIR --out FRESH_DIR [--repo DIR] [--stages 1|2] [--dry-run]; assembly: --assemble-only --out FRESH_DIR [--repo DIR]; bootstrap only: --seed-build --jet RUST_HOST --out FRESH_DIR [--repo DIR] [--dry-run]';
const options = { repo: resolve(dirname(fileURLToPath(import.meta.url)), '../..'), stages: 2, dryRun: false };
for (let i = 2; i < process.argv.length; i++) {
    const flag = process.argv[i];
    if (flag === '--help') { console.log(usage); process.exit(0); }
    if (flag === '--dry-run') { options.dryRun = true; continue; }
    if (flag === '--seed-build') { options.seedBuild = true; continue; }
    if (flag === '--assemble-only') { options.assembleOnly = true; continue; }
    const key = { '--jet': 'jet', '--runtime-dir': 'runtime', '--out': 'out', '--repo': 'repo', '--stages': 'stages' }[flag];
    if (!key || !process.argv[i + 1] || process.argv[i + 1].startsWith('--')) throw Error(usage);
    options[key] = key === 'stages' ? Number(process.argv[++i]) : resolve(process.argv[++i]);
}
if (!options.out || ![1, 2].includes(options.stages) || (!options.assembleOnly && (!options.jet || (!options.seedBuild && !options.runtime)))) throw Error(usage);
if (options.assembleOnly && (options.jet || options.runtime || options.seedBuild || options.dryRun)) throw Error('assembly-only does not execute or plan a compiler build');
const { repo, out } = options;
if (existsSync(out)) throw Error(`output already exists; use a fresh directory: ${out}`);
mkdirSync(out, { recursive: true });
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const json = (path, value) => writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
function execute(command, args, env, cwd, log) {
    const result = spawnSync(command, args, { cwd, env, stdio: log ? ['ignore', 'pipe', 'pipe'] : 'inherit', maxBuffer: 128 * 1024 * 1024 });
    if (log) writeFileSync(log, Buffer.concat([result.stdout ?? Buffer.alloc(0), result.stderr ?? Buffer.alloc(0)]));
    if (result.error || result.status !== 0) throw result.error ?? Error(`${command} exited ${result.status ?? result.signal}; see ${log ?? 'assembler output'}`);
}

const home = join(out, 'assembly-home');
mkdirSync(home);
execute(process.execPath, [join(repo, 'Compiler/Bootstrap/assemble.mjs')],
    { ...process.env, HOME: home, JET_BOOTSTRAP_SOURCE_ROOT: repo }, repo);
const bootstrap = join(home, '.cache/jet-luna/compiler-bootstrap');
const aggregate = readFileSync(join(bootstrap, 'project/src/compiler.jet'), 'utf8');
// Preserve the actual canonical entry bytes. Its Core import already appears
// in the aggregate (Cache.jet); deduplicate exactly as assemble.mjs does.
const entryPath = 'Compiler/JetCli/run.jet';
const entry = readFileSync(join(repo, entryPath), 'utf8');
if (!/^fn run\(\)/m.test(entry) || !entry.includes('jet_cli_main(')) throw Error(`${entryPath} is not the production CLI entry`);
const existingCoreImports = new Set([...aggregate.matchAll(/^use core\.[^\[\n]*$/gm)].map(match => match[0].trimEnd()));
const appended = entry.replace(/^use core\.[^\[\n]*$/gm, line => existingCoreImports.has(line.trimEnd()) ? line.replace(/[^\n]/g, ' ') : line);
const marker = `\n// [jet-native compiler entry: ${entryPath}]\n`;
const source = aggregate + marker + appended;
const project = join(out, 'compiler');
mkdirSync(project);
const unit = join(project, 'selfhost.jet');
writeFileSync(unit, source);
// Package dependencies have been flattened by the canonical assembler; their
// relative paths in JetCli/package.jet must not escape this standalone unit.
const packageManifest = readFileSync(join(repo, 'Compiler/JetCli/package.jet'), 'utf8')
    .replace(/^deps: \{[\s\S]*?^\}\r?\n/m, '');
// INTERIM for the D: authority deny-only port gap. The compiler's checked rows
// carry the deny-only Mem.Rc (visitor callbacks); Rust exempts deny-only
// rights at the application boundary (jet-foundation Authority.rs
// `undecided_effects`), but a pinned JetCli Authority.jet whose
// `jet_cli_undecided` exempts only Panic stops the native `jet build` stage
// with E1803. Only for such a pin, grant exactly Mem.Rc in the assembled
// manifest (factory and CLI stages read this same file) and record the patch.
const authoritySource = readFileSync(join(repo, 'Compiler/JetCli/Source/Cli/Authority.jet'), 'utf8');
const denyOnlyPorted = /fn jet_cli_undecided[^\n]*\n(?:[^\n]*\n){0,3}[^\n]*sema_effect_without_deny_only\(/.test(authoritySource);
let manifest = packageManifest, manifestPatch = null;
if (!denyOnlyPorted) {
    const holds = /(holds:\s*\{\s*allow:\s*\[)([^\]]*)\]/;
    const allowed = holds.exec(packageManifest)?.[2].split(',').map(right => right.trim()) ?? null;
    if (!allowed?.includes('Mem.Alloc')) throw Error('Compiler/JetCli/package.jet authority.holds.allow no longer has the expected Mem.Alloc row');
    if (!allowed.includes('Mem.Rc')) {
        manifest = packageManifest.replace(holds, (_, head) => `${head}${allowed.flatMap(right => right === 'Mem.Alloc' ? [right, 'Mem.Rc'] : [right]).join(', ')}]`);
        manifestPatch = { interim: 'D: authority deny-only port gap (JetCli Authority.jet jet_cli_undecided exempts only Panic)', file: 'package.jet', path: 'authority.holds.allow', added: ['Mem.Rc'], before: holds.exec(packageManifest)[0], after: holds.exec(manifest)[0] };
    }
}
writeFileSync(join(project, 'package.jet'), manifest);
const map = JSON.parse(readFileSync(join(bootstrap, 'compiler.map.json'), 'utf8'));
map.source_segments.push({ generated: { start: Buffer.byteLength(aggregate + marker), end: Buffer.byteLength(source) }, source_file: entryPath,
    line_starts: [0, ...[...Buffer.from(entry).entries()].filter(([, byte]) => byte === 10).map(([index]) => index + 1)] });
json(join(out, 'compiler.map.json'), map);
if (options.assembleOnly) {
    const assembly = { schema: 'jet-cli-assembly/v1', sourceRoot: repo, source: unit, sourceSha256: hash(Buffer.from(source)), project, entry: entryPath, map: join(out, 'compiler.map.json'), manifestPatch };
    json(join(out, 'plan.json'), assembly);
    console.log(JSON.stringify(assembly, null, 2));
    process.exit(0);
}

const environment = { ...process.env, JET_TOOLCHAIN_ROOT: repo, JET_KEEP_GOING: '1' };
// Factory/library observation hooks are not compilation policy; they must not
// redirect the production compiler or make a previous queue job's outputs live.
for (const name of Object.keys(environment)) {
    if (name.startsWith('JET_FRONT_') || name.startsWith('JET_BOOTSTRAP_') || name === 'JET_DUMP_MIR') delete environment[name];
}
if (options.seedBuild) {
    // Rust host compilation cannot produce Jet Host/Entry's all-phases TSV.
    delete environment.JET_KEEP_GOING;
    delete environment.JET_KEEP_GOING_REPORT;
    const command = [join(repo, 'Tools/agent/jet-env'), options.jet, 'build', '--release', unit];
    const destination = join(out, 'seed/jet');
    const seedPlan = { schema: 'jet-cli-seed-plan/v1', bootstrap: 'rust-emission', command, cwd: project,
        source: unit, sourceSha256: hash(Buffer.from(source)), entry: entryPath, jet: destination, manifestPatch };
    json(join(out, 'plan.json'), seedPlan);
    if (options.dryRun) {
        console.log(JSON.stringify({ dryRun: true, ...seedPlan }, null, 2));
    } else {
        execute(command[0], command.slice(1), environment, project, join(out, 'seed-build.log'));
        const seedLog = readFileSync(join(out, 'seed-build.log'));
        const hostedCensus = { ...hostedSeedCensus(0, seedLog.toString('utf8')), log: join(out, 'seed-build.log'), logSha256: hash(seedLog) };
        const binary = join(project, '.jet/build/selfhost');
        accessSync(binary, constants.X_OK);
        mkdirSync(dirname(destination));
        copyFileSync(binary, destination);
        chmodSync(destination, 0o755);
        const seedBytes = readFileSync(destination);
        if (!seedBytes.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))) throw Error('hosted seed is not an ELF executable');
        const sha256 = hash(seedBytes);
        json(join(out, 'seed.json'), { ...seedPlan, sha256, hostedCensus,
            bootstrapHostSha256: hash(readFileSync(options.jet)), native: false });
        console.log(JSON.stringify({ jet: destination, sha256, native: false }));
    }
    process.exit(0);
}
const stages = Array.from({ length: options.stages }, (_, index) => {
    const stage = index + 1;
    const builder = join(out, stage === 1 ? 'seed' : `stage${stage - 1}`, 'jet');
    return { stage, builder, command: [join(repo, 'Tools/agent/jet-env'), builder, 'build', unit], cwd: project,
        object: join(project, '.jet/build/selfhost.o'), binary: join(project, '.jet/build/selfhost'),
        destination: join(out, `stage${stage}`, 'jet'), keepGoingReport: join(out, `stage${stage}-keep-going.tsv`) };
});
const plan = { schema: 'jet-native-compiler-plan/v1', seed: options.jet, runtimeDir: options.runtime,
    sourceRoot: repo, source: unit, sourceSha256: hash(Buffer.from(source)), entry: entryPath, manifestPatch,
    backend: 'jet-native', optimization: 'O1', environment: { JET_TOOLCHAIN_ROOT: repo, JET_KEEP_GOING: '1' },
    lowering: 'lir_lower_entry', objectWriter: 'x64_relocatable_object', linker: 'x64_link_executable',
    stages, comparisons: options.stages === 2 ? ['stage1/jet.o == stage2/jet.o', 'stage1/jet == stage2/jet'] : [] };
json(join(out, 'plan.json'), plan);
if (options.dryRun) { console.log(JSON.stringify({ dryRun: true, plan: join(out, 'plan.json'), ...plan }, null, 2)); process.exit(0); }

accessSync(options.jet, constants.X_OK);
const runtime = readNativeRuntimePack(options.runtime);
const { pack } = runtime;
const exposeRuntime = directory => cpSync(options.runtime, join(directory, 'jet-runtime'), { recursive: true, dereference: true });
const seedDir = join(out, 'seed');
mkdirSync(seedDir);
copyFileSync(options.jet, join(seedDir, 'jet'));
chmodSync(join(seedDir, 'jet'), 0o755);
exposeRuntime(seedDir);
const receipts = [];
for (const stage of stages) {
    // This output is owned scratch. Remove only our previous stage's files;
    // success must include a freshly written object and executable.
    rmSync(stage.binary, { force: true });
    rmSync(stage.object, { force: true });
    const start = Date.now();
    execute(stage.command[0], stage.command.slice(1), { ...environment, JET_KEEP_GOING_REPORT: stage.keepGoingReport }, stage.cwd, join(out, `stage${stage.stage}.log`));
    if (!existsSync(stage.keepGoingReport)) throw Error(`stage${stage.stage} did not produce its all-phases diagnostic report`);
    const diagnostics = readFileSync(stage.keepGoingReport, 'utf8').split('\n')
        .filter(line => line !== '' && !line.startsWith('phase\titem\tcode\tmessage\tspan'));
    if (diagnostics.length !== 0) throw Error(`stage${stage.stage} contains ${diagnostics.length} all-phases failures; see ${stage.keepGoingReport}`);
    accessSync(stage.binary, constants.X_OK);
    const bytes = readFileSync(stage.binary), object = readFileSync(stage.object);
    if (bytes.subarray(0, 4).toString('hex') !== '7f454c46' || object.subarray(0, 4).toString('hex') !== '7f454c46') throw Error('native build did not produce ELF outputs');
    const directory = dirname(stage.destination);
    mkdirSync(directory);
    copyFileSync(stage.binary, stage.destination);
    copyFileSync(stage.object, `${stage.destination}.o`);
    chmodSync(stage.destination, 0o755);
    exposeRuntime(directory);
    receipts.push({ stage: stage.stage, binary: stage.destination, sha256: hash(bytes), objectSha256: hash(object), seconds: (Date.now() - start) / 1000,
        keepGoingReport: stage.keepGoingReport, keepGoingFailures: diagnostics.length });
    json(join(out, 'stages.json'), receipts);
}
let fixedPoint = null;
if (options.stages === 2) {
    fixedPoint = ['jet', 'jet.o'].every(name => readFileSync(join(out, 'stage1', name)).equals(readFileSync(join(out, 'stage2', name))));
    json(join(out, 'comparisons.json'), { fixedPoint, stages: [1, 2] });
    if (!fixedPoint) throw Error('native fixed point differs: stage1 and stage2 object/executable must both be byte-identical');
}
const final = stages.at(-1).destination;
json(join(out, 'native-compiler.json'), { schema: 'jet-native-compiler/v1', sourceSha256: plan.sourceSha256,
    sourceRoot: repo, seedSha256: hash(readFileSync(options.jet)), runtimePackSha256: hash(pack),
    optimization: 'O1', backend: 'jet-native', fixedPoint, stages: receipts, jet: final, runtimeDir: join(dirname(final), 'jet-runtime') });
console.log(JSON.stringify({ jet: final, runtimeDir: join(dirname(final), 'jet-runtime'), fixedPoint }));
