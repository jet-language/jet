// A criterion's proof: the `jet test` run that settles it. The Compiler
// package's profiles (cases, ui, golden, core, packages) carry a filter, the
// substring `jet test --filter` matches against test ids; `cmd` carries the
// arguments of any other `jet test` run. `tower prove` runs the proofs and meets
// the criteria that pass; `tower lint --proofs` finds filters that match no test.
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative } from 'node:path';
import { TowerError } from './store.mjs';
import { proofCommand } from './ui/board-state.js';

export const PROOF_PROFILES = ['cases', 'ui', 'golden', 'core', 'packages', 'cmd'];
// Profiles whose suites are corpora: `--list` names their case ids and
// `--cases FILE` runs any set of them, so one run covers every filter.
const CORPUS_PROFILES = new Set(['cases', 'ui', 'golden']);
const PACKAGE = 'Compiler';
// Arguments that would need a shell to mean what they say.
const SHELL_CHARS = /["'`|;&$<>(){}\\]/;

const fail = (code, msg) => { throw new TowerError(code, msg); };

// The proof a `jet test …` command states, or null when it states none.
export function parseProofCommand(command) {
  const words = String(command ?? '').trim().replace(/^`|`$/g, '').trim().split(/\s+/);
  if (words[0] !== 'jet' || words[1] !== 'test' || words.length < 3) return null;
  const args = words.slice(2);
  if (args.some(arg => SHELL_CHARS.test(arg))) return null;
  const options = new Map();
  let target = null;
  for (let i = 0; i < args.length; i++) {
    const [flag, inline] = args[i].split(/=(.*)/s);
    if (flag === '--profile' || flag === '--filter') {
      const value = inline ?? args[++i];
      if (!value || options.has(flag)) { options.clear(); target = null; break; }
      options.set(flag, value);
    } else if (target == null && !args[i].startsWith('-')) target = args[i];
    else { options.clear(); target = null; break; }
  }
  const profile = options.get('--profile');
  if (target === PACKAGE && options.size === 2 && PROOF_PROFILES.includes(profile) && profile !== 'cmd')
    return { profile, filter: options.get('--filter') };
  return { profile: 'cmd', args };
}

// A stored or supplied proof, checked; `label` names it in the error.
export function checkProof(proof, label = 'proof') {
  if (proof === null || typeof proof !== 'object' || Array.isArray(proof)) fail('E_INVALID', `${label} must be an object`);
  if (!PROOF_PROFILES.includes(proof.profile)) fail('E_INVALID', `${label}.profile must be one of ${PROOF_PROFILES.join(', ')}`);
  if (proof.profile === 'cmd') {
    if (!Array.isArray(proof.args) || !proof.args.length || proof.args.some(arg => typeof arg !== 'string' || !arg || SHELL_CHARS.test(arg)))
      fail('E_INVALID', `${label}.args must be the words of a jet test command, with no shell quoting`);
    if (proof.filter !== undefined) fail('E_INVALID', `${label} with profile cmd takes args, not filter`);
  } else {
    if (typeof proof.filter !== 'string' || !proof.filter.trim() || /\s/.test(proof.filter) || SHELL_CHARS.test(proof.filter))
      fail('E_INVALID', `${label}.filter must be one word naming the tests`);
    if (proof.args !== undefined) fail('E_INVALID', `${label} with profile ${proof.profile} takes filter, not args`);
  }
  return proof.profile === 'cmd' ? { profile: 'cmd', args: [...proof.args] } : { profile: proof.profile, filter: proof.filter };
}

// `--proof` input: a `jet test …` command, '' to clear, or a proof object.
export function proofInput(raw) {
  if (raw === '' || raw === null) return null;
  if (typeof raw === 'object') return checkProof(raw);
  const parsed = parseProofCommand(raw);
  if (!parsed) fail('E_INVALID', `--proof takes a jet test command, such as "jet test Compiler --profile cases --filter parser/" (got "${raw}")`);
  return parsed;
}

// The filter a proof selects tests with, or null when its command has none.
export function proofFilter(proof) {
  if (proof.profile !== 'cmd') return proof.filter;
  const at = proof.args.findIndex(arg => arg === '--filter' || arg.startsWith('--filter='));
  if (at < 0) return null;
  return proof.args[at].includes('=') ? proof.args[at].slice('--filter='.length) : proof.args[at + 1] ?? null;
}

// Criteria written before the proof field state it as a closing sentence,
// "… Proof: `jet test …`." Move that command into the field. Returns whether
// the row changed.
const TRAILING_PROOF = /\s*Proof:\s*`(jet test [^`]+)`\.?\s*$/;
export function migrateCriterionProof(item) {
  if (!item || typeof item !== 'object' || item.proof !== undefined || typeof item.text !== 'string') return false;
  const found = item.text.match(TRAILING_PROOF);
  const proof = found && parseProofCommand(found[1]);
  if (!proof) return false;
  item.proof = proof;
  const rest = item.text.slice(0, found.index).trim();
  if (rest) item.text = rest;
  return true;
}

// ---- tower prove ------------------------------------------------------------

// The open, unblocked criteria with a proof on the live cards in scope. A
// blocked row waits on other work, so its proof is not run yet.
export function proveTargets(s, { card = null, epoch = null } = {}) {
  const cards = s.cards.filter(c => c.phase !== 'done'
    && (card == null || c.id === card.id)
    && (epoch == null || c.epoch === epoch));
  return cards.flatMap(c => (c.criteria || [])
    .filter(item => item.status === 'open' && item.proof && !item.blockedBy)
    .map(item => ({ card: c, item })));
}

// The runs that settle `targets`: one per corpus profile with every filter
// combined, and one per distinct command otherwise.
export function proveRuns(targets) {
  const runs = new Map();
  for (const target of targets) {
    const { proof } = target.item;
    const key = CORPUS_PROFILES.has(proof.profile) ? proof.profile : proofCommand(proof);
    if (!runs.has(key)) runs.set(key, { profile: proof.profile, corpus: CORPUS_PROFILES.has(proof.profile), command: key === proof.profile ? `jet test ${PACKAGE} --profile ${proof.profile} --cases FILE` : key, targets: [] });
    runs.get(key).targets.push(target);
  }
  return [...runs.values()];
}

const readRows = (path) => {
  try { return JSON.parse(readFileSync(path, 'utf8')).results || []; } catch { return null; }
};

// One verdict per target of `run`: { target, pass, passed, total, detail }.
function judge(run, rows, exit, idsOf) {
  return run.targets.map(target => {
    if (rows == null) return { target, pass: false, passed: 0, total: 0, detail: `exit ${exit}, no results document` };
    const mine = rows.filter(row => idsOf(target).includes(row.case));
    const passed = mine.filter(row => row.status === 'PASS').length;
    const failing = mine.filter(row => row.status !== 'PASS').map(row => `${row.status} ${row.case}${row.tier && row.tier !== '-' ? ` ${row.tier}` : ''}`);
    if (!run.corpus && exit !== 0 && !failing.length) failing.push(`exit ${exit}`);
    if (!mine.length && run.corpus) return { target, pass: false, passed: 0, total: 0, detail: 'no result rows' };
    return { target, pass: !failing.length, passed, total: mine.length, detail: failing.slice(0, 3).join('; ') };
  });
}

// Run every proof in `runs` with the `jet` binary in `tree`. `spawn` is
// injectable for tests; the runs' own output goes to stderr.
export function executeRuns(runs, { jet, tree, spawn = spawnSync }) {
  const work = mkdtempSync(join(tmpdir(), 'tower-prove-'));
  const exec = (args) => spawn(jet, args, { cwd: tree, encoding: 'utf8', maxBuffer: 1 << 30, stdio: ['ignore', 'pipe', 'inherit'] });
  try {
    return runs.flatMap((run, index) => {
      const results = join(work, `results-${index}.json`);
      if (!run.corpus) {
        const ran = exec(['test', ...proofArgs(run.targets[0].item.proof), `--results=${results}`]);
        if (ran.stdout) process.stderr.write(ran.stdout);
        const rows = readRows(results);
        return judge(run, rows ?? (ran.status === 0 ? [] : null), ran.status, () => (rows || []).map(row => row.case));
      }
      const listed = exec(['test', PACKAGE, '--profile', run.profile, '--list']);
      if (listed.status !== 0)
        return run.targets.map(target => ({ target, pass: false, passed: 0, total: 0, detail: `--list exit ${listed.status}` }));
      const ids = String(listed.stdout).split('\n').map(line => (line.includes('\t') ? line.split('\t')[1] : line).trim()).filter(Boolean);
      const chosen = new Map(run.targets.map(target => [target, ids.filter(id => id.includes(target.item.proof.filter))]));
      const empty = run.targets.filter(target => !chosen.get(target).length)
        .map(target => ({ target, pass: false, passed: 0, total: 0, detail: `filter ${target.item.proof.filter} matches no test` }));
      const live = { ...run, targets: run.targets.filter(target => chosen.get(target).length) };
      if (!live.targets.length) return empty;
      const selection = join(work, `cases-${index}.txt`);
      writeFileSync(selection, `${[...new Set(live.targets.flatMap(target => chosen.get(target)))].join('\n')}\n`);
      const ran = exec(['test', PACKAGE, '--profile', run.profile, `--cases=${selection}`, `--results=${results}`]);
      if (ran.stdout) process.stderr.write(ran.stdout);
      return [...empty, ...judge(live, readRows(results), ran.status, target => chosen.get(target))];
    });
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

const proofArgs = (proof) => proof.profile === 'cmd' ? proof.args : [PACKAGE, '--profile', proof.profile, '--filter', proof.filter];

// A run that wrote no result rows (a code suite) proves by its exit alone.
export const proveEvidence = (sha, verdict) =>
  `${sha}: ${proofCommand(verdict.target.item.proof)} PASS${verdict.total ? ` ${verdict.passed}/${verdict.total}` : ''}`;

// ---- tower lint --proofs ----------------------------------------------------

// Every test file under the tree's Tests and Examples directories, as paths
// relative to the tree, lower-cased for the case-insensitive match.
function testPaths(tree) {
  const paths = [];
  const walk = (dir) => {
    let entries;
    try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
    for (const entry of entries) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walk(path);
      else paths.push(relative(tree, path).split('\\').join('/').toLowerCase());
    }
  };
  for (const root of ['Tests', 'Examples']) walk(join(tree, root));
  return paths;
}

// Live criteria whose proof filter matches no test file under `tree`.
export function unmatchedProofs(s, tree) {
  const paths = testPaths(tree);
  if (!paths.length) fail('E_INVALID', `--tree ${tree} holds no Tests or Examples files`);
  const findings = [];
  for (const card of s.cards) {
    if (card.phase === 'done') continue;
    for (const item of card.criteria || []) {
      const filter = item.proof && proofFilter(item.proof);
      if (!filter) continue;
      const needle = filter.toLowerCase();
      if (paths.some(path => path.includes(needle))) continue;
      findings.push({ rule: 'proof-unmatched', ref: `#${card.num} c${item.n}`, msg: `${proofCommand(item.proof)}: the filter matches no file under Tests or Examples` });
    }
  }
  return findings;
}
