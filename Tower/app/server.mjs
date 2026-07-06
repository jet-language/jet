// Std-only HTTP server: static UI + JSON API + SSE live stream + web push.
//
// Auth: non-localhost requests need the token from config.auth (cookie,
// Bearer header, or ?key=…). Localhost is always exempt so local CLIs and
// agents just work. Static PWA plumbing (manifest, sw.js) is public.
//
// Live: every mutation broadcasts the projected state over /api/stream
// (SSE). Ratifications + greenlights collapse into ONE batched notification
// to listening agents after a quiet period (config.notifyBatchSeconds).
// New messages to the owner also fan out as payload-less web pushes.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { readFileSync, writeFileSync, readdirSync, mkdirSync, existsSync, statSync } from 'node:fs';
import { join, extname, normalize, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomBytes } from 'node:crypto';
import { UI, newId, readJSON } from './paths.mjs';
import { saveConfig } from './config.mjs';
import { generateVapid, pushTo } from './webpush.mjs';
import * as db from './store.mjs';
import { TowerError } from './store.mjs';

const MIME = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.webmanifest': 'application/manifest+json' };

const body = (req, limit = 5_000_000) => new Promise((res, rej) => {
  const chunks = [];
  let size = 0;
  req.on('data', c => { size += c.length; if (size > limit) { rej(new TowerError('E_INVALID', 'body too large')); req.destroy(); } else chunks.push(c); });
  req.on('end', () => res(Buffer.concat(chunks)));
  req.on('error', rej);
});
const jsonBody = async (req) => {
  const buf = await body(req);
  try { return buf.length ? JSON.parse(buf.toString('utf8')) : {}; }
  catch { throw new TowerError('E_INVALID', 'body is not valid JSON'); }
};
const send = (res, code, obj) => { res.writeHead(code, { 'content-type': 'application/json' }); res.end(JSON.stringify(obj)); };

// ---- agent presence + message wakeups (server memory; messages persist) ----
const presence = new Map();  // name → { kind, lastSeen, state, statusText }
const waiters = new Map();   // name → [res] long-poll responses to flush on send
export const touch = (name, kind, state, statusText) => {
  if (!name) return;
  const p = presence.get(name) || {};
  presence.set(name, {
    kind: kind || p.kind || 'agent', lastSeen: Date.now(),
    state: state === undefined ? (p.state ?? null) : state,
    statusText: statusText === undefined ? (p.statusText ?? null) : statusText,
  });
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
  names.delete('owner'); names.delete('tower');
  return [...names].map(name => {
    const cfg = (store.config.agents || []).find(a => a.name === name) || {};
    const live = presence.get(name);
    const online = !!live && Date.now() - live.lastSeen < 45_000;
    return { name, kind: live?.kind || cfg.kind || 'agent',
      online, state: online ? live.state : null, statusText: online ? live.statusText : null,
      lastSeen: live ? new Date(live.lastSeen).toISOString() : null,
      launchable: !!(store.config.commands || {})[cfg.kind || live?.kind || name] };
  });
}

// ---- SSE live stream --------------------------------------------------------
// BOOT identifies this server process; clients reload (when idle) if it
// changes, so a server upgrade never leaves stale UI code running.
const BOOT = randomBytes(6).toString('base64url');
const TOWER_BIN = join(dirname(fileURLToPath(import.meta.url)), '..', 'tower.mjs');
const projected = (store) => ({ ...store.project(), boot: BOOT, cli: `node ${TOWER_BIN}` });
const sseClients = new Set();
function broadcast(store) {
  if (!sseClients.size) return;
  const data = `data: ${JSON.stringify(projected(store))}\n\n`;
  for (const res of [...sseClients]) { try { res.write(`event: state\n${data}`); } catch { sseClients.delete(res); } }
}

// ---- web push ----------------------------------------------------------------
let lastPushAt = 0;
async function pushOwner(store, reason) {
  const push = store.config.push;
  if (!push?.subscriptions?.length) return;
  if (Date.now() - lastPushAt < 25_000) return;   // burst throttle; SW fetches fresh state anyway
  lastPushAt = Date.now();
  const dead = [];
  await Promise.all(push.subscriptions.map(async (sub) => {
    const r = await pushTo(sub, { privateJwk: push.privateJwk, publicKey: push.publicKey });
    if (r.gone) dead.push(sub.endpoint);
  }));
  if (dead.length) {
    push.subscriptions = push.subscriptions.filter(s => !dead.includes(s.endpoint));
    saveConfig(store.dataDir, { push });
  }
}

