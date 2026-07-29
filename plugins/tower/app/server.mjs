// Std-only HTTP server: static UI + JSON API + SSE live stream.
//
// Auth: non-localhost requests need the token from config.auth (cookie,
// Bearer header, or ?key=…). Localhost is always exempt so local CLIs and
// agents just work. Static PWA plumbing (manifest, sw.js) is public.
//
// Live: every mutation broadcasts the projected state over /api/stream
// (SSE). Web push / VAPID removed (owner D-VERDICT-460-1, 2026-07-14).
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { readdirSync, existsSync } from 'node:fs';
import { join, extname, normalize, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomBytes } from 'node:crypto';
import { UI, readJSON } from './paths.mjs';
import * as db from './store.mjs';
import { TowerError } from './store.mjs';
import { lint } from './lint.mjs';
import { computeVersion } from './version.mjs';
import * as docs from './docs.mjs';

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
const ACCEPTANCE_TTL_MS = 30_000;
const OWNER_SESSION_TTL_MS = 8 * 60 * 60 * 1000;
const OWNER_SESSION_COOKIE = 'tower-owner-session';
const ownerSessions = new Map();
const acceptanceChallenges = new Map();
const resolveAcceptance = db.createAcceptanceResolver();
const TOWER_ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const TOWER_BIN = join(TOWER_ROOT, 'tower.mjs');
// #522 — content-hash of the source this PROCESS actually loaded at boot.
// Compared against a fresh computeVersion() of what's on disk NOW (exposed
// via /api/version and stamped into every served index.html) so a stale
// process — one the self-restart watcher failed to swap out — is visible
// to the owner instead of silently 404ing new routes.
const START_VERSION = computeVersion(TOWER_ROOT);
const projected = (store) => ({ ...store.project(), boot: BOOT, cli: `node ${TOWER_BIN}` });
const sseClients = new Set();
function broadcast(store) {
  if (!sseClients.size) return;
  const data = `data: ${JSON.stringify(projected(store))}\n\n`;
  for (const res of [...sseClients]) { try { res.write(`event: state\n${data}`); } catch { sseClients.delete(res); } }
}

// route → (state, payload, config) mutation. Same verbs as the CLI.
const routes = {
  'card/add':        (s, p, cfg) => db.addCard(s, p, cfg),
  'card/update':     (s, p, cfg) => db.updateCard(s, p.id, p, cfg),
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
  'message/add':     (s, p) => db.addMessage(s, p),
  'message/done':    (s, p) => db.doneMessage(s, p.id, p.by),
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
  'done/clear':      (s) => db.clearDoneQueue(s),
};

const STATUS = { E_NOT_FOUND: 404, E_INVALID: 400, E_USAGE: 400, E_CONFLICT: 409, E_CLAIMED: 409, E_NO_DATA: 500, E_CRITERIA: 409, E_CRITERIA_SELF: 400,
  E_BALLOT: 400, E_OWNER_ONLY: 403, E_OWNER_LANE: 403, E_ACCEPTANCE_OWNER_UI: 403, E_HAS_RATIFIED: 409, E_HANDOFF: 400 };

