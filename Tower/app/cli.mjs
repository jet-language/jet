// Tower CLI — the full agent + owner surface. Every operation the UI can do
// is available here, so nobody ever hand-edits tower.json.
//
//   tower <noun> <verb> [args] [--flags]     e.g. tower card update 12 --phase building
//   --json on any command → machine-readable output
//   complex payloads (decisions) → --file payload.json or `-` for stdin
import { readFileSync, mkdirSync, existsSync, writeFileSync, readdirSync, chmodSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import * as db from './store.mjs';
import { openStore, TowerError, PHASE_IDS } from './store.mjs';
import { findDataDir, readJSON, writeJSON } from './paths.mjs';
import { DEFAULTS } from './config.mjs';
import { migrate } from './migrate.mjs';

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

function cmdInit({ flags }) {
  const dir = resolve(flags.dir || '.', '.tower');
  const file = join(dir, 'tower.json');
  if (existsSync(file)) { console.error(`tower: already initialized at ${file}`); process.exitCode = 1; return; }
  mkdirSync(dir, { recursive: true });
  const name = flags.name || 'Project';
  writeJSON(file, db.empty(name));
  const cfg = { project: name };
  if (!existsSync(join(dir, 'config.json'))) writeJSON(join(dir, 'config.json'), cfg);
  const gi = join(dir, '.gitignore');
  if (!existsSync(gi)) writeFileSync(gi, 'backups/\n*.lock/\nfiles/\nserver.log\n');
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
  console.log(`\n  BLOCKED ON OWNER  ${s.counts.decide} decisions · ${s.counts.activate} to activate`);
  console.log(`  AGENT-READY       ${s.counts.agentReady}  (plan / implement / build / verify)`);
  console.log(`  open questions    ${s.counts.openQuestions}   sidequests ${s.counts.sidequests}   ideas ${s.counts.ideas}\n`);
  const show = (label, lane) => {
    const cs = s.cards.filter(c => c.lane.lane === lane);
    if (!cs.length) return;
    console.log(`  ${label}:`);
    for (const c of cs.slice(0, 12)) console.log(`   · ${cardLine(c)}`);
  };
  show('OWNER — decide', 'decide'); show('OWNER — activate', 'activate');
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
      if (!c) throw new TowerError('E_NOT_FOUND', `no card ${ref}`);
      const full = s.cards.find(x => x.id === c.id);
      return out(flags, null, full);
    }
    case 'add': {
      const p = readPayload(flags) || {};
      const { result } = store.mutate((s, cfg) => db.addCard(s, {
        title: flags.title ?? p.title, body: flags.body ?? p.body, kind: flags.kind ?? p.kind,
        track: flags.track ?? p.track, epoch: flags.epoch ?? p.epoch, milestoneId: flags.milestone ?? p.milestoneId,
        phase: flags.phase ?? p.phase, priority: flags.priority ?? p.priority, plan: flags.plan ?? p.plan,
        blockedBy: flags.blockedBy ? String(flags.blockedBy).split(',') : p.blockedBy,
        workOrder: flags.workOrder ?? p.workOrder, by,
      }, cfg));
      return out(flags, `added card #${result.num} (${result.id})`, result);
    }
    case 'update': {
      const p = readPayload(flags) || {};
      const patch = { ...p, by };
      for (const [f, k] of [['title', 'title'], ['body', 'body'], ['kind', 'kind'], ['track', 'track'], ['epoch', 'epoch'],
        ['milestone', 'milestoneId'], ['phase', 'phase'], ['priority', 'priority'], ['plan', 'plan'],
        ['workOrder', 'workOrder'], ['assignee', 'assignee'], ['log', 'logEntry']])
        if (flags[f] !== undefined) patch[k] = flags[f];
      if (flags.blockedBy !== undefined) patch.blockedBy = flags.blockedBy === '' ? [] : String(flags.blockedBy).split(',');
      const { result, state } = store.mutate((s, cfg) => db.updateCard(s, ref, patch, cfg), { expectRev: flags.expectRev });
      return out(flags, `updated card #${result.num} → ${db.laneOf(result, state.decisions, state.cards).lane}`, result);
    }
    case 'activate': {
      const { result } = store.mutate((s, cfg) => db.activate(s, ref, {
        track: flags.track, epoch: flags.epoch, milestoneId: flags.milestone,
        phase: flags.phase, workOrder: flags.workOrder, by,
      }, cfg));
      return out(flags, `activated card #${result.num} → ${result.phase}`, result);
    }
    case 'claim': {
      const { result } = store.mutate((s) => db.claimCard(s, ref, by));
      return out(flags, `card #${result.num} claimed by ${by}`, result);
    }
    case 'release': {
      const { result } = store.mutate((s) => db.releaseCard(s, ref, by));
      return out(flags, `card #${result.num} released`, result);
    }
    case 'delete': {
      const { result } = store.mutate((s) => db.deleteCard(s, ref, { by }));
      return out(flags, `deleted card ${result.id}`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown card verb "${verb}" — list/show/add/update/activate/claim/release/delete`);
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
      for (const d of ds) console.log(`${d.id.padEnd(16)} ${(d.status || 'open').padEnd(9)} ${d.title.slice(0, 60)}${d.outcome ? ` → ${d.outcome}` : ''}`);
      if (!ds.length) console.log('(no decisions match)');
      return;
    }
    case 'show': {
      const s = store.load();
      const d = s.decisions.find(x => x.id === id) || (() => { throw new TowerError('E_NOT_FOUND', `no decision ${id}`); })();
      return out(flags, null, d);
    }
    case 'add': {
      const p = readPayload(flags) || {};
      const payload = { ...p, by };
      for (const f of ['id', 'cardId', 'title', 'gist', 'story', 'explainer', 'inWild', 'detail', 'rec', 'group'])
        if (flags[f] !== undefined) payload[f] = flags[f];
      if (flags.card !== undefined) payload.cardId = flags.card;
      const { result } = store.mutate((s) => db.addDecision(s, payload));
      return out(flags, `added decision ${result.id} on card ${result.cardId}`, result);
    }
    case 'update': {
      const p = readPayload(flags) || {};
      for (const f of ['title', 'gist', 'story', 'explainer', 'inWild', 'detail', 'rec', 'group'])
        if (flags[f] !== undefined) p[f] = flags[f];
      const { result } = store.mutate((s) => db.updateDecision(s, id, p, by));
      return out(flags, `updated decision ${result.id}`, result);
    }
    case 'ratify': {
      const { result } = store.mutate((s) => db.ratify(s, id, flags.outcome, flags.comment, by), { expectRev: flags.expectRev });
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
  const picks = db.nextCards(s, { epoch: flags.epoch, track: flags.track, agent: flags.agent, limit: Number(flags.limit || 5) });
  const proj = db.project(s);
  const rich = picks.map(p => proj.cards.find(c => c.id === p.id));
  if (flags.json) return out(flags, null, rich);
  if (!rich.length) return console.log('(nothing agent-workable — board is either empty, blocked on the owner, or done)');
  console.log('next up (workOrder → building > verify > implement > plan):');
  for (const c of rich) console.log(` · ${cardLine(c)}`);
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
  if (!existsSync(join(dir, '.gitignore'))) writeFileSync(join(dir, '.gitignore'), 'backups/\n*.lock/\nfiles/\nserver.log\n');
  console.log(`imported ${s.cards.length} cards, ${s.decisions.length} decisions, ${s.questions.length} questions, ${s.ideas.length} ideas → ${file}`);
}

// ---- messaging: owner ⇄ agents ------------------------------------------------

// Local CLIs are trusted (they can read config.json anyway) — send the token
// so `tower …` also works against a server bound on a non-loopback address.
const authHeaders = (store) => (store.config.auth?.token ? { authorization: `Bearer ${store.config.auth.token}` } : {});

function cmdMessage(store, { pos, flags }) {
  const [verb] = pos;
  switch (verb) {
    case 'send': {
      const text = flags.text ?? pos.slice(1).join(' ');
      const payload = { from: flags.from || flags.by || 'owner', to: flags.to, text, cardId: flags.card };
      if (flags.attach) {
        // store the file in <dataDir>/files/ directly (same layout the server uses)
        const buf = readFileSync(flags.attach);
        const id = `f${Date.now().toString(36)}${Math.random().toString(36).slice(2, 7)}`;
        const filesDir = join(store.dataDir, 'files');
        mkdirSync(filesDir, { recursive: true });
        const name = flags.attach.split('/').pop();
        const type = ({ '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.gif': 'image/gif', '.webp': 'image/webp', '.svg': 'image/svg+xml', '.txt': 'text/plain', '.log': 'text/plain' })[name.slice(name.lastIndexOf('.'))] || 'application/octet-stream';
        writeFileSync(join(filesDir, id), buf);
        writeFileSync(join(filesDir, id + '.json'), JSON.stringify({ id, name, type, size: buf.length, at: new Date().toISOString() }));
        payload.file = { id, name, type };
      }
      // Prefer the running server so a live long-poll listener wakes instantly;
      // fall back to writing the file directly (listener catches up in ≤3s).
      return (async () => {
        try {
          const r = await fetch(`http://localhost:${store.config.port}/api/message/send`, { method: 'POST', headers: authHeaders(store), body: JSON.stringify(payload) });
          const j = await r.json();
          // A Tower server rejected it on the merits → real error. Anything
          // else on that port (or no JSON) → treat as no server, write direct.
          if (!r.ok) {
            if (['E_INVALID', 'E_NOT_FOUND', 'E_CONFLICT'].includes(j.error)) throw new TowerError(j.error, j.message);
            throw new Error('not a tower server');
          }
          return out(flags, `sent ${j.result.id} → ${j.result.to}`, j.result);
        } catch (e) {
          if (e instanceof TowerError) throw e;
          const { result } = store.mutate((s) => db.sendMessage(s, payload));
          return out(flags, `sent ${result.id} → ${result.to} (no live server — queued in file)`, result);
        }
      })();
    }
    case 'list': {
      const s = store.load();
      let ms = s.messages;
      if (flags.thread) ms = ms.filter(m => db.threadKey(m) === flags.thread);
      if (flags.unread) ms = ms.filter(m => m.to === (flags.for || 'owner') && !m.readAt);
      ms = ms.slice(-Number(flags.limit || 30));
      if (flags.json) return out(flags, null, ms);
      for (const m of ms) console.log(`${m.at}  ${m.from} → ${m.to}: ${m.text.slice(0, 100).replace(/\n/g, ' ')}`);
      if (!ms.length) console.log('(no messages)');
      return;
    }
    case 'read': {
      const s = store.load();
      const who = flags.for || 'owner';
      const ids = s.messages.filter(m => m.to === who && !m.readAt).map(m => m.id);
      const { result } = store.mutate((s2) => db.markMessages(s2, ids, 'readAt'));
      return out(flags, `marked ${result.marked.length} read`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown message verb "${verb}" — send/list/read`);
  }
}

function cmdAgents(store, { flags }) {
  return (async () => {
    try {
      const r = await fetch(`http://localhost:${store.config.port}/api/agents`, { headers: authHeaders(store) });
      const roster = await r.json();
      if (flags.json) return out(flags, null, roster);
      for (const a of roster) console.log(`${a.name.padEnd(16)} ${a.kind.padEnd(8)} ${a.online ? (a.state || 'online') : 'offline'}${a.launchable ? '  (launchable)' : ''}`);
      if (!roster.length) console.log('(no agents known — declare in config.json agents[] or start a listener)');
    } catch {
      const names = new Set([...(store.config.agents || []).map(a => a.name),
        ...store.load().messages.map(m => (m.from === 'owner' ? m.to : m.from))]);
      names.delete('owner');
      if (flags.json) return out(flags, null, [...names].map(name => ({ name, online: null })));
      for (const n of names) console.log(`${n}  (server down — presence unknown)`);
      if (!names.size) console.log('(no agents known)');
    }
  })();
}

