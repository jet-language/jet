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
};

const STATUS = { E_NOT_FOUND: 404, E_INVALID: 400, E_USAGE: 400, E_CONFLICT: 409, E_CLAIMED: 409, E_NO_DATA: 500 };

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
      if (req.method === 'POST' && req.url.startsWith('/api/')) {
        const name = req.url.slice(5).split('?')[0];
        const fn = routes[name];
        if (!fn) return send(res, 404, { error: 'E_USAGE', message: `unknown route ${name}` });
        const p = await body(req);
        const { result } = store.mutate((s, cfg) => fn(s, p, cfg), { expectRev: p.expectRev });
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
