// Std-only HTTP server: static UI + JSON API + SSE live stream.
//
// Network trust: Tower is intentionally keyless for private-network use. The
// request must come from a private/link-local client and carry a normal local
// host name or private address; DNS-rebinding and public-host requests reject.
// State-changing requests also need browser same-origin evidence or the
// explicit non-browser `X-Tower-Client: cli` channel.
//
// Live: every mutation broadcasts the projected state over /api/stream
// (SSE). Web push / VAPID removed (owner D-VERDICT-460-1, 2026-07-14).
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { isIP } from 'node:net';
import { hostname as machineHostname } from 'node:os';
import { join, extname, normalize, dirname, relative, isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash, randomBytes } from 'node:crypto';
import { gzipSync } from 'node:zlib';
import { UI, projectRoot, readLatestJSON } from './paths.mjs';
import * as db from './store.mjs';
import { TowerError } from './store.mjs';
import { lint } from './lint.mjs';
import { computeVersion } from './version.mjs';
import * as docs from './docs.mjs';

const MIME = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.webmanifest': 'application/manifest+json' };
const MEDIA_MIME = {
  '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg',
  '.webp': 'image/webp', '.gif': 'image/gif', '.webm': 'video/webm', '.mp4': 'video/mp4',
};

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

function etagFor(data, key = 'body') {
  const digest = createHash('sha256').update(data).digest('hex').slice(0, 16);
  return `"tower-${key}-${digest}"`;
}

export function encodeResponse(data, { revision = null, key = 'body', gzip = true } = {}) {
  const raw = Buffer.isBuffer(data) ? data : Buffer.from(String(data));
  const compressed = gzip ? gzipSync(raw) : raw;
  return { raw, compressed, etag: etagFor(raw, revision == null ? key : `rev-${revision}`) };
}

function sendBytes(req, res, code, data, { contentType = 'application/octet-stream', revision = null, cacheControl = 'private, no-cache' } = {}) {
  const raw = Buffer.isBuffer(data) ? data : Buffer.from(data);
  const etag = etagFor(raw, revision == null ? 'body' : `rev-${revision}`);
  const common = { 'content-type': contentType, etag, 'cache-control': cacheControl, vary: 'Accept-Encoding' };
  const supplied = String(req?.headers?.['if-none-match'] || '');
  if (supplied.split(',').map(x => x.trim()).includes(etag)) {
    res.writeHead(304, common);
    return res.end();
  }
  const acceptsGzip = /\bgzip\b/i.test(String(req?.headers?.['accept-encoding'] || ''));
  const compressed = acceptsGzip ? gzipSync(raw) : raw;
  if (acceptsGzip) common['content-encoding'] = 'gzip';
  common['content-length'] = compressed.length;
  res.writeHead(code, common);
  res.end(compressed);
}