// Long-lived: print each message addressed to --name as one line on stdout.
// Under Claude Code, run it inside the Monitor tool so each line wakes the
// agent. Prefers the server's long-poll; falls back to polling the file.
async function cmdListen(store, { flags }) {
  const name = flags.name || flags.by;
  if (!name) throw new TowerError('E_USAGE', 'agent listen needs --name <agent-name>');
  const kind = flags.kind || 'agent';
  const base = `http://localhost:${store.config.port}`;
  console.log(`listening as ${name} (kind ${kind}) — each owner message prints below`);
  for (;;) {
    let batch = null;
    try {
      const r = await fetch(`${base}/api/messages/wait?for=${encodeURIComponent(name)}&kind=${encodeURIComponent(kind)}`, { headers: authHeaders(store) });
      if (r.ok) batch = await r.json();
    } catch { /* server down */ }
    if (batch === null) {
      const pending = db.pendingFor(store.load(), name);
      if (pending.length) {
        store.mutate((s) => db.markMessages(s, pending.map(m => m.id), 'deliveredAt'));
        batch = pending;
      } else {
        await new Promise(r => setTimeout(r, 3000));
        batch = [];
      }
    }
    for (const m of batch) console.log(`[${m.from}] ${m.text}${m.cardId ? `  (card ${m.cardId})` : ''}`);
    if (flags.once && batch.length) return;
  }
}

