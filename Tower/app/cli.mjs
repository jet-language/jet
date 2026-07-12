// Tower CLI — the full agent + owner surface. Every operation the UI can do
// is available here, so nobody ever hand-edits tower.json.
//
//   tower <noun> <verb> [args] [--flags]     e.g. tower card update 12 --phase building
//   --json on any command → machine-readable output
//   complex payloads (decisions) → --file payload.json or `-` for stdin
import { readFileSync, mkdirSync, existsSync, writeFileSync, readdirSync, chmodSync, statSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import * as db from './store.mjs';
import { openStore, TowerError, PHASE_IDS } from './store.mjs';
import { findDataDir, readJSON, writeJSON, historyFile } from './paths.mjs';
import { ConfigError, DEFAULTS } from './config.mjs';
import { migrate } from './migrate.mjs';
import { lint } from './lint.mjs';

// ---- arg parsing (zero-dep) ------------------------------------------------

function parseArgs(argv) {
  const pos = []; const flags = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith('--')) {
      const key = a.slice(2).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
      const next = argv[i + 1];
      if (next === undefined || next.startsWith('--')) flags[key] = true;
      else { flags[key] = next; i++; }
    } else pos.push(a);
  }
  return { pos, flags };
}

const readPayload = (flags) => {
  if (flags.file === '-' || flags.stdin) return JSON.parse(readFileSync(0, 'utf8'));
  if (flags.file) return JSON.parse(readFileSync(flags.file, 'utf8'));
  return null;
};

const out = (flags, human, data) => {
  if (flags.json) console.log(JSON.stringify(data ?? human, null, 2));
  else if (typeof human === 'string') console.log(human);
  else console.log(JSON.stringify(human, null, 2));
};

const cardLine = (c) => `#${String(c.num).padEnd(4)} ${(c.priority || '').padEnd(3)} ${c.lane.lane.padEnd(9)} ${c.title.slice(0, 60)}${c.assignee ? `  [${c.assignee}]` : ''}`;

// ---- commands ----------------------------------------------------------------

const DATA_IGNORES = ['backups/', '*.lock/', 'files/', 'server.log', 'secrets.json', '.secrets.json.tmp-*'];
function ensureDataIgnores(dir) {
  const file = join(dir, '.gitignore');
  const current = existsSync(file) ? readFileSync(file, 'utf8') : '';
  const lines = new Set(current.split(/\r?\n/));
  const missing = DATA_IGNORES.filter(entry => !lines.has(entry));
  if (!missing.length) return;
  const prefix = current && !current.endsWith('\n') ? '\n' : '';
  writeFileSync(file, current + prefix + missing.join('\n') + '\n');
}

function cmdInit({ flags }) {
  const dir = resolve(flags.dir || '.', '.tower');
  const file = join(dir, 'tower.json');
  if (existsSync(file)) { console.error(`tower: already initialized at ${file}`); process.exitCode = 1; return; }
  mkdirSync(dir, { recursive: true });
  const name = flags.name || 'Project';
  writeJSON(file, db.empty(name));
  const cfg = { project: name };
  if (!existsSync(join(dir, 'config.json'))) writeJSON(join(dir, 'config.json'), cfg);
  ensureDataIgnores(dir);
  console.log(`initialized Tower for "${name}" at ${dir}`);
  console.log('next: tower epoch add e1 --name "First epoch" && tower serve --open');
}

function cmdStatus(store, { flags }) {
  const s = store.project();
  if (flags.json) return out(flags, null, { meta: s.meta, counts: s.counts });
  const bar = (n) => '█'.repeat(Math.min(12, n)) + '░'.repeat(Math.max(0, 12 - n));
  console.log(`\n  TOWER · ${s.meta.project} · ${store.config.terms.epoch.toLowerCase()} ${s.meta.currentEpoch || '—'} · rev ${s.meta.rev}\n`);
  for (const ph of db.PHASES) {
    const n = s.counts.byPhase[ph.id];
    if (n) console.log(`  ${ph.label.padEnd(9)} ${bar(n)} ${n}`);
  }
  console.log(`\n  BLOCKED ON OWNER  ${s.counts.decide} decisions`);
  console.log(`  AGENT-READY       ${s.counts.agentReady}  (plan / implement / build / verify)`);
  console.log(`  open questions    ${s.counts.openQuestions}   sidequests ${s.counts.sidequests}   ideas ${s.counts.ideas}\n`);
  const show = (label, lane) => {
    const cs = s.cards.filter(c => c.lane.lane === lane);
    if (!cs.length) return;
    console.log(`  ${label}:`);
    for (const c of cs.slice(0, 12)) console.log(`   · ${cardLine(c)}`);
  };
  show('OWNER — decide', 'decide');
  show('AGENT — plan', 'plan'); show('AGENT — implement', 'implement');
  show('AGENT — building', 'building'); show('AGENT — verify', 'verify');
  console.log('');
}