const revisionOf = (obj) => obj?.meta?.rev ?? obj?.rev ?? obj?.state?.meta?.rev ?? null;
const send = (res, code, obj, options = {}) => sendBytes(
  res.__towerReq, res, code, Buffer.from(JSON.stringify(obj)),
  { contentType: 'application/json', revision: options.revision ?? revisionOf(obj) },
);

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
// #1738 — the highest store rev this process has served to its clients,
// tracked on the store handle (per data dir, survives a reopen). Every
// write route compares a fresh on-disk read against it and refuses with a
// conflict when another writer (CLI, other agent) advanced the store past
// what this server last saw — surfaced, never overwritten.
const noteSeen = (store, rev) => { if (rev != null && rev > (store.serveSeenRev ?? 0)) store.serveSeenRev = rev; };
const projectedStates = new WeakMap();
const projectState = (store, state) => {
  const cached = projectedStates.get(state);
  if (cached) return cached;
  const p = { ...db.projectBoard(state, store.config), boot: BOOT, cli: `node ${TOWER_BIN}` };
  noteSeen(store, p.meta?.rev);
  projectedStates.set(state, p);
  return p;
};
const projected = (store) => projectState(store, store.loadLive());
const closedProjected = (store) => {
  const pair = store.loadPair();
  return db.projectClosed(pair.state, store.config, pair.history);
};
const sseClients = new Set();
function broadcast(store, state = null) {
  if (!sseClients.size) return;
  const data = `data: ${JSON.stringify(state ? projectState(store, state) : projected(store))}\n\n`;
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
  'card/criteria-reopen': (s, p) => db.reopenCriterion(s, p.id, p.n, { reason: p.reason, by: p.by }),
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
  'papercut/add':    (s, p) => db.addPapercut(s, p),
  'papercut/resolve': (s, p) => db.resolvePapercut(s, p.id, p.by),
  'idea/add':        (s, p) => db.addIdea(s, p),
  'idea/update':     (s, p) => db.updateIdea(s, p.id, p),
  'idea/delete':     (s, p) => db.deleteIdea(s, p.id, p.by),
  'idea/promote':    (s, p, cfg) => db.promoteIdea(s, p.id, p, cfg),
  'epoch/add':       (s, p) => db.addEpoch(s, p),
  'epoch/update':    (s, p) => db.updateEpoch(s, p.id, p),
  'epoch/current':   (s, p) => db.setCurrentEpoch(s, p.epoch),
  'milestone/add':   (s, p) => db.addMilestone(s, p),
  'milestone/update': (s, p) => db.updateMilestone(s, p.id, p, p.by),
  'milestone/criteria-add':    (s, p) => db.addMilestoneCriterion(s, p.id, p.text, p.by),
  'milestone/criteria-meet':   (s, p) => db.meetMilestoneCriterion(s, p.id, p.n, { evidence: p.evidence, by: p.by }),
  'milestone/criteria-verify': (s, p) => db.verifyMilestoneCriterion(s, p.id, p.n, { evidence: p.evidence, by: p.by }),
  'milestone/criteria-reopen': (s, p) => db.reopenMilestoneCriterion(s, p.id, p.n, { reason: p.reason, by: p.by }),
  'milestone/verify': (s, p, cfg, history) => db.verifyMilestone(s, p.id, { evidence: p.evidence, by: p.by }, history.cards),
  'milestone/delete': (s, p) => db.deleteMilestone(s, p.id, p.by),
  'ui/toggle':       (s, p) => db.toggleOpen(s, p.key),
  'done/clear':      (s, p) => db.clearDoneQueue(s, p),
};

const STATUS = { E_NOT_FOUND: 404, E_INVALID: 400, E_USAGE: 400, E_CONFLICT: 409, E_CLAIMED: 409, E_CLOSE_READY: 409, E_NO_DATA: 500, E_CRITERIA: 409, E_CRITERIA_SELF: 400, E_MILESTONE: 409, E_MILESTONE_VERIFY: 400,
  E_BALLOT: 400, E_OWNER_ONLY: 403, E_OWNER_LANE: 403, E_ACCEPTANCE_OWNER_UI: 403, E_HAS_RATIFIED: 409, E_HANDOFF: 400 };

// ---- network trust + acceptance session ------------------------------------
const OWNER_COOKIE = (sessionToken) => `${OWNER_SESSION_COOKIE}=${sessionToken}; Path=/; Max-Age=${OWNER_SESSION_TTL_MS / 1000}; SameSite=Strict; HttpOnly`;
const cookieValue = (req, name) => new RegExp(`(?:^|;\\s*)${name}=([^;]+)`).exec(req.headers.cookie || '')?.[1];
const MACHINE_HOSTNAME = machineHostname().trim().toLowerCase().replace(/\.$/, '');
const LAN_SUFFIXES = ['.local', '.lan', '.home.arpa'];