// ---- undo, agent status, git hook ---------------------------------------------

function cmdUndo(store, { flags }) {
  const bdir = join(store.dataDir, 'backups');
  const files = existsSync(bdir) ? readdirSync(bdir).filter(f => f.startsWith('tower-')).sort() : [];
  if (!files.length) throw new TowerError('E_INVALID', 'nothing to undo (no backups yet)');
  const cur = store.load();
  const prev = readJSON(join(bdir, files.at(-1)));
  store.restore(prev, { expectRev: flags.expectRev ?? cur.meta.rev });
  return out(flags, `undid last write — board back to rev ${prev.meta?.rev ?? '?'} content (now rev ${cur.meta.rev + 1})`, { ok: true });
}

async function cmdAgentStatus(store, { flags }) {
  const name = flags.name || flags.by;
  if (!name) throw new TowerError('E_USAGE', 'agent status needs --name <me> --text "…"');
  try {
    const r = await fetch(`http://localhost:${store.config.port}/api/agent/status`, { method: 'POST', headers: authHeaders(store), body: JSON.stringify({ name, kind: flags.kind, text: flags.text || '' }) });
    if (!r.ok) throw new Error('bad status');
    return out(flags, `status set for ${name}`, { ok: true });
  } catch {
    throw new TowerError('E_INVALID', 'no Tower server reachable — status is live-only (start `tower serve`)');
  }
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
  tower next [--epoch E] [--track T] [--agent A] [--limit N]
                                            what an agent should pick up next

  tower card     list|show|add|update|activate|claim|release|delete
  tower decision list|show|add|update|ratify|reopen|delete
  tower question list|ask|answer|delete
  tower idea     list|add|promote|delete
  tower epoch    list|add|update|current
  tower milestone list|add|update|delete
  tower events   [--limit 30]
  tower import <old-tower.json> [--name X] [--force]

  tower message send --to <agent|owner> --text "…" [--by <name>]
                     [--card '#12'] [--attach path.png]
  tower message list [--thread <agent>] [--unread] [--json]
  tower agents                              roster + live presence
  tower agent listen --name <me> [--kind claude|codex]
                                            long-lived; prints each owner
                                            message as a line (Monitor-friendly)
  tower agent status --name <me> --text "building #187 — tests green"
  tower undo                                revert the last write (rev-guarded)
  tower githook [install]                   commits mentioning #N → card log

  Cards accept #num or id. --json everywhere for machine output.
  Complex payloads: --file payload.json or --file - (stdin).
  Writers should pass --by <agent-name>; owner ops use --by owner.
  Optimistic concurrency: --expect-rev N (exit 2 on conflict).
  Phases: ${PHASE_IDS.join(' ')}
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
      case 'events':    return cmdEvents(store, sub);
      case 'message':   return await cmdMessage(store, sub);
      case 'agents':    return await cmdAgents(store, sub);
      case 'undo':      return cmdUndo(store, sub);
      case 'githook':
        if (sub.pos[0] === 'post-commit') return await githookPostCommit(store);
        return cmdGithookInstall(store);
      case 'agent': {
        if (sub.pos[0] === 'listen') return await cmdListen(store, { flags: sub.flags });
        if (sub.pos[0] === 'status') return await cmdAgentStatus(store, { flags: sub.flags });
        throw new TowerError('E_USAGE', 'agent verbs: listen | status (see also `tower agents`)');
      }
      default: throw new TowerError('E_USAGE', `unknown command "${cmd}" — run \`tower help\``);
    }
  } catch (e) {
    if (e instanceof TowerError) {
      console.error(`tower: ${e.message}`);
      process.exitCode = e.code === 'E_CONFLICT' ? 2 : 1;
      if (flags.json) console.log(JSON.stringify({ error: e.code, message: e.message }));
      return;
    }
    throw e;
  }
}
