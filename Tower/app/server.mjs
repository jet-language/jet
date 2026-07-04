// Std-only HTTP server: static UI + a JSON API over the store.
//
// GET  /api/state                → full projected state (+ config, + rev)
// POST /api/<route>              → mutation; body may carry expectRev for
//                                  optimistic concurrency (409 on stale rev)
// Errors are structured: { error: CODE, message } with a matching status.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { join, extname, normalize } from 'node:path';
import { UI } from './paths.mjs';
import * as db from './store.mjs';
import { TowerError } from './store.mjs';

const MIME = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json', '.svg': 'image/svg+xml', '.woff2': 'font/woff2' };

const body = (req) => new Promise((res, rej) => {
  let s = '';
  req.on('data', c => { s += c; if (s.length > 5_000_000) { rej(new TowerError('E_INVALID', 'body too large')); req.destroy(); } });
  req.on('end', () => { try { res(s ? JSON.parse(s) : {}); } catch { rej(new TowerError('E_INVALID', 'body is not valid JSON')); } });
});
const send = (res, code, obj) => { res.writeHead(code, { 'content-type': 'application/json' }); res.end(JSON.stringify(obj)); };

async function serveStatic(req, res) {
  let p = req.url.split('?')[0];
  if (p === '/') p = '/index.html';
  const file = join(UI, normalize(p).replace(/^(\.\.[/\\])+/, ''));
  if (!file.startsWith(UI)) { res.writeHead(403); return res.end(); }
  try {
    const data = await readFile(file);
    res.writeHead(200, { 'content-type': MIME[extname(file)] || 'application/octet-stream', 'cache-control': 'no-store' });
    res.end(data);
  } catch { res.writeHead(404); res.end('not found'); }
}

// ---- agent presence + message wakeups (server memory; messages persist) ----
// presence: name → { kind, lastSeen, state: 'listening'|'running'|null }
const presence = new Map();
const waiters = new Map();   // name → [res] long-poll responses to flush on send
export const touch = (name, kind, state) => {
  if (!name) return;
  const p = presence.get(name) || {};
  presence.set(name, { kind: kind || p.kind || 'agent', lastSeen: Date.now(), state: state ?? p.state ?? null });
};
function flushWaiters(name) {
  for (const res of waiters.get(name) || []) { try { res.__flush(); } catch { /* gone */ } }
  waiters.set(name, []);
}
function agentRoster(store) {
  const s = store.load();
  const names = new Set([
    ...(store.config.agents || []).map(a => a.name),
    ...presence.keys(),
    ...s.messages.map(m => (m.from === 'owner' ? m.to : m.from)),
  ]);
  names.delete('owner');
  return [...names].map(name => {
    const cfg = (store.config.agents || []).find(a => a.name === name) || {};
    const live = presence.get(name);
    const online = !!live && Date.now() - live.lastSeen < 45_000;
    return { name, kind: live?.kind || cfg.kind || 'agent',
      online, state: online ? live.state : null,
      lastSeen: live ? new Date(live.lastSeen).toISOString() : null,
      launchable: !!(store.config.commands || {})[cfg.kind || live?.kind || name] };
  });
}