// ---- batched agent notifications ---------------------------------------------
// Ratifying a stack of ballots (or greenlighting several cards) produces ONE
// "[tower] …" message per listening agent after the owner goes quiet.
const batch = { decisions: [], cards: [], timer: null };
function queueNotify(store, kind, entry) {
  batch[kind].push(entry);
  clearTimeout(batch.timer);
  batch.timer = setTimeout(() => flushNotify(store), (store.config.notifyBatchSeconds ?? 90) * 1000);
  if (batch.timer.unref) batch.timer.unref();
}
function flushNotify(store) {
  const dec = batch.decisions.splice(0);
  const cards = batch.cards.splice(0);
  clearTimeout(batch.timer); batch.timer = null;
  if (!dec.length && !cards.length) return;
  const bits = [];
  if (dec.length) bits.push(`${dec.length} decision${dec.length > 1 ? 's' : ''} ratified: ${dec.map(d => `${d.id}→${d.outcome}${d.comment ? ' ("' + d.comment.slice(0, 80) + '")' : ''}`).join(' · ')}`);
  if (cards.length) bits.push(`greenlit: ${cards.map(c => '#' + c.num).join(' ')}`);
  const text = `[tower] ${bits.join(' — ')}. Board is current; pick up with \`tower next\`.`;
  const roster = agentRoster(store);
  const online = roster.filter(a => a.online);
  const targets = (online.length ? online : roster).map(a => a.name);
  if (!targets.length) return;
  store.mutate((s) => { for (const t of targets) db.sendMessage(s, { from: 'tower', to: t, text }); });
  for (const t of targets) flushWaiters(t);
  broadcast(store);
}

