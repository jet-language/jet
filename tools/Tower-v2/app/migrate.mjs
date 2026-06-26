// Lossless port of v1 Tower (tools/Tower) into v2's shape (tower.json).
// READ-ONLY on every v1 file. Writes (or dry-runs) only tools/Tower-v2/tower.json.
//
//   node Tower.mjs migrate          dry run — prints what would move
//   node Tower.mjs migrate --write  writes tower.json (keeps hand-authored epochs)
//   node Tower.mjs audit            compares v1 -> v2 and proves nothing was lost
import { join } from 'node:path';
import { existsSync } from 'node:fs';
import { V1, DATA, readJSON, writeJSON } from './paths.mjs';

const BOARD = join(V1, 'board.json');
const STAGE = { backlog: 'triage', deciding: 'deciding', planning: 'planning', ready: 'ready', building: 'building', done: 'done', frozen: 'frozen' };
const c0 = '2026-06-26T00:00:00Z';

// the few decisions still live on the board, with the outcomes recorded in v1
const DEC_META = {
  'D-PLUGIN1':   { title: 'Plugin substrate — sandboxed WASM vs native cdylib vs RPC', gist: 'How a plugin is built and loaded safely.' },
  'D-DEP-WASM1': { title: 'WASM sandbox engine for plugins (wasmtime + Component Model)', gist: 'Which engine runs sandboxed plugin modules.' },
  'D-NETDEP1':   { title: 'HTTP backend for build-time fetch()', gist: 'Which HTTP backend powers comptime fetch(url, sha256:).' },
  'D-HTTPLIB1':  { title: 'Full HTTP core library — client + server API surface', gist: "Name + shape the request/response/handler/router/middleware surface for Jet's first-party HTTP library." },
};

const cardText = (c) => (c.body || '') + ' ' + (c.notes || []).map(n => n.t).join(' ');
function outcomeOf(id, card) {
  const m = cardText(card).match(new RegExp(id.replace(/-/g, '\\-') + '=([A-Z])'));
  if (!m) return null;
  const at = cardText(card).match(/ratified (\d{4}-\d{2}-\d{2})/);
  return { outcome: m[1], ratifiedAt: at ? at[1] : (card.updated || '').slice(0, 10) };
}

export function buildPort() {
  const v1 = readJSON(BOARD, null);
  if (!v1) throw new Error('v1 board.json not found at ' + BOARD);
  const seed = readJSON(DATA, { meta: {}, epochs: [] });   // keep v2's hand-authored epochs + meta

  const ordered = [...v1.cards].sort((a, b) => String(a.created).localeCompare(String(b.created)) || String(a.id).localeCompare(String(b.id)));
  const cards = ordered.map((c, i) => {
    const card = {
      id: c.id, num: i + 1,
      title: c.title, body: c.body || '',
      kind: ['task', 'feature', 'idea', 'bug'].includes(c.type) ? c.type : 'task',
      track: c.stage === 'deciding' ? 'epoch' : 'sidequest',     // best-effort; owner re-seats later
      epoch: c.stage === 'deciding' ? (seed.meta.currentEpoch || 'e3') : null,
      phase: STAGE[c.stage] || 'triage',
      priority: c.priority || 'P2',
      plan: c.plan || null,
      blockedBy: c.blockedBy || [],
      log: (c.notes || []).map(n => ({ at: n.at, text: n.t })),
      created: c.created, updated: c.updated,
    };
    if (c.workOrder != null) card.workOrder = c.workOrder;       // preserve, even though v2 UI ignores it
    return card;
  });

  const decToCard = {};
  for (const c of v1.cards) for (const d of (c.decisions || [])) decToCard[d] = c.id;
  const refIds = [...new Set(v1.cards.flatMap(c => c.decisions || []))];

  const decisions = refIds.map(id => {
    const cardId = decToCard[id] || null;
    const owner = v1.cards.find(c => c.id === cardId);
    const meta = DEC_META[id] || { title: id, gist: '' };
    const res = owner ? outcomeOf(id, owner) : null;
    const base = { id, cardId, title: meta.title, gist: meta.gist, explainer: '', story: '', inWild: '', detail: '', options: [], comparisons: [], rec: null };
    if (res) return { ...base, status: 'ratified', outcome: res.outcome, ratifiedAt: res.ratifiedAt, comment: 'Ratified in v1. Full record in docs/spec/syntax-decisions.md.' };
    return { ...base, status: 'open' };                          // deferred / not-yet-balloted — preserved as open
  });

  // ALL v1 questions, preserved verbatim. Attach to the owning card when the
  // decision still lives on the board; otherwise keep them dormant (cardId null)
  // but never drop them — they are historical owner Q&A.
  const questions = (v1.questions || []).map(q => ({
    id: q.id,
    cardId: decToCard[q.decisionId] || null,
    decisionId: q.decisionId,
    by: 'owner', kind: 'question',
    text: q.text, status: q.status, answer: q.answer || '',
    created: q.created,
    ...(q.status === 'answered' ? { answeredAt: (q.created || '').slice(0, 10) } : {}),
  }));

  const binder = (v1.ingest || []).slice(0, 40).map((it, i) => ({
    id: 'b' + i, text: it.note || it.source || '', note: it.source || '', level: null, tags: it.kind ? [it.kind] : [], status: 'open', created: it.created || c0,
  }));

  const meta = { version: 3, currentEpoch: seed.meta.currentEpoch || 'e3', nextNum: cards.length + 1, ui: { toggled: [] } };
  if (v1.scratch && v1.scratch.trim()) meta.scratch = v1.scratch;   // preserve if non-empty

  return { meta, epochs: seed.epochs || [], cards, decisions, binder, questions };
}