// route → (state, payload, config) mutation. Same verbs as the CLI.
const routes = {
  'card/add':        (s, p, cfg) => db.addCard(s, p, cfg),
  'card/update':     (s, p, cfg) => db.updateCard(s, p.id, p, cfg),
  'card/activate':   (s, p, cfg) => db.activate(s, p.id, p, cfg),
  'card/claim':      (s, p) => db.claimCard(s, p.id, p.by),
  'card/release':    (s, p) => db.releaseCard(s, p.id, p.by),
  'card/delete':     (s, p) => db.deleteCard(s, p.id, p),
  'decision/add':    (s, p) => db.addDecision(s, p),
  'decision/update': (s, p) => db.updateDecision(s, p.id, p, p.by),
  'decision/delete': (s, p) => db.deleteDecision(s, p.id, p.by),
  'clearance':       (s, p) => db.ratify(s, p.decisionId, p.outcome, p.comment, p.by),
  'clearance/batch': (s, p) => (p.decisions || []).map(d => db.ratify(s, d.decisionId, d.outcome, d.comment, p.by)),
  'clearance/reopen': (s, p) => db.reopenDecision(s, p.decisionId, p.by),
  'question/add':    (s, p) => db.addQuestion(s, p),
  'question/answer': (s, p) => db.answerQuestion(s, p.id, p.answer, p.by),
  'question/delete': (s, p) => db.deleteQuestion(s, p.id, p.by),
  'idea/add':        (s, p) => db.addIdea(s, p),
  'idea/update':     (s, p) => db.updateIdea(s, p.id, p),
  'idea/delete':     (s, p) => db.deleteIdea(s, p.id, p.by),
  'idea/promote':    (s, p, cfg) => db.promoteIdea(s, p.id, p, cfg),
  'epoch/add':       (s, p) => db.addEpoch(s, p),
  'epoch/update':    (s, p) => db.updateEpoch(s, p.id, p),
  'epoch/current':   (s, p) => db.setCurrentEpoch(s, p.epoch),
  'milestone/add':   (s, p) => db.addMilestone(s, p),
  'milestone/update': (s, p) => db.updateMilestone(s, p.id, p, p.by),
  'milestone/delete': (s, p) => db.deleteMilestone(s, p.id, p.by),
  'ui/toggle':       (s, p) => db.toggleOpen(s, p.key),
  'message/send':    (s, p) => db.sendMessage(s, p),
  'message/mark':    (s, p) => db.markMessages(s, p.ids || [], p.field || 'readAt'),
};

const STATUS = { E_NOT_FOUND: 404, E_INVALID: 400, E_USAGE: 400, E_CONFLICT: 409, E_CLAIMED: 409, E_NO_DATA: 500 };

// Spawn a configured headless agent command; its stdout becomes the reply.
// The message rides in $TOWER_PROMPT (env, not argv — no quoting pitfalls).
function launch(store, agent, kind, cmd, text) {
  import('node:child_process').then(({ spawn }) => {
    import('node:path').then(({ dirname }) => {
      touch(agent, kind, 'running');
      const child = spawn('/bin/sh', ['-c', `${cmd} "$TOWER_PROMPT"`], {
        cwd: dirname(store.dataDir),
        env: { ...process.env, TOWER_PROMPT: text },
        stdio: ['ignore', 'pipe', 'pipe'],
      });
      let out = '';
      const eat = (c) => { out += c; if (out.length > 100_000) out = out.slice(-100_000); };
      child.stdout.on('data', eat); child.stderr.on('data', eat);
      const timer = setTimeout(() => child.kill('SIGTERM'), 15 * 60_000);
      child.on('close', (code) => {
        clearTimeout(timer);
        touch(agent, kind, null);
        const text2 = (out.trim() || `(no output — exit ${code})`).slice(0, 20_000);
        store.mutate((s) => db.sendMessage(s, { from: agent, to: 'owner', text: text2 }));
      });
    });
  });
}

