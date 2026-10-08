// Candidate bootstrap factory: checked Jet MIR -> Jet lowering -> Jet linker.
// The receipt observes completion only; no Rust MIR or Rust source is a feed.
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';

export function candidateJetc0() {
  if (process.env.JET_FRONT_JETC0) return resolve(process.env.JET_FRONT_JETC0);
  const artifacts = JSON.parse(readFileSync(join(process.env.HOME, '.cache/jet-dev/dev-artifacts.json'), 'utf8'));
  if (!artifacts.candidate_jetc0) throw new Error('no candidate_jetc0: publish one or set JET_FRONT_JETC0');
  return resolve(artifacts.candidate_jetc0);
}

export function frontReceipt(text, run) {
  const reports = text.split('\n').filter(line => line.startsWith('report_json=')).map(line => JSON.parse(line.slice(12)));
  const errors = reports.filter(report => !/warning|lint/i.test(String(report.severity ?? report.level ?? 'error')));
  if (run.status !== 0 || !text.split('\n').includes('complete=true') || errors.length) {
    const first = errors[0];
    return { verdict: 'no-mir', detail: first ? `${first.code ?? 'UNKNOWN'}: ${first.what ?? first.message}` : `Jet front exit ${run.status ?? run.signal}: ${run.error?.message ?? 'incomplete receipt'}` };
  }
  return null;
}

export function runJetFront({ candidate, file, root, cwd, prefix, receipt, pack, store }) {
  if (!pack || !existsSync(pack)) throw new Error('--front jet requires JET_RUNTIME_PACK from the candidate tree');
  if (!existsSync(candidate)) throw new Error(`candidate jetc0 missing: ${candidate}`);
  const packageFile = join(root, 'package.jet');
  if (!existsSync(packageFile)) writeFileSync(packageFile, 'name: "native_front_case"\nversion: "0.0.1"\nedition: "2028"\n');
  for (const suffix of ['.o', '.exe', '.lir', '.issues', '.pack-issues']) rmSync(prefix + suffix, { force: true });
  rmSync(receipt, { force: true });
  const run = spawnSync(candidate, [], {
    cwd,
    env: {
      ...process.env,
      JET_BOOTSTRAP_MODE: 'factory', JET_BOOTSTRAP_FACTORY_TIER: 'aot',
      JET_BOOTSTRAP_SOURCE_ROOT: resolve(root), JET_BOOTSTRAP_ENTRY: relative(root, file),
      JET_BOOTSTRAP_OUTPUT: `${prefix}.unused.output`, JET_BOOTSTRAP_RECEIPT: resolve(receipt),
      JET_FRONT_NATIVE_OUTPUT: resolve(prefix), JET_RUNTIME_PACK: resolve(pack),
      JET_STORE_DIR: resolve(store), RUST_MIN_STACK: '8388608',
    },
    encoding: 'utf8', timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 120000), maxBuffer: 1 << 27,
  });
  writeFileSync(`${prefix}.front.log`, `${run.stdout ?? ''}${run.stderr ?? ''}`);
  const text = existsSync(receipt) ? readFileSync(receipt, 'utf8') : '';
  const problem = frontReceipt(text, run);
  if (problem) return problem;
  if (!existsSync(`${prefix}.exe`) && !existsSync(`${prefix}.issues`) && !existsSync(`${prefix}.pack-issues`)) {
    return { verdict: 'no-mir', detail: 'candidate has no JET_FRONT_NATIVE_OUTPUT entry (rebuild it from this tree)' };
  }
  return null;
}
