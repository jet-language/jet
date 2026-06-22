// HTTP server. Serves the static UI (app/ui/*) and a small JSON API over the
// board + docs. Std-only http; no framework.
import { createServer } from "node:http";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { P, UI, ROOT, read, rel, now, stamp, C, out } from "./paths.mjs";
import { loadBoard, saveBoard, makeCard, STAGES, PRIORITIES, normalizeWorkOrder } from "./board.mjs";
import { buildState, resolveDoc } from "./state.mjs";
import { renderMd } from "./markdown.mjs";
import { writeResults, queueRegen, recordQuestion, queueIngest } from "./writes.mjs";

const MIME = { ".html": "text/html; charset=utf-8", ".css": "text/css; charset=utf-8", ".js": "text/javascript; charset=utf-8" };
const STATIC = { "/": "index.html", "/index.html": "index.html", "/tower.css": "tower.css", "/tower.js": "tower.js" };

export function serve(port) {
  const json = (res, code, obj) => { res.writeHead(code, { "content-type": "application/json" }); res.end(JSON.stringify(obj)); };
  const server = createServer((req, res) => {
    if (req.method === "GET") {
      const path = req.url.split("?")[0];
      if (STATIC[path]) {
        const file = join(UI, STATIC[path]);
        if (!existsSync(file)) { res.writeHead(404); return res.end("missing ui asset"); }
        const ext = STATIC[path].slice(STATIC[path].lastIndexOf("."));
        res.writeHead(200, { "content-type": MIME[ext] || "text/plain" });
        return res.end(readFileSync(file));
      }
      if (path === "/api/state") return json(res, 200, buildState());
      res.writeHead(404); return res.end("not found");
    }
    if (req.method === "POST") {
      let data = "";
      req.on("data", (c) => (data += c));
      req.on("end", () => {
        let p = {};
        try { p = JSON.parse(data || "{}"); } catch { return json(res, 400, { ok: false, error: "bad json" }); }
        try { return handlePost(req.url, p, res, json); }
        catch (e) { return json(res, 500, { ok: false, error: String(e) }); }
      });
      return;
    }
    res.writeHead(404); res.end("not found");
  });
  server.listen(port, "127.0.0.1", () => {
    const url = `http://127.0.0.1:${port}`;
    out(`${C.grn}Tower${C.rst} → ${C.b}${url}${C.rst}`);
    out(`${C.dim}board: tools/Tower/board.json · ballot: tools/Tower/docs/ballots/decision-ballots.md · Ctrl-C to stop${C.rst}`);
    if (process.argv.includes("--open") || process.argv.includes("-o")) openBrowser(url);
  });
}

function handlePost(url, p, res, json) {
  const b = loadBoard();
  switch (url) {
    case "/api/card/add": {
      const card = makeCard(p);
      if (!card.title) return json(res, 400, { ok: false, error: "title required" });
      b.cards.push(card); saveBoard(b); return json(res, 200, { ok: true, card });
    }
    case "/api/card/update": {
      const c = b.cards.find((x) => x.id === p.id);
      if (!c) return json(res, 404, { ok: false, error: "no card" });
      if (p.stage && STAGES.includes(p.stage)) c.stage = p.stage;
      if (p.type && ["task", "idea", "bug"].includes(p.type)) c.type = p.type;
      if (p.priority && PRIORITIES.includes(String(p.priority).toUpperCase())) c.priority = String(p.priority).toUpperCase();
      if (Object.prototype.hasOwnProperty.call(p, "workOrder")) c.workOrder = normalizeWorkOrder(p.workOrder);
      if (typeof p.title === "string" && p.title.trim()) c.title = p.title.trim();
      if (typeof p.body === "string") c.body = p.body.trim();
      if (typeof p.plan === "string") c.plan = p.plan.trim() || null;
      if (Array.isArray(p.notes)) {
        c.notes = p.notes
          .filter((n) => n && typeof n.t === "string")
          .map((n) => ({ t: n.t.trim(), at: n.at || stamp() }))
          .filter((n) => n.t);
      }
      if (p.note && p.note.trim()) c.notes.push({ t: p.note.trim(), at: stamp() });
      c.updated = now(); saveBoard(b); return json(res, 200, { ok: true, card: c });
    }
    case "/api/card/delete": {
      b.cards = b.cards.filter((x) => x.id !== p.id); saveBoard(b); return json(res, 200, { ok: true });
    }
    case "/api/scratch": { b.scratch = p.text || ""; saveBoard(b); return json(res, 200, { ok: true }); }
    case "/api/submit": return json(res, 200, { ok: true, path: writeResults(p) });
    case "/api/regen": return json(res, 200, { ok: true, path: queueRegen(p.id, p.title) });
    case "/api/ask": {
      if (!p.text || !p.text.trim()) return json(res, 400, { ok: false, error: "empty" });
      return json(res, 200, { ok: true, q: recordQuestion(p.decisionId, p.text.trim()) });
    }
    case "/api/ingest": {
      if (!(p.source || "").trim() && !(p.note || "").trim())
        return json(res, 400, { ok: false, error: "need a file path or some text" });
      const item = queueIngest(p);
      return json(res, 200, { ok: true, item, path: rel(P.ingestQueue) });
    }
    case "/api/doc/get": {
      const f = resolveDoc(p.kind, p.slug);
      if (!f || !existsSync(f)) return json(res, 404, { ok: false, error: "no such doc" });
      const raw = read(f);
      const title = (raw.match(/^#\s+(.+)$/m) || [, p.slug])[1].trim();
      return json(res, 200, { ok: true, kind: p.kind, slug: p.slug, title, path: rel(f), raw, html: renderMd(raw) });
    }
    case "/api/doc/save": {
      const f = resolveDoc(p.kind, p.slug);
      if (!f) return json(res, 400, { ok: false, error: "bad doc id" });
      if (typeof p.text !== "string") return json(res, 400, { ok: false, error: "no text" });
      writeFileSync(f, p.text);
      return json(res, 200, { ok: true, html: renderMd(p.text), path: rel(f) });
    }
    default: return json(res, 404, { ok: false, error: "unknown endpoint" });
  }
}

function openBrowser(url) {
  const cmd = process.platform === "darwin" ? "open" : process.platform === "win32" ? "start" : "xdg-open";
  try { spawn(cmd, [url], { stdio: "ignore", detached: true }).unref(); } catch { /* best-effort */ }
}