export function serve(store, port = 7878, open = false) {
  const server = createServer(async (req, res) => {
    try {
      if (req.method === 'GET' && req.url.startsWith('/api/state')) {
        return send(res, 200, store.project());
      }
      if (req.method === 'GET' && req.url.startsWith('/api/next')) {
        const q = new URL(req.url, 'http://x').searchParams;
        const picks = db.nextCards(store.load(), { epoch: q.get('epoch') || undefined, track: q.get('track') || undefined, agent: q.get('agent') || undefined, limit: Number(q.get('limit') || 5) });
        return send(res, 200, picks);
      }
      if (req.method === 'GET' && req.url.startsWith('/api/events')) {
        const q = new URL(req.url, 'http://x').searchParams;
        return send(res, 200, store.load().events.slice(0, Number(q.get('limit') || 50)));
      }
      if (req.method === 'GET' && req.url.startsWith('/api/agents')) {
        return send(res, 200, agentRoster(store));
      }
      // Long-poll: hold up to 25s until a message lands for `for`. A listening
      // agent loops on this; each response marks the batch delivered.
      if (req.method === 'GET' && req.url.startsWith('/api/messages/wait')) {
        const q = new URL(req.url, 'http://x').searchParams;
        const name = q.get('for');
        if (!name) return send(res, 400, { error: 'E_INVALID', message: 'messages/wait needs ?for=<name>' });
        touch(name, q.get('kind'), 'listening');
        const deliver = () => {
          const pending = db.pendingFor(store.load(), name);
          if (!pending.length) return false;
          store.mutate((s) => db.markMessages(s, pending.map(m => m.id), 'deliveredAt'));
          send(res, 200, pending);
          return true;
        };
        if (deliver()) return;
        const timer = setTimeout(() => { unhook(); send(res, 200, []); }, 25_000);
        const unhook = () => { clearTimeout(timer); waiters.set(name, (waiters.get(name) || []).filter(r => r !== res)); };
        res.__flush = () => { unhook(); touch(name); if (!deliver()) send(res, 200, []); };
        waiters.set(name, [...(waiters.get(name) || []), res]);
        req.on('close', unhook);
        return;
      }
      // Launch bridge (opt-in via config.commands): start a headless agent turn
      // with the owner's message; the reply lands in the thread when it exits.
      if (req.method === 'POST' && req.url.startsWith('/api/agent/launch')) {
        const p = await body(req);
        const roster = agentRoster(store);
        const a = roster.find(x => x.name === p.agent);
        const kind = a?.kind || p.agent;
        const cmd = (store.config.commands || {})[kind];
        if (!cmd) return send(res, 400, { error: 'E_INVALID', message: `no launch command configured for "${kind}" — add config.commands.${kind}` });
        if (!p.text || !String(p.text).trim()) return send(res, 400, { error: 'E_INVALID', message: 'launch needs text' });
        store.mutate((s) => db.sendMessage(s, { from: 'owner', to: p.agent, text: p.text, cardId: p.cardId }));
        launch(store, p.agent, kind, cmd, String(p.text));
        return send(res, 200, { ok: true, state: store.project() });
      }
      if (req.method === 'POST' && req.url.startsWith('/api/')) {
        const name = req.url.slice(5).split('?')[0];
        const fn = routes[name];
        if (!fn) return send(res, 404, { error: 'E_USAGE', message: `unknown route ${name}` });
        const p = await body(req);
        const { result } = store.mutate((s, cfg) => fn(s, p, cfg), { expectRev: p.expectRev });
        if (name === 'message/send' && result?.to) flushWaiters(result.to);
        return send(res, 200, { ok: true, result, state: store.project() });
      }
      if (req.method === 'GET') return serveStatic(req, res);
      res.writeHead(405); res.end();
    } catch (e) {
      if (e instanceof TowerError) return send(res, STATUS[e.code] || 400, { error: e.code, message: e.message });
      console.error(e);
      send(res, 500, { error: 'E_INTERNAL', message: String(e.message || e) });
    }
  });
  server.on('error', (e) => {
    if (e.code === 'EADDRINUSE') {
      console.error(`tower: port ${port} is already in use (another Tower or app?) — try --port ${port + 1}`);
      process.exit(1);
    }
    throw e;
  });
  server.listen(port, () => {
    const url = `http://localhost:${port}`;
    console.log(`\n  ▲ Tower — ${store.config.project} — ${url}\n    data: ${store.file}\n`);
    if (open) import('node:child_process').then(({ spawn }) => {
      const cmd = process.platform === 'darwin' ? 'open' : 'xdg-open';
      spawn(cmd, [url], { stdio: 'ignore', detached: true }).unref();
    });
  });
  return server;
}