function cmdCard(store, { pos, flags }) {
  const [verb, ref] = pos;
  const by = flags.by;
  switch (verb) {
    case 'list': {
      const s = store.project();
      let cs = s.cards;
      if (flags.lane) cs = cs.filter(c => c.lane.lane === flags.lane);
      if (flags.epoch) cs = cs.filter(c => c.epoch === flags.epoch);
      if (flags.track) cs = cs.filter(c => c.track === flags.track);
      if (flags.phase) cs = cs.filter(c => c.phase === flags.phase);
      if (flags.milestone) cs = cs.filter(c => c.milestoneId === flags.milestone);
      if (flags.json) return out(flags, null, cs);
      for (const c of cs) console.log(cardLine(c));
      if (!cs.length) console.log('(no cards match)');
      return;
    }
    case 'show': {
      const s = store.project();
      const c = db.findCard({ cards: s.cards }, ref);
      if (c) return out(flags, null, s.cards.find(x => x.id === c.id));
      // #461: fall through to history once it's not live any more.
      const arch = db.findInHistory(store.loadHistory(), ref);
      if (arch) return out(flags, null, { ...arch, archived: true });
      throw new TowerError('E_NOT_FOUND', `no card ${ref}`);
    }
    case 'add': {
      const p = readPayload(flags) || {};
      const { result } = store.mutate((s, cfg) => db.addCard(s, {
        title: flags.title ?? p.title, body: flags.body ?? p.body, kind: flags.kind ?? p.kind,
        track: flags.track ?? p.track, epoch: flags.epoch ?? p.epoch, milestoneId: flags.milestone ?? p.milestoneId,
        phase: flags.phase ?? p.phase, priority: flags.priority ?? p.priority, plan: flags.plan ?? p.plan,
        blockedBy: flags.blockedBy ? String(flags.blockedBy).split(',') : p.blockedBy,
        refs: flags.refs ? String(flags.refs).split(',').map(x => x.trim()).filter(Boolean) : p.refs,
        workOrder: flags.workOrder ?? p.workOrder, by,
      }, cfg));
      return out(flags, `added card #${result.num} (${result.id})`, result);
    }
    case 'update': {
      const p = readPayload(flags) || {};
      const patch = { ...p, by };
      for (const [f, k] of [['title', 'title'], ['body', 'body'], ['kind', 'kind'], ['track', 'track'], ['epoch', 'epoch'],
        ['milestone', 'milestoneId'], ['phase', 'phase'], ['priority', 'priority'], ['plan', 'plan'],
        ['workOrder', 'workOrder'], ['assignee', 'assignee'], ['log', 'logEntry'], ['needsAcceptance', 'needsAcceptance']])
        if (flags[f] !== undefined) patch[k] = flags[f];
      if (flags.blockedBy !== undefined) patch.blockedBy = flags.blockedBy === '' ? [] : String(flags.blockedBy).split(',');
      if (flags.refs !== undefined) patch.refs = flags.refs === '' ? [] : String(flags.refs).split(',').map(x => x.trim()).filter(Boolean);
      const current = db.findCard(store.load(), ref);
      const openAcceptance = current && store.load().decisions.find(d => d.cardId === current.id && d.group === 'acceptance' && d.status !== 'ratified');
      const clearsAcceptance = 'needsAcceptance' in patch && !(patch.needsAcceptance === true || patch.needsAcceptance === 'true');
      if (current && ((current.needsAcceptance && patch.phase === 'done' && by === 'owner') || (openAcceptance && clearsAcceptance))) {
        const id = openAcceptance?.id || `D-ACCEPT-${current.num}`;
        store.mutate((s) => db.auditAcceptanceRejection(s, id, 'cli card update', 'owner-verification bypass rejected', by));
        throw new TowerError('E_ACCEPTANCE_OWNER_UI', `card #${current.num} requires the dedicated owner verification UI`);
      }
      const { result, state } = store.mutate((s, cfg) => db.updateCard(s, ref, patch, cfg), { expectRev: flags.expectRev });
      return out(flags, `updated card #${result.num} → ${db.laneOf(result, state.decisions, state.cards).lane}`, result);
    }
    case 'criteria': {
      if (flags.add !== undefined) {
        const { result } = store.mutate((s) => db.addCriterion(s, ref, flags.add, by));
        return out(flags, `added criterion #${result.n} to card #${result.cardNum}`, result);
      }
      if (flags.meet !== undefined) {
        const { result } = store.mutate((s) => db.meetCriterion(s, ref, flags.meet, { evidence: flags.evidence, by }));
        return out(flags, `criterion #${result.n} met on card #${result.cardNum}`, result);
      }
      if (flags.verify !== undefined) {
        const { result } = store.mutate((s) => db.verifyCriterion(s, ref, flags.verify, { evidence: flags.evidence, by }));
        return out(flags, `criterion #${result.n} verified on card #${result.cardNum}`, result);
      }
      const s = store.load();
      const found = db.findCard(s, ref) || (() => { throw new TowerError('E_NOT_FOUND', `no card ${ref}`); })();
      if (flags.json) return out(flags, null, found.criteria || []);
      for (const it of found.criteria || []) console.log(`#${it.n} [${it.status}] ${it.text}${it.metBy ? `  met:${it.metBy}` : ''}${it.verifiedBy ? `  verified:${it.verifiedBy}` : ''}${it.evidence ? `  — ${it.evidence}` : ''}`);
      if (!(found.criteria || []).length) console.log('(no criteria)');
      return;
    }
    case 'claim': {
      const { result } = store.mutate((s) => db.claimCard(s, ref, by));
      return out(flags, `card #${result.num} claimed by ${by}`, result);
    }
    case 'release': {
      const { result } = store.mutate((s) => db.releaseCard(s, ref, by, flags.handoff));
      return out(flags, `card #${result.num} released`, result);
    }
    case 'delete': {
      const { result } = store.mutate((s) => db.deleteCard(s, ref, { by }));
      return out(flags, `deleted card ${result.id}`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown card verb "${verb}" — list/show/add/update/claim/release/delete/criteria`);
  }
}

function cmdDecision(store, { pos, flags }) {
  const [verb, id] = pos;
  const by = flags.by;
  switch (verb) {
    case 'list': {
      const s = store.load();
      let ds = s.decisions;
      if (flags.open) ds = ds.filter(d => d.status !== 'ratified');
      if (flags.card) { const c = db.findCard(s, flags.card); ds = ds.filter(d => c && d.cardId === c.id); }
      if (flags.json) return out(flags, null, ds);
      for (const d of ds) console.log(`${String(d.id || '(no id)').padEnd(16)} ${(d.status || 'open').padEnd(9)} ${String(d.title || '(untitled)').slice(0, 60)}${d.outcome ? ` → ${d.outcome}` : ''}`);
      if (!ds.length) console.log('(no decisions match)');
      return;
    }
    case 'show': {
      const s = store.load();
      const d = s.decisions.find(x => x.id === id);
      if (d) return out(flags, null, d);
      // #461: fall through to history once it's not live any more.
      const arch = store.loadHistory().decisions.find(x => x.id === id);
      if (arch) return out(flags, null, { ...arch, archived: true });
      throw new TowerError('E_NOT_FOUND', `no decision ${id}`);
    }
    case 'add': {
      const p = readPayload(flags) || {};
      const payload = { ...p, by };
      for (const f of ['id', 'cardId', 'title', 'gist', 'lesson', 'story', 'explainer', 'inWild', 'detail', 'rec', 'group'])
        if (flags[f] !== undefined) payload[f] = flags[f];
      if (flags.card !== undefined) payload.cardId = flags.card;
      if (flags.draft !== undefined) payload.draft = flags.draft === true || flags.draft === 'true';
      const { result } = store.mutate((s) => db.addDecision(s, payload));
      return out(flags, `added decision ${result.id} on card ${result.cardId}${result.draft ? ' (draft)' : ''}`, result);
    }
    case 'update': {
      const p = readPayload(flags) || {};
      for (const f of ['title', 'gist', 'lesson', 'story', 'explainer', 'inWild', 'detail', 'rec', 'group'])
        if (flags[f] !== undefined) p[f] = flags[f];
      if (flags.ready !== undefined) p.ready = flags.ready === true || flags.ready === 'true';
      const { result } = store.mutate((s) => db.updateDecision(s, id, p, by));
      return out(flags, `updated decision ${result.id}${p.ready ? ' — ballot-ready' : ''}`, result);
    }
    case 'ratify': {
      const decision = store.load().decisions.find(d => d.id === id);
      if (decision && (decision.group === 'acceptance' || decision.id.startsWith('D-ACCEPT-'))) {
        store.mutate((s) => db.auditAcceptanceRejection(s, id, 'cli decision ratify',
          'CLI cannot resolve owner verification', by));
        throw new TowerError('E_ACCEPTANCE_OWNER_UI', `${id} requires the dedicated owner verification UI; --by owner and --quote are not accepted`);
      }
      const { result } = store.mutate((s) => db.ratify(s, id, flags.outcome, flags.comment, by, flags.quote), { expectRev: flags.expectRev });
      return out(flags, `ratified ${result.id} → ${result.outcome}`, result);
    }
    case 'reopen': {
      const { result } = store.mutate((s) => db.reopenDecision(s, id, by));
      return out(flags, `reopened ${result.id}`, result);
    }
    case 'delete': {
      const { result } = store.mutate((s) => db.deleteDecision(s, id, by));
      return out(flags, `deleted decision ${id}`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown decision verb "${verb}" — list/show/add/update/ratify/reopen/delete`);
  }
}

function cmdQuestion(store, { pos, flags }) {
  const [verb, id] = pos;
  switch (verb) {
    case 'list': {
      const s = store.load();
      let qs = s.questions;
      if (flags.open) qs = qs.filter(q => q.status === 'open');
      if (flags.card) { const c = db.findCard(s, flags.card); qs = qs.filter(q => c && q.cardId === c.id); }
      if (flags.json) return out(flags, null, qs);
      for (const q of qs) console.log(`${q.id.padEnd(14)} ${(q.status || '').padEnd(9)} [${q.by}] ${q.text.slice(0, 70)}`);
      if (!qs.length) console.log('(no questions match)');
      return;
    }
    case 'ask': {
      const { result } = store.mutate((s) => db.addQuestion(s, { cardId: id, text: flags.text, by: flags.by || 'owner', kind: flags.kind, decisionId: flags.decision }));
      return out(flags, `asked ${result.id} on card ${result.cardId}`, result);
    }
    case 'answer': {
      const { result } = store.mutate((s) => db.answerQuestion(s, id, flags.text, flags.by));
      return out(flags, `answered ${result.id}`, result);
    }
    case 'delete': {
      const { result } = store.mutate((s) => db.deleteQuestion(s, id, flags.by));
      return out(flags, `deleted question ${id}`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown question verb "${verb}" — list/ask/answer/delete`);
  }
}

function cmdIdea(store, { pos, flags }) {
  const [verb, id] = pos;
  switch (verb) {
    case 'list': {
      const s = store.load();
      const bs = flags.all ? s.ideas : s.ideas.filter(b => b.status !== 'tagged');
      if (flags.json) return out(flags, null, bs);
      for (const b of bs) console.log(`${b.id.padEnd(14)} ${b.text.slice(0, 70)}`);
      if (!bs.length) console.log('(no ideas)');
      return;
    }
    case 'add': {
      const { result } = store.mutate((s) => db.addIdea(s, { text: flags.text ?? pos.slice(1).join(' '), note: flags.note, tags: flags.tags ? String(flags.tags).split(',') : [], by: flags.by }));
      return out(flags, `captured idea ${result.id}`, result);
    }
    case 'promote': {
      const { result } = store.mutate((s, cfg) => db.promoteIdea(s, id, { title: flags.title, body: flags.body, kind: flags.kind, track: flags.track, priority: flags.priority, by: flags.by }, cfg));
      return out(flags, `promoted → card #${result.num} (${result.id})`, result);
    }
    case 'delete': {
      const { result } = store.mutate((s) => db.deleteIdea(s, id, flags.by));
      return out(flags, `deleted idea ${id}`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown idea verb "${verb}" — list/add/promote/delete`);
  }
}

function cmdEpoch(store, { pos, flags }) {
  const [verb, id] = pos;
  switch (verb) {
    case 'list': {
      const s = store.load();
      if (flags.json) return out(flags, null, s.epochs);
      for (const e of s.epochs) console.log(`${e.id.padEnd(6)} ${(e.status || 'open').padEnd(9)} ${e.name}${e.id === s.meta.currentEpoch ? '  ← current' : ''}`);
      if (!s.epochs.length) console.log('(no epochs — tower epoch add e1 --name "...")');
      return;
    }
    case 'add': {
      const { result } = store.mutate((s) => db.addEpoch(s, { id, name: flags.name, goal: flags.goal, status: flags.status, by: flags.by }));
      return out(flags, `added epoch ${result.id} — ${result.name}`, result);
    }
    case 'update': {
      const patch = {};
      for (const f of ['name', 'goal', 'status']) if (flags[f] !== undefined) patch[f] = flags[f];
      const { result } = store.mutate((s) => db.updateEpoch(s, id, patch));
      return out(flags, `updated epoch ${result.id}`, result);
    }
    case 'current': {
      const { result } = store.mutate((s) => db.setCurrentEpoch(s, id === 'none' ? null : id));
      return out(flags, `current ${store.config.terms.epoch.toLowerCase()}: ${result.currentEpoch || '—'}`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown epoch verb "${verb}" — list/add/update/current`);
  }
}

function cmdMilestone(store, { pos, flags }) {
  const [verb, id] = pos;
  switch (verb) {
    case 'list': {
      const s = store.project();
      let ms = s.milestones;
      if (flags.epoch) ms = ms.filter(m => m.epochId === flags.epoch);
      if (flags.json) return out(flags, null, ms);
      for (const m of ms) console.log(`${m.id.padEnd(12)} ${m.epochId.padEnd(6)} ${(m.status || 'open').padEnd(6)} ${m.progress.done}/${m.progress.total}  ${m.title.slice(0, 56)}`);
      if (!ms.length) console.log('(no milestones)');
      return;
    }
    case 'add': {
      const { result } = store.mutate((s) => db.addMilestone(s, { id: flags.id, epochId: flags.epoch, title: flags.title, goal: flags.goal, criteria: flags.criteria, by: flags.by }));
      return out(flags, `added milestone ${result.id} in ${result.epochId} — ${result.title}`, result);
    }
    case 'update': {
      const patch = {};
      for (const [f, k] of [['title', 'title'], ['goal', 'goal'], ['criteria', 'criteria'], ['status', 'status'], ['epoch', 'epochId']])
        if (flags[f] !== undefined) patch[k] = flags[f];
      const { result } = store.mutate((s) => db.updateMilestone(s, id, patch, flags.by));
      return out(flags, `updated milestone ${result.id}${result.status === 'met' ? ' — MET' : ''}`, result);
    }
    case 'delete': {
      const { result } = store.mutate((s) => db.deleteMilestone(s, id, flags.by));
      return out(flags, `deleted milestone ${id}`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown milestone verb "${verb}" — list/add/update/delete`);
  }
}

function cmdNext(store, { flags }) {
  const s = store.load();
  const scope = flags.burndown ? 'burndown' : undefined;
  const picks = db.nextCards(s, { epoch: flags.epoch, track: flags.track, agent: flags.agent, limit: Number(flags.limit || 5), scope });
  const proj = db.project(s);
  const rich = picks.map(p => proj.cards.find(c => c.id === p.id));
  if (flags.json) return out(flags, null, rich);
  if (!rich.length) return console.log('(nothing agent-workable — board is either empty, blocked on the owner, or done)');
  console.log(scope === 'burndown' ? 'next up — burndown scope (current epoch + sidequests):' : 'next up (workOrder → building > verify > implement > plan):');
  for (const c of rich) console.log(` · ${cardLine(c)}`);
}

// #457 — durability sweeper: rule-based lint over the live board, optionally
// extended with a docs/ballots/ scan (--docs). Read-only; exit 1 on any
// finding so it's CI/pre-flight friendly, 0 clean.
function cmdLint(store, { flags }) {
  const s = store.load();
  const history = store.loadHistory();
  const docsRoot = flags.docsRoot || join(dirname(store.dataDir), 'docs');
  const findings = lint(s, history, { docs: !!flags.docs, docsRoot });
  if (flags.json) {
    console.log(JSON.stringify(findings, null, 2));
    process.exitCode = findings.length ? 1 : 0;
    return;
  }
  for (const f of findings) console.log(`${f.rule}  ${f.ref}  ${f.msg}`);
  if (!findings.length) console.log('(clean)');
  process.exitCode = findings.length ? 1 : 0;
}

// #462 — tower brief: one-shot agent work packet. No ref → pick the top
// card via the canonical nextCards() picker. --agent + not --no-claim →
// claim it (E_CLAIMED if someone else already holds it; a no-op if the same
// agent already does). No --agent → read-only, never claims.
function cmdBrief(store, { pos, flags }) {
  const [ref] = pos;
  let s = store.load();
  let card;
  if (ref) {
    card = db.findCard(s, ref);
    if (!card) throw new TowerError('E_NOT_FOUND', `no card ${ref}`);
  } else {
    const picks = db.nextCards(s, { agent: flags.agent, limit: 1 });
    if (!picks.length) throw new TowerError('E_NOT_FOUND', 'nothing agent-workable — board is either empty, blocked on the owner, or done');
    card = db.findCard(s, picks[0].id);
  }
  if (flags.agent && !flags.noClaim) {
    const { state } = store.mutate((s2) => db.claimCard(s2, card.id, flags.agent));
    s = state;
    card = db.findCard(s, card.id);
  }
  const packet = db.buildBrief(s, card.id);
  if (flags.json) return out(flags, null, packet);
  console.log(renderBrief(packet));
}

// Compact, one-screen-target human render — sections omitted when empty.
function renderBrief(p) {
  const c = p.card;
  const L = [];
  L.push(`#${c.num} ${c.title}`);
  L.push(`  ${c.phase}${c.priority ? ` · ${c.priority}` : ''}${c.track ? ` · ${c.track}` : ''}${c.workOrder != null ? ` · workOrder ${c.workOrder}` : ''}${c.assignee ? ` · claimed by ${c.assignee}` : ''}`);
  if (c.epoch) L.push(`  epoch ${c.epoch.id}${c.epoch.name ? ` — ${c.epoch.name}` : ''}${c.epoch.goal ? `: ${c.epoch.goal}` : ''}`);
  if (c.milestone) L.push(`  milestone ${c.milestone.title}${c.milestone.goal ? ` — ${c.milestone.goal}` : ''}${c.milestone.criteria ? `  criteria: ${c.milestone.criteria}` : ''}`);
  if (c.body) { L.push('', 'BODY', c.body); }
  if (c.plan) { L.push('', 'PLAN', c.plan); }
  if (p.blockers.length) {
    L.push('', 'BLOCKED BY');
    for (const b of p.blockers) L.push(`  ${b.done ? '✓' : '✗'} ${b.id} (${b.kind}) ${b.title || ''}${b.kind === 'card' ? ` [${b.phase}]` : b.kind === 'decision' ? ` [${b.status}]` : ''}`);
  }
  const items = p.criteria.items;
  if (items.length) {
    L.push('', `CRITERIA${p.criteria.needsAcceptance ? '  (needsAcceptance — owner ballot on close)' : ''}`);
    for (const it of items) L.push(`  #${it.n} [${it.status}] ${it.text}${it.metBy ? `  met:${it.metBy}` : ''}${it.verifiedBy ? `  verified:${it.verifiedBy}` : ''}${it.evidence ? `  — ${it.evidence}` : ''}`);
  } else if (p.criteria.needsAcceptance) {
    L.push('', 'CRITERIA  (none — needsAcceptance: owner ballot on close)');
  }
  if (p.decisions.length) {
    L.push('', 'DECISIONS');
    for (const d of p.decisions) {
      L.push(`  ${d.id} [${d.status}${d.draft ? ' draft' : ''}] ${d.title}`);
      if (d.gist) L.push(`    ${d.gist}`);
      if (d.status === 'ratified') {
        L.push(`    → ${d.outcome}${d.comment ? `  — ${d.comment}` : ''}`);
      } else {
        if (d.lesson) L.push(`    learn first: ${d.lesson}`);
        if (d.story) L.push(`    story: ${d.story}`);
        if (d.inWild) L.push(`    in the wild: ${d.inWild}`);
        if (d.rec) L.push(`    rec: ${d.rec}`);
        if (d.recommendation?.why) L.push(`    why: ${d.recommendation.why}`);
        for (const rejected of d.recommendation?.whyNot || []) L.push(`    why not ${rejected.key}: ${rejected.reason}`);
        if (d.recommendation?.tradeoff) L.push(`    accepted tradeoff: ${d.recommendation.tradeoff}`);
        if (d.hybrid?.synthesis) L.push(`    hybrid result ${d.hybrid.result}: ${d.hybrid.synthesis}`);
        for (const item of d.hybrid?.harvest || []) L.push(`    harvest ${item.key}: ${item.aspect} — ${item.use}`);
        for (const o of d.options || []) L.push(`    [${o.key}] ${o.name}${o.detail ? ` — ${o.detail}` : ''}${o.technical ? `\n      technical: ${String(o.technical).split('\n').join('\n      ')}` : ''}${o.code ? `\n      ${String(o.code).split('\n').join('\n      ')}` : ''}`);
      }
    }
  }
  if (p.questions.length) {
    L.push('', 'OPEN QUESTIONS');
    for (const q of p.questions) L.push(`  ${q.id} [${q.by}] ${q.text}`);
  }
  if (p.refs.length) L.push('', 'REFS', `  ${p.refs.join(', ')}`);
  if (p.log.length) {
    L.push('', 'RECENT LOG');
    for (const l of p.log) L.push(`  ${l.at}  ${l.by ? `[${l.by}] ` : ''}${l.text}`);
  }
  L.push('', 'RULES');
  for (const r of p.rules) L.push(`  · ${r}`);
  return L.join('\n');
}

// tower verdict '#N' --outcome "..." [--title "…"] --by owner — mints an
// ALREADY-ratified decision recording an owner verdict, so it can never be
// mis-filed as a mere log note (D-TWRGUARD1=C #458). Owner-only, no --quote
// escape: this command IS the owner speaking.
function cmdVerdict(store, { pos, flags }) {
  const [ref] = pos;
  const { result } = store.mutate((s) => db.mintVerdict(s, ref, flags.outcome, flags.title, flags.by));
  return out(flags, `verdict ${result.id} recorded on card #${result.cardNum} → ${result.outcome}`, result);
}

// #461 — tower archive status|show|restore. Cards/decisions retire to
// history.json on their own (store.mutate's chokepoint); this surface reads
// it back and lets the owner walk one back.
function cmdArchive(store, { pos, flags }) {
  const [verb, ref] = pos;
  switch (verb) {
    case 'status': {
      const h = store.loadHistory();
      const hf = historyFile(store.dataDir);
      const historyBytes = existsSync(hf) ? statSync(hf).size : 0;
      const liveBytes = existsSync(store.file) ? statSync(store.file).size : 0;
      const info = { cards: h.cards.length, decisions: h.decisions.length, events: h.events.length, historyBytes, liveBytes };
      return out(flags, `history: ${info.cards} cards, ${info.decisions} decisions, ${info.events} events — ${(historyBytes / 1024).toFixed(1)} KB (live tower.json ${(liveBytes / 1024).toFixed(1)} KB)`, info);
    }
    case 'show': {
      const h = store.loadHistory();
      const c = db.findInHistory(h, ref);
      if (c) return out(flags, null, { ...c, archived: true });
      const d = h.decisions.find(x => x.id === ref);
      if (d) return out(flags, null, { ...d, archived: true });
      throw new TowerError('E_NOT_FOUND', `no archived card or decision ${ref}`);
    }
    case 'restore': {
      const { result } = store.restoreArchived(ref, flags.by);
      return out(flags, `restored ${result.kind} ${result.id}${result.num ? ` (#${result.num})` : ''} from archive`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown archive verb "${verb}" — status/show/restore`);
  }
}

function cmdEvents(store, { flags }) {
  const s = store.load();
  const es = s.events.slice(0, Number(flags.limit || 30));
  if (flags.json) return out(flags, null, es);
  for (const e of es) console.log(`${e.at}  ${String(e.by || '').padEnd(10)} ${e.action.padEnd(16)} ${e.ref || ''}  ${e.note || ''}`);
  if (!es.length) console.log('(no events yet)');
}

function cmdImport({ pos, flags }) {
  const [src] = pos;
  if (!src) throw new TowerError('E_USAGE', 'tower import <old-tower.json> [--name Project] [--dir .]');
  const old = readJSON(resolve(src));
  if (!old) throw new TowerError('E_NOT_FOUND', `cannot read ${src}`);
  const dir = resolve(flags.dir || '.', '.tower');
  const file = join(dir, 'tower.json');
  if (existsSync(file) && !flags.force) throw new TowerError('E_EXISTS', `${file} exists — pass --force to overwrite`);
  mkdirSync(dir, { recursive: true });
  const s = migrate(old, { project: flags.name || 'Project' });
  writeJSON(file, s);
  if (!existsSync(join(dir, 'config.json'))) writeJSON(join(dir, 'config.json'), { project: s.meta.project });
  ensureDataIgnores(dir);
  console.log(`imported ${s.cards.length} cards, ${s.decisions.length} decisions, ${s.questions.length} questions, ${s.ideas.length} ideas → ${file}`);
}

// ---- undo, git hook -------------------------------------------------------

function cmdUndo(store, { flags }) {
  const bdir = join(store.dataDir, 'backups');
  const files = existsSync(bdir) ? readdirSync(bdir).filter(f => f.startsWith('tower-')).sort() : [];
  if (!files.length) throw new TowerError('E_INVALID', 'nothing to undo (no backups yet)');
  const cur = store.load();
  const prev = readJSON(join(bdir, files.at(-1)));
  store.restore(prev, { expectRev: flags.expectRev ?? cur.meta.rev });
  return out(flags, `undid last write — board back to rev ${prev.meta?.rev ?? '?'} content (now rev ${cur.meta.rev + 1})`, { ok: true });
}

// Installs a post-commit hook that appends any commit mentioning #<num> to
// that card's log. Silent + always exit 0, so it can never break a commit.
function cmdGithookInstall(store) {
  const projectRoot = dirname(store.dataDir);
  const gitDir = (() => {
    let d = projectRoot;
    for (;;) { if (existsSync(join(d, '.git'))) return join(d, '.git'); const p = dirname(d); if (p === d) return null; d = p; }
  })();
  if (!gitDir) throw new TowerError('E_NOT_FOUND', 'no .git found at or above the project root');
  const hooksDir = join(gitDir, 'hooks');
  mkdirSync(hooksDir, { recursive: true });
  const hookPath = join(hooksDir, 'post-commit');
  const towerBin = join(dirname(new URL(import.meta.url).pathname), '..', 'tower.mjs');
  const line = `node "${towerBin}" githook post-commit >/dev/null 2>&1 || true   # tower card-link`;
  let cur = existsSync(hookPath) ? readFileSync(hookPath, 'utf8') : '#!/bin/sh\n';
  if (!cur.includes('tower card-link')) {
    cur = cur.trimEnd() + '\n' + line + '\n';
    writeFileSync(hookPath, cur);
    chmodSync(hookPath, 0o755);
  }
  console.log(`installed post-commit hook → ${hookPath}\ncommits mentioning #<num> now append to that card's log`);
}

// The real post-commit worker: read HEAD, find #N refs, log them.
async function githookPostCommit(store) {
  const { execSync } = await import('node:child_process');
  let subject = '', hash = '';
  try {
    hash = execSync('git rev-parse --short HEAD', { cwd: dirname(store.dataDir), encoding: 'utf8' }).trim();
    subject = execSync('git log -1 --format=%s', { cwd: dirname(store.dataDir), encoding: 'utf8' }).trim();
  } catch { return; }
  const nums = [...new Set([...subject.matchAll(/#(\d+)\b/g)].map(m => Number(m[1])))];
  if (!nums.length) return;
  const s = store.load();
  for (const n of nums) {
    const c = s.cards.find(x => x.num === n);
    if (!c) continue;
    try {
      store.mutate((s2, cfg) => db.updateCard(s2, c.id, { logEntry: `commit ${hash}: ${subject.slice(0, 110)}`, by: 'git' }, cfg));
    } catch { /* never break a commit */ }
  }
}

const HELP = `tower — file-backed project board for an owner + AI agents

  tower init [--name X] [--dir .]           set up .tower/ in a project
  tower serve [--port ${DEFAULTS.port}] [--open]          board UI + HTTP API
  tower status [--json]                     terminal snapshot
  tower state                               full projected state (JSON)
  tower next [--epoch E] [--track T] [--agent A] [--limit N] [--burndown]
                                            what an agent should pick up next;
                                            --burndown narrows to current
                                            epoch (meta.currentEpoch) + all
                                            sidequests, agent lanes only
  tower lint [--json] [--docs] [--docs-root DIR]
                                            durability sweeper over the live
                                            board (done-without-evidence,
                                            claimed-idle, missing-attribution,
                                            ballot-gaps, stale-draft, orphan-
                                            blockers); --docs also flags a
                                            ratified decision id still listed
                                            in docs/ballots/*.md; exit 1 on
                                            any finding, 0 clean
  tower brief [ref] [--agent me] [--json] [--no-claim]
                                            one-shot work packet: card, blockers,
                                            criteria, decisions VERBATIM, questions,
                                            refs, recent log, rules — zero other reads
                                            needed to start. No ref → picks the top
                                            card via next's picker. --agent claims it
                                            unless --no-claim; no --agent → read-only.

  tower card     list|show|add|update|claim|release|delete
  tower card update <ref> --needs-acceptance true|false   flag for owner accept ballot on close
  tower card update <ref> --refs "docs/a.md,examples/b.jet"   explicit doc-path pointers
  tower card criteria <ref> --add "text" --by X           add an exit criterion
                            --meet n --evidence "…" --by X    builder: mark met
                            --verify n --evidence "…" --by Y  verifier ≠ builder: mark verified
                            --list                            show the checklist
  tower card release <ref> --by X [--handoff "…"]         --handoff required if the card is building
  tower decision list|show|add|update|ratify|reopen|delete
  tower decision add --draft                              save a work-in-progress ballot, skip validation
  tower decision update <id> --ready                       validate + clear draft
  tower decision ratify <id> --outcome K [--quote "…"]     generic ballots only; acceptance requires owner UI
  tower verdict '#N' --outcome "..." [--title "…"] --by owner
                                            record an owner ruling as a ratified decision (not a log note)
  tower archive  status|show <id>|restore <id> --by owner
                                            done cards + ratified decisions retire here on their own
                                            after config.retireAfterDays (default 3); restore brings one back
  tower question list|ask|answer|delete
  tower idea     list|add|promote|delete
  tower epoch    list|add|update|current
  tower milestone list|add|update|delete
  tower events   [--limit 30]
  tower import <old-tower.json> [--name X] [--force]

  tower undo                                revert the last write (rev-guarded)
  tower githook [install]                   commits mentioning #N → card log

  Cards accept #num or id. --json everywhere for machine output.
  Complex payloads: --file payload.json or --file - (stdin).
  Writers should pass --by <agent-name>; owner ops use --by owner.
  Optimistic concurrency: --expect-rev N (exit 2 on conflict).
  Phases: ${PHASE_IDS.join(' ')}

  Guards (agent-hard, owner-soft — --by owner bypasses; see Tower/AGENTS.md):
    ballot validation (lesson included; E_BALLOT), owner-only ratify (E_OWNER_ONLY),
    frozen write guard (E_OWNER_LANE), ratified-decision delete guard
    (E_HAS_RATIFIED), building-release handoff (E_HANDOFF).
`;

// ---- dispatch ----------------------------------------------------------------

export async function run(argv) {
  const { pos, flags } = parseArgs(argv);
  const [cmd, ...rest] = pos;
  const sub = { pos: rest, flags };
  try {
    if (!cmd || cmd === 'help' || flags.help) return console.log(HELP);
    if (cmd === 'init') return cmdInit(sub);
    if (cmd === 'import') return cmdImport(sub);

    const dataDir = flags.data
      ? (String(flags.data).endsWith('.json') ? dirname(resolve(flags.data)) : resolve(flags.data))
      : findDataDir();
    const store = openStore(dataDir);

    switch (cmd) {
      case 'serve': {
        const { serve } = await import('./server.mjs');
        return serve(store, Number(flags.port || store.config.port), !!flags.open);
      }
      case 'status':    return cmdStatus(store, sub);
      case 'state':     return console.log(JSON.stringify(store.project(), null, 2));
      case 'card':      return cmdCard(store, sub);
      case 'decision':  return cmdDecision(store, sub);
      case 'question':  return cmdQuestion(store, sub);
      case 'idea':      return cmdIdea(store, sub);
      case 'epoch':     return cmdEpoch(store, sub);
      case 'milestone': return cmdMilestone(store, sub);
      case 'next':      return cmdNext(store, sub);
      case 'brief':     return cmdBrief(store, sub);
      case 'lint':      return cmdLint(store, sub);
      case 'verdict':   return cmdVerdict(store, sub);
      case 'archive':   return cmdArchive(store, sub);
      case 'events':    return cmdEvents(store, sub);
      case 'undo':      return cmdUndo(store, sub);
      case 'githook':
        if (sub.pos[0] === 'post-commit') return await githookPostCommit(store);
        return cmdGithookInstall(store);
      default: throw new TowerError('E_USAGE', `unknown command "${cmd}" — run \`tower help\``);
    }
  } catch (e) {
    if (e instanceof TowerError || e instanceof ConfigError) {
      console.error(`tower: ${e.message}`);
      process.exitCode = e.code === 'E_CONFLICT' ? 2 : 1;
      if (flags.json) console.log(JSON.stringify({ error: e.code, message: e.message }));
      return;
    }
    throw e;
  }
}
