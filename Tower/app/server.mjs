// Std-only HTTP server: static UI + JSON API + SSE live stream + web push.
//
// Auth: non-localhost requests need the token from config.auth (cookie,
// Bearer header, or ?key=…). Localhost is always exempt so local CLIs and
// agents just work. Static PWA plumbing (manifest, sw.js) is public.
//
// Live: every mutation broadcasts the projected state over /api/stream
// (SSE). New ballots and questions fan out as payload-less web pushes to
// the owner's subscribed devices.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { readdirSync, existsSync } from 'node:fs';
import { join, extname, normalize, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomBytes } from 'node:crypto';
import { UI, readJSON } from './paths.mjs';
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

// route → (state, payload, config) mutation. Same verbs as the CLI.
const routes = {
  'card/add':        (s, p, cfg) => db.addCard(s, p, cfg),
  'card/update':     (s, p, cfg) => db.updateCard(s, p.id, p, cfg),
  'card/activate':   (s, p, cfg) => db.activate(s, p.id, p, cfg),
  'card/claim':      (s, p) => db.claimCard(s, p.id, p.by),
  'card/release':    (s, p) => db.releaseCard(s, p.id, p.by, p.handoff),
  'card/delete':     (s, p) => db.deleteCard(s, p.id, p),
  'card/criteria-add':    (s, p) => db.addCriterion(s, p.id, p.text, p.by),
  'card/criteria-meet':   (s, p) => db.meetCriterion(s, p.id, p.n, { evidence: p.evidence, by: p.by }),
  'card/criteria-verify': (s, p) => db.verifyCriterion(s, p.id, p.n, { evidence: p.evidence, by: p.by }),
  'decision/add':    (s, p) => db.addDecision(s, p),
  'decision/update': (s, p) => db.updateDecision(s, p.id, p, p.by),
  'decision/delete': (s, p) => db.deleteDecision(s, p.id, p.by),
  'clearance':       (s, p) => db.ratify(s, p.decisionId, p.outcome, p.comment, p.by, p.quote),
  'clearance/batch': (s, p) => (p.decisions || []).map(d => db.ratify(s, d.decisionId, d.outcome, d.comment, p.by, d.quote || p.quote)),
  'clearance/reopen': (s, p) => db.reopenDecision(s, p.decisionId, p.by),
  'verdict':         (s, p) => db.mintVerdict(s, p.id, p.outcome, p.title, p.by),
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
};

const STATUS = { E_NOT_FOUND: 404, E_INVALID: 400, E_USAGE: 400, E_CONFLICT: 409, E_CLAIMED: 409, E_NO_DATA: 500, E_CRITERIA: 409, E_CRITERIA_SELF: 400,
  E_BALLOT: 400, E_OWNER_ONLY: 403, E_OWNER_LANE: 403, E_HAS_RATIFIED: 409, E_HANDOFF: 400 };

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

  const server = createServer(async (req, res) => {
    try {
      const url = new URL(req.url, 'http://x');
      const ok = authed(req, res, token, url);
      if (ok !== true) return;

      // ---- reads ----
      if (req.method === 'GET' && url.pathname === '/api/state') return send(res, 200, projected(store));
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
      if (req.method === 'GET' && url.pathname === '/api/history') {
        // #461: archived (retired) cards, optionally filtered by epoch —
        // the Board UI's done-subgroup uses this for a lazy archived count.
        const epoch = url.searchParams.get('epoch');
        const h = store.loadHistory();
        const cards = epoch ? h.cards.filter(c => c.epoch === epoch) : h.cards;
        return send(res, 200, { cards, count: cards.length });
      }
      if (req.method === 'GET' && url.pathname === '/api/next') {
        const q = url.searchParams;
        return send(res, 200, db.nextCards(store.load(), { epoch: q.get('epoch') || undefined, track: q.get('track') || undefined, agent: q.get('agent') || undefined, limit: Number(q.get('limit') || 5) }));
      }
      // ---- writes ----
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
        // side effect: new ballots/questions push to the owner's devices
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