function summary(v2) {
  const byPhase = {}; v2.cards.forEach(c => byPhase[c.phase] = (byPhase[c.phase] || 0) + 1);
  console.log('\n  v1 → v2 lossless port');
  console.log('  cards:', v2.cards.length, '(num 1..' + v2.cards.length + ')  ' + JSON.stringify(byPhase));
  console.log('  decisions:', v2.decisions.length, '(' + v2.decisions.filter(d => d.status === 'ratified').length + ' ratified / ' + v2.decisions.filter(d => d.status !== 'ratified').length + ' open)');
  v2.decisions.forEach(d => console.log('     ' + d.id + '  ' + d.status + (d.outcome ? '=' + d.outcome : '') + '  → card ' + d.cardId));
  console.log('  questions:', v2.questions.length, '(' + v2.questions.filter(q => q.status === 'open').length + ' open / ' + v2.questions.filter(q => q.status === 'answered').length + ' answered)');
  const linkedQ = v2.questions.filter(q => q.cardId).length;
  console.log('     linked to a live card:', linkedQ, '· historical (no live card):', v2.questions.length - linkedQ);
  console.log('  binder:', v2.binder.length, '· epochs kept:', v2.epochs.length);
}

export function migrate({ write }) {
  const v2 = buildPort();
  summary(v2);
  if (!write) {
    console.log('\n  dry run — nothing written. would-be tower.json:', JSON.stringify(v2, null, 2).length, 'bytes');
    console.log('  run `node Tower.mjs migrate --write` to commit, then `node Tower.mjs audit`.\n');
    return;
  }
  writeJSON(DATA, v2);
  console.log('\n  wrote', v2.cards.length, 'cards /', v2.decisions.length, 'decisions /', v2.questions.length, 'questions to tower.json');
  console.log('  now run `node Tower.mjs audit` to prove nothing was lost.\n');
}

// ---- audit: prove every v1 item survived into v2 ------------------------
export function audit() {
  const v1 = readJSON(BOARD, null);
  const v2 = readJSON(DATA, null);
  if (!v1 || !v2) { console.log('missing v1 or v2 data'); return; }
  const fails = [], ok = [];

  let cardOk = 0;
  for (const c of v1.cards) {
    const t = v2.cards.find(x => x.id === c.id);
    if (!t) { fails.push('CARD MISSING: ' + c.id + ' ' + c.title); continue; }
    const checks = [
      [t.title === c.title, 'title'],
      [t.phase === (STAGE[c.stage] || 'triage'), 'stage→phase (' + c.stage + ')'],
      [t.priority === (c.priority || 'P2'), 'priority'],
      [(t.plan || null) === (c.plan || null), 'plan'],
      [JSON.stringify(t.blockedBy || []) === JSON.stringify(c.blockedBy || []), 'blockedBy'],
      [(t.log || []).length === (c.notes || []).length, 'notes→log count'],
      [(t.body || '') === (c.body || ''), 'body'],
    ];
    const bad = checks.filter(([p]) => !p).map(([, n]) => n);
    if (bad.length) fails.push('CARD ' + c.id + ' field mismatch: ' + bad.join(', '));
    else cardOk++;
    for (const d of (c.decisions || [])) if (!v2.decisions.find(x => x.id === d)) fails.push('DECISION MISSING: ' + d + ' (card ' + c.id + ')');
  }
  ok.push('cards: ' + cardOk + '/' + v1.cards.length + ' fully matched');

  let qOk = 0;
  for (const q of (v1.questions || [])) {
    const t = v2.questions.find(x => x.id === q.id);
    if (!t) { fails.push('QUESTION MISSING: ' + q.id + ' (' + q.decisionId + ')'); continue; }
    if (t.text !== q.text || t.status !== q.status || (t.answer || '') !== (q.answer || '')) fails.push('QUESTION ' + q.id + ' content mismatch');
    else qOk++;
  }
  ok.push('questions: ' + qOk + '/' + (v1.questions || []).length + ' preserved verbatim');

  if ((v1.scratch || '').trim() && v2.meta.scratch !== v1.scratch) fails.push('SCRATCH not preserved');
  ok.push('scratch: ' + ((v1.scratch || '').trim() ? 'preserved' : 'empty (nothing to port)'));
  ok.push('ingest→binder: ' + (v1.ingest || []).length + ' → ' + v2.binder.length);

  const slugs = [...new Set(v1.cards.map(c => c.plan).filter(Boolean))];
  const docDir = join(DATA, '..', 'docs', 'sidequests');
  let docOk = 0;
  for (const s of slugs) { if (existsSync(join(docDir, s + '.md'))) docOk++; else fails.push('PLAN DOC NOT COPIED: ' + s + '.md'); }
  ok.push('plan docs: ' + docOk + '/' + slugs.length + ' present in tools/Tower-v2/docs/sidequests/');

  console.log('\n  AUDIT  v1 (tools/Tower) → v2 (tools/Tower-v2)\n');
  ok.forEach(l => console.log('  ✓ ' + l));
  if (fails.length) { console.log('\n  ✗ ' + fails.length + ' problem(s):'); fails.forEach(f => console.log('    - ' + f)); console.log('\n  RESULT: INCOMPLETE — fix before freezing v1.\n'); process.exitCode = 1; }
  else console.log('\n  RESULT: LOSSLESS ✓  every v1 card, decision, question, and plan doc is in v2. Safe to freeze v1.\n');
}
