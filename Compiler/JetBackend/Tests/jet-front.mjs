// Candidate bootstrap factory: checked Jet MIR -> Jet lowering -> Jet object.
// The receipt observes completion only; no Rust MIR or Rust source is a feed.
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

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
