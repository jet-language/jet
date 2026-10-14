// Admit the retained, source-bound bootstrap factory before feeding real Jet
// library/native requests. A factory is not an argv CLI or a native candidate.
import { createHash } from 'node:crypto';
import { accessSync, constants, existsSync, mkdirSync, mkdtempSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { assertPinnedSource } from '../proof-source.mjs';
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex');
export function admitFactoryReceipt(text, censusText) {
  const rows = new Map(text.trim().split('\n').map(line => {
    const split = line.indexOf('=');
    return [line.slice(0, split), line.slice(split + 1)];
  }));
  for (const [name, value] of Object.entries({ mode: 'factory', complete: 'true', selected_factory_tier: 'Aot', actual_factory_tier: 'Aot' })) {
    if (rows.get(name) !== value) throw Error(`factory receipt ${name} is not ${value}`);
  }
  // Reports are admitted by severity: a lint warning is not a failed compile,
  // an error report is. Every report_json line is read (the row map keeps one).
  const reports = text.split('\n').filter(line => line.startsWith('report_json=')).map(line => JSON.parse(line.slice('report_json='.length)));
  if (reports.length !== Number(rows.get('report_count') ?? 0)) throw Error(`factory receipt lists ${reports.length} reports but report_count is ${rows.get('report_count')}`);
  const errors = reports.filter(report => report.severity !== 'warning' && report.severity !== 'note' && report.severity !== 'info');
  if (errors.length) throw Error(`factory receipt has ${errors.length} error report(s): ${errors.slice(0, 5).map(report => `${report.code} ${report.what} (${report.file}:${report.line})`).join('; ')}`);
  const census = censusText.trim().split('\n');
  if (census.length !== 1 || census[0] !== 'phase\titem\tcode\tmessage\tspan') throw Error('native/library factory request has a missing or non-empty all-phases census');
  // emitted_source is legitimately absent for library/native outputs. Do not
  // impose the Rust-source source_bytes>0 predicate on these typed boundaries.
  return Object.fromEntries(rows);
}
export function readStageZeroFactory(recordFile, repo) {
  const record = JSON.parse(readFileSync(recordFile, 'utf8'));
  const source = resolve(repo);
  if (record.schema !== 'jet-sharded-stage-zero/v1' || record.rc !== 0 || record.verdict !== 'GREEN' || record.source !== source || record.factory?.native !== false || record.factory?.protocol !== 'JET_BOOTSTRAP_* factory; NOT an argv CLI') throw Error('source-bound canonical sharded factory receipt is required');
  const manifest = assertPinnedSource(source, record.commit);
  if (manifest.tree !== record.tree || record.factory.identity.repo !== source || record.factory.identity.package !== 'jet_bootstrap_stage_zero' || record.factory.identity.task_roots !== 'false' || !record.factory.identity.id) throw Error('sharded factory source/identity mismatch');
  const binary = record.factory.path;
  accessSync(binary, constants.X_OK);
  if (digest(binary) !== record.factory.sha256 || digest(record.factory.generated) !== record.factory.generated_sha256) throw Error('retained sharded factory binary/source hash mismatch');
  const magic = readFileSync(binary).subarray(0, 4);
  if (!magic.equals(Buffer.from([127, 69, 76, 70]))) throw Error('retained sharded factory is not ELF');
  return record;
}
function runFactoryProtocol({ recordFile, repo, sourceRoot, entry, work, policy, cpus }, linkRequired) {
  const record = readStageZeroFactory(recordFile, repo);
  const library = policy.JET_FRONT_LIBRARY_OBJECT_OUTPUT;
  const native = policy.JET_FRONT_NATIVE_OUTPUT;
  if (Boolean(library) === Boolean(native)) throw Error('exactly one real library/native request is required');
  const allowed = new Set(['JET_FRONT_LIBRARY_OBJECT_OUTPUT', 'JET_FRONT_LIBRARY_LIFECYCLE', 'JET_FRONT_RUNTIME_PACK_OUTPUT', 'JET_FRONT_RUNTIME_PLATFORM_INPUTS', 'JET_FRONT_NATIVE_OUTPUT', 'JET_RUNTIME_PACK']);
  for (const key of Object.keys(policy)) if (!allowed.has(key)) throw Error(`unsupported factory policy ${key}`);
  if (library && (!policy.JET_FRONT_LIBRARY_LIFECYCLE || !policy.JET_FRONT_RUNTIME_PACK_OUTPUT || !policy.JET_FRONT_RUNTIME_PLATFORM_INPUTS)) throw Error('real library lifecycle, pack output and platform archives required');
  if (native && linkRequired && !policy.JET_RUNTIME_PACK) throw Error('native factory request requires the proven runtime pack');
  mkdirSync(work, { recursive: true });
  work = mkdtempSync(join(work, 'request-'));
  const receipt = join(work, 'factory.env');
  const census = join(work, 'keep-going.tsv');
  const home = join(work, 'home');
  const store = join(work, 'store');
  mkdirSync(home, { recursive: true });
  mkdirSync(store, { recursive: true });
  // The native route compiles the compiler's #FFI(c) leaves with the CLI's C
  // toolchain (JetCli ForeignLink.jet). The factory therefore runs inside the
  // SAME Tools/agent/jet-env shell, entered from the SAME caller environment
  // (real HOME, PATH, CC, AR, NIX_* wrapper policy, JET_PACK_MUSL_LIB), as
  // the native CLI stages, so the inline-C archive is identical at every
  // stage. Inherited JET_*/RUST_* never leak: only the explicit policy below.
  // jet-env misresolves the shell under a synthetic HOME, so the private
  // factory HOME is applied inside it, just before exec.
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('JET_') && !key.startsWith('RUST_')));
  Object.assign(env, { LC_ALL: 'C', RUST_MIN_STACK: '268435456', RUST_BACKTRACE: '1', JET_TOOLCHAIN_ROOT: resolve(repo), JET_BOOTSTRAP_MODE: 'factory', JET_BOOTSTRAP_FACTORY_TIER: 'aot', JET_BOOTSTRAP_SOURCE_ROOT: resolve(sourceRoot), JET_BOOTSTRAP_ENTRY: entry, JET_BOOTSTRAP_OUTPUT: join(work, 'unused-rust-output.rs'), JET_BOOTSTRAP_RECEIPT: receipt, JET_STORE_DIR: store, JET_KEEP_GOING: '1', JET_KEEP_GOING_REPORT: census, ...policy });
  if (process.env.JET_PACK_MUSL_LIB) env.JET_PACK_MUSL_LIB = process.env.JET_PACK_MUSL_LIB;
  const jetEnv = join(resolve(repo), 'Tools/agent/jet-env');
  const toolchain = nativeCToolchain(jetEnv, env, sourceRoot);
  // RUST_MIN_STACK covers spawned Rust threads, not the main stack. Native
  // compiler recursion needs the same explicit 1 GiB main-stack bound as CLI
  // stages; GNU time records the factory's elapsed time and peak RSS.
  const timing = join(work, 'factory.time');
  const start = Date.now();
  const pin = cpus === undefined ? [] : compilerCpuPin(cpus);
  const run = spawnSync(gnuTime(), ['-v', '-o', timing, jetEnv, 'env', `HOME=${home}`, 'prlimit', '--stack=1073741824:1073741824', '--', ...pin, record.factory.path], { cwd: sourceRoot, env, stdio: 'inherit' });
  if (run.error) throw run.error;
  const seconds = (Date.now() - start) / 1000;
  return { record, receipt, census, rc: run.status ?? 1, signal: run.signal, compilerProtocol: 'bootstrap-factory', sourceCommit: record.commit, toolchain, timing, seconds, peakRssKb: peakRss(timing), ...(cpus === undefined ? {} : { compilerCpus: cpus }) };
}
// Compiler thread admission (JetFoundation Threads.jet compiler_threads_admit)
// starts at the CPUs the process may run on: the factory's request fills
// `cores` from Rust std available_parallelism (Bootstrap/Host/Native.rs) and
// the native CLI's from the runtime's port of it (JetRuntime
// HostParallelism.jet), both the sched_getaffinity count capped by the cgroup
// CPU quota. Neither side reads any other thread knob (the factory takes no
// `--threads` cap), so pinning the affinity to the same `count` CPUs gives
// every compile that must match byte-for-byte the same worker ceiling. The
// memory term of admission (MemAvailable / cgroup headroom) still lowers the
// ceiling when free memory cannot hold `count` threads' scratch.
export function compilerCpuPin(count) {
  if (!Number.isInteger(count) || count < 1) throw Error(`compiler CPU count must be a positive integer, not ${count}`);
  return ['taskset', '-c', count === 1 ? '0' : `0-${count - 1}`];
}
// GNU time, never the shell keyword.
export function gnuTime() {
  const path = spawnSync('bash', ['-c', 'type -P time'], { encoding: 'utf8' }).stdout.trim();
  if (!path || !spawnSync(path, ['--version'], { encoding: 'utf8' }).stdout.includes('GNU')) throw Error('GNU time is required to measure native stages');
  return path;
}
export function peakRss(timing) {
  const text = existsSync(timing) ? readFileSync(timing, 'utf8') : '';
  const match = /Maximum resident set size \(kbytes\): (\d+)/.exec(text);
  return match ? Number(match[1]) : null;
}
// The C compiler and archiver a native CLI route selects (`$CC`/`$AR`, else
// cc/ar) inside `jetEnv`, with their resolved paths and version lines.
export function nativeCToolchain(jetEnv, env, cwd) {
  const probe = 'for tool in "${CC:-cc}" "${AR:-ar}"; do path=$(command -v -- "$tool") || exit 3; printf "%s\\t%s\\n" "$path" "$("$path" --version 2>&1 | head -n 1)"; done; printf "%s\\n" "${NIX_HARDENING_ENABLE-<unset>}"';
  const result = spawnSync(jetEnv, ['bash', '-c', probe], { cwd, env, encoding: 'utf8' });
  const rows = result.stdout.trim().split('\n');
  if (result.status !== 0 || rows.length !== 3) throw Error(`cannot resolve the native C toolchain through ${jetEnv} (rc ${result.status}): ${result.stdout}${result.stderr}`);
  const [cc, ar] = rows.slice(0, 2).map(row => { const [path, version] = row.split('\t'); return { path, version }; });
  return { jetEnv, cc, ar, nixHardeningEnable: rows[2] };
}
export function invokeStageZeroFactory(options) {
  const result = runFactoryProtocol(options, true);
  if (result.rc !== 0) throw Error(`bootstrap factory exited ${result.rc} ${result.signal ?? ''}`);
  const admitted = admitFactoryReceipt(readFileSync(result.receipt, 'utf8'), readFileSync(result.census, 'utf8'));
  return { ...result, admitted };
}
export function observeStageZeroNativeLowering(options) {
  if (Object.keys(options.policy).length !== 1 || !options.policy.JET_FRONT_NATIVE_OUTPUT) throw Error('discovery accepts only an unlinked native output prefix');
  // Entry.native emits its checked LIR/object before the absent pack stops
  // linking. This observation can NEVER qualify the whole factory or a CLI.
  return { ...runFactoryProtocol(options, false), admitted: false, link: 'BLOCKED: runtime pack deliberately absent during parallel discovery' };
}
