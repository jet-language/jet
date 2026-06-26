// Std-only HTTP server: static UI + a small JSON API over the store.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { join, extname, normalize } from 'node:path';
import { UI } from './paths.mjs';
import * as db from './store.mjs';

const MIME = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json', '.svg': 'image/svg+xml' };

const body = (req) => new Promise((res) => {
  let s = ''; req.on('data', c => (s += c)); req.on('end', () => { try { res(s ? JSON.parse(s) : {}); } catch { res({}); } });
});
const send = (res, code, obj) => { res.writeHead(code, { 'content-type': 'application/json' }); res.end(JSON.stringify(obj)); };

async function serveStatic(req, res) {
  let p = req.url.split('?')[0];
  if (p === '/') p = '/index.html';
  const file = join(UI, normalize(p).replace(/^(\.\.[/\\])+/, ''));
  if (!file.startsWith(UI)) { res.writeHead(403); return res.end(); }
  try {
    const data = await readFile(file);
    res.writeHead(200, { 'content-type': MIME[extname(file)] || 'application/octet-stream' });
    res.end(data);
  } catch { res.writeHead(404); res.end('not found'); }
}

const withSave = (fn) => (s, p) => { const r = fn(s, p); db.save(s); return r; };

const routes = {
  'card/add':       withSave((s, p) => db.addCard(s, p)),
  'card/update':    withSave((s, p) => db.updateCard(s, p.id, p)),
  'card/delete':    withSave((s, p) => { db.deleteCard(s, p.id); return { ok: true }; }),
  'card/activate':  withSave((s, p) => db.activate(s, p.id, p)),
  'clearance':      withSave((s, p) => db.clear(s, p.decisionId, p.outcome, p.comment)),
  'clearance/reopen': withSave((s, p) => db.reopenDecision(s, p.decisionId)),
  'decision/add':   withSave((s, p) => db.addDecision(s, p)),
  'question/add':   withSave((s, p) => db.addQuestion(s, p)),
  'question/answer': withSave((s, p) => db.answerQuestion(s, p.id, p.answer)),
  'question/delete': withSave((s, p) => { db.deleteQuestion(s, p.id); return { ok: true }; }),
  'binder/add':     withSave((s, p) => db.addBinder(s, p)),
  'binder/update':  withSave((s, p) => db.updateBinder(s, p.id, p)),
  'binder/delete':  withSave((s, p) => { db.deleteBinder(s, p.id); return { ok: true }; }),
  'binder/promote': withSave((s, p) => db.promote(s, p.id, p)),
  'ui/toggle':      withSave((s, p) => db.toggleOpen(s, p.key)),
  'epoch/current':  withSave((s, p) => { s.meta.currentEpoch = p.epoch; return s.meta; }),
};

export function serve(port = 7878, open = false) {
  const server = createServer(async (req, res) => {
    if (req.method === 'GET' && req.url.startsWith('/api/state')) {
      return send(res, 200, db.project(db.load()));
    }
    if (req.method === 'POST' && req.url.startsWith('/api/')) {
      const name = req.url.slice(5).split('?')[0];
      const fn = routes[name];
      if (!fn) return send(res, 404, { error: 'unknown route' });
      const s = db.load();
      const result = fn(s, await body(req));
      return send(res, 200, { ok: true, result, state: db.project(db.load()) });
    }
    if (req.method === 'GET') return serveStatic(req, res);
    res.writeHead(405); res.end();
  });
  server.listen(port, () => {
    const url = `http://localhost:${port}`;
    console.log(`\n  ▲ Tower v2 airborne — ${url}\n`);
    if (open) import('node:child_process').then(({ spawn }) => {
      const cmd = process.platform === 'darwin' ? 'open' : 'xdg-open';
      spawn(cmd, [url], { stdio: 'ignore', detached: true }).unref();
    });
  });
  return server;
}
