#!/usr/bin/env node
// Tower — the command surface for building Jet. Sequences the workflow:
//   far-horizon → pre-plan → planned → decisions → implementation → done
// plus the decision ballot and a bug list. No dependencies; pure node.
//
// Usage:
//   node tools/Tower/Tower.mjs serve [port] [--open]   # the dashboard (main UI)
//   node tools/Tower/Tower.mjs status                  # console snapshot
//   node tools/Tower/Tower.mjs new <slug> "Title"      # scaffold a sidequest plan
//
// State the owner inputs (tasks/bugs/notes/decision answers/questions) lives in
// tools/Tower/board.json — management state only; it references plan files by
// slug and never copies their content, so the docs stay the single source of
// truth. The ballot renders straight from tools/Tower/docs/ballots/decision-ballots.md.

import { readFileSync, writeFileSync, readdirSync, existsSync, appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { createServer } from "node:http";
import { spawn } from "node:child_process";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const P = {
  sidequests: join(ROOT, "tools/Tower/docs/sidequests"),
  proposals: join(ROOT, "tools/Tower/docs/proposals"),
  plansDir: join(ROOT, "tools/Tower/docs/plans"),
  ballotMd: join(ROOT, "tools/Tower/docs/ballots/decision-ballots.md"),
  ratified: join(ROOT, "docs/spec/syntax-decisions.md"),
  results: join(ROOT, "tools/Tower/docs/ballots/ballot-results.md"),
  board: join(ROOT, "tools/Tower/board.json"),
  regenQueue: join(ROOT, "tools/Tower/regen-queue.md"),
  askQueue: join(ROOT, "tools/Tower/questions-queue.md"),
};

const read = (p) => (existsSync(p) ? readFileSync(p, "utf8") : "");
const C = { dim: "\x1b[2m", b: "\x1b[1m", grn: "\x1b[32m", yel: "\x1b[33m", cyn: "\x1b[36m", rst: "\x1b[0m" };
const now = () => new Date().toISOString();
const stamp = () => now().replace("T", " ").slice(0, 16);

// ---- board store -----------------------------------------------------------

// Pipeline stages, in order. Each is a distinct holding state:
//   far-horizon    — ideas kept for reference; not being pursued
//   pre-plan       — wanted, but a plan still needs to be generated
//   planned        — has an existing plan, not yet locked in for implementation
//   decisions      — blocked on a pending owner decision (see the Decisions tab)
//   implementation — actively being implemented
//   done           — shipped
const STAGES = ["far-horizon", "pre-plan", "planned", "decisions", "implementation", "done"];

function loadBoard() {
  if (!existsSync(P.board)) return { scratch: "", cards: [], questions: [] };
  try {
    const b = JSON.parse(read(P.board));
    b.scratch ??= ""; b.cards ??= []; b.questions ??= [];
    return b;
  } catch {
    return { scratch: "", cards: [], questions: [] };
  }
}
function saveBoard(b) { writeFileSync(P.board, JSON.stringify(b, null, 2) + "\n"); }
function newId() { return "c" + Date.now().toString(36) + Math.floor(Math.random() * 1e4).toString(36); }

// ---- readers (plans) -------------------------------------------------------

function readPlans() {
  if (!existsSync(P.sidequests)) return [];
  return readdirSync(P.sidequests)
    .filter((f) => f.endsWith(".md") && f.toLowerCase() !== "readme.md" && !/implementation-plan/i.test(f))
    .sort()
    .map((f) => {
      const md = read(join(P.sidequests, f));
      const title = (md.match(/^#\s+(.+)$/m) || [, f.replace(/\.md$/, "")])[1].trim();
      const status = (md.match(/^\*\*Status:\*\*\s*(.+)$/m) || [, ""])[1].trim();
      return { slug: f.replace(/\.md$/, ""), title, status };
    });
}

// Generic markdown-doc listing (proposals, etc.) — slug, title, status line.
function readDocList(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => f.endsWith(".md") && f.toLowerCase() !== "readme.md")
    .sort()
    .map((f) => {
      const md = read(join(dir, f));
      const title = (md.match(/^#\s+(.+)$/m) || [, f.replace(/\.md$/, "")])[1].trim();
      const status = (md.match(/^\*\*Status:\*\*\s*(.+)$/m) || [, ""])[1].trim();
      return { slug: f.replace(/\.md$/, ""), file: f, title, status };
    });
}

// Map a doc "kind" to its base dir. Only these are reachable from the web UI;
// resolveDoc refuses any slug that escapes its base (no "../" traversal).
const DOC_DIRS = { sidequest: P.sidequests, proposal: P.proposals, plan: P.plansDir };
function resolveDoc(kind, slug) {
  const base = DOC_DIRS[kind];
  if (!base || !slug) return null;
  const clean = String(slug).replace(/\.md$/, "");
  const file = resolve(base, clean + ".md");
  if (file !== base && !file.startsWith(base + "/")) return null; // containment
  return file;
}

function ratifiedCount() {
  const md = read(P.ratified);
  return new Set([...md.matchAll(/\b([DSU]-?[A-Z]*\d+[A-Z]*)\b/g)].map((m) => m[1])).size;
}

// ---- ballot parser (single source: decision-ballots.md) --------------------

const DECISION_ID = /^(D-[A-Z0-9-]+|S\d+-[A-Z]+|S\d+|N\d+|U\d+)$/;

// The "## Open decisions" section. Everything open lives here — no other
// sections to hide things behind.
function openSection(md) {
  const m = md.match(/^## Open decisions\s*$/m);
  if (!m) return "";
  const body = md.slice(m.index + m[0].length);
  const next = body.search(/^## /m);
  return next >= 0 ? body.slice(0, next) : body;
}

// Parse the open section into a flat, ordered list of entries. Tolerant of two
// shapes mixed freely:
//   • Full card:   "### <ID> — <title> (rec X)" with intro / Option bullets / Recommendation
//                  → kind:"decision", selectable.
//   • Group head:  "### <group name>" (no leading ID) whose "- **<ID>** — …"
//                  bullets each become kind:"open" (visible, ask-to-expand).
// Loose intro prose before the first "###" renders as a kind:"explainer".
function parseBallot(md) {
  const body = openSection(md);
  if (!body) return [];
  const blocks = [];
  let pre = [], cur = null;
  for (const line of body.split("\n")) {
    if (line.startsWith("### ")) {
      if (cur) blocks.push(cur); else if (pre.join("").trim()) blocks.push({ header: null, lines: pre });
      pre = null; cur = { header: line.slice(4).trim(), lines: [] };
    } else if (cur) cur.lines.push(line);
    else if (pre) pre.push(line);
  }
  if (cur) blocks.push(cur);

  const out = [];
  let group = "";
  for (const blk of blocks) {
    if (blk.header === null) {
      const html = renderMd(blk.lines.join("\n"));
      if (html.trim()) out.push({ kind: "explainer", title: "", html });
      continue;
    }
    const dash = blk.header.indexOf(" — ");
    const maybeId = dash > 0 ? blk.header.slice(0, dash).trim() : "";
    if (dash > 0 && DECISION_ID.test(maybeId)) {
      // full decision card — tagged with the last group heading we saw
      let title = blk.header.slice(dash + 3).trim();
      let rec = "";
      const rm = title.match(/\(([^)]*)\)\s*$/);
      if (rm) { rec = rm[1].trim(); title = title.slice(0, rm.index).trim(); }
      out.push({ kind: "decision", id: maybeId, group, title, rec, ...splitCard(blk.lines) });
    } else if (bulletItems(blk.lines).length) {
      // group header with one-liner bullets → emit ask-to-expand entries
      group = blk.header;
      for (const item of bulletItems(blk.lines)) {
        const m = item.match(/^\*\*([^*]+)\*\*\s*(?:—|-)?\s*([\s\S]*)$/);
        const id = m ? m[1].trim() : "";
        const rest = m ? m[2].trim() : item;
        out.push({ kind: "open", group, id, html: renderMd("- " + rest) });
      }
    } else {
      // bare group heading (its decisions follow as their own ### cards)
      group = blk.header;
    }
  }
  return out;
}

// Collect top-level "- " bullets, folding continuation/indented lines into them.
function bulletItems(lines) {
  const items = [];
  for (const raw of lines) {
    if (/^- /.test(raw)) items.push(raw.replace(/^- /, ""));
    else if (items.length && /^\s+\S/.test(raw)) items[items.length - 1] += " " + raw.trim();
  }
  return items;
}

function splitCard(lines) {
  const isOpt = (l) => /^- \*\*Option [A-Za-z0-9] —/.test(l);
  const isRec = (l) => /^\*\*Recommendation:/.test(l);
  const intro = [], options = [];
  let rec = [], mode = "intro", optBuf = null;
  const flushOpt = () => { if (optBuf) { options.push(finishOption(optBuf)); optBuf = null; } };
  for (const line of lines) {
    if (isRec(line)) { flushOpt(); mode = "rec"; rec.push(line); continue; }
    if (isOpt(line)) {
      flushOpt(); mode = "opt";
      const m = line.match(/^- \*\*Option ([A-Za-z0-9]) — (.+?)\*\*(.*)$/);
      const optName = m[2].replace(/\.\s*$/, "").replace(/\s*\(recommended\)\s*$/i, "").trim();
      optBuf = { key: m[1], name: optName, lines: [m[3].trim()] };
      continue;
    }
    if (mode === "intro") intro.push(line);
    else if (mode === "opt") optBuf.lines.push(line);
    else rec.push(line);
  }
  flushOpt();
  return {
    intro: renderMd(intro.join("\n")),
    options,
    recommendation: renderMd(rec.join("\n").replace(/^\*\*Recommendation:\*\*/, "").trim()),
  };
}
function finishOption(o) { return { key: o.key, name: o.name, html: renderMd(o.lines.join("\n").trim()) }; }

// ---- minimal markdown -> HTML ----------------------------------------------

function escapeHtml(s) { return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;"); }

function inline(s) {
  return escapeHtml(s)
    .replace(/`([^`]+)`/g, (_, c) => `<code>${c}</code>`)
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/\*([^*]+)\*/g, "<em>$1</em>");
}

function renderMd(md) {
  const lines = md.split("\n");
  const out = [];
  let i = 0;
  const para = [];
  const flushPara = () => { if (para.length) { out.push(`<p>${inline(para.join(" "))}</p>`); para.length = 0; } };
  while (i < lines.length) {
    const line = lines[i];
    const fence = line.match(/^(\s*)```(\w+)?\s*$/);
    if (fence) {
      flushPara();
      const indent = fence[1].length, lang = fence[2] || "", buf = [];
      i++;
      while (i < lines.length && !/^\s*```/.test(lines[i])) { buf.push(lines[i].slice(indent)); i++; }
      i++;
      out.push(`<pre class="code"><code>${highlight(buf.join("\n"), lang)}</code></pre>`);
      continue;
    }
    if (line.trim().startsWith("|") && i + 1 < lines.length && /^\s*\|?\s*:?-{2,}/.test(lines[i + 1])) {
      flushPara();
      const rows = [];
      while (i < lines.length && lines[i].trim().startsWith("|")) { rows.push(lines[i]); i++; }
      out.push(renderTable(rows));
      continue;
    }
    if (/^\s*-\s+/.test(line)) {
      flushPara();
      const items = [];
      while (i < lines.length && /^\s*-\s+/.test(lines[i])) { items.push(`<li>${inline(lines[i].replace(/^\s*-\s+/, ""))}</li>`); i++; }
      out.push(`<ul>${items.join("")}</ul>`);
      continue;
    }
    const h = line.match(/^(#{2,6})\s+(.+)$/);
    if (h) { flushPara(); out.push(`<h4>${inline(h[2])}</h4>`); i++; continue; }
    if (line.trim() === "") { flushPara(); i++; continue; }
    para.push(line.trim());
    i++;
  }
  flushPara();
  return out.join("\n");
}

function renderTable(rows) {
  const cells = (r) => r.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((c) => c.trim());
  const head = cells(rows[0]);
  const body = rows.slice(2).map(cells);
  const th = head.map((c) => `<th>${inline(c)}</th>`).join("");
  const trs = body.map((r) => `<tr>${r.map((c) => `<td>${inline(c)}</td>`).join("")}</tr>`).join("");
  return `<table><thead><tr>${th}</tr></thead><tbody>${trs}</tbody></table>`;
}

// ---- syntax highlighter (multi-language) -----------------------------------

const KEYWORDS = new Set(
  ("fn struct enum trait impl use module pub return self mut take const if else loop break continue " +
   "for in while match when new init derive let var val error ok value " +
   "func type package import range defer go chan map interface " +
   "comptime class extension protocol guard def").split(/\s+/)
);

function highlight(code) { return code.split("\n").map(highlightLine).join("\n"); }

function highlightLine(line) {
  let comment = "";
  const cidx = line.indexOf("//");
  let codePart = line;
  if (cidx >= 0) { codePart = line.slice(0, cidx); comment = line.slice(cidx); }
  let html = "";
  const re = /("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')/g;
  let last = 0, m;
  while ((m = re.exec(codePart)) !== null) {
    html += highlightCode(codePart.slice(last, m.index));
    html += `<span class="s">${escapeHtml(m[0])}</span>`;
    last = m.index + m[0].length;
  }
  html += highlightCode(codePart.slice(last));
  if (comment) html += `<span class="c">${escapeHtml(comment)}</span>`;
  return html;
}

function highlightCode(s) {
  return escapeHtml(s)
    .replace(/\b([A-Za-z_][A-Za-z0-9_]*)\b/g, (w) =>
      KEYWORDS.has(w) ? `<span class="k">${w}</span>`
      : /^[A-Z]/.test(w) ? `<span class="t">${w}</span>` : w)
    .replace(/\b(\d+\.?\d*)\b/g, '<span class="n">$1</span>');
}

// ---- ballot-results parser (the merge target) ------------------------------

// Parse ballot-results.md into an ordered map: id → {id, title, choice, comment}.
// Keyed off the "**<ID>** — <title>" / "Decision: **<choice>**" / "Comment: …"
// blocks that writeResults emits — same shape, round-trips cleanly.
function parseResults(md) {
  const map = new Map();
  let cur = null;
  for (const raw of md.split("\n")) {
    const idm = raw.match(/^\*\*([^*]+)\*\*\s*—\s*(.*)$/);
    if (idm) { cur = { id: idm[1].trim(), title: idm[2].trim(), choice: "", comment: "" }; map.set(cur.id, cur); continue; }
    if (!cur) continue;
    const dm = raw.match(/^Decision:\s*\*\*(.+?)\*\*\s*$/);
    if (dm) { cur.choice = dm[1].trim(); continue; }
    const cm = raw.match(/^Comment:\s*(.*)$/);
    if (cm) { cur.comment = cm[1].trim(); continue; }
  }
  return map;
}

function answeredIds() {
  return new Set([...parseResults(read(P.results)).keys()]);
}

// ---- state for the page ----------------------------------------------------

function buildState() {
  const md = read(P.ballotMd);
  const answered = answeredIds();
  // Hide already-submitted decisions/open items so the ballot only shows the
  // outstanding ones. ballot-results.md is the answered record (cleared by Claude).
  const ballot = parseBallot(md).filter((x) => !(x.id && answered.has(x.id)));
  return {
    board: loadBoard(),
    stages: STAGES,
    ballot,
    plans: readPlans(),
    proposals: readDocList(P.proposals),
    ratified: ratifiedCount(),
    lastSubmit: existsSync(P.results) ? (read(P.results).match(/_submitted (.+?)_/) || [, ""])[1] : "",
  };
}

// ---- write-backs -----------------------------------------------------------

// Submit MERGES into ballot-results.md — never overwrites. Existing decisions
// not in this submission are preserved; incoming ones add or replace by id.
function writeResults(payload) {
  const map = parseResults(read(P.results));
  for (const r of payload.results || []) {
    if (!r.id || !r.choice) continue; // client sends only answered, but guard anyway
    map.set(r.id, { id: r.id, title: r.title || r.id, choice: r.choice, comment: (r.comment || "").trim() });
  }
  const lines = [
    "# Owner ballot results", "", `_submitted ${stamp()}_`, "",
    'Decisions captured from Tower. Tell Claude **"go"** to ratify these',
    "into syntax-decisions.md, strip the cards, and implement the plans.", "", "## Decisions", "",
  ];
  for (const r of map.values()) {
    lines.push(`**${r.id}** — ${r.title}`);
    lines.push(`Decision: **${r.choice}**`);
    if (r.comment) lines.push(`Comment: ${r.comment}`);
    lines.push("");
  }
  writeFileSync(P.results, lines.join("\n"));
  return P.results.replace(ROOT + "/", "");
}

function queueRegen(id, title) {
  if (!existsSync(P.regenQueue)) {
    writeFileSync(P.regenQueue,
      "# Example-regeneration queue\n\nClaude reviews each open item against the example criteria " +
      "(human voice, plain language, a user-story scenario, inline cross-language comparison) and improves " +
      "the ballot card before the owner re-reads it. Checked = done.\n\n");
  }
  appendFileSync(P.regenQueue, `- [ ] ${id} — ${title}  _(requested ${stamp()})_\n`);
}

function recordQuestion(decisionId, text) {
  const b = loadBoard();
  const q = { id: newId(), decisionId, text, status: "open", answer: "", created: now() };
  b.questions.push(q);
  saveBoard(b);
  if (!existsSync(P.askQueue)) {
    writeFileSync(P.askQueue,
      "# Owner questions queue\n\nQuestions the owner asked from the dashboard. Claude answers each — either " +
      "replying in board.json (shown back on the card) or updating the ballot card itself — then marks it done.\n\n");
  }
  appendFileSync(P.askQueue, `- [ ] **${decisionId}**: ${text}  _(asked ${stamp()}, id ${q.id})_\n`);
  return q;
}

// ---- server ----------------------------------------------------------------

function serve(port) {
  const json = (res, code, obj) => { res.writeHead(code, { "content-type": "application/json" }); res.end(JSON.stringify(obj)); };
  const server = createServer((req, res) => {
    if (req.method === "GET" && (req.url === "/" || req.url.startsWith("/?"))) {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" }); return res.end(page());
    }
    if (req.method === "GET" && req.url === "/api/state") return json(res, 200, buildState());
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
      const card = { id: newId(), type: p.type || "task", title: (p.title || "").trim(),
        body: (p.body || "").trim(), stage: STAGES.includes(p.stage) ? p.stage : "far-horizon",
        plan: p.plan || null, notes: [], created: now(), updated: now() };
      if (!card.title) return json(res, 400, { ok: false, error: "title required" });
      b.cards.push(card); saveBoard(b); return json(res, 200, { ok: true, card });
    }
    case "/api/card/update": {
      const c = b.cards.find((x) => x.id === p.id);
      if (!c) return json(res, 404, { ok: false, error: "no card" });
      if (p.stage && STAGES.includes(p.stage)) c.stage = p.stage;
      if (p.type && ["task", "idea", "bug"].includes(p.type)) c.type = p.type;
      if (typeof p.title === "string" && p.title.trim()) c.title = p.title.trim();
      if (typeof p.body === "string") c.body = p.body.trim();
      // Full replace lets the UI edit/delete past notes; empty notes drop out.
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
    case "/api/regen": { queueRegen(p.id, p.title); return json(res, 200, { ok: true, path: P.regenQueue.replace(ROOT + "/", "") }); }
    case "/api/ask": {
      if (!p.text || !p.text.trim()) return json(res, 400, { ok: false, error: "empty" });
      const q = recordQuestion(p.decisionId, p.text.trim()); return json(res, 200, { ok: true, q });
    }
    case "/api/doc/get": {
      const f = resolveDoc(p.kind, p.slug);
      if (!f || !existsSync(f)) return json(res, 404, { ok: false, error: "no such doc" });
      const raw = read(f);
      const title = (raw.match(/^#\s+(.+)$/m) || [, p.slug])[1].trim();
      return json(res, 200, { ok: true, kind: p.kind, slug: p.slug, title,
        path: f.replace(ROOT + "/", ""), raw, html: renderMd(raw) });
    }
    case "/api/doc/save": {
      const f = resolveDoc(p.kind, p.slug);
      if (!f) return json(res, 400, { ok: false, error: "bad doc id" });
      if (typeof p.text !== "string") return json(res, 400, { ok: false, error: "no text" });
      writeFileSync(f, p.text);
      return json(res, 200, { ok: true, html: renderMd(p.text), path: f.replace(ROOT + "/", "") });
    }
    default: return json(res, 404, { ok: false, error: "unknown endpoint" });
  }
}

// ---- the page --------------------------------------------------------------

function page() {
  return `<!DOCTYPE html>
<html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Tower — Jet mission control</title>
<style>
/* ============================================================
   TOWER — mission control for building Jet.
   Coherent dark UI. Layered ink surfaces, one indigo accent,
   restrained signal colors. The signature is a horizontal
   PIPELINE RIBBON: the six workflow stages flow across the
   hero as live, jumpable chevrons — the whole product at a
   glance. Collapsed sections stay informative (counts + preview).
   ============================================================ */
:root{
  --bg:#0b0e13;
  --s1:#151a22;        /* panel / card                 */
  --s2:#1d2531;        /* inset / option / nested       */
  --s3:#10141b;        /* recessed section shell        */
  --code:#0f141c;      /* code well (near bg, not black)*/
  --line:#262f3d;
  --line2:#384354;
  --ink:#eef2f8;        /* bright primary                */
  --ink2:#aab7c8;
  --ink3:#6f7c8e;
  --accent:#74a9ff;     /* the one accent — indigo       */
  --accent2:#1d3358;    /* accent fill                   */
  --amber:#f0c562;
  --amber2:#352a10;
  --green:#67c97a;
  --green2:#163524;
  --red:#ff7c72;
  --red2:#3a1d1d;
  --r:9px;
}
*{box-sizing:border-box;margin:0;padding:0}
html{-webkit-text-size-adjust:100%}
body{
  background:var(--bg);color:var(--ink);
  font:14px/1.55 ui-sans-serif,-apple-system,"Segoe UI",Roboto,sans-serif;
  padding:0 0 92px;-webkit-font-smoothing:antialiased;
}
.mono{font-family:ui-monospace,"SF Mono",Menlo,Consolas,monospace}
::selection{background:#2c4a78}

/* ---------- hero: callsign + pipeline ribbon ---------- */
header{position:sticky;top:0;z-index:30;background:#0b0e13f2;backdrop-filter:blur(10px);border-bottom:1px solid var(--line)}
.hero{max-width:1180px;margin:0 auto;padding:15px 26px 0}
.brandrow{display:flex;align-items:center;gap:13px;flex-wrap:wrap}
.wm{font:800 16px/1 ui-monospace,monospace;letter-spacing:.36em;color:var(--ink);text-transform:uppercase;padding:6px 11px 6px 14px;border:1px solid var(--line2);border-radius:6px;background:var(--s1)}
.tagline{font:600 11px/1 ui-monospace,monospace;letter-spacing:.16em;text-transform:uppercase;color:var(--ink3)}
.status{margin-left:auto;display:flex;align-items:center;gap:20px;font:600 10.5px/1 ui-monospace,monospace;letter-spacing:.13em;text-transform:uppercase;color:var(--ink3)}
.status b{color:var(--ink);font-weight:800}
.live{display:inline-flex;align-items:center;gap:7px}
.live i{width:7px;height:7px;border-radius:50%;background:var(--green);box-shadow:0 0 0 3px #67c97a22;animation:beat 2.6s infinite}
@keyframes beat{0%,100%{opacity:1}50%{opacity:.3}}

/* the signature: a flowing pipeline of the seven stages */
.ribbon{display:flex;gap:0;margin:14px 0 0;overflow-x:auto;scrollbar-width:none}
.ribbon::-webkit-scrollbar{display:none}
.seg{flex:1 1 0;min-width:104px;position:relative;padding:9px 14px 11px;cursor:pointer;background:#0000;border:none;text-align:left;color:inherit;font:inherit}
.seg:not(:last-child):after{content:"";position:absolute;right:-7px;top:50%;transform:translateY(-50%);border-left:7px solid var(--s3);border-top:7px solid #0000;border-bottom:7px solid #0000;z-index:2}
.seg .sg{display:block;font:700 9.5px/1.2 ui-monospace,monospace;letter-spacing:.1em;text-transform:uppercase;color:var(--ink3);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.seg .sc{display:block;font:800 19px/1 ui-monospace,monospace;color:var(--ink2);margin-top:5px}
.seg{background:var(--s3);border-right:1px solid var(--line)}
.seg:hover .sg{color:var(--ink2)}
.seg:hover .sc{color:var(--ink)}
.seg.hot .sc{color:var(--accent)}
.seg.hot .sg{color:var(--accent)}
.seg.done .sc{color:var(--green)}
.seg:first-child{border-radius:7px 0 0 7px}
.seg:last-child{border-radius:0 7px 7px 0;border-right:none}

/* ---------- tabs ---------- */
.tabs{display:flex;gap:2px;padding:13px 26px 0;max-width:1180px;margin:0 auto}
.tab{padding:10px 16px 12px;font:700 11.5px/1 ui-monospace,monospace;letter-spacing:.14em;text-transform:uppercase;color:var(--ink3);cursor:pointer;border-bottom:2px solid transparent;background:#0000}
.tab:hover{color:var(--ink2)}
.tab.on{color:var(--ink);border-bottom-color:var(--accent)}
.tab .n{display:inline-block;min-width:16px;text-align:center;margin-left:8px;font-size:10px;font-weight:800;color:var(--ink2);background:var(--s2);border-radius:9px;padding:1px 6px}
.tab.on .n{background:var(--accent2);color:var(--accent)}

main{max-width:1180px;margin:0 auto;padding:24px 26px}
.view{display:none}.view.on{display:block;animation:fade .2s ease}
@keyframes fade{from{opacity:0;transform:translateY(3px)}to{opacity:1;transform:none}}

.hint{font:600 12px/1.55 ui-monospace,monospace;letter-spacing:.02em;color:var(--ink3);margin:0 0 18px;padding-left:13px;border-left:2px solid var(--line2)}
.hint b{color:var(--ink2)}

/* ---------- file form ---------- */
.filebox{background:var(--s1);border:1px solid var(--line);border-radius:var(--r);padding:15px;margin-bottom:20px}
.filebox .lbl{font:700 10px/1 ui-monospace,monospace;letter-spacing:.18em;text-transform:uppercase;color:var(--ink3);margin-bottom:11px;display:block}
.filebox .r{display:flex;gap:9px;flex-wrap:wrap;align-items:center}
input,textarea,select.sel{background:var(--bg);border:1px solid var(--line2);color:var(--ink);border-radius:7px;padding:9px 11px;font:13px/1.4 ui-monospace,monospace;outline:none}
input::placeholder,textarea::placeholder{color:var(--ink3)}
input:focus,textarea:focus,select.sel:focus{border-color:var(--accent);box-shadow:0 0 0 3px #74a9ff22}
input.grow{flex:1;min-width:200px}
select.sel{text-transform:uppercase;font-weight:600;font-size:11px;letter-spacing:.05em;cursor:pointer}
textarea{width:100%;margin-top:9px;resize:vertical;min-height:44px}
button{background:var(--accent);border:none;color:#06101f;border-radius:7px;padding:9px 16px;font:800 11.5px/1 ui-monospace,monospace;letter-spacing:.09em;text-transform:uppercase;cursor:pointer;transition:filter .12s}
button:hover{filter:brightness(1.1)}
button.ghost{background:var(--s2);color:var(--ink2);border:1px solid var(--line2)}
button.ghost:hover{background:var(--line2);color:var(--ink)}
button.sm{padding:7px 12px;font-size:10.5px}
button:disabled{opacity:.45;cursor:default;filter:none}
button.go{background:var(--green);color:#04130a}

/* ---------- section (collapsible, informative when shut) ---------- */
.sec{margin-bottom:11px;border:1px solid var(--line);border-radius:var(--r);background:var(--s3);overflow:hidden}
.sec.hot{border-color:var(--accent2)}
.sechead{display:flex;align-items:center;gap:12px;cursor:pointer;padding:13px 16px;user-select:none}
.sechead:hover{background:#ffffff05}
.caret{font:700 11px/1 monospace;color:var(--ink3);transition:transform .15s,color .15s;flex-shrink:0}
.sec.open .caret{transform:rotate(90deg);color:var(--accent)}
.sname{font:700 12px/1 ui-monospace,monospace;letter-spacing:.13em;text-transform:uppercase;color:var(--ink2);flex-shrink:0}
.sec:hover .sname,.sec.open .sname{color:var(--ink)}
.count{font:700 10px/1 ui-monospace,monospace;color:var(--ink2);background:var(--s2);padding:3px 9px;border-radius:10px;flex-shrink:0}
.count.ok{background:var(--green2);color:var(--green)}
.preview{flex:1;font:500 11.5px/1.4 ui-monospace,monospace;color:var(--ink3);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
.sec.open .preview{opacity:0}
.secbody{display:none;padding:2px 14px 15px}
.sec.open .secbody{display:block}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(280px,1fr));gap:11px}
.empty{font:600 11px/1 ui-monospace,monospace;color:var(--ink3);padding:6px 2px;letter-spacing:.03em}

/* ---------- task / bug card ---------- */
.card{background:var(--s1);border:1px solid var(--line);border-radius:8px;padding:0;overflow:hidden;transition:border-color .12s;display:flex}
.card:hover{border-color:var(--line2)}
.card .edge{flex:0 0 3px}
.card.task .edge{background:var(--accent)}
.card.idea .edge{background:var(--amber)}
.card.bug .edge{background:var(--red)}
.card .body{flex:1;padding:12px 13px 11px;min-width:0}
.card .top{display:flex;align-items:flex-start;gap:8px}
.card .ttl{font-weight:600;font-size:13px;line-height:1.35;flex:1;color:var(--ink);word-break:break-word;border-radius:4px}
/* type picker (replaces the static badge — change a card's type inline) */
.card .typesel{font:700 8.5px/1 ui-monospace,monospace;letter-spacing:.1em;text-transform:uppercase;padding:3px 7px;border-radius:5px;flex-shrink:0;border:1px solid transparent;cursor:pointer;appearance:none;-webkit-appearance:none}
.card.task .typesel{background:var(--accent2);color:var(--accent)}
.card.idea .typesel{background:var(--amber2);color:var(--amber)}
.card.bug .typesel{background:var(--red2);color:var(--red)}
.card .typesel:focus{outline:none;border-color:currentColor}
.card .bd{font-size:11.5px;color:var(--ink2);margin-top:6px;white-space:pre-wrap;line-height:1.5;border-radius:4px;min-height:1.2em}
/* inline-editable fields show their edge only on hover/focus */
.card .ttl[contenteditable]:hover,.card .bd[contenteditable]:hover,.card .nt[contenteditable]:hover{box-shadow:inset 0 0 0 1px var(--line2)}
.card .ttl[contenteditable]:focus,.card .bd[contenteditable]:focus,.card .nt[contenteditable]:focus{outline:none;box-shadow:inset 0 0 0 1px var(--accent);background:var(--bg)}
.card .bd:empty:before{content:attr(data-ph);color:var(--ink3);font-style:italic}
.card .meta{display:flex;align-items:center;gap:7px;margin-top:11px;padding-top:9px;border-top:1px solid var(--line)}
/* direct-manipulation advance controls instead of a dropdown */
.nav{display:flex;align-items:center;gap:2px}
.nav button{background:var(--s2);border:1px solid var(--line2);color:var(--ink2);border-radius:6px;padding:4px 8px;font:700 11px/1 monospace;letter-spacing:0}
.nav button:hover:not(:disabled){background:var(--accent2);color:var(--accent);border-color:var(--accent2);filter:none}
/* stage jump dropdown sits between the arrows — pick a stage directly */
.nav .stagesel{font:700 9px/1 ui-monospace,monospace;letter-spacing:.06em;text-transform:uppercase;color:var(--ink2);background:var(--s2);border:1px solid var(--line2);border-radius:6px;padding:4px 5px;min-width:96px;text-align:center;cursor:pointer;appearance:none;-webkit-appearance:none}
.nav .stagesel:hover{color:var(--accent);border-color:var(--accent2)}
.nav .stagesel:focus{outline:none;border-color:var(--accent)}
.card .plan{font:600 10px/1 ui-monospace,monospace;color:var(--accent);text-decoration:none;border-bottom:1px solid var(--accent2);margin-left:2px}
.card .x{margin-left:auto;color:var(--ink3);cursor:pointer;font-size:13px;line-height:1;padding:2px 3px}
.card .x:hover{color:var(--red)}
.card .note-in{font:600 10px/1 ui-monospace,monospace;color:var(--accent);cursor:pointer;border-bottom:1px dotted var(--accent2)}
.card .notes{margin-top:8px;border-top:1px solid var(--line);padding-top:7px}
.card .note{display:flex;align-items:baseline;gap:5px;font-size:10.5px;color:var(--ink2);margin-top:3px;font-family:ui-monospace,monospace}
.card .note .nt{flex:1;border-radius:3px;word-break:break-word}
.card .notes .at{color:var(--ink3);flex-shrink:0}
.card .note .ndel{color:var(--ink3);cursor:pointer;flex-shrink:0;padding:0 2px}
.card .note .ndel:hover{color:var(--red)}

/* ---------- board filter bar ---------- */
.filterbar{display:flex;gap:9px;align-items:center;margin-bottom:16px}
.filterbar .grow{flex:1;min-width:160px}

/* ---------- decisions ---------- */
.ballotbar{position:sticky;top:0;z-index:5;display:flex;align-items:center;gap:14px;background:var(--s1);border:1px solid var(--line);border-radius:var(--r);padding:13px 16px;margin-bottom:18px}
.ballotbar .meter{flex:1;min-width:120px}
.ballotbar .mtop{display:flex;justify-content:space-between;font:700 11px/1 ui-monospace,monospace;letter-spacing:.08em;text-transform:uppercase;color:var(--ink2);margin-bottom:7px}
.ballotbar .mtop b{color:var(--accent)}
.track{height:6px;border-radius:3px;background:var(--s2);overflow:hidden}
.track i{display:block;height:100%;background:var(--accent);border-radius:3px;transition:width .25s}
.dgroup{margin-bottom:11px;border:1px solid var(--line);border-radius:var(--r);background:var(--s3);overflow:hidden}
.dcard{background:var(--s1);border:1px solid var(--line);border-radius:8px;margin-top:11px;overflow:hidden}
.dcard:first-child{margin-top:2px}
.dcard.decided{border-color:var(--green2)}
.dcard.explain{background:var(--s2);border-style:dashed}
.dhead{display:flex;align-items:center;gap:11px;padding:15px 18px 0;flex-wrap:wrap}
.did{font:800 11.5px/1 ui-monospace,monospace;letter-spacing:.08em;color:var(--ink);background:var(--s2);padding:5px 9px;border-radius:6px}
.rec{font:800 9px/1 ui-monospace,monospace;letter-spacing:.1em;padding:4px 8px;border-radius:5px;background:var(--green2);color:var(--green)}
.rec.no{background:var(--s2);color:var(--ink3)}
.ddecided{margin-left:auto;font:800 9px/1 ui-monospace,monospace;letter-spacing:.1em;color:var(--green);background:var(--green2);padding:4px 9px;border-radius:5px;display:none}
.dcard.decided .ddecided{display:inline-block}
.dttl{font-size:17px;font-weight:700;color:var(--ink);padding:10px 18px 2px;letter-spacing:-.2px}
.dbody{padding:4px 18px;font-size:13px;color:var(--ink2);line-height:1.6}
.dbody p{margin:9px 0}.dbody ul{margin:9px 0 9px 19px}.dbody li{margin:4px 0}
.dbody strong{color:var(--ink)}
.dbody table{border-collapse:collapse;margin:12px 0;width:100%;font-size:12px}
.dbody th,.dbody td{border:1px solid var(--line2);padding:7px 10px;text-align:left;vertical-align:top}
.dbody th{background:var(--s2);color:var(--ink)}
.opts{display:flex;flex-direction:column;gap:9px;padding:8px 18px 4px}
.opt{border:1px solid var(--line2);border-radius:8px;padding:13px 15px;cursor:pointer;background:var(--s2);transition:border-color .12s,background .12s}
.opt:hover{border-color:var(--accent)}
.opt.sel{border-color:var(--green);background:#13241a}
.opt-h{display:flex;align-items:center;gap:11px;font-weight:700;color:var(--ink);font-size:13.5px}
.radio{width:17px;height:17px;border:2px solid var(--ink3);border-radius:50%;flex-shrink:0;display:flex;align-items:center;justify-content:center}
.opt.sel .radio{border-color:var(--green)}
.opt.sel .radio:after{content:"";width:9px;height:9px;border-radius:50%;background:var(--green)}
.opt .dbody{padding:8px 0 0;color:var(--ink2)}
.opt .dbody p:first-child{margin-top:0}
pre.code{background:var(--code);border:1px solid var(--line);border-radius:7px;padding:12px 14px;overflow-x:auto;margin:9px 0;line-height:1.55}
pre.code code{font-family:ui-monospace,Menlo,monospace;font-size:12px;white-space:pre;color:#cdd6e2}
code{background:var(--s2);border-radius:5px;padding:1px 5px;font-family:ui-monospace,monospace;font-size:.87em;color:#cdd6e2}
.k{color:#ff9e7a}.t{color:#82c8ff}.s{color:#9fe0a0}.c{color:#5d6b7d;font-style:italic}.n{color:#d4a8ff}
.recline{padding:8px 18px 2px;font-size:12.5px;color:var(--ink2)}
.recline strong{color:var(--ink)}
textarea.comment{margin:6px 18px 0;width:calc(100% - 36px);background:var(--bg)}
.drow{display:flex;align-items:center;gap:10px;padding:13px 18px;flex-wrap:wrap;border-top:1px solid var(--line);margin-top:9px}
.clr{font:700 10.5px/1 ui-monospace,monospace;letter-spacing:.05em;text-transform:uppercase;color:var(--red);cursor:pointer;border-bottom:1px solid var(--red2);display:none}
.clr.on{display:inline-block}
.qbox{padding:4px 18px 14px}
.q{background:var(--s2);border:1px solid var(--line);border-radius:7px;padding:10px 12px;margin-top:8px;font-size:12px;color:var(--ink)}
.q .qa{color:var(--ink3);font-style:italic}
.q .st{font:800 8.5px/1 ui-monospace,monospace;letter-spacing:.08em;padding:2px 6px;border-radius:4px;margin-left:8px}
.q .st.open{background:var(--amber2);color:var(--amber)}.q .st.answered{background:var(--green2);color:var(--green)}
.q .ans{margin-top:6px;color:var(--ink2);border-left:2px solid var(--green);padding-left:9px}

/* ---------- scratch ---------- */
#scratch{width:100%;min-height:460px;font-family:ui-monospace,monospace;font-size:13px;line-height:1.65;background:var(--s1);color:var(--ink)}
.savebar{display:flex;align-items:center;gap:13px;margin-top:11px}
.savebar .s{font:600 11px/1 ui-monospace,monospace;color:var(--ink3);letter-spacing:.05em}

/* ---------- submit bar / toast ---------- */
.bar{position:fixed;left:0;right:0;bottom:0;z-index:35;background:#0b0e13f7;backdrop-filter:blur(10px);border-top:1px solid var(--line);padding:13px 26px;display:none;align-items:center;gap:18px}
.bar.on{display:flex}
.bar .inner{max-width:1180px;margin:0 auto;width:100%;display:flex;align-items:center;gap:16px}
.bar .p{flex:1;font:700 12px/1 ui-monospace,monospace;letter-spacing:.08em;text-transform:uppercase;color:var(--ink3)}
.bar .p b{color:var(--accent);font-size:15px}
.toast{position:fixed;bottom:84px;left:50%;transform:translateX(-50%);background:var(--s1);border:1px solid var(--line2);border-left:4px solid var(--green);color:var(--ink);padding:12px 18px;border-radius:7px;font:600 12px/1.4 ui-monospace,monospace;opacity:0;transition:opacity .2s;pointer-events:none;max-width:92%;z-index:50;box-shadow:0 8px 30px #0008}
.toast.on{opacity:1}

@media (max-width:640px){
  .grid{grid-template-columns:1fr}
  main,.hero,.tabs{padding-left:14px;padding-right:14px}
  .ribbon .seg{min-width:88px}
}
@media (prefers-reduced-motion:reduce){*{animation:none!important;transition:none!important}}

/* ---------- markdown doc viewer / editor (pops over everything) ---------- */
.docback{position:fixed;inset:0;z-index:60;background:#06080cdd;backdrop-filter:blur(5px);display:none;align-items:flex-start;justify-content:center;padding:38px 18px;overflow-y:auto}
.docback.on{display:flex}
.docmodal{background:var(--s1);border:1px solid var(--line2);border-radius:12px;width:100%;max-width:920px;margin:auto;box-shadow:0 26px 90px #000b;display:flex;flex-direction:column;max-height:calc(100vh - 76px);overflow:hidden;animation:pop .16s ease}
@keyframes pop{from{opacity:0;transform:translateY(8px) scale(.99)}to{opacity:1;transform:none}}
.dochead{display:flex;align-items:center;gap:14px;padding:14px 20px;border-bottom:1px solid var(--line);background:var(--s1);flex-shrink:0}
.docmeta{flex:1;min-width:0}
.doctitle{display:block;font:700 15px/1.3 ui-sans-serif,-apple-system,sans-serif;color:var(--ink)}
.docpath{display:block;font-size:10.5px;color:var(--ink3);margin-top:3px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.doctools{display:flex;gap:8px;flex-shrink:0}
.docbody{padding:22px 28px 34px;overflow-y:auto;font-size:13.5px;line-height:1.68;color:var(--ink2)}
.docbody>:first-child{margin-top:0}
.docbody h4{color:var(--ink);margin:20px 0 9px;font-size:15px;font-weight:700;letter-spacing:-.2px}
.docbody p{margin:10px 0}.docbody ul{margin:10px 0 10px 20px}.docbody li{margin:5px 0}
.docbody strong{color:var(--ink)}.docbody a{color:var(--accent)}
.docbody table{border-collapse:collapse;margin:13px 0;width:100%;font-size:12px}
.docbody th,.docbody td{border:1px solid var(--line2);padding:7px 10px;text-align:left;vertical-align:top}
.docbody th{background:var(--s2);color:var(--ink)}
.docedit{display:block;width:100%;margin:0;border:none;border-top:1px solid var(--line);border-radius:0;min-height:58vh;resize:vertical;font-family:ui-monospace,Menlo,monospace;font-size:12.5px;line-height:1.6;background:var(--code);color:var(--ink);padding:18px 22px}
.docedit:focus{box-shadow:none;border-color:var(--line)}
.pcard{cursor:pointer}.pcard .ttl{font-size:13.5px}.pcard:hover{border-color:var(--accent)}
.card .plan{cursor:pointer}
</style></head><body>
<header>
  <div class="hero">
    <div class="brandrow">
      <span class="wm">TOWER</span><span class="tagline">jet · mission control</span>
      <div class="status">
        <span class="live"><i></i>on station</span>
        <span>ratified <b id="rat">—</b></span>
        <span>last file <b id="last">—</b></span>
      </div>
    </div>
    <div class="ribbon" id="ribbon"></div>
  </div>
  <div class="tabs" id="tabs"></div>
</header>
<main>
  <section class="view" id="v-board"></section>
  <section class="view" id="v-proposals"></section>
  <section class="view" id="v-decisions"></section>
  <section class="view" id="v-scratch"></section>
</main>
<div class="bar" id="bar"><div class="inner"><div class="p" id="prog"></div><button class="ghost sm" onclick="jumpNext()">Next undecided ↓</button><button class="go" id="submit" onclick="submitBallot()">Sign &amp; file</button></div></div>
<div class="docback" id="docback" onclick="closeDoc(event)">
  <div class="docmodal" onclick="event.stopPropagation()">
    <div class="dochead">
      <div class="docmeta"><span class="doctitle" id="doc-title">—</span><span class="docpath mono" id="doc-path"></span></div>
      <div class="doctools">
        <button class="ghost sm" id="doc-edit" onclick="toggleEdit()">Edit</button>
        <button class="go sm" id="doc-save" onclick="saveDoc()" style="display:none">Save</button>
        <button class="ghost sm" id="doc-cancel" onclick="cancelEdit()" style="display:none">Cancel</button>
        <button class="ghost sm" onclick="closeDoc()">✕ Close</button>
      </div>
    </div>
    <div class="docbody" id="doc-view"></div>
    <textarea class="docedit" id="doc-edit-area" style="display:none" spellcheck="false"></textarea>
  </div>
</div>
<div class="toast" id="toast"></div>
<script>
let S=null;
const TABS=[['board','Board'],['proposals','Proposals'],['decisions','Decisions'],['scratch','Scratch']];
const answers={},comments={};
const sec={};                 /* collapse state by key — default closed */
const filter={q:'',type:'all'};   /* board filter — by name + type */
let active=location.hash.slice(1)||'board';

function esc(s){return (s||'').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');}
function attr(s){return esc(s).replace(/"/g,'&quot;');}
function key(s){return (s||'').replace(/[^a-z0-9]+/gi,'_');}
function toast(m){const t=document.getElementById('toast');t.textContent=m;t.classList.add('on');clearTimeout(t._);t._=setTimeout(()=>t.classList.remove('on'),3400);}
async function api(url,body){const r=await fetch(url,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)});return r.json();}
async function load(){S=await (await fetch('/api/state')).json();render();}

function render(){
  if(!TABS.some(t=>t[0]===active))active='board';
  const dec=S.ballot.filter(x=>x.kind==='decision');
  const cards=S.board.cards;
  const counts={board:cards.length,proposals:(S.proposals||[]).length,decisions:dec.length,scratch:0};
  document.getElementById('tabs').innerHTML=TABS.map(([id,label])=>
    '<div class="tab'+(id===active?' on':'')+'" onclick="go(\\''+id+'\\')">'+label+
    (counts[id]?'<span class="n">'+counts[id]+'</span>':'')+'</div>').join('');
  TABS.forEach(([id])=>document.getElementById('v-'+id).classList.toggle('on',id===active));
  document.getElementById('bar').classList.toggle('on',active==='decisions');
  document.getElementById('rat').textContent=S.ratified||'—';
  document.getElementById('last').textContent=S.lastSubmit||'—';
  renderRibbon(cards);renderBoard();renderProposals();renderDecisions();renderScratch();
}
function go(id){active=id;location.hash=id;render();window.scrollTo(0,0);}

/* ---- pipeline ribbon (the signature) ---- */
function renderRibbon(cards){
  const h=S.stages.map((st,i)=>{
    const n=cards.filter(c=>c.stage===st).length;
    const cls=st==='done'?'done':(n>0&&st!=='far-horizon'&&st!=='done'?'hot':'');
    return '<button class="seg '+cls+'" title="jump to '+st+'" onclick="jumpStage(\\''+st+'\\')">'+
      '<span class="sg">'+esc(st.replace(/-/g,' '))+'</span><span class="sc">'+n+'</span></button>';
  }).join('');
  document.getElementById('ribbon').innerHTML=h;
}
function jumpStage(st){if(active!=='board')go('board');sec['stage_'+key(st)]=true;render();setTimeout(()=>{const el=document.getElementById('sec-stage_'+key(st));if(el)el.scrollIntoView({behavior:'smooth',block:'start'});},30);}

/* ---- collapsible section shell ---- */
function section(skey,name,countHtml,preview,inner,extraCls){
  const isOpen=!!sec[skey];
  return '<div class="sec'+(isOpen?' open':'')+(extraCls?' '+extraCls:'')+'" id="sec-'+skey+'">'+
    '<div class="sechead" onclick="toggleSec(\\''+skey+'\\')">'+
      '<span class="caret">&#9656;</span><span class="sname">'+esc(name)+'</span>'+countHtml+
      '<span class="preview">'+preview+'</span></div>'+
    '<div class="secbody">'+inner+'</div></div>';
}
function toggleSec(k){sec[k]=!sec[k];const el=document.getElementById('sec-'+k);if(el)el.classList.toggle('open',sec[k]);}

/* ---- board ---- */
function renderBoard(){
  let h='<div class="hint">File a task, idea, or bug, then move it down the pipeline. Click any <b>title, description, or note</b> to edit it; use the dropdowns to change a card\\u2019s <b>type</b> or jump its <b>stage</b>.</div>';
  h+=filterBar();
  h+=fileForm();
  h+='<div id="board-stages"></div>';
  document.getElementById('v-board').innerHTML=h;
  renderStages();
}
function filterBar(){
  const types=['all','task','idea','bug'];
  return '<div class="filterbar">'+
    '<input class="grow" id="filter-q" placeholder="Filter by name\\u2026" value="'+attr(filter.q)+'" oninput="filter.q=this.value;renderStages()">'+
    '<select class="sel" id="filter-type" onchange="filter.type=this.value;renderStages()">'+
      types.map(t=>'<option value="'+t+'"'+(filter.type===t?' selected':'')+'>'+(t==='all'?'all types':t)+'</option>').join('')+
    '</select></div>';
}
function matchFilter(c){
  if(filter.type!=='all'&&c.type!==filter.type)return false;
  if(filter.q){const q=filter.q.toLowerCase();if(!((c.title||'').toLowerCase().includes(q)||(c.body||'').toLowerCase().includes(q)))return false;}
  return true;
}
function renderStages(){
  const cards=S.board.cards.filter(matchFilter);
  let h='';
  for(const st of S.stages){
    const inSt=cards.filter(c=>c.stage===st);
    const inner=inSt.length?'<div class="grid">'+inSt.map(card).join('')+'</div>':'<div class="empty">— empty —</div>';
    const prev=inSt.length?esc(inSt.slice(0,2).map(c=>c.title).join(' · ')+(inSt.length>2?' +'+(inSt.length-2):'')):'';
    h+=section('stage_'+key(st),st.replace(/-/g,' '),'<span class="count">'+inSt.length+'</span>',prev,inner);
  }
  const el=document.getElementById('board-stages');if(el)el.innerHTML=h;
}
function fileForm(){
  return '<div class="filebox"><span class="lbl">File new work</span><div class="r">'+
    '<input class="grow" id="add-ttl" placeholder="Task, idea, or bug\\u2026">'+
    '<select class="sel" id="add-type"><option value="task">task</option><option value="idea">idea</option><option value="bug">bug</option></select>'+
    '<select class="sel" id="add-stage">'+S.stages.map(s=>'<option'+(s==='far-horizon'?' selected':'')+'>'+s+'</option>').join('')+'</select>'+
    '<button class="sm" onclick="addCard()">File</button></div>'+
    '<textarea id="add-body" placeholder="Details (optional)"></textarea></div>';
}
async function addCard(){
  const t=document.getElementById('add-ttl');
  if(!t.value.trim())return;
  const j=await api('/api/card/add',{type:document.getElementById('add-type').value,title:t.value,body:document.getElementById('add-body').value,stage:document.getElementById('add-stage').value});
  if(j.ok){t.value='';document.getElementById('add-body').value='';await load();toast('Filed.');}
}
function findCard(id){return S.board.cards.find(c=>c.id===id);}
function card(c){
  const i=S.stages.indexOf(c.stage);
  const back=i>0?S.stages[i-1]:null,fwd=i<S.stages.length-1?S.stages[i+1]:null;
  const planLink=c.plan?'<a class="plan" href="#" title="open plan" onclick="openDoc(\\'sidequest\\',\\''+attr(c.plan)+'\\');return false;">▤ '+esc(c.plan)+'</a>':'';
  const typeSel='<select class="typesel" title="change type" onchange="changeType(\\''+c.id+'\\',this.value)">'+
    ['task','idea','bug'].map(t=>'<option'+(c.type===t?' selected':'')+'>'+t+'</option>').join('')+'</select>';
  const stageSel='<select class="stagesel" title="jump to stage" onchange="moveCard(\\''+c.id+'\\',this.value)">'+
    S.stages.map(s=>'<option value="'+s+'"'+(s===c.stage?' selected':'')+'>'+esc(s.replace(/-/g,' '))+'</option>').join('')+'</select>';
  const notes=(c.notes&&c.notes.length)?'<div class="notes">'+c.notes.map((n,idx)=>
    '<div class="note">•&nbsp;<span class="nt" contenteditable="true" spellcheck="false" data-id="'+c.id+'" data-i="'+idx+'" data-orig="'+attr(n.t)+'" onblur="saveNote(this)" onkeydown="fieldKey(event,this)">'+esc(n.t)+'</span>'+
    '<span class="at">'+esc(n.at)+'</span><span class="ndel" title="delete note" onclick="delNote(\\''+c.id+'\\','+idx+')">✕</span></div>').join('')+'</div>':'';
  return '<div class="card '+c.type+'"><div class="edge"></div><div class="body">'+
    '<div class="top"><span class="ttl" contenteditable="true" spellcheck="false" data-id="'+c.id+'" data-orig="'+attr(c.title)+'" onblur="saveField(this,\\'title\\')" onkeydown="fieldKey(event,this)">'+esc(c.title)+'</span>'+typeSel+'</div>'+
    '<div class="bd" contenteditable="true" spellcheck="false" data-ph="'+(c.type==='bug'?'Describe the defect\\u2026':'Add a description\\u2026')+'" data-id="'+c.id+'" data-orig="'+attr(c.body||'')+'" onblur="saveField(this,\\'body\\')" onkeydown="fieldKey(event,this)">'+esc(c.body||'')+'</div>'+
    '<div class="meta"><div class="nav">'+
      '<button onclick="moveCard(\\''+c.id+'\\',\\''+(back||'')+'\\')" '+(back?'':'disabled')+' title="'+(back||'')+'">◀</button>'+
      stageSel+
      '<button onclick="moveCard(\\''+c.id+'\\',\\''+(fwd||'')+'\\')" '+(fwd?'':'disabled')+' title="'+(fwd||'')+'">▶</button></div>'+
      planLink+'<span class="note-in" onclick="addNote(\\''+c.id+'\\')">+note</span>'+
      '<span class="x" title="delete" onclick="delCard(\\''+c.id+'\\')">✕</span></div>'+notes+'</div></div>';
}
async function moveCard(id,stage){if(!stage)return;const c=findCard(id);if(c)c.stage=stage;await api('/api/card/update',{id,stage});await load();}
async function changeType(id,type){const c=findCard(id);if(c)c.type=type;await api('/api/card/update',{id,type});await load();}
async function delCard(id){if(confirm('Delete this card?')){await api('/api/card/delete',{id});await load();toast('Deleted.');}}
async function addNote(id){const n=prompt('Add a note:');if(n&&n.trim()){await api('/api/card/update',{id,note:n});await load();}}
async function saveField(el,field){
  const id=el.dataset.id,val=el.innerText.trim(),orig=el.dataset.orig||'';
  if(val===orig)return;
  if(field==='title'&&!val){el.innerText=orig;return;}
  const j=await api('/api/card/update',{id,[field]:val});
  if(j.ok){el.dataset.orig=val;const c=findCard(id);if(c)c[field]=val;toast('Saved.');}
  else{el.innerText=orig;toast('Save failed.');}
}
async function saveNote(el){
  const id=el.dataset.id,i=+el.dataset.i,val=el.innerText.trim(),orig=el.dataset.orig||'';
  if(val===orig)return;
  const c=findCard(id);if(!c)return;
  const notes=(c.notes||[]).map((n,idx)=>idx===i?{t:val,at:n.at}:n).filter(n=>n.t);
  const j=await api('/api/card/update',{id,notes});
  if(j.ok){c.notes=notes;el.dataset.orig=val;if(!val)await load();toast('Saved.');}
  else{el.innerText=orig;toast('Save failed.');}
}
async function delNote(id,i){
  const c=findCard(id);if(!c)return;
  if(!confirm('Delete this note?'))return;
  const notes=(c.notes||[]).filter((_,idx)=>idx!==i);
  const j=await api('/api/card/update',{id,notes});
  if(j.ok){c.notes=notes;await load();toast('Note deleted.');}
}
function fieldKey(e,el){
  if(e.key==='Enter'&&!e.shiftKey&&el.classList.contains('ttl')){e.preventDefault();el.blur();}
  else if(e.key==='Escape'){el.innerText=el.dataset.orig||'';el.blur();}
}

/* ---- decisions ---- */
function renderDecisions(){
  const dec=S.ballot.filter(x=>x.kind==='decision');
  const done=Object.keys(answers).length;
  let h='<div class="ballotbar"><div class="meter"><div class="mtop"><span>Decisions</span><span><b>'+done+'</b> / '+dec.length+' decided</span></div>'+
    '<div class="track"><i style="width:'+(dec.length?Math.round(done/dec.length*100):0)+'%"></i></div></div></div>';
  h+='<div class="hint">Open a group, tick an option (click again or clear to undo), ask if something\\u2019s missing, then <b>sign &amp; file</b>. Tell Claude \\u201cgo\\u201d to ratify + implement.</div>';
  for(const s of S.ballot){if(s.kind==='explainer')h+='<div class="dcard explain"><div class="dbody">'+s.html+'</div></div>';}
  const groups=[],byG={};
  for(const s of S.ballot){if(s.kind!=='decision'&&s.kind!=='open')continue;const g=s.group||'Other';if(!byG[g]){byG[g]=[];groups.push(g);}byG[g].push(s);}
  for(const g of groups){
    const items=byG[g];
    const ids=items.filter(s=>s.kind==='decision').map(s=>s.id);
    const gdone=ids.filter(id=>answers[id]).length;
    const all=ids.length&&gdone===ids.length;
    const cnt='<span class="count'+(all?' ok':'')+'">'+gdone+' / '+ids.length+'</span>';
    const prev=esc(items.map(s=>s.id).filter(Boolean).join('  '));
    const inner=items.map(s=>s.kind==='decision'?decisionCard(s):openCard(s)).join('');
    h+=section('grp_'+key(g),g,cnt,prev,inner,'dgroup');
  }
  document.getElementById('v-decisions').innerHTML=h;
  for(const s of S.ballot){if(s.id&&answers[s.id])markPick(s.id,answers[s.id]);}
}
function decisionCard(s){
  const rec=s.rec?(/no rec/i.test(s.rec)?'<span class="rec no">no rec</span>':'<span class="rec">rec '+esc(s.rec.replace(/^rec\\s+/i,'').toUpperCase())+'</span>'):'';
  const opts=(s.options||[]).map(o=>'<div class="opt" id="o-'+s.id+'-'+o.key+'" onclick="pick(\\''+s.id+'\\',\\''+o.key+'\\')">'+
    '<div class="opt-h"><span class="radio"></span>Option '+o.key+' \\u2014 '+esc(o.name)+'</div><div class="dbody">'+o.html+'</div></div>').join('');
  const qs=S.board.questions.filter(q=>q.decisionId===s.id);
  const qhtml=qs.length?'<div class="qbox">'+qs.map(q=>'<div class="q">'+esc(q.text)+'<span class="st '+q.status+'">'+q.status+'</span>'+
    (q.answer?'<div class="ans">'+esc(q.answer)+'</div>':'<div class="qa">awaiting Claude</div>')+'</div>').join('')+'</div>':'';
  return '<div class="dcard'+(answers[s.id]?' decided':'')+'" id="d-'+s.id+'"><div class="dhead"><span class="did">'+esc(s.id)+'</span>'+rec+
    '<span class="ddecided">✓ chosen</span></div>'+
    '<div class="dttl">'+esc(s.title)+'</div><div class="dbody">'+s.intro+'</div><div class="opts">'+opts+'</div>'+
    (s.recommendation?'<div class="recline"><strong>Recommendation:</strong> '+strip2(s.recommendation)+'</div>':'')+
    '<textarea class="comment" id="c-'+s.id+'" placeholder="Comment (optional)" oninput="comments[\\''+s.id+'\\']=this.value">'+esc(comments[s.id]||'')+'</textarea>'+
    '<div class="drow"><span class="clr" id="clr-'+s.id+'" onclick="clearPick(\\''+s.id+'\\')">✕ clear</span>'+
    '<button class="ghost sm" onclick="ask(\\''+s.id+'\\')">Ask a question</button>'+
    '<button class="ghost sm" onclick="regen(\\''+s.id+'\\')">↻ Improve examples</button></div>'+qhtml+'</div>';
}
function openCard(s){
  const qs=S.board.questions.filter(q=>q.decisionId===s.id);
  const qhtml=qs.length?'<div class="qbox">'+qs.map(q=>'<div class="q">'+esc(q.text)+'<span class="st '+q.status+'">'+q.status+'</span>'+
    (q.answer?'<div class="ans">'+esc(q.answer)+'</div>':'<div class="qa">awaiting Claude</div>')+'</div>').join('')+'</div>':'';
  return '<div class="dcard"><div class="dhead"><span class="did">'+esc(s.id||'\\u2014')+'</span><span class="rec no">open</span></div>'+
    '<div class="dbody">'+strip2(s.html)+'</div>'+
    '<div class="drow"><button class="ghost sm" onclick="ask(\\''+s.id+'\\')">Ask / expand into a full card</button>'+
    '<button class="ghost sm" onclick="regen(\\''+s.id+'\\')">↻ Improve examples</button></div>'+qhtml+'</div>';
}
function strip2(html){return (html||'').replace(/^<p>/,'').replace(/<\\/p>$/,'').replace(/^<ul>/,'').replace(/<\\/ul>$/,'');}
function markPick(id,k){const s=S.ballot.find(x=>x.id===id);if(!s||!s.options)return;for(const o of s.options){const el=document.getElementById('o-'+id+'-'+o.key);if(el)el.classList.toggle('sel',o.key===k);}document.getElementById('clr-'+id)?.classList.add('on');document.getElementById('d-'+id)?.classList.add('decided');}
function pick(id,k){answers[id]=k;markPick(id,k);refreshProgress();}
function clearPick(id){delete answers[id];const s=S.ballot.find(x=>x.id===id);if(s&&s.options)for(const o of s.options)document.getElementById('o-'+id+'-'+o.key)?.classList.remove('sel');document.getElementById('clr-'+id)?.classList.remove('on');document.getElementById('d-'+id)?.classList.remove('decided');refreshProgress();}
function refreshProgress(){renderDecisions();}
function jumpNext(){
  const dec=S.ballot.filter(x=>x.kind==='decision');
  const next=dec.find(s=>!answers[s.id]);
  if(!next){toast('All decisions made — sign & file.');return;}
  const g=next.group||'Other';sec['grp_'+key(g)]=true;renderDecisions();
  setTimeout(()=>{const el=document.getElementById('d-'+next.id);if(el)el.scrollIntoView({behavior:'smooth',block:'center'});},40);
}
async function ask(id){const t=prompt('What do you want to know about '+id+'?');if(t&&t.trim()){const j=await api('/api/ask',{decisionId:id,text:t});if(j.ok){await load();toast('Question saved — Claude will answer on this card.');}}}
async function regen(id){const s=S.ballot.find(x=>x.id===id);const title=s?s.title:id;const j=await api('/api/regen',{id,title});if(j.ok)toast('Queued — Claude will improve this card\\u2019s examples.');}
async function submitBallot(){
  const dec=S.ballot.filter(x=>x.kind==='decision');
  const results=dec.map(s=>({id:s.id,title:s.title,choice:answers[s.id]||'',comment:comments[s.id]||''}));
  const btn=document.getElementById('submit');btn.disabled=true;btn.textContent='Filing…';
  const j=await api('/api/submit',{results});
  if(!j.ok){toast('Error');btn.textContent='Sign & file';btn.disabled=false;return;}
  toast('Filed to '+j.path+' — reloading…');
  btn.textContent='Filed ✓';
  setTimeout(()=>location.reload(),900);
}

/* ---- scratch (autosaves: debounced while typing + on blur) ---- */
let scratchT=null;
function renderScratch(){
  const v=document.getElementById('v-scratch');
  if(v.dataset.init)return; v.dataset.init='1';
  v.innerHTML='<div class="hint">A free scratch pad — anything goes. Autosaves to board.json as you type; persists across restarts.</div>'+
    '<textarea id="scratch" placeholder="Notes, half-thoughts, paste anything…" oninput="scratchChanged()" onblur="saveScratch()">'+esc(S.board.scratch)+'</textarea>'+
    '<div class="savebar"><span class="s" id="scratch-s">saved</span></div>';
}
function scratchChanged(){const s=document.getElementById('scratch-s');if(s)s.textContent='editing…';clearTimeout(scratchT);scratchT=setTimeout(saveScratch,1500);}
async function saveScratch(){
  clearTimeout(scratchT);
  const el=document.getElementById('scratch');if(!el)return;
  const t=el.value,s=document.getElementById('scratch-s');
  if(t===S.board.scratch){if(s)s.textContent='saved';return;}
  const j=await api('/api/scratch',{text:t});
  if(j.ok){S.board.scratch=t;if(s)s.textContent='saved '+new Date().toLocaleTimeString();}
  else if(s)s.textContent='save failed';
}

/* ---- proposals tab ---- */
function renderProposals(){
  const v=document.getElementById('v-proposals');if(!v)return;
  const ps=S.proposals||[];
  let h='<div class="hint">Feature <b>proposals</b> — exploratory ideas being shaped into plans. Click any card to read or edit it inline.</div>';
  h+=ps.length?'<div class="grid">'+ps.map(p=>
    '<div class="card task pcard" onclick="openDoc(\\'proposal\\',\\''+attr(p.slug)+'\\')"><div class="edge"></div><div class="body">'+
    '<div class="top"><span class="ttl">'+esc(p.title)+'</span></div>'+
    (p.status?'<div class="bd">'+esc(p.status)+'</div>':'')+
    '<div class="meta"><span class="plan">▤ '+esc(p.slug)+'.md</span></div></div></div>').join('')+'</div>'
    :'<div class="empty">— no proposals yet —</div>';
  v.innerHTML=h;
}

/* ---- markdown doc viewer / editor (shared by plan links + proposal cards) ---- */
let curDoc=null,curDocRaw='';
const dEl=(id)=>document.getElementById(id);
async function openDoc(kind,slug){
  if(!slug)return;
  const j=await api('/api/doc/get',{kind,slug});
  if(!j.ok){toast('Could not open '+slug);return;}
  curDoc={kind,slug};curDocRaw=j.raw;
  dEl('doc-title').textContent=j.title||slug;
  dEl('doc-path').textContent=j.path||'';
  dEl('doc-view').innerHTML=j.html;
  dEl('doc-edit-area').value=j.raw;
  setDocMode(false);
  dEl('docback').classList.add('on');
  dEl('doc-view').scrollTop=0;
}
function setDocMode(edit){
  dEl('doc-view').style.display=edit?'none':'block';
  dEl('doc-edit-area').style.display=edit?'block':'none';
  dEl('doc-save').style.display=edit?'inline-block':'none';
  dEl('doc-cancel').style.display=edit?'inline-block':'none';
  dEl('doc-edit').style.display=edit?'none':'inline-block';
}
function toggleEdit(){setDocMode(true);dEl('doc-edit-area').focus();}
function cancelEdit(){dEl('doc-edit-area').value=curDocRaw;setDocMode(false);}
async function saveDoc(){
  if(!curDoc)return;
  const text=dEl('doc-edit-area').value,b=dEl('doc-save');
  b.disabled=true;b.textContent='Saving…';
  const j=await api('/api/doc/save',{kind:curDoc.kind,slug:curDoc.slug,text});
  b.disabled=false;b.textContent='Save';
  if(!j.ok){toast('Save failed');return;}
  curDocRaw=text;dEl('doc-view').innerHTML=j.html;setDocMode(false);toast('Saved '+(j.path||''));
}
function closeDoc(){dEl('docback').classList.remove('on');curDoc=null;}
document.addEventListener('keydown',(e)=>{if(e.key==='Escape'&&dEl('docback')&&dEl('docback').classList.contains('on'))closeDoc();});

load();
window.addEventListener('hashchange',()=>{const h=location.hash.slice(1);if(h&&h!==active){active=h;render();}});
</script></body></html>`;
}

// ---- console status --------------------------------------------------------

function status() {
  const s = buildState();
  const dec = s.ballot.filter((x) => x.kind === "decision");
  const open = s.ballot.filter((x) => x.kind === "open");
  const cards = s.board.cards;
  const line = "─".repeat(64);
  out(`${C.b}Tower${C.rst}  ${C.dim}far-horizon → pre-plan → planned → decisions → implementation → done${C.rst}`);
  out(line);
  out(`${C.cyn}BOARD${C.rst}  ${cards.length} card${cards.length === 1 ? "" : "s"} (${cards.filter((c) => c.type === "bug").length} bugs)`);
  for (const st of STAGES) {
    const n = cards.filter((c) => c.stage === st);
    if (n.length) out(`  ${C.dim}${st}${C.rst} ${n.map((c) => c.title).slice(0, 4).join("; ")}`);
  }
  out("");
  out(`${C.cyn}PLANS${C.rst}  ${s.plans.length} sidequest${s.plans.length === 1 ? "" : "s"}`);
  s.plans.forEach((p) => out(`  • ${C.b}${p.slug}${C.rst} ${C.dim}— ${truncate(p.status || p.title, 54)}${C.rst}`));
  out("");
  out(`${C.cyn}DECISIONS${C.rst}  ${dec.length} carded · ${open.length} open (ask to expand)`);
  dec.forEach((d) => out(`  • ${C.b}${d.id}${C.rst} ${truncate(d.title, 52)}  ${/no rec/i.test(d.rec) ? C.yel + "NO REC" : C.grn + (d.rec || "").toUpperCase()}${C.rst}`));
  open.forEach((d) => out(`  ${C.dim}· ${d.id || "—"}${C.rst}`));
  out(line);
  out(`${C.grn}▸ node tools/Tower/Tower.mjs serve --open${C.rst}`);
}

// ---- scaffold --------------------------------------------------------------

function scaffold(slug, title) {
  if (!slug) die('usage: pipeline new <slug> "Title"');
  if (!/^[a-z0-9][a-z0-9-]*$/.test(slug)) die(`bad slug "${slug}" — use kebab-case`);
  const file = join(P.sidequests, `${slug}.md`);
  if (existsSync(file)) die(`already exists: ${file.replace(ROOT + "/", "")}`);
  const t = title || slug.replace(/-/g, " ");
  writeFileSync(file, `# ${t}

**Status:** plan, awaiting owner sign-off.

## Goal

<one paragraph: what changes for the user, and why>

## Current state

<verified findings — cite code by SYMBOL (e.g. src/sema/checker_infer.rs \`check_call\`), not line number>

## Approach

<across the pipeline: parser → sema → codegen → fmt → diagnostics → examples/tests>

## Decisions for the owner

<each needs a before/after Jet example per option + a recommendation; these
feed tools/Tower/docs/ballots/decision-ballots.md per the house rule>

## Acceptance checklist

- [ ] failing test/example written first
- [ ] spec updated (docs/spec/spec.md)
- [ ] all tests green, zero unintended snapshot reblessing
- [ ] docs touched match behavior
`);
  out(`${C.grn}created${C.rst} ${file.replace(ROOT + "/", "")}`);
}

// ---- util ------------------------------------------------------------------
const out = (s = "") => process.stdout.write(s + "\n");
const die = (s) => { process.stderr.write(s + "\n"); process.exit(1); };
const truncate = (s, n) => (s.length > n ? s.slice(0, n - 1) + "…" : s);
function openBrowser(url) {
  const cmd = process.platform === "darwin" ? "open" : process.platform === "win32" ? "start" : "xdg-open";
  try { spawn(cmd, [url], { stdio: "ignore", detached: true }).unref(); } catch { /* best-effort */ }
}

const [cmd, ...rest] = process.argv.slice(2);
switch (cmd) {
  case undefined:
  case "status": status(); break;
  case "serve": serve(Number(rest.find((a) => /^\d+$/.test(a))) || 4173); break;
  case "new": scaffold(rest[0], rest.slice(1).join(" ")); break;
  default: die(`unknown command "${cmd}". commands: status | serve [port] [--open] | new <slug> "Title"`);
}