// ---- launch bridge (streams output into a live message) -----------------------
function launch(store, agent, kind, cmd, text) {
  Promise.all([import('node:child_process')]).then(([{ spawn }]) => {
    touch(agent, kind, 'running');
    const { result: live } = store.mutate((s) => db.sendMessage(s, { from: agent, to: 'owner', text: '⟳ running…' }));
    broadcast(store);
    const child = spawn('/bin/sh', ['-c', `${cmd} "$TOWER_PROMPT"`], {
      cwd: dirname(store.dataDir),
      env: { ...process.env, TOWER_PROMPT: text },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let out = '', dirty = false;
    const eat = (c) => { out += c; if (out.length > 100_000) out = out.slice(-100_000); dirty = true; };
    child.stdout.on('data', eat); child.stderr.on('data', eat);
    const tick = setInterval(() => {
      if (!dirty) return; dirty = false;
      store.mutate((s) => db.updateMessageText(s, live.id, '⟳ running…\n\n' + out.trim()));
      broadcast(store);
    }, 1200);
    const timer = setTimeout(() => child.kill('SIGTERM'), 15 * 60_000);
    child.on('close', (code) => {
      clearTimeout(timer); clearInterval(tick);
      touch(agent, kind, null);
      const final = (out.trim() || `(no output — exit ${code})`).slice(0, 20_000);
      store.mutate((s) => db.updateMessageText(s, live.id, final));
      broadcast(store); pushOwner(store, 'launch-done');
    });
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
  'digest/seen':     (s) => db.setDigestCursor(s),
  'message/send':    (s, p) => db.sendMessage(s, p),
  'message/mark':    (s, p) => db.markMessages(s, p.ids || [], p.field || 'readAt'),
};

const STATUS = { E_NOT_FOUND: 404, E_INVALID: 400, E_USAGE: 400, E_CONFLICT: 409, E_CLAIMED: 409, E_NO_DATA: 500 };

// ---- auth ----------------------------------------------------------------------
const PUBLIC = new Set(['/manifest.webmanifest', '/sw.js', '/icon.svg']);
const COOKIE = (token) => `tower=${token}; Path=/; Max-Age=31536000; SameSite=Lax; HttpOnly`;
function isLocal(req) {
  const a = req.socket.remoteAddress || '';
  return a === '127.0.0.1' || a === '::1' || a.startsWith('::ffff:127.');
}
// A locked-out navigation gets a real unlock page, never raw JSON.
const UNLOCK_HTML = `<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Tower — unlock</title><body style="margin:0;min-height:100vh;display:grid;place-items:center;background:#060608;color:#f4f3f5;font:15px/1.5 system-ui">
<form method="GET" action="/" style="display:grid;gap:12px;width:min(340px,90vw);text-align:center">
<div style="font:700 22px/1 sans-serif;letter-spacing:.06em">TOWER<span style="color:#ff2e4d">.</span></div>
<div style="color:#9d9ca8;font-size:13px">This device isn't unlocked. Paste the access key<br>(<code>auth.token</code> in <code>.tower/config.json</code>).</div>
<input name="key" autofocus placeholder="access key" style="padding:11px;border-radius:8px;border:1px solid #343444;background:#0e0e12;color:#f4f3f5;text-align:center">
<button style="padding:11px;border-radius:8px;border:0;background:#b3122d;color:#fff;font-weight:600;cursor:pointer">Unlock</button>
</form></body>`;
function authed(req, res, token, url) {
  if (!token || isLocal(req) || PUBLIC.has(url.pathname)) return true;
  const key = url.searchParams.get('key');
  if (key === token) {
    // first visit with ?key= → set cookie, redirect to clean URL
    res.writeHead(302, { 'set-cookie': COOKIE(token), location: url.pathname + url.hash });
    res.end();
    return 'redirected';
  }
  const cookie = /(?:^|;\s*)tower=([^;]+)/.exec(req.headers.cookie || '')?.[1];
  const bearer = /^Bearer (.+)$/.exec(req.headers.authorization || '')?.[1];
  if (cookie === token || bearer === token) return true;
  if (req.method === 'GET' && !url.pathname.startsWith('/api/') && (req.headers.accept || '').includes('text/html')) {
    res.writeHead(401, { 'content-type': 'text/html' });
    res.end(UNLOCK_HTML);
    return false;
  }
  send(res, 401, { error: 'E_AUTH', message: 'unauthorized — unlock this device with the access key (auth.token in .tower/config.json)' });
  return false;
}

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

export function serve(store, port = 7878, open = false) {
  // one-time provisioning: VAPID push keys. Auth is OPT-IN: set
  //   "auth": { "token": "…" }
  // in config.json to require a key from non-localhost devices.
  if (!store.config.push?.publicKey) {
    const v = generateVapid();
    store.config.push = { publicKey: v.publicKey, privateJwk: v.privateJwk, subscriptions: store.config.push?.subscriptions || [] };
    saveConfig(store.dataDir, { push: store.config.push });
  }
  const token = store.config.auth?.token || null;
  const filesDir = join(store.dataDir, 'files');

  const server = createServer(async (req, res) => {
    try {
      const url = new URL(req.url, 'http://x');
      const ok = authed(req, res, token, url);
      if (ok !== true) return;

      // ---- reads ----
      if (req.method === 'GET' && url.pathname === '/api/state') return send(res, 200, projected(store));
      if (req.method === 'GET' && url.pathname === '/api/agents') return send(res, 200, agentRoster(store));
      if (req.method === 'GET' && url.pathname === '/api/push/key') return send(res, 200, { key: store.config.push.publicKey, subscribed: store.config.push.subscriptions.length });
      if (req.method === 'GET' && url.pathname === '/api/events') {
        return send(res, 200, store.load().events.slice(0, Number(url.searchParams.get('limit') || 50)));
      }
      if (req.method === 'GET' && url.pathname === '/api/stream') {
        res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store', connection: 'keep-alive' });
        res.write(`event: state\ndata: ${JSON.stringify(store.project())}\n\n`);
        sseClients.add(res);
        const ping = setInterval(() => { try { res.write(': ping\n\n'); } catch { /* closed */ } }, 20_000);
        req.on('close', () => { clearInterval(ping); sseClients.delete(res); });
        return;
      }
      if (req.method === 'GET' && url.pathname === '/api/next') {
        const q = url.searchParams;
        return send(res, 200, db.nextCards(store.load(), { epoch: q.get('epoch') || undefined, track: q.get('track') || undefined, agent: q.get('agent') || undefined, limit: Number(q.get('limit') || 5) }));
      }
      if (req.method === 'GET' && url.pathname === '/api/messages/wait') {
        const name = url.searchParams.get('for');
        if (!name) return send(res, 400, { error: 'E_INVALID', message: 'messages/wait needs ?for=<name>' });
        touch(name, url.searchParams.get('kind'), 'listening');
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
      if (req.method === 'GET' && url.pathname.startsWith('/files/')) {
        const id = url.pathname.slice(7).replace(/[^A-Za-z0-9._-]/g, '');
        const meta = readJSON(join(filesDir, id + '.json'), null);
        if (!meta) { res.writeHead(404); return res.end(); }
        const data = readFileSync(join(filesDir, id));
        res.writeHead(200, { 'content-type': meta.type || 'application/octet-stream', 'cache-control': 'max-age=86400' });
        return res.end(data);
      }

      // ---- writes ----
      if (req.method === 'POST' && url.pathname === '/api/file') {
        const buf = await body(req, 12_000_000);
        if (!buf.length) return send(res, 400, { error: 'E_INVALID', message: 'empty file' });
        const id = newId('f');
        mkdirSync(filesDir, { recursive: true });
        writeFileSync(join(filesDir, id), buf);
        const meta = { id, name: (url.searchParams.get('name') || 'file').slice(0, 120), type: (url.searchParams.get('type') || 'application/octet-stream').slice(0, 80), size: buf.length, at: new Date().toISOString() };
        writeFileSync(join(filesDir, id + '.json'), JSON.stringify(meta));
        return send(res, 200, { ok: true, file: meta });
      }
      if (req.method === 'POST' && url.pathname === '/api/push/subscribe') {
        const p = await jsonBody(req);
        if (!p.subscription?.endpoint) return send(res, 400, { error: 'E_INVALID', message: 'missing subscription' });
        const push = store.config.push;
        push.subscriptions = [...push.subscriptions.filter(s => s.endpoint !== p.subscription.endpoint), { ...p.subscription, ua: (req.headers['user-agent'] || '').slice(0, 80), created: new Date().toISOString() }];
        saveConfig(store.dataDir, { push });
        return send(res, 200, { ok: true, subscribed: push.subscriptions.length });
      }
      if (req.method === 'POST' && url.pathname === '/api/push/test') {
        lastPushAt = 0; await pushOwner(store, 'test');
        return send(res, 200, { ok: true, sent: store.config.push.subscriptions.length });
      }
      if (req.method === 'POST' && url.pathname === '/api/agent/status') {
        const p = await jsonBody(req);
        touch(p.name, p.kind, 'listening', (p.text || '').slice(0, 140));
        return send(res, 200, { ok: true });
      }
      if (req.method === 'POST' && url.pathname === '/api/agent/launch') {
        const p = await jsonBody(req);
        const roster = agentRoster(store);
        const a = roster.find(x => x.name === p.agent);
        const kind = a?.kind || p.agent;
        const cmd = (store.config.commands || {})[kind];
        if (!cmd) return send(res, 400, { error: 'E_INVALID', message: `no launch command configured for "${kind}" — add config.commands.${kind}` });
        if (!p.text || !String(p.text).trim()) return send(res, 400, { error: 'E_INVALID', message: 'launch needs text' });
        store.mutate((s) => db.sendMessage(s, { from: 'owner', to: p.agent, text: p.text, cardId: p.cardId }));
        launch(store, p.agent, kind, cmd, String(p.text));
        broadcast(store);
        return send(res, 200, { ok: true, state: projected(store) });
      }
      if (req.method === 'POST' && url.pathname === '/api/undo') {
        const p = await jsonBody(req);
        const bdir = join(store.dataDir, 'backups');
        const files = existsSync(bdir) ? readdirSync(bdir).filter(f => f.startsWith('tower-')).sort() : [];
        if (!files.length) return send(res, 400, { error: 'E_INVALID', message: 'nothing to undo (no backups yet)' });
        const prev = readJSON(join(bdir, files.at(-1)), null);
        if (!prev) return send(res, 500, { error: 'E_INTERNAL', message: 'backup unreadable' });
        store.restore(prev, { expectRev: p.expectRev });
        broadcast(store);
        return send(res, 200, { ok: true, state: projected(store) });
      }
      if (req.method === 'POST' && url.pathname.startsWith('/api/')) {
        const name = url.pathname.slice(5);
        const fn = routes[name];
        if (!fn) return send(res, 404, { error: 'E_USAGE', message: `unknown route ${name}` });
        const p = await jsonBody(req);
        const { result } = store.mutate((s, cfg) => fn(s, p, cfg), { expectRev: p.expectRev });
        // side effects: wake listeners, batch agent notifications, push owner
        if (name === 'message/send' && result?.to) {
          flushWaiters(result.to);
          if (result.to === 'owner' && result.from !== 'tower') pushOwner(store, 'message');
        }
        if (name === 'clearance') queueNotify(store, 'decisions', { id: result.id, outcome: result.outcome, comment: result.comment });
        if (name === 'clearance/batch') for (const d of result) queueNotify(store, 'decisions', { id: d.id, outcome: d.outcome, comment: d.comment });
        if (name === 'card/activate') queueNotify(store, 'cards', { num: result.num });
        if (name === 'decision/add' || name === 'question/add') pushOwner(store, name);
        broadcast(store);
        return send(res, 200, { ok: true, result, state: projected(store) });
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
    console.log(`\n  ▲ Tower — ${store.config.project} — ${url}\n    data: ${store.file}${token ? `\n    remote access: http://<host>:${port}/?key=${token}` : ''}\n`);
    if (open) import('node:child_process').then(({ spawn }) => {
      const cmd = process.platform === 'darwin' ? 'open' : 'xdg-open';
      spawn(cmd, [url], { stdio: 'ignore', detached: true }).unref();
    });
  });
  return server;
}