function requestHostname(req) {
  const host = String(req.headers.host || '').trim();
  if (!host || /[\\/?#@]/.test(host)) return '';
  try {
    return new URL(`http://${host}`).hostname.replace(/^\[|\]$/g, '').toLowerCase().replace(/\.$/, '');
  } catch { return ''; }
}

function privateIPv4(host) {
  if (isIP(host) !== 4) return false;
  const parts = host.split('.').map(Number);
  const [a, b] = parts;
  return a === 10 || a === 127 || (a === 169 && b === 254)
    || (a === 172 && b >= 16 && b <= 31)
    || (a === 192 && b === 168);
}

function privateIPv6(host) {
  if (isIP(host) !== 6) return false;
  let normalized;
  try {
    normalized = new URL(`http://[${host}]`).hostname.slice(1, -1).toLowerCase();
  } catch { return false; }
  if (normalized === '::1' || /^(?:fc|fd)/.test(normalized) || /^fe[89ab]/.test(normalized))
    return true;
  const mapped = /^::ffff:([0-9a-f]{1,4}):([0-9a-f]{1,4})$/.exec(normalized);
  if (!mapped) return false;
  const high = Number.parseInt(mapped[1], 16);
  const low = Number.parseInt(mapped[2], 16);
  return privateIPv4(`${high >> 8}.${high & 0xff}.${low >> 8}.${low & 0xff}`);
}

function privateAddress(address) {
  const host = String(address || '').trim().replace(/^\[|\]$/g, '').toLowerCase();
  return privateIPv4(host) || privateIPv6(host);
}

function privateHostname(host) {
  if (!host || isIP(host) !== 0) return false;
  return host === 'localhost' || host === MACHINE_HOSTNAME
    || !host.includes('.')
    || LAN_SUFFIXES.some(suffix => host.endsWith(suffix) && host.length > suffix.length);
}

function hostAllowed(req) {
  const host = requestHostname(req);
  return !!host && (privateAddress(host) || privateHostname(host));
}

function forwardedAddresses(req) {
  const raw = String(req.headers['x-forwarded-for'] || '').trim();
  return raw ? raw.split(',').map(value => value.trim()).filter(Boolean) : [];
}

function networkTrusted(req) {
  if (!hostAllowed(req) || !privateAddress(req.socket.remoteAddress)) return false;
  return forwardedAddresses(req).every(privateAddress);
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

function ownerSessionTrusted(req) {
  return networkTrusted(req) && !!ownerSession(req);
}

function requestOrigin(req) {
  const host = String(req.headers.host || '').trim();
  if (!host) return null;
  try { return new URL(`http://${host}`).origin; }
  catch { return null; }
}

function sameOrigin(req) {
  const site = String(req.headers['sec-fetch-site'] || '').trim().toLowerCase();
  if (site && site !== 'same-origin') return false;
  const expected = requestOrigin(req);
  if (!expected) return false;

  const origin = String(req.headers.origin || '').trim();
  if (origin) {
    if (origin === 'null') return false;
    try { return new URL(origin).origin === expected; }
    catch { return false; }
  }

  const referer = String(req.headers.referer || '').trim();
  if (referer) {
    try { return new URL(referer).origin === expected; }
    catch { return false; }
  }
  return site === 'same-origin';
}

function cliChannel(req) {
  const client = String(req.headers['x-tower-client'] || '').trim().toLowerCase();
  if (client !== 'cli' || !networkTrusted(req)) return false;
  // A browser cannot attach this non-simple header cross-origin without a
  // preflight. Reject contradictory browser metadata if a raw client sends it.
  return !String(req.headers.origin || '').trim()
    && !String(req.headers.referer || '').trim()
    && !String(req.headers['sec-fetch-site'] || '').trim();
}

function csrfAllowed(req, res, url) {
  const mutates = req.method === 'POST'
    || (req.method === 'GET' && url.pathname === '/api/brief' && url.searchParams.get('claim') === '1');
  if (!mutates || sameOrigin(req) || cliChannel(req)) return true;
  send(res, 403, { error: 'E_CSRF', message: 'cross-origin mutation rejected' });
  return false;
}

function auditAcceptanceReject(store, decisionId, route, reason, by, ownerAuthenticated = false) {
  // Rejected owner-verification requests are still audit mutations. Do not let
  // an unauthenticated payload forge the actor on that event.
  const auditBy = by === 'owner' && !ownerAuthenticated ? undefined : by;
  store.mutate((s) => db.auditAcceptanceRejection(s, decisionId, route, reason, auditBy));
  broadcast(store);
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
      return sendBytes(req, res, 200, Buffer.from(html), {
        contentType: MIME['.html'], cacheControl: 'private, no-cache',
      });
    }
    return sendBytes(req, res, 200, data, {
      contentType: MIME[extname(file)] || 'application/octet-stream',
      cacheControl: 'public, no-cache',
    });
  } catch { res.writeHead(404); res.end('not found'); }
}

async function serveMedia(req, res, store, url) {
  if (req.method !== 'GET') { res.writeHead(405, { allow: 'GET' }); return res.end(); }
  let name;
  try { name = decodeURIComponent(url.pathname.slice('/media/'.length)); }
  catch { res.writeHead(400); return res.end('bad path'); }
  if (!name || name.includes('\\') || name.includes('\0') || name.startsWith('/')
    || name.split('/').some(part => part === '.' || part === '..')) {
    res.writeHead(403); return res.end('forbidden');
  }
  const project = projectRoot(store.dataDir);
  if (!project) { res.writeHead(404); return res.end('not found'); }
  const root = join(project, 'docs/proposals/visual-acceptance/media');
  const file = join(root, name);
  const rel = relative(root, file);
  const contentType = MEDIA_MIME[extname(file).toLowerCase()];
  if (!rel || rel.startsWith('..') || isAbsolute(rel) || !contentType) {
    res.writeHead(404); return res.end('not found');
  }
  try {
    return sendBytes(req, res, 200, await readFile(file), { contentType, cacheControl: 'private, no-cache' });
  } catch { res.writeHead(404); return res.end('not found'); }
}

export function serve(store, port = 7878, open = false) {
  // #1738 criterion 1: re-read the store before every board write; if the
  // on-disk rev moved past what this process last served, refuse with 409,
  // note the fresh rev, and broadcast so every client catches up — the
  // caller retries against current state instead of silently overwriting.
  const guardWrite = (res) => {
    const diskRev = store.loadLive().meta.rev;
    if (diskRev > (store.serveSeenRev ?? 0)) {
      noteSeen(store, diskRev);
      broadcast(store);
      send(res, 409, { error: 'E_CONFLICT', message: `another writer advanced the board to rev ${diskRev} — state refreshed, re-read and retry` });
      return false;
    }
    return true;
  };
  noteSeen(store, store.loadLive().meta.rev);

  const server = createServer(async (req, res) => {
    res.__towerReq = req;
    try {
      const url = new URL(req.url, 'http://x');
      if (!networkTrusted(req)) {
        return send(res, 403, { error: 'E_HOST', message: 'request is outside Tower\'s private-network Host boundary' });
      }
      if (!csrfAllowed(req, res, url)) return;

      // A trusted browser navigation establishes an HttpOnly, process-local
      // owner UI session. It is not a login or a bearer credential.
      if (req.method === 'GET' && (url.pathname === '/' || url.pathname === '/index.html') && !ownerSession(req)) {
        const sessionToken = randomBytes(32).toString('base64url');
        ownerSessions.set(sessionToken, { auditId: randomBytes(8).toString('base64url'), expires: Date.now() + OWNER_SESSION_TTL_MS });
        res.setHeader('set-cookie', OWNER_COOKIE(sessionToken));
      }

      if (url.pathname.startsWith('/media/')) return serveMedia(req, res, store, url);

      // ---- reads ----
      if (req.method === 'GET' && url.pathname === '/api/state') return send(res, 200, projected(store));
      if (req.method === 'GET' && url.pathname === '/api/gauntlet') {
        const root = projectRoot(store.dataDir);
        const file = root && join(root, 'gauntlet', 'status.json');
        if (!file) return send(res, 404, { error: 'E_NOT_FOUND', message: 'Gauntlet status is unavailable: project root is unknown' });
        let text;
        try {
          text = await readFile(file, 'utf8');
        } catch (error) {
          if (error.code === 'ENOENT') {
            return send(res, 404, { error: 'E_NOT_FOUND', message: `Gauntlet status is unavailable: ${file}` });
          }
          return send(res, 500, { error: 'E_INVALID', message: `Gauntlet status could not be read: ${error.message}` });
        }
        try {
          return send(res, 200, JSON.parse(text));
        } catch {
          return send(res, 500, { error: 'E_INVALID', message: `Gauntlet status is not valid JSON: ${file}` });
        }
      }
      if (req.method === 'GET' && url.pathname === '/api/card') {
        const pair = store.loadPair();
        const s = pair.state;
        const card = db.projectCard(s, url.searchParams.get('id') || url.searchParams.get('card'), pair.history);
        if (!card) return send(res, 404, { error: 'E_NOT_FOUND', message: `no card ${url.searchParams.get('id') || url.searchParams.get('card')}` });
        return send(res, 200, { rev: s.meta.rev, card }, { revision: s.meta.rev });
      }
      if (req.method === 'GET' && url.pathname === '/api/closed') {
        return send(res, 200, closedProjected(store));
      }
      // #522 — belt+braces to the self-restart watcher: `start` is what
      // THIS process loaded at boot; `current` is a fresh read of what's on
      // disk right now. A mismatch means the process needs a restart.
      if (req.method === 'GET' && url.pathname === '/api/version') {
        const current = computeVersion(TOWER_ROOT);
        return send(res, 200, { start: START_VERSION, current, stale: current !== START_VERSION });
      }
      if (req.method === 'GET' && url.pathname === '/api/events') {
        const s = store.loadLive();
        return send(res, 200, s.events.slice(0, Number(url.searchParams.get('limit') || 50)), { revision: s.meta.rev });
      }
      if (req.method === 'GET' && url.pathname === '/api/messages') {
        const s = store.loadLive();
        return send(res, 200, db.listMessages(s, {
          cardId: url.searchParams.get('card') || undefined,
          status: url.searchParams.has('status') ? url.searchParams.get('status') : 'open',
        }), { revision: s.meta.rev });
      }
      if (req.method === 'GET' && url.pathname === '/api/stream') {
        res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-store', connection: 'keep-alive' });
        res.write(`event: state\ndata: ${JSON.stringify(projected(store))}\n\n`);
        sseClients.add(res);
        const ping = setInterval(() => { try { res.write(': ping\n\n'); } catch { /* closed */ } }, 20_000);
        req.on('close', () => { clearInterval(ping); sseClients.delete(res); });
        return;
      }
      if (req.method === 'GET' && url.pathname === '/api/history') {
        // #461: archived (retired) cards, optionally filtered by epoch —
        // the Board UI's done-subgroup uses this for a lazy archived count.
        const epoch = url.searchParams.get('epoch');
        const pair = store.loadPair();
        const h = pair.history;
        const cards = epoch ? h.cards.filter(c => c.epoch === epoch) : h.cards;
        const revision = pair.state.meta.rev;
        if (url.searchParams.get('count') === '1') return send(res, 200, { count: cards.length }, { revision });
        return send(res, 200, { cards, count: cards.length }, { revision });
      }
      if (req.method === 'GET' && url.pathname === '/api/next') {
        const q = url.searchParams;
        const scope = q.get('scope') === 'ready-across' || q.get('parallel') === '1' ? 'ready-across'
          : q.get('burndown') === '1' || q.get('scope') === 'burndown' ? 'burndown'
          : undefined;
        const limit = Number(q.get('limit') || (scope === 'ready-across' ? 50 : 5));
        const s = store.loadLive();
        return send(res, 200, db.nextCards(s, { epoch: q.get('epoch') || undefined, track: q.get('track') || undefined, agent: q.get('agent') || undefined, limit, scope }), { revision: s.meta.rev });
      }
      // Docs — durable markdown under docs/ + pinned scratchpad.
      if (req.method === 'GET' && url.pathname === '/api/docs') {
        const q = url.searchParams;
        if (q.get('scratch') === '1') return send(res, 200, docs.showScratchPad(store.dataDir));
        if (q.get('path')) return send(res, 200, docs.showDoc(store.dataDir, q.get('path')));
        return send(res, 200, docs.listDocs(store.dataDir));
      }
      if (req.method === 'GET' && url.pathname === '/api/guidance') {
        return send(res, 200, docs.showOwnerGuidance(store.dataDir));
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
      // #457 — durability sweeper, same rules as `tower lint`.
      if (req.method === 'GET' && url.pathname === '/api/lint') {
        const q = url.searchParams;
        const pair = store.loadPair();
        const s = pair.state;
        const history = pair.history;
        const docsRoot = join(projectRoot(store.dataDir), 'docs');
        return send(res, 200, lint(s, history, { docs: q.get('docs') === '1', docsRoot }), { revision: s.meta.rev });
      }
      // #462 — one-shot agent work packet. ?card=&agent=&claim=0|1 (claim
      // only takes effect when both an agent AND claim=1 are given).
      if (req.method === 'GET' && url.pathname === '/api/brief') {
        const q = url.searchParams;
        const agent = q.get('agent') || undefined;
        const cardRef = q.get('card') || undefined;
        let s = store.loadLive();
        let card = cardRef ? db.findCard(s, cardRef) : null;
        if (cardRef && !card) return send(res, 404, { error: 'E_NOT_FOUND', message: `no card ${cardRef}` });
        if (!card) {
          const picks = db.nextCards(s, { agent, limit: 1 });
          card = picks[0] && db.findCard(s, picks[0].id);
          if (!card) return send(res, 404, { error: 'E_NOT_FOUND', message: 'nothing agent-workable — board is either empty, blocked on the owner, or done' });
        }
        if (agent && q.get('claim') === '1') {
          if (!guardWrite(res)) return;
          const { state } = store.mutate((s2) => db.claimCard(s2, card.id, agent));
          s = state;
          card = db.findCard(s, card.id);
          broadcast(store, state);
        }
        return send(res, 200, db.buildBrief(s, card.id), { revision: s.meta.rev });
      }
      // ---- writes ----
      if (req.method === 'POST' && url.pathname === '/api/guidance/update') {
        const p = await jsonBody(req);
        return send(res, 200, { ok: true, result: docs.updateOwnerGuidance(store.dataDir, p) });
      }
      if (req.method === 'POST' && url.pathname === '/api/undo') {
        const p = await jsonBody(req);
        // #1738: a whole-board replace must prove which rev it thinks it is
        // undoing — an expectRev-less undo from a stale tab is an overwrite.
        if (p.expectRev == null) return send(res, 400, { error: 'E_USAGE', message: 'undo requires expectRev — read /api/state and pass its meta.rev' });
        if (!guardWrite(res)) return;
        const bdir = join(store.dataDir, 'backups');
        const prev = readLatestJSON(bdir, 'tower-', null);
        if (prev === null) return send(res, 400, { error: 'E_INVALID', message: 'nothing to undo (no backups yet)' });
        const { state } = store.restore(prev, { expectRev: p.expectRev });
        broadcast(store, state);
        return send(res, 200, { ok: true, state: projectState(store, state) });
      }
      if (req.method === 'POST' && (url.pathname === '/api/acceptance/challenge' || url.pathname === '/api/acceptance/resolve')) {
        const p = await jsonBody(req);
        const route = url.pathname.slice(5);
        const reject = (message) => {
          auditAcceptanceReject(store, p.decisionId, route, message, 'owner-ui-rejected');
          return send(res, 403, { error: 'E_ACCEPTANCE_OWNER_UI', message });
        };
        // Host trust is enforced once for every request above. The dedicated
        // session check below is the only acceptance owner-authentication gate.
        if (req.headers['x-tower-owner-action'] !== 'verify') return reject('missing owner verification UI interaction marker');
        const session = ownerSession(req);
        if (!session) return reject('missing or expired owner UI session');

        if (url.pathname.endsWith('/challenge')) {
          const s = store.loadLive();
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
        if (!guardWrite(res)) return;
        const { result, state } = store.mutate((s) => resolveAcceptance(s, p.decisionId, p.outcome, p.comment, provenance));
        broadcast(store, state);
        return send(res, 200, { ok: true, result, state: projectState(store, state) });
      }
      if (req.method === 'POST' && url.pathname.startsWith('/api/')) {
        const name = url.pathname.slice(5);
        const fn = routes[name];
        if (!fn) return send(res, 404, { error: 'E_USAGE', message: `unknown route ${name}` });
        const p = await jsonBody(req);
        if (name === 'clearance' || name === 'clearance/batch') {
          const ids = name === 'clearance' ? [p.decisionId] : (p.decisions || []).map(d => d.decisionId);
          const s = store.loadLive();
          const acceptance = ids.filter(id => {
            const d = s.decisions.find(x => x.id === id);
            return d && (d.group === 'acceptance' || d.id.startsWith('D-ACCEPT-'));
          });
          if (acceptance.length) {
            for (const id of acceptance) auditAcceptanceReject(store, id, name, 'generic clearance cannot resolve owner verification', p.by, ownerSessionTrusted(req));
            return send(res, 403, { error: 'E_ACCEPTANCE_OWNER_UI', message: 'owner-verification ballots require the dedicated owner UI action' });
          }
        }
        if (name === 'card/update') {
          const s = store.loadLive();
          const c = db.findCard(s, p.id);
          const ballot = c && s.decisions.find(d => d.cardId === c.id && d.group === 'acceptance' && d.status !== 'ratified');
          const clearsFlag = 'needsAcceptance' in p && !(p.needsAcceptance === true || p.needsAcceptance === 'true');
          if ((c?.needsAcceptance && p.phase === 'done' && p.by === 'owner') || (ballot && clearsFlag)) {
            const id = ballot?.id || `D-ACCEPT-${c.num}`;
            auditAcceptanceReject(store, id, name, 'caller-supplied by:owner or flag clearing cannot bypass owner verification', p.by, ownerSessionTrusted(req));
            return send(res, 403, { error: 'E_ACCEPTANCE_OWNER_UI', message: 'owner verification requires the dedicated owner UI action' });
          }
        }
        if (!guardWrite(res)) return;
        const { result, state } = store.mutate((s, cfg, history) => fn(s, p, cfg, history), { expectRev: p.expectRev });
        broadcast(store, state);
        return send(res, 200, { ok: true, result, state: projectState(store, state) });
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
  const onListen = () => {
    const url = `http://localhost:${port}`;
    console.log(`\n  ▲ Tower — ${store.config.project} — ${url}\n    data: ${store.file}\n    network: private LAN only; no access key or public-host mode\n`);
    if (open) import('node:child_process').then(({ spawn }) => {
      const cmd = process.platform === 'darwin' ? 'open' : 'xdg-open';
      spawn(cmd, [url], { stdio: 'ignore', detached: true }).unref();
    });
  };
  server.listen(port, onListen);
  return server;
}