// ---- auth ----------------------------------------------------------------------
const PUBLIC = new Set(['/manifest.webmanifest', '/sw.js', '/icon.svg']);
const COOKIE = (token) => `tower=${token}; Path=/; Max-Age=31536000; SameSite=Lax; HttpOnly`;
const OWNER_COOKIE = (token) => `${OWNER_SESSION_COOKIE}=${token}; Path=/; Max-Age=${OWNER_SESSION_TTL_MS / 1000}; SameSite=Strict; HttpOnly`;
const cookieValue = (req, name) => new RegExp(`(?:^|;\\s*)${name}=([^;]+)`).exec(req.headers.cookie || '')?.[1];
function isLocal(req) {
  const a = req.socket.remoteAddress || '';
  return a === '127.0.0.1' || a === '::1' || a.startsWith('::ffff:127.');
}
function ownerLoopback(req) {
  if (!isLocal(req)) return false;
  const forwarded = String(req.headers['x-forwarded-for'] || '').trim();
  return !forwarded || forwarded === '127.0.0.1' || forwarded === '::1';
}
// #515 P0 fix: the dev box (loopback) is always trusted, but a remote/phone
// device was ALWAYS rejected here even though the rest of this server
// already treats a device presenting the configured auth.token as the
// owner (see authed() below) — the exact case the README's "Live + remote"
// PWA/push setup exists for. Reuse that same trust boundary: loopback OR
// the shared token, never a bare LAN/tailnet IP with no proof of identity.
function ownerTrusted() {
  // Owner order 2026-07-12: no loopback/token restriction on owner
  // verification — any device that can reach the board acts as the owner.
  // The board is a single-owner LAN/tailnet tool; if that changes, revisit
  // via card #460 (auth.token hardening).
  return true;
}
function ownerSession(req) {
  const token = cookieValue(req, OWNER_SESSION_COOKIE);
  const session = token && ownerSessions.get(token);
  if (!session || session.expires < Date.now()) {
    if (token) ownerSessions.delete(token);
    return null;
  }
  return { token, ...session };
}
function auditAcceptanceReject(store, decisionId, route, reason, by) {
  store.mutate((s) => db.auditAcceptanceRejection(s, decisionId, route, reason, by));
  broadcast(store);
}
// A locked-out navigation gets a real unlock page, never raw JSON.
const UNLOCK_HTML = `<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Tower — unlock</title><body style="margin:0;min-height:100vh;display:grid;place-items:center;background:#060608;color:#f4f3f5;font:15px/1.5 system-ui">
<form method="GET" action="/" style="display:grid;gap:12px;width:min(340px,90vw);text-align:center">
<div style="font:700 22px/1 sans-serif;letter-spacing:.06em">TOWER<span style="color:#ff2e4d">.</span></div>
<div style="color:#9d9ca8;font-size:13px">This device isn't unlocked. Paste the access key<br>(<code>auth.token</code> in <code>.tower/secrets.json</code>).</div>
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
  send(res, 401, { error: 'E_AUTH', message: 'unauthorized — unlock this device with the access key (auth.token in .tower/secrets.json)' });
  return false;
}

async function serveStatic(req, res) {
  let p = req.url.split('?')[0];
  if (p === '/') p = '/index.html';
  const file = join(UI, normalize(p).replace(/^(\.\.[/\\])+/, ''));
  if (!file.startsWith(UI)) { res.writeHead(403); return res.end(); }
  try {
    const data = await readFile(file);
    if (extname(file) === '.html') {
      // Stamp the CURRENT on-disk version into the page every time — this
      // file is read fresh per request, so it always reflects the latest
      // source even when the running process (START_VERSION) hasn't caught
      // up yet. See version.mjs + the stale-banner logic in tower.js.
      const html = data.toString('utf8').replace('__TOWER_VERSION__', computeVersion(TOWER_ROOT));
      res.writeHead(200, { 'content-type': MIME['.html'], 'cache-control': 'no-store' });
      return res.end(html);
    }
    res.writeHead(200, { 'content-type': MIME[extname(file)] || 'application/octet-stream', 'cache-control': 'no-store' });
    res.end(data);
  } catch { res.writeHead(404); res.end('not found'); }
}

export function serve(store, port = 7878, open = false) {
  // Auth is OPT-IN: set "auth": { "token": "…" } in untracked secrets.json
  // to require a key from non-localhost devices. No VAPID provisioning.
  const token = store.config.auth?.token || null;

  const server = createServer(async (req, res) => {
    try {
      const url = new URL(req.url, 'http://x');
      const ok = authed(req, res, token, url);
      if (ok !== true) return;

      // A loopback (or token-authenticated remote) browser navigation
      // establishes an HttpOnly, process-local owner UI session. It is not
      // a file-readable/static bearer credential.
      if (req.method === 'GET' && (url.pathname === '/' || url.pathname === '/index.html') && ownerTrusted(req, token) && !ownerSession(req)) {
        const sessionToken = randomBytes(32).toString('base64url');
        ownerSessions.set(sessionToken, { auditId: randomBytes(8).toString('base64url'), expires: Date.now() + OWNER_SESSION_TTL_MS });
        res.setHeader('set-cookie', OWNER_COOKIE(sessionToken));
      }

      // ---- reads ----
      if (req.method === 'GET' && url.pathname === '/api/state') return send(res, 200, projected(store));
      // #522 — belt+braces to the self-restart watcher: `start` is what
      // THIS process loaded at boot; `current` is a fresh read of what's on
      // disk right now. A mismatch means the process needs a restart.
      if (req.method === 'GET' && url.pathname === '/api/version') {
        const current = computeVersion(TOWER_ROOT);
        return send(res, 200, { start: START_VERSION, current, stale: current !== START_VERSION });
      }
      if (req.method === 'GET' && url.pathname === '/api/events') {
        return send(res, 200, store.load().events.slice(0, Number(url.searchParams.get('limit') || 50)));
      }
      if (req.method === 'GET' && url.pathname === '/api/messages') {
        return send(res, 200, db.listMessages(store.load(), {
          cardId: url.searchParams.get('card') || undefined,
          status: url.searchParams.get('open') === '1' ? 'open' : null,
        }));
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
        const scope = q.get('scope') === 'ready-across' || q.get('parallel') === '1' ? 'ready-across'
          : q.get('burndown') === '1' || q.get('scope') === 'burndown' ? 'burndown'
          : undefined;
        const limit = Number(q.get('limit') || (scope === 'ready-across' ? 50 : 5));
        return send(res, 200, db.nextCards(store.load(), { epoch: q.get('epoch') || undefined, track: q.get('track') || undefined, agent: q.get('agent') || undefined, limit, scope }));
      }
      // Docs — durable markdown under docs/ + pinned scratchpad.
      if (req.method === 'GET' && url.pathname === '/api/docs') {
        docs.migrateOwnerScratch(store.dataDir);
        docs.migrateScratchReports(store.dataDir);
        const q = url.searchParams;
        if (q.get('scratch') === '1') return send(res, 200, docs.showScratchPad(store.dataDir));
        if (q.get('path')) return send(res, 200, docs.showDoc(store.dataDir, q.get('path')));
        return send(res, 200, docs.listDocs(store.dataDir));
      }
      if (req.method === 'POST' && url.pathname === '/api/docs/add') {
        const p = await jsonBody(req);
        return send(res, 200, { ok: true, result: docs.addDoc(store.dataDir, p) });
      }
      if (req.method === 'POST' && url.pathname === '/api/docs/update') {
        const p = await jsonBody(req);
        if (p.scratch) return send(res, 200, { ok: true, result: docs.updateScratchPad(store.dataDir, p) });
        return send(res, 200, { ok: true, result: docs.updateDoc(store.dataDir, p.path, p) });
      }
      if (req.method === 'POST' && url.pathname === '/api/docs/delete') {
        const p = await jsonBody(req);
        return send(res, 200, { ok: true, result: docs.deleteDoc(store.dataDir, p.path) });
      }
      if (req.method === 'POST' && url.pathname === '/api/docs/archive') {
        const p = await jsonBody(req);
        return send(res, 200, { ok: true, result: docs.archiveDoc(store.dataDir, p.path) });
      }
      // #457 — durability sweeper, same rules as `tower lint`.
      if (req.method === 'GET' && url.pathname === '/api/lint') {
        const q = url.searchParams;
        const s = store.load();
        const history = store.loadHistory();
        const docsRoot = join(dirname(store.dataDir), 'docs');
        return send(res, 200, lint(s, history, { docs: q.get('docs') === '1', docsRoot }));
      }
      // #462 — one-shot agent work packet. ?card=&agent=&claim=0|1 (claim
      // only takes effect when both an agent AND claim=1 are given).
      if (req.method === 'GET' && url.pathname === '/api/brief') {
        const q = url.searchParams;
        const agent = q.get('agent') || undefined;
        const cardRef = q.get('card') || undefined;
        let s = store.load();
        let card = cardRef ? db.findCard(s, cardRef) : null;
        if (cardRef && !card) return send(res, 404, { error: 'E_NOT_FOUND', message: `no card ${cardRef}` });
        if (!card) {
          const picks = db.nextCards(s, { agent, limit: 1 });
          card = picks[0] && db.findCard(s, picks[0].id);
          if (!card) return send(res, 404, { error: 'E_NOT_FOUND', message: 'nothing agent-workable — board is either empty, blocked on the owner, or done' });
        }
        if (agent && q.get('claim') === '1') {
          const { state } = store.mutate((s2) => db.claimCard(s2, card.id, agent));
          s = state;
          card = db.findCard(s, card.id);
          broadcast(store);
        }
        return send(res, 200, db.buildBrief(s, card.id));
      }
      // ---- writes ----
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
      if (req.method === 'POST' && (url.pathname === '/api/acceptance/challenge' || url.pathname === '/api/acceptance/resolve')) {
        const p = await jsonBody(req);
        const route = url.pathname.slice(5);
        const reject = (message) => {
          auditAcceptanceReject(store, p.decisionId, route, message, 'owner-ui-rejected');
          return send(res, 403, { error: 'E_ACCEPTANCE_OWNER_UI', message });
        };
        if (!ownerTrusted(req, token)) return reject(token
          ? 'owner verification is accepted only from a direct loopback connection or this device\'s auth.token'
          : 'owner verification is accepted only from a direct loopback connection — set auth.token in .tower/secrets.json to allow a remote device to act as owner');
        if (req.headers['x-tower-owner-action'] !== 'verify') return reject('missing owner verification UI interaction marker');
        const session = ownerSession(req);
        if (!session) return reject('missing or expired owner UI session');

        if (url.pathname.endsWith('/challenge')) {
          const s = store.load();
          const d = s.decisions.find(x => x.id === p.decisionId);
          if (!d || d.status === 'ratified' || d.group !== 'acceptance' || !d.id.startsWith('D-ACCEPT-'))
            return reject('decision is not a live owner-verification ballot');
          if (p.outcome !== 'accept' && p.outcome !== 'bounce') return reject('acceptance outcome must be accept or bounce');
          const c = s.cards.find(x => x.id === d.cardId);
          if (!c || c.phase !== 'verify') return reject('owner-verification card is not in verify');
          const challenge = randomBytes(32).toString('base64url');
          acceptanceChallenges.set(challenge, { session: session.token, sessionAudit: session.auditId,
            challengeAudit: randomBytes(8).toString('base64url'), decisionId: d.id, outcome: p.outcome,
            expires: Date.now() + ACCEPTANCE_TTL_MS });
          return send(res, 200, { ok: true, result: { challenge, expiresInMs: ACCEPTANCE_TTL_MS } });
        }

        const challenge = acceptanceChallenges.get(p.challenge);
        if (p.challenge) acceptanceChallenges.delete(p.challenge); // consume before every validation: one attempt only
        if (!challenge || challenge.expires < Date.now()) return reject('missing, expired, or replayed owner-verification challenge');
        if (challenge.session !== session.token) return reject('owner-verification challenge belongs to another UI session');
        if (challenge.decisionId !== p.decisionId) return reject('owner-verification challenge is bound to another decision');
        if (challenge.outcome !== p.outcome) return reject('owner-verification challenge is bound to another outcome');
        const provenance = { kind: 'owner-ui', session: session.auditId, challenge: challenge.challengeAudit,
          issuedFor: challenge.decisionId, outcome: challenge.outcome, resolvedAt: new Date().toISOString() };
        const { result } = store.mutate((s) => resolveAcceptance(s, p.decisionId, p.outcome, p.comment, provenance));
        broadcast(store);
        return send(res, 200, { ok: true, result, state: projected(store) });
      }
      if (req.method === 'POST' && url.pathname.startsWith('/api/')) {
        const name = url.pathname.slice(5);
        const fn = routes[name];
        if (!fn) return send(res, 404, { error: 'E_USAGE', message: `unknown route ${name}` });
        const p = await jsonBody(req);
        if (name === 'clearance' || name === 'clearance/batch') {
          const ids = name === 'clearance' ? [p.decisionId] : (p.decisions || []).map(d => d.decisionId);
          const acceptance = ids.filter(id => {
            const d = store.load().decisions.find(x => x.id === id);
            return d && (d.group === 'acceptance' || d.id.startsWith('D-ACCEPT-'));
          });
          if (acceptance.length) {
            for (const id of acceptance) auditAcceptanceReject(store, id, name, 'generic clearance cannot resolve owner verification', p.by);
            return send(res, 403, { error: 'E_ACCEPTANCE_OWNER_UI', message: 'owner-verification ballots require the dedicated owner UI action' });
          }
        }
        if (name === 'card/update') {
          const s = store.load();
          const c = db.findCard(s, p.id);
          const ballot = c && s.decisions.find(d => d.cardId === c.id && d.group === 'acceptance' && d.status !== 'ratified');
          const clearsFlag = 'needsAcceptance' in p && !(p.needsAcceptance === true || p.needsAcceptance === 'true');
          if ((c?.needsAcceptance && p.phase === 'done' && p.by === 'owner') || (ballot && clearsFlag)) {
            const id = ballot?.id || `D-ACCEPT-${c.num}`;
            auditAcceptanceReject(store, id, name, 'caller-supplied by:owner or flag clearing cannot bypass owner verification', p.by);
            return send(res, 403, { error: 'E_ACCEPTANCE_OWNER_UI', message: 'owner verification requires the dedicated owner UI action' });
          }
        }
        const { result } = store.mutate((s, cfg) => fn(s, p, cfg), { expectRev: p.expectRev });
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
    console.log(`\n  ▲ Tower — ${store.config.project} — ${url}\n    data: ${store.file}${token ? `\n    remote access enabled; unlock key is in .tower/secrets.json` : ''}\n`);
    if (open) import('node:child_process').then(({ spawn }) => {
      const cmd = process.platform === 'darwin' ? 'open' : 'xdg-open';
      spawn(cmd, [url], { stdio: 'ignore', detached: true }).unref();
    });
  });
  return server;
}
