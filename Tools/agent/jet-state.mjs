#!/usr/bin/env node
// On-demand candidate retrieval, not a work ledger or semantic completeness proof.
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, isAbsolute, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { canonicalDataDir, dataFile, findDataDir, historyFile, readJSON } from '../tower/app/paths.mjs';
import { clearanceOf, laneOf, normalize, validateStoredHistory } from '../tower/app/store.mjs';
import { hasPendingRepair } from '../tower/app/repair-journal.mjs';
import { readLedger, SYNTAX } from './lexical-ledger.mjs';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const hash = value => createHash('sha256').update(value).digest('hex');
const text = value => JSON.stringify(value ?? '').normalize('NFKC').toLowerCase();
const words = value => [...new Set(value.normalize('NFKC').toLowerCase().match(/[\p{L}\p{N}_-]+/gu) ?? [])];
const errorText = error => error instanceof Error ? error.message : String(error);
const provider = (status, detail = null) => ({ status, detail });
function stamp(path) {
  try { const s = statSync(path, { bigint: true }); return `${s.dev}:${s.ino}:${s.size}:${s.mtimeNs}:${s.ctimeNs}`; }
  catch { return null; }
}
function git(root, args) {
  return execFileSync('git', ['-C', root, ...args], { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024,
    env: { ...process.env, GIT_OPTIONAL_LOCKS: '0' }, stdio: ['ignore', 'pipe', 'pipe'] });
}
function ref(root, name) {
  try { return git(root, ['rev-parse', '--verify', `${name}^{commit}`]).trim(); } catch { return null; }
}
function branch(root) {
  try { return git(root, ['symbolic-ref', '--quiet', '--short', 'HEAD']).trim(); } catch { return null; }
}
export function workingFingerprint(root, status, indexPath = resolve(root, '.git/index')) {
  const tokens = status.split('\0'); const paths = [];
  for (let i = 0; i < tokens.length && tokens[i]; i++) {
    const row = tokens[i]; paths.push(row.slice(3));
    if (/[RC]/.test(row.slice(0, 2)) && tokens[i + 1]) paths.push(tokens[++i]);
  }
  return hash(JSON.stringify({ status, index: stamp(indexPath), files: paths.map(path => [path, stamp(resolve(root, path))]) }));
}
function worktreeIdentity(root) {
  const status = git(root, ['status', '--porcelain=v1', '-z', '--untracked-files=all']);
  const indexPath = resolve(root, git(root, ['rev-parse', '--git-path', 'index']).trim());
  return { head: ref(root, 'HEAD'), status, fingerprint: workingFingerprint(root, status, indexPath) };
}
export function parseWorktrees(raw) {
  return raw.trim().split(/\n\n+/).filter(Boolean).map(block => {
    const row = {};
    for (const line of block.split('\n')) {
      const at = line.indexOf(' '); row[at < 0 ? line : line.slice(0, at)] = at < 0 ? true : line.slice(at + 1);
    }
    return { path: row.worktree, head: row.HEAD, branch: row.branch?.replace(/^refs\/heads\//, '') ?? null,
      locked: !!row.locked, prunable: !!row.prunable };
  });
}
export function parseNameStatus(raw) {
  const tokens = raw.split('\0'); const rows = [];
  for (let i = 0; i < tokens.length && tokens[i];) {
    const status = tokens[i++]; const path = tokens[i++];
    if (!path) throw new Error('incomplete git name-status row');
    if (/^[RC]/.test(status)) {
      const destination = tokens[i++]; if (!destination) throw new Error('incomplete git rename/copy row');
      rows.push({ status, old_path: path, path: destination });
    } else rows.push({ status, path });
  }
  return rows;
}
export function queryMatcher(subject) {
  const terms = words(subject);
  if (!terms.length) throw new Error('subject must contain a word or source identity');
  return value => {
    const body = text(value);
    // OR retrieval intentionally includes aliases/common-job leads; it is not an equivalence join.
    return terms.filter(term => body.includes(term));
  };
}
function retrievalClass(subject, record) {
  const phrase = subject.normalize('NFKC').toLowerCase().replace(/[_-]/g, ' ');
  const title = text(record.title).replace(/[_-]/g, ' ');
  const body = text(record).replace(/[_-]/g, ' ');
  const terms = words(phrase);
  if (title.includes(phrase)) return 'subject-in-title';
  if (terms.every(term => title.includes(term))) return 'title-terms';
  if (body.includes(phrase)) return 'body-phrase';
  if (terms.every(term => body.includes(term))) return 'body-all-terms';
  return terms.some(term => body.includes(term)) ? 'term-lead' : 'exact-link-only';
}
const RETRIEVAL_ORDER = ['subject-in-title', 'title-terms', 'body-phrase', 'body-all-terms', 'term-lead', 'exact-link-only'];
function retrievalOrder(a, b) {
  return RETRIEVAL_ORDER.indexOf(a.retrieval_class) - RETRIEVAL_ORDER.indexOf(b.retrieval_class);
}

export function decisionState(decision, card, { head, dirty, receipts = [], witnesses = {} } = {}) {
  if (decision.status !== 'ratified' || decision.draft) return { intended: decision.draft ? 'draft' : decision.status,
    observed: 'unknown', work: card?.phase ?? 'unknown', built: 'not-ratified' };
  const receipt = receipts.find(r => r && r.decision_id === decision.id && r.scope === 'decision-acceptance'
    && /^[0-9a-f]{40}(?:[0-9a-f]{24})?$/.test(head ?? '') && r.commit === head && dirty === false
    && r.exercised === true && typeof r.command === 'string' && r.command.trim()
    && Array.isArray(r.witnesses) && r.witnesses.length && r.witnesses.every(w => w
      && typeof w.path === 'string' && w.path.length && /^[0-9a-f]{64}$/.test(w.sha256 ?? '')
      && witnesses[w.path] === w.sha256));
  const proposal = card?.kind === 'research' || /\b(proposal[- ]only|evidence[- ]only)\b/i.test(card?.title ?? '');
  return { intended: 'ratified', observed: receipt ? 'exercised-at-matching-identity' : 'unknown',
    work: card?.phase ?? 'unknown', built: proposal ? 'proposal-only' : receipt ? 'ratified-built'
      : card && card.phase !== 'done' ? 'ratified-unbuilt-or-partial' : 'ratified-build-evidence-unknown',
    receipt: receipt ?? null,
    limitation: receipt ? 'Supplied exercised receipt covers the decision acceptance scope; this tool did not execute it.'
      : 'Board phase is not implementation or runtime proof; absence and completeness are not established.' };
}

// Pure indexed projection shared by fixtures and live collection.
export function subjectRows({ subject, state, history = {}, head, dirty, receipts = [], witnesses = {} }) {
  const match = queryMatcher(subject);
  const liveCardIds = new Set((state.cards ?? []).map(c => c.id));
  const liveDecisionIds = new Set((state.decisions ?? []).map(d => d.id));
  const cards = [...(state.cards ?? []), ...(history.cards ?? []).filter(h => !liveCardIds.has(h.id))];
  const decisions = [...(state.decisions ?? []), ...(history.decisions ?? []).filter(h => !liveDecisionIds.has(h.id))];
  const cardMap = new Map(cards.map(c => [c.id, c]));
  const exactRefs = new Map([...cards.flatMap(c => [[`#${c.num}`, c.id], [c.id, c.id]]), ...decisions.map(d => [d.id, d.id])]);
  const decisionMap = new Map(decisions.map(d => [d.id, d]));
  const decisionsByCard = new Map();
  for (const d of decisions) {
    const linked = decisionsByCard.get(d.cardId) ?? []; linked.push(d); decisionsByCard.set(d.cardId, linked);
  }
  const reasons = new Map();
  for (const record of [...cards, ...decisions]) {
    const hits = match(record); if (hits.length) reasons.set(record.id, [`terms: ${hits.join(', ')}`]);
  }
  // Follow exact card/decision and explicit supersession links to closure, never fuzzy equality.
  let changed = true;
  while (changed) {
    changed = false;
    const add = (id, why) => { if (id && !reasons.has(id)) { reasons.set(id, [why]); changed = true; } };
    for (const d of decisions) {
      if (reasons.has(d.id)) { add(d.cardId, `decision owner: ${d.id}`); add(d.supersededBy, `supersession field: ${d.id}`); }
      if (reasons.has(d.cardId) || reasons.has(d.supersededBy)) add(d.id, `exact link: ${d.cardId ?? d.supersededBy}`);
    }
    for (const c of cards) if (reasons.has(c.id)) {
      for (const reference of [...(c.refs ?? []), ...(c.blockedBy ?? [])]) {
        if (typeof reference === 'string') add(exactRefs.get(reference), `explicit card link: #${c.num}`);
      }
    }
  }
  const successor = new Map();
  // Tower verdicts store the replaced decision in supersededBy (despite the field's name).
  for (const d of decisions) if (d.status === 'ratified' && !d.draft && d.group === 'verdict' && d.supersededBy) {
    const list = successor.get(d.supersededBy) ?? []; list.push(d.id); successor.set(d.supersededBy, list);
  }
  const decisionRows = decisions.filter(d => reasons.has(d.id)).map(d => {
    const c = cardMap.get(d.cardId);
    return { ref: d.id, title: d.title, card: c ? `#${c.num}` : d.cardId ?? null, status: d.status,
      draft: !!d.draft, outcome: d.outcome ?? null, owner_comment: d.comment ?? null,
      ratified_at: d.ratifiedAt ?? null, acceptance: d.options?.find(o => o.key === d.outcome) ?? d.outcome ?? null,
      authoritative_record: { provider: 'tower', collection: liveDecisionIds.has(d.id) ? 'decisions' : 'history.decisions', id: d.id },
      match_reasons: reasons.get(d.id),
      retrieval_class: retrievalClass(subject, d),
      axes: { ...decisionState(d, c, { head, dirty, receipts, witnesses }),
        ...(successor.has(d.id) ? { intended: 'retired/superseded' } : {}) },
      supersession_field: d.supersededBy ?? null, replaced_by: successor.get(d.id) ?? [],
      authority: successor.has(d.id) ? 'explicitly-superseded' : 'no-explicit-successor-known',
      archived: !liveDecisionIds.has(d.id) };
  }).sort(retrievalOrder);
  const selectedCards = cards.filter(c => reasons.has(c.id)).map(c => {
    const linked = decisionsByCard.get(c.id) ?? [];
    return { ref: `#${c.num}`, id: c.id, title: c.title, kind: c.kind, phase: c.phase,
      lane: laneOf(c, [...linked, ...(c.blockedBy ?? []).map(id => decisionMap.get(id)).filter(Boolean)], cards),
      clearance: clearanceOf(c, linked),
      in_flight: ['building', 'verify'].includes(c.phase),
      assignee: c.assignee ?? null, claimed_at: c.claimedAt ?? null, blocked_by: c.blockedBy ?? [],
      refs: c.refs ?? [], criteria: c.criteria ?? [], handoff: c.handoff ?? null,
      match_reasons: reasons.get(c.id), archived: !liveCardIds.has(c.id),
      retrieval_class: retrievalClass(subject, c),
      authoritative_record: { provider: 'tower', collection: liveCardIds.has(c.id) ? 'cards' : 'history.cards', id: c.id } };
  }).sort(retrievalOrder);
  const events = [...(state.events ?? []), ...(history.events ?? [])].filter(e => match(e).length || reasons.has(e.ref));
  return { decisions: decisionRows, cards: selectedCards,
    open_ballots: decisionRows.filter(d => d.draft || d.status === 'open'),
    historical_context: events.filter(e => /delete|reject|merge|retire/.test(e.action ?? '')),
    owner_directions: (state.questions ?? []).filter(q => reasons.has(q.cardId) && (q.by === 'owner' || q.answer))
      .map(q => ({ ref: q.id, card_id: q.cardId, text: q.text, answer: q.answer, by: q.by })),
    authority_limit: 'Explicit supersession only. Unlinked overlapping decisions remain unresolved; dates do not amend law.' };
}

function documents(root, match, inputs) {
  const rows = [];
  for (const category of ['research', 'proposals']) {
    const visit = dir => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const path = resolve(dir, entry.name);
        if (entry.isDirectory()) visit(path);
        else if (entry.isFile() && /\.(md|html?)$/.test(entry.name)) {
          const identity = stamp(path);
          const body = readFileSync(path, 'utf8'); const rel = relative(root, path);
          inputs.push({ path: rel, sha256: hash(body), observed_stamp: identity });
          const lines = body.split('\n'); let heading = '';
          const hits = [];
          for (let i = 0; i < lines.length; i++) {
            if (/^#{1,6} /.test(lines[i])) heading = lines[i].replace(/^#+\s*/, '');
            const terms = match(lines[i]);
            if (terms.length) hits.push({ line: i + 1, heading, terms, excerpt: lines[i].slice(0, 240) });
          }
          if (hits.length) rows.push({ path: rel, category, state: category === 'proposals' ? 'unratified-proposal'
            : 'dated-research-not-current-law', matches: hits.slice(0, 8), total_matching_lines: hits.length,
            source_urls: [...new Set(body.match(/https?:\/\/[^\s<>"\)]+/g) ?? [])],
            coverage: 'text indexed; body not technically reviewed by this tool' });
        }
      }
    };
    try { visit(resolve(root, 'Docs', category)); }
    catch (error) { rows.push({ category, state: 'unknown', error: errorText(error) }); }
  }
  return rows;
}
function changes(root, args, match, lane) {
  const files = parseNameStatus(git(root, ['diff', '--no-ext-diff', '--no-textconv', '--name-status', '-z', '-M', ...args]));
  // Tower owns these work-state bytes; they are not code changes. Preserve paths
  // separately instead of repeatedly decoding enormous board/history patches.
  const ledgerPath = path => /^(?:Tools\/tower\/)?\.tower\//.test(path);
  const ledger = files.filter(f => ledgerPath(f.path) || (f.old_path && ledgerPath(f.old_path)));
  const sourceFiles = files.filter(f => !ledger.includes(f));
  const result = { total_changed_files: files.length, relevant_files: [],
    ledger_changes: ledger.map(f => ({ ...f, lane, semantic_state: 'work-ledger-not-code' })),
    limitation: 'Filename/hunk matches are candidate semantic changes, not reviewed semantics or active assignments.' };
  if (!sourceFiles.length) return result;
  const relevant = sourceFiles.filter(f => match(f).length).map(f => ({ ...f, lane, match: 'path', semantic_state: 'unreviewed' }));
  result.relevant_files = relevant;
  let patch;
  try {
    patch = git(root, ['diff', '--no-ext-diff', '--no-textconv', '--unified=0', ...args, '--', '.',
      ':(exclude).tower/**', ':(exclude)Tools/tower/.tower/**']);
  } catch (error) {
    result.hunk_coverage = 'unknown'; result.error = errorText(error);
    result.unknown_files = sourceFiles.map(f => ({ ...f, lane, semantic_state: 'unknown' }));
    return result;
  }
  for (const block of patch.split(/(?=^diff --git )/m)) {
    if (!block || !match(block).length) continue;
    const header = block.split('\n', 1)[0];
    const file = sourceFiles.find(f => header.includes(`b/${f.path}`) || (f.old_path && header.includes(`a/${f.old_path}`)));
    if (file) {
      let row = relevant.find(r => r.path === file.path);
      if (!row) { row = { ...file, lane, match: 'hunk', semantic_state: 'unreviewed' }; relevant.push(row); }
      row.candidate_hunks = block.slice(0, 2400); row.truncated = block.length > 2400;
    } else relevant.push({ lane, path: null, semantic_state: 'unknown', candidate_hunks: block.slice(0, 2400),
      limitation: 'Unparsed git path/header; retained rather than silently dropped.' });
  }
  return result;
}
function collectGit(root, devRef, baseRef, match) {
  const head = ref(root, 'HEAD'); const base = ref(root, baseRef); const dev = ref(root, devRef);
  const worktrees = parseWorktrees(git(root, ['worktree', 'list', '--porcelain']));
  const current = worktreeIdentity(root);
  const identity = { head, branch: branch(root),
    dev_ref: devRef, dev, base_ref: baseRef, base, status_sha256: current.fingerprint, dirty: !!current.status };
  const branches = [];
  const committedByHead = new Map();
  const committed = commit => {
    if (!base || !commit) return { state: 'unknown', reason: 'missing base/head' };
    if (!committedByHead.has(commit)) committedByHead.set(commit, changes(root, [`${base}...${commit}`], match, 'committed'));
    return committedByHead.get(commit);
  };
  const emptyChanges = () => ({ total_changed_files: 0, relevant_files: [], ledger_changes: [] });
  if (dev && base) {
    const row = { name: devRef, head: dev, base, state: 'branch-diff-not-assignment' };
    try { row.committed = committed(dev); }
    catch (error) { row.state = 'unknown'; row.error = errorText(error); }
    branches.push(row);
  }
  for (const wt of worktrees) {
    const row = { name: relative(root, wt.path) || '.', branch: wt.branch, head: wt.head,
      state: 'worktree-presence-not-assignment' };
    try {
      const before = worktreeIdentity(wt.path); const status = before.status;
      row.status_sha256 = before.fingerprint; row.dirty = !!status;
      row.committed = committed(before.head);
      const trackedStatus = status.split('\0').filter(line => /^[ MARCUDT?!]{2} /.test(line) && !line.startsWith('??'));
      row.staged = trackedStatus.some(line => line[0] !== ' ') ? changes(wt.path, ['--cached'], match, 'staged') : emptyChanges();
      row.unstaged = trackedStatus.some(line => line[1] !== ' ') ? changes(wt.path, [], match, 'unstaged') : emptyChanges();
      row.untracked = status.split('\0').filter(line => line.startsWith('?? ') && match(line.slice(3)).length)
        .map(line => ({ path: line.slice(3), semantic_state: 'unknown', lane: 'untracked' }));
      row.identity_before = `${before.head}:${before.fingerprint}`;
    } catch (error) { row.state = 'unknown'; row.error = errorText(error); }
    branches.push(row);
  }
  return { identity, branches, worktrees, worktree_list_sha256: hash(JSON.stringify(worktrees)) };
}

export function collectDigest({ root = ROOT, subject, devRef = 'dev', baseRef = 'master', receiptsPath = null } = {}) {
  root = resolve(root); const match = queryMatcher(subject); const inputs = []; const providers = {};
  let gitData = null; let state = null; let history = {}; let boardPath = null; let boardBefore = null; let historyBefore = null;
  let boardDir = null; let towerPending = false;
  try { gitData = collectGit(root, devRef, baseRef, match); providers.git = provider('available'); }
  catch (error) { providers.git = provider('unknown', errorText(error)); }
  try {
    // findDataDir's vendored fallback is relative to its module checkout, not
    // --root. Search the selected checkout's CLI home through the same resolver.
    const found = findDataDir(root) ?? findDataDir(resolve(root, 'Tools/tower'));
    const dir = found && canonicalDataDir(found);
    if (!dir) throw new Error('live canonical Tower board unavailable; no worktree-board fallback');
    boardDir = dir;
    boardPath = resolve(dir, basename(dataFile(found)));
    towerPending = hasPendingRepair(dir);
    if (towerPending) throw new Error('Tower pair transaction pending; read-only digest will not repair or consume it');
    boardBefore = stamp(boardPath); historyBefore = stamp(historyFile(dir));
    const raw = readJSON(boardPath, null); if (!raw) throw new Error('live Tower board missing');
    history = readJSON(historyFile(dir), { cards: [], decisions: [], events: [] }); validateStoredHistory(history);
    state = normalize(raw, history.cards, false);
    inputs.push({ path: relative(root, boardPath), sha256: hash(JSON.stringify(raw)), identity_kind: 'normalized-json' });
    inputs.push({ path: relative(root, resolve(dirname(boardPath), 'history.json')),
      sha256: hash(JSON.stringify(history)), identity_kind: 'normalized-json' });
    providers.tower = provider('available', { revision: state.meta.rev, path: relative(root, boardPath) });
  } catch (error) { providers.tower = provider('unknown', errorText(error)); state = null; }
  let syntax = [];
  const syntaxBefore = stamp(resolve(root, SYNTAX));
  try {
    syntax = readLedger(root);
    inputs.push({ path: `${SYNTAX}::LEXICAL_LEDGER`,
      sha256: hash(JSON.stringify(syntax)), identity_kind: 'parsed-registry-rows' });
    providers.syntax = provider('available');
  }
  catch (error) { providers.syntax = provider('unknown', errorText(error)); }
  let receipts = []; const witnesses = {};
  providers.receipts = provider('unknown', 'No exercised receipt provider supplied. Board done does not prove built.');
  if (receiptsPath) {
    try {
      const body = readFileSync(resolve(root, receiptsPath), 'utf8'); receipts = JSON.parse(body);
      if (!Array.isArray(receipts)) throw new Error('receipts must be an array');
      inputs.push({ path: relative(root, resolve(root, receiptsPath)), sha256: hash(body),
        observed_stamp: stamp(resolve(root, receiptsPath)) });
      for (const r of receipts) for (const w of r.witnesses ?? []) {
        if (isAbsolute(w.path) || relative(root, resolve(root, w.path)).startsWith('..')) throw new Error('witness must be root-relative');
        if (!(w.path in witnesses)) {
          const path = resolve(root, w.path); const identity = stamp(path);
          witnesses[w.path] = hash(readFileSync(path));
          inputs.push({ path: w.path, sha256: witnesses[w.path], observed_stamp: identity });
        }
      }
      providers.receipts = provider('available', 'Explicit supplied receipts; no tests executed.');
    } catch (error) { receipts = []; providers.receipts = provider('unknown', errorText(error)); }
  }
  const research = documents(root, match, inputs);
  providers.research = provider(research.some(r => r.state === 'unknown') ? 'partial' : 'available');
  providers.pending_cache = provider('unknown', 'Not a v1 provider; proposals remain proposal-only.');
  providers.unrecorded_owner_chat = provider('unknown', 'Read latest owner directions separately; this tool cannot discover unrecorded chat.');
  const rows = state ? subjectRows({ subject, state, history, head: gitData?.identity.head,
    dirty: gitData?.identity.dirty ?? true, receipts, witnesses }) : { decisions: [], cards: [], open_ballots: [], historical_context: [] };
  const refs = new Set(rows.decisions.map(d => d.ref));
  const idioms = syntax.filter(row => match(row).length || refs.has(row.decision)).map(row => ({ ...row,
    authoritative_ref: `${SYNTAX}::LEXICAL_LEDGER`, observed: 'registry-declaration-present',
    runtime_proof: 'unknown' }));
  const consistency = [];
  if (stamp(resolve(root, SYNTAX)) !== syntaxBefore
    || inputs.some(input => input.observed_stamp && stamp(resolve(root, input.path)) !== input.observed_stamp)) {
    consistency.push('Source/registry/receipt identity changed during collection');
  }
  if (boardPath && (stamp(boardPath) !== boardBefore || stamp(resolve(dirname(boardPath), 'history.json')) !== historyBefore)) consistency.push('Tower changed during collection');
  try {
    if (towerPending || (boardDir && hasPendingRepair(boardDir))) consistency.push('Tower pair transaction pending; fresh read required');
  } catch (error) { consistency.push(`Tower transaction freshness unknown: ${errorText(error)}`); }
  if (gitData) {
    try {
      const worktreeChanged = gitData.worktrees.map(wt => {
        const row = gitData.branches.find(b => b.name === (relative(root, wt.path) || '.') && b.branch === wt.branch);
        if (!row?.identity_before) return false;
        const after = worktreeIdentity(wt.path);
        row.identity_after = `${after.head}:${after.fingerprint}`;
        row.consistent = row.identity_before === row.identity_after;
        return !row.consistent;
      }).some(Boolean);
      if (ref(root, 'HEAD') !== gitData.identity.head || ref(root, devRef) !== gitData.identity.dev
        || ref(root, baseRef) !== gitData.identity.base
        || worktreeIdentity(root).fingerprint !== gitData.identity.status_sha256
        || hash(JSON.stringify(parseWorktrees(git(root, ['worktree', 'list', '--porcelain'])))) !== gitData.worktree_list_sha256
        || worktreeChanged) consistency.push('Git/worktree identity changed during collection');
    } catch (error) { consistency.push(`Git freshness unknown: ${errorText(error)}`); }
    if (!gitData.identity.dev) providers.development_branch = provider('unknown', `No ${devRef} ref; supply --dev-ref. No substitute assumed.`);
    if (!gitData.identity.base) providers.base_branch = provider('unknown', `No ${baseRef} ref; supply --base-ref.`);
  }
  return { schema: 'jet-state:v1', generated_at: new Date().toISOString(), subject,
    checkout: { root, ...gitData?.identity }, providers, inputs,
    consistent: consistency.length === 0, consistency_warnings: consistency,
    ...rows, branches: gitData?.branches ?? [], research, idioms,
    retrieval_limits: ['Normalized term matches are candidate discovery, not semantic equivalence or exhaustive coverage.',
      'No code absence, completeness, latest unrecorded authority, or runtime behavior inferred from keyword matches.',
      'Reinvoke before publication when board or checkout identity changed; inconsistent digests cannot authorize publication.'] };
}

export function renderText(digest, limit = 7) {
  const lines = [`# Jet state: ${digest.subject}`, `Identity: ${digest.checkout.head ?? 'unknown'}; Tower ${digest.providers.tower.detail?.revision ?? 'unknown'}; ${digest.consistent ? 'consistent' : 'INCONSISTENT — rerun before publication'}`];
  for (const [name, p] of Object.entries(digest.providers)) if (p.status !== 'available') lines.push(`${name}: ${p.status} — ${typeof p.detail === 'string' ? p.detail : JSON.stringify(p.detail)}`);
  const section = (name, rows, format) => {
    lines.push(`\n## ${name} (${rows.length})`);
    lines.push(...rows.slice(0, limit).map(row => `- ${format(row)}`));
    if (rows.length > limit) lines.push(`- ${rows.length - limit} more in JSON (text is a bounded preview, not scope completion).`);
  };
  section('Ratified decisions', digest.decisions.filter(d => d.status === 'ratified' && !d.draft), d => `${d.ref}: ${d.title} — ${d.axes.built}; ${d.authority}${d.replaced_by.length ? ` → ${d.replaced_by.join(', ')}` : ''}`);
  section('Open/draft ballots', digest.open_ballots, d => `${d.ref}: ${d.title} — ${d.draft ? 'draft' : d.status}`);
  section('Matching cards', digest.cards, c => `${c.ref}: ${c.title} — ${c.phase}${c.in_flight ? ' (in flight)' : ''}`);
  section('Branch/worktree changes', digest.branches.filter(b => b.state === 'unknown'
    || [b.committed, b.staged, b.unstaged].some(x => x?.relevant_files?.length || x?.hunk_coverage === 'unknown')
    || b.untracked?.length), b => {
    if (b.state === 'unknown') return `${b.name}: unknown — ${b.error}`;
    const lanes = [b.committed, b.staged, b.unstaged];
    const files = [...lanes.flatMap(x => x?.relevant_files ?? []), ...(b.untracked ?? [])];
    const preview = files.slice(0, 5).map(f => `${f.lane}:${f.status ?? '?'} ${f.path ?? '[unparsed]'}`).join('; ');
    const remainder = files.length > 5 ? `; ${files.length - 5} more files in JSON` : '';
    const gap = lanes.some(x => x?.hunk_coverage === 'unknown') ? '; hunk coverage UNKNOWN (paths retained in JSON)' : '';
    return `${b.name} (${b.branch ?? 'branch'}): ${preview}${remainder}${gap} — unreviewed candidate changes`;
  });
  section('Research/proposals', digest.research, r => `${r.path ?? r.category}: ${r.state}${r.matches?.[0] ? `; line ${r.matches[0].line}: ${r.matches[0].excerpt}` : ''}`);
  section('Registry spellings', digest.idioms, r => `${JSON.stringify(r.spelling)} — ${r.meaning} (${r.decision || 'unlinked'}); runtime proof unknown`);
  lines.push('\nLimits: candidate retrieval only; done ≠ built; read latest owner direction and focused source before proposing.');
  lines.push(...digest.consistency_warnings.map(w => `WARNING: ${w}`));
  return `${lines.join('\n')}\n`;
}

export function main(argv = process.argv.slice(2)) {
  try {
    const options = {}; let format = 'json'; let output = null;
    const names = { '--root': 'root', '--subject': 'subject', '--dev-ref': 'devRef', '--base-ref': 'baseRef', '--receipts': 'receiptsPath' };
    for (let i = 0; i < argv.length; i++) {
      const arg = argv[i];
      if (arg === '--help' || arg === '-h') {
        process.stdout.write('Usage: node Tools/agent/jet-state.mjs --subject TEXT [--root PATH] [--format json|text|markdown] [--dev-ref dev] [--base-ref master] [--receipts PATH] [--output PATH]\nRead-only on-demand candidate view. No default output file, Tower writes, remote fetches, builds or tests.\n'); return 0;
      }
      if (!(arg in names) && !['--format', '--output'].includes(arg)) throw new Error(`unknown option ${arg}`);
      const value = argv[++i]; if (!value || value.startsWith('--')) throw new Error(`missing value for ${arg}`);
      if (arg === '--format') format = value; else if (arg === '--output') output = value; else options[names[arg]] = value;
    }
    if (!options.subject) throw new Error('--subject is required');
    if (!['json', 'text', 'markdown'].includes(format)) throw new Error('format must be json, text or markdown');
    const digest = collectDigest(options); const rendered = format === 'json' ? `${JSON.stringify(digest, null, 2)}\n` : renderText(digest);
    if (output) writeFileSync(resolve(output), rendered, { flag: 'wx' }); else process.stdout.write(rendered);
    return digest.consistent ? 0 : 2;
  } catch (error) { process.stderr.write(`jet-state: ${errorText(error)}\n`); return 1; }
}
if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) process.exitCode = main();
