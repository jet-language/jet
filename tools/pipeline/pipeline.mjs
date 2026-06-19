#!/usr/bin/env node
// jet pipeline — the single management tool for the language workflow:
//   scratch → todo → review → plan → plan-review → implementing → done
// plus the decision ballot and a bug list. No dependencies; pure node.
//
// Usage:
//   node tools/pipeline/pipeline.mjs serve [port] [--open]   # the dashboard (main UI)
//   node tools/pipeline/pipeline.mjs status                  # console snapshot
//   node tools/pipeline/pipeline.mjs new <slug> "Title"      # scaffold a sidequest plan
//
// State the owner inputs (tasks/bugs/notes/decision answers/questions) lives in
// tools/pipeline/board.json — management state only; it references plan files by
// slug and never copies their content, so the docs stay the single source of
// truth. The ballot renders straight from docs/spec/decision-ballots.md.

import { readFileSync, writeFileSync, readdirSync, existsSync, appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { createServer } from "node:http";
import { spawn } from "node:child_process";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const P = {
  sidequests: join(ROOT, "docs/plans/sidequests"),
  ballotMd: join(ROOT, "docs/spec/decision-ballots.md"),
  ratified: join(ROOT, "docs/spec/syntax-decisions.md"),
  results: join(ROOT, "docs/spec/ballot-results.md"),
  board: join(ROOT, "tools/pipeline/board.json"),
  regenQueue: join(ROOT, "tools/pipeline/regen-queue.md"),
  askQueue: join(ROOT, "tools/pipeline/questions-queue.md"),
};

const read = (p) => (existsSync(p) ? readFileSync(p, "utf8") : "");
const C = { dim: "\x1b[2m", b: "\x1b[1m", grn: "\x1b[32m", yel: "\x1b[33m", cyn: "\x1b[36m", rst: "\x1b[0m" };
const now = () => new Date().toISOString();
const stamp = () => now().replace("T", " ").slice(0, 16);

// ---- board store -----------------------------------------------------------

const STAGES = ["backlog", "todo", "review", "plan", "plan-review", "implementing", "done"];

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

function ratifiedCount() {
  const md = read(P.ratified);
  return new Set([...md.matchAll(/\b([DSU]-?[A-Z]*\d+[A-Z]*)\b/g)].map((m) => m[1])).size;
}

// ---- ballot parser (single source: decision-ballots.md) --------------------

const DECISION_ID = /^(D-[A-Z0-9]+|S\d+-[A-Z]+|S\d+|N\d+|U\d+)$/;

function ballotSection(md) {
  const start = md.indexOf("## Next Tasks — open ballots");
  if (start < 0) return "";
  const rest = md.slice(start);
  const end = rest.indexOf("\n## Parked");
  return end >= 0 ? rest.slice(0, end) : rest;
}

// Open ballot cards + explainer blocks.
function parseBallot(md) {
  const body = ballotSection(md);
  if (!body) return [];
  const blocks = [];
  let cur = null;
  for (const line of body.split("\n")) {
    if (line.startsWith("### ")) {
      if (cur) blocks.push(cur);
      cur = { header: line.slice(4).trim(), lines: [] };
    } else if (cur) cur.lines.push(line);
  }
  if (cur) blocks.push(cur);

  return blocks.map((blk) => {
    const dash = blk.header.indexOf(" — ");
    const maybeId = dash > 0 ? blk.header.slice(0, dash).trim() : "";
    if (dash > 0 && DECISION_ID.test(maybeId)) {
      let title = blk.header.slice(dash + 3).trim();
      let rec = "";
      const rm = title.match(/\(([^)]*)\)\s*$/);
      if (rm) { rec = rm[1].trim(); title = title.slice(0, rm.index).trim(); }
      return { kind: "decision", id: maybeId, title, rec, ...splitCard(blk.lines) };
    }
    return { kind: "explainer", title: blk.header, html: renderMd(blk.lines.join("\n")) };
  });
}

// "## Parked — not open ballots" → read-only deferred items (nothing hidden).
function parseParked(md) {
  const i = md.indexOf("## Parked — not open ballots");
  if (i < 0) return [];
  let body = md.slice(i).replace("## Parked — not open ballots", "");
  const next = body.indexOf("\n## ");
  if (next >= 0) body = body.slice(0, next);
  // top-level bullets only (lines starting "- ")
  const items = [];
  for (const raw of body.split("\n")) {
    if (/^- /.test(raw)) items.push(raw.replace(/^- /, ""));
    else if (items.length && /^\s+\S/.test(raw)) items[items.length - 1] += " " + raw.trim();
  }
  return items.map((t) => renderMd("- " + t));
}

// "## Open — captured, not yet drafted as full cards" → rendered list (visibility).
function parseOpenList(md) {
  const head = "## Open — captured, not yet drafted as full cards";
  const i = md.indexOf(head);
  if (i < 0) return "";
  let body = md.slice(i).replace(head, "");
  const next = body.indexOf("\n## ");
  if (next >= 0) body = body.slice(0, next);
  return renderMd(body.trim());
}

// Ratified-but-not-yet-implemented rows from the syntax-decisions decision log.
function ratifiedPending() {
  const md = read(P.ratified);
  const out = [];
  for (const line of md.split("\n")) {
    if (!/^\|/.test(line) || !/not yet implemented/i.test(line)) continue;
    const cols = line.split("|").map((s) => s.trim()).filter(Boolean);
    if (cols.length >= 3) {
      const text = cols[2].replace(/\.?\s*\*\*Ratified, not yet implemented\*\*\.?/i, "").trim();
      out.push({ id: cols[1], text: text.length > 150 ? text.slice(0, 149) + "…" : text });
    }
  }
  return out;
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

// ---- state for the page ----------------------------------------------------

function buildState() {
  const md = read(P.ballotMd);
  return {
    board: loadBoard(),
    stages: STAGES,
    ballot: parseBallot(md),
    openCaptured: parseOpenList(md),
    pendingImpl: ratifiedPending(),
    deferred: parseParked(md),
    plans: readPlans(),
    ratified: ratifiedCount(),
    lastSubmit: existsSync(P.results) ? (read(P.results).match(/_submitted (.+?)_/) || [, ""])[1] : "",
  };
}

// ---- write-backs -----------------------------------------------------------

function writeResults(payload) {
  const lines = [
    "# Owner ballot results", "", `_submitted ${stamp()}_`, "",
    'Decisions captured from the dashboard. Tell Claude **"go"** to ratify these',
    "into syntax-decisions.md, strip the cards, and implement the plans.", "", "## Next Tasks", "",
  ];
  for (const r of payload.results) {
    lines.push(`**${r.id}** — ${r.title}`);
    lines.push(`Decision: **${r.choice || "(no answer)"}**`);
    if (r.comment && r.comment.trim()) lines.push(`Comment: ${r.comment.trim()}`);
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
    out(`${C.grn}Jet dashboard${C.rst} → ${C.b}${url}${C.rst}`);
    out(`${C.dim}board: tools/pipeline/board.json · ballot: docs/spec/decision-ballots.md · Ctrl-C to stop${C.rst}`);
    if (process.argv.includes("--open") || process.argv.includes("-o")) openBrowser(url);
  });
}

function handlePost(url, p, res, json) {
  const b = loadBoard();
  switch (url) {
    case "/api/card/add": {
      const card = { id: newId(), type: p.type || "task", title: (p.title || "").trim(),
        body: (p.body || "").trim(), stage: STAGES.includes(p.stage) ? p.stage : "backlog",
        plan: p.plan || null, notes: [], created: now(), updated: now() };
      if (!card.title) return json(res, 400, { ok: false, error: "title required" });
      b.cards.push(card); saveBoard(b); return json(res, 200, { ok: true, card });
    }
    case "/api/card/update": {
      const c = b.cards.find((x) => x.id === p.id);
      if (!c) return json(res, 404, { ok: false, error: "no card" });
      if (p.stage && STAGES.includes(p.stage)) c.stage = p.stage;
      if (typeof p.title === "string") c.title = p.title.trim();
      if (typeof p.body === "string") c.body = p.body.trim();
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
    default: return json(res, 404, { ok: false, error: "unknown endpoint" });
  }
}

// ---- the page --------------------------------------------------------------

function page() {
  return `<!DOCTYPE html>
<html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Jet — Pipeline</title>
<style>
:root{--bg:#0b0e14;--panel:#11151f;--panel2:#161b28;--line:#222a3a;--line2:#2d3852;--ink:#e6edf3;--dim:#8b97a8;--blue:#6cb6ff;--blueb:#1b3a6b;--grn:#56d364;--grnb:#10331a;--yel:#e3b341;--red:#f47067;--purple:#d2a8ff}
*{box-sizing:border-box;margin:0;padding:0}
body{background:var(--bg);color:var(--ink);font:14px/1.6 ui-sans-serif,-apple-system,Segoe UI,Roboto,sans-serif;padding:0 0 80px}
.mono{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}
header{position:sticky;top:0;z-index:20;background:#0b0e14ee;backdrop-filter:blur(8px);border-bottom:1px solid var(--line);padding:14px 26px}
h1{font-size:17px;color:#fff;font-weight:700;letter-spacing:.2px}
.tag{font-size:11px;color:var(--dim);font-weight:500}
.tabs{display:flex;gap:4px;margin-top:12px;flex-wrap:wrap}
.tab{padding:7px 15px;border-radius:7px 7px 0 0;border:1px solid transparent;border-bottom:none;color:var(--dim);cursor:pointer;font-size:13px;font-weight:600}
.tab:hover{color:var(--ink)}
.tab.on{background:var(--panel);color:#fff;border-color:var(--line)}
.tab .b{display:inline-block;min-width:18px;text-align:center;background:var(--line2);color:var(--ink);border-radius:10px;font-size:11px;padding:0 6px;margin-left:6px}
.tab.on .b{background:var(--blueb);color:var(--blue)}
main{max-width:1180px;margin:0 auto;padding:24px 26px}
.view{display:none}.view.on{display:block}
h2{font-size:12px;color:var(--dim);letter-spacing:.1em;text-transform:uppercase;margin:26px 0 12px;font-weight:700}
h2:first-child{margin-top:4px}
.hint{color:var(--dim);font-size:12.5px;margin-bottom:16px}
/* board */
.stage-row{margin-bottom:22px}
.stage-h{display:flex;align-items:center;gap:9px;margin-bottom:10px}
.stage-h .name{font-size:13px;font-weight:700;color:var(--ink);text-transform:capitalize}
.stage-h .ct{font-size:11px;color:var(--dim)}
.stage-h .lane{flex:1;height:1px;background:var(--line)}
.cards{display:grid;grid-template-columns:repeat(auto-fill,minmax(260px,1fr));gap:11px}
.kcard{background:var(--panel);border:1px solid var(--line);border-radius:9px;padding:13px 14px}
.kcard:hover{border-color:var(--line2)}
.kcard .top{display:flex;align-items:flex-start;gap:8px}
.kcard .ttl{font-weight:600;font-size:13.5px;flex:1;color:#fff}
.btype{font-size:9.5px;font-weight:700;letter-spacing:.04em;padding:2px 6px;border-radius:5px;text-transform:uppercase;flex-shrink:0}
.btype.task{background:var(--blueb);color:var(--blue)}
.btype.bug{background:#3a1a1a;color:var(--red)}
.btype.idea{background:#2d2600;color:var(--yel)}
.kcard .bd{font-size:12px;color:var(--dim);margin-top:6px;white-space:pre-wrap}
.kcard .meta{display:flex;align-items:center;gap:8px;margin-top:10px;flex-wrap:wrap}
.kcard select,.kcard .plan{font-size:11px;background:var(--panel2);border:1px solid var(--line2);color:var(--ink);border-radius:6px;padding:3px 6px}
.kcard .plan{color:var(--blue);text-decoration:none}
.kcard .x{margin-left:auto;color:var(--dim);cursor:pointer;font-size:14px;line-height:1}
.kcard .x:hover{color:var(--red)}
.kcard .note-in{visibility:hidden;font-size:11px;color:var(--dim);cursor:pointer;text-decoration:underline}
.kcard:hover .note-in{visibility:visible}
.kcard .notes{margin-top:8px;border-top:1px solid var(--line);padding-top:7px}
.kcard .notes div{font-size:11px;color:var(--dim);margin-top:3px}
.empty{color:var(--dim);font-size:12px;font-style:italic;padding:6px 2px}
/* add form */
.addbox{background:var(--panel);border:1px dashed var(--line2);border-radius:9px;padding:14px;margin-bottom:20px}
.addbox .r{display:flex;gap:9px;flex-wrap:wrap;align-items:center}
input,textarea,select.sel{background:#0a0d13;border:1px solid var(--line2);color:var(--ink);border-radius:7px;padding:8px 10px;font:13px/1.4 inherit;outline:none}
input:focus,textarea:focus{border-color:var(--blue)}
input.grow{flex:1;min-width:200px}
textarea{width:100%;margin-top:9px;resize:vertical;min-height:42px}
button{background:var(--blue);border:none;color:#06101f;border-radius:7px;padding:8px 16px;font:600 13px/1 inherit;cursor:pointer}
button:hover{filter:brightness(1.08)}button.ghost{background:var(--panel2);color:var(--ink);border:1px solid var(--line2)}
button.sm{padding:6px 11px;font-size:12px}
button:disabled{opacity:.5;cursor:default}
/* decisions */
.dcard{background:var(--panel);border:1px solid var(--line);border-radius:11px;padding:22px;margin-bottom:18px}
.explain{background:var(--panel2);border-color:var(--line2)}
.did{font-size:11px;color:var(--blue);font-weight:700;letter-spacing:.04em}
.dttl{font-size:17px;font-weight:700;color:#fff;margin:5px 0 6px}
.rec{display:inline-block;font-size:10px;font-weight:700;padding:2px 8px;border-radius:20px;background:var(--grnb);color:var(--grn);border:1px solid #2c7a3f;margin-left:8px;vertical-align:middle}
.rec.no{background:#2d2000;color:var(--yel);border-color:#9e7b1b}
.body{font-size:13.5px;color:#cdd6e3}
.body p{margin:10px 0}.body ul{margin:10px 0 10px 20px}.body li{margin:4px 0}
.body strong{color:#fff}
.body table{border-collapse:collapse;margin:14px 0;width:100%;font-size:12.5px}
.body th,.body td{border:1px solid var(--line2);padding:7px 10px;text-align:left;vertical-align:top}
.body th{background:#1a2436;color:#fff}
.opts{display:flex;flex-direction:column;gap:11px;margin:16px 0}
.opt{border:2px solid var(--line);border-radius:9px;padding:15px 16px;cursor:pointer;transition:border-color .12s,background .12s}
.opt:hover{border-color:#3b4d6e}
.opt.sel{border-color:var(--blue);background:#0e1f3a}
.opt-h{display:flex;align-items:center;gap:10px;font-weight:700;color:#fff;font-size:14px}
.dot{width:16px;height:16px;border-radius:50%;border:2px solid #59657d;flex-shrink:0}
.opt.sel .dot{border-color:var(--blue);background:var(--blue);box-shadow:inset 0 0 0 3px #0e1f3a}
.opt .body{margin-top:8px}
pre.code{background:#06090f;border:1px solid var(--line);border-radius:7px;padding:13px 15px;overflow-x:auto;margin:10px 0;line-height:1.55}
pre.code code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:12.5px;white-space:pre;color:#c9d3e0}
code{background:#1a2336;border-radius:5px;padding:1px 5px;font-family:ui-monospace,monospace;font-size:.9em}
.body p code,.opt code{background:#13243a}
.k{color:var(--red)}.t{color:var(--blue)}.s{color:#a5d6ff}.c{color:#6e7d92;font-style:italic}.n{color:var(--purple)}
.drow{display:flex;align-items:center;gap:10px;margin-top:12px;flex-wrap:wrap}
.clr{font-size:12px;color:var(--yel);cursor:pointer;text-decoration:underline;visibility:hidden}
.clr.on{visibility:visible}
.qbox{margin-top:14px;border-top:1px solid var(--line);padding-top:13px}
.q{background:var(--panel2);border:1px solid var(--line2);border-radius:8px;padding:10px 12px;margin-bottom:8px;font-size:12.5px}
.q .qa{color:var(--dim)}.q .st{font-size:10px;font-weight:700;padding:1px 7px;border-radius:10px;margin-left:8px}
.q .st.open{background:#2d2000;color:var(--yel)}.q .st.answered{background:var(--grnb);color:var(--grn)}
.q .ans{margin-top:6px;color:#cdd6e3;border-left:2px solid var(--grn);padding-left:9px}
.parked{background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:6px 16px;font-size:13px;color:#bcc7d6}
.parked li{margin:9px 0}
/* scratch */
#scratch{width:100%;min-height:420px;font-family:ui-monospace,monospace;font-size:13px;line-height:1.6}
.savebar{display:flex;align-items:center;gap:12px;margin-top:10px}.savebar .s{color:var(--dim);font-size:12px}
.bar{position:fixed;left:0;right:0;bottom:0;background:#0b0e14ee;backdrop-filter:blur(8px);border-top:1px solid var(--line);padding:13px 26px;display:none;align-items:center;gap:16px}
.bar.on{display:flex}.bar .p{flex:1;color:var(--dim);font-size:13px}.bar .p b{color:var(--grn)}
.toast{position:fixed;bottom:80px;left:50%;transform:translateX(-50%);background:#1a2436;border:1px solid #2c7a3f;color:var(--ink);padding:11px 18px;border-radius:8px;font-size:12.5px;opacity:0;transition:opacity .2s;pointer-events:none;max-width:92%;z-index:40}
.toast.on{opacity:1}
</style></head><body>
<header>
  <h1>Jet — Pipeline <span class="tag">· one place: tasks · decisions · bugs · scratch</span></h1>
  <div class="tabs" id="tabs"></div>
</header>
<main>
  <section class="view" id="v-board"></section>
  <section class="view" id="v-decisions"></section>
  <section class="view" id="v-bugs"></section>
  <section class="view" id="v-scratch"></section>
</main>
<div class="bar" id="bar"><div class="p" id="prog"></div><button id="submit" onclick="submitBallot()">Submit decisions</button></div>
<div class="toast" id="toast"></div>
<script>
let S=null;
const TABS=[['board','Board'],['decisions','Decisions'],['bugs','Bugs'],['scratch','Scratch']];
const answers={},comments={};
let active=location.hash.slice(1)||'board';

function esc(s){return (s||'').replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');}
function jq(s){return (s||'').replace(/'/g,"\\\\'").replace(/\\n/g,' ');}
function toast(m){const t=document.getElementById('toast');t.textContent=m;t.classList.add('on');clearTimeout(t._);t._=setTimeout(()=>t.classList.remove('on'),3400);}
async function api(url,body){const r=await fetch(url,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)});return r.json();}
async function load(){S=await (await fetch('/api/state')).json();render();}

function render(){
  const dec=S.ballot.filter(x=>x.kind==='decision');
  const bugs=S.board.cards.filter(c=>c.type==='bug');
  const counts={board:S.board.cards.filter(c=>c.type!=='bug').length,decisions:dec.length+S.deferred.length,bugs:bugs.length,scratch:0};
  document.getElementById('tabs').innerHTML=TABS.map(([id,label])=>
    '<div class="tab'+(id===active?' on':'')+'" onclick="go(\\''+id+'\\')">'+label+
    (counts[id]?'<span class="b">'+counts[id]+'</span>':'')+'</div>').join('');
  TABS.forEach(([id])=>document.getElementById('v-'+id).classList.toggle('on',id===active));
  document.getElementById('bar').classList.toggle('on',active==='decisions');
  renderBoard();renderDecisions();renderBugs();renderScratch();
}
function go(id){active=id;location.hash=id;render();}

/* ---- board ---- */
function renderBoard(){
  const cards=S.board.cards.filter(c=>c.type!=='bug');
  let h='<div class="hint">Drop a task or idea below. I move cards through the stages as I work — this view is live status; refresh to see updates.</div>';
  h+=addForm('task');
  for(const st of S.stages){
    const inSt=cards.filter(c=>c.stage===st);
    h+='<div class="stage-row"><div class="stage-h"><span class="name">'+st.replace('-',' ')+'</span><span class="ct">'+inSt.length+'</span><span class="lane"></span></div>';
    h+= inSt.length?'<div class="cards">'+inSt.map(card).join('')+'</div>':'<div class="empty">—</div>';
    h+='</div>';
  }
  document.getElementById('v-board').innerHTML=h;
}
function addForm(type){
  return '<div class="addbox"><div class="r">'+
    '<input class="grow" id="add-ttl-'+type+'" placeholder="'+(type==='bug'?'Describe a bug…':'New task or idea…')+'">'+
    (type==='bug'?'':'<select class="sel" id="add-type"><option value="task">task</option><option value="idea">idea</option></select>')+
    '<select class="sel" id="add-stage-'+type+'">'+S.stages.map(s=>'<option'+(s==='backlog'?' selected':'')+'>'+s+'</option>').join('')+'</select>'+
    '<button class="sm" onclick="addCard(\\''+type+'\\')">Add</button></div>'+
    '<textarea id="add-body-'+type+'" placeholder="Details (optional)"></textarea></div>';
}
async function addCard(type){
  const t=document.getElementById('add-ttl-'+type);
  if(!t.value.trim())return;
  const realType=type==='bug'?'bug':(document.getElementById('add-type').value);
  const j=await api('/api/card/add',{type:realType,title:t.value,body:document.getElementById('add-body-'+type).value,stage:document.getElementById('add-stage-'+type).value});
  if(j.ok){t.value='';document.getElementById('add-body-'+type).value='';await load();toast('Added.');}
}
function card(c){
  const planLink=c.plan?'<a class="plan" href="#" title="sidequest plan">▤ '+esc(c.plan)+'</a>':'';
  const notes=c.notes&&c.notes.length?'<div class="notes">'+c.notes.map(n=>'<div>• '+esc(n.t)+' <span style="opacity:.6">'+esc(n.at)+'</span></div>').join('')+'</div>':'';
  return '<div class="kcard"><div class="top"><span class="ttl">'+esc(c.title)+'</span><span class="btype '+c.type+'">'+c.type+'</span></div>'+
    (c.body?'<div class="bd">'+esc(c.body)+'</div>':'')+
    '<div class="meta"><select onchange="moveCard(\\''+c.id+'\\',this.value)">'+
      S.stages.map(s=>'<option'+(s===c.stage?' selected':'')+'>'+s+'</option>').join('')+'</select>'+
      planLink+'<span class="note-in" onclick="addNote(\\''+c.id+'\\')">+ note</span>'+
      '<span class="x" title="delete" onclick="delCard(\\''+c.id+'\\')">✕</span></div>'+notes+'</div>';
}
async function moveCard(id,stage){await api('/api/card/update',{id,stage});await load();}
async function delCard(id){if(confirm('Delete this card?')){await api('/api/card/delete',{id});await load();toast('Deleted.');}}
async function addNote(id){const n=prompt('Add a note:');if(n&&n.trim()){await api('/api/card/update',{id,note:n});await load();}}

/* ---- bugs ---- */
function renderBugs(){
  const bugs=S.board.cards.filter(c=>c.type==='bug');
  let h='<div class="hint">Known defects. Same pipeline stages as tasks; I move them as they get fixed.</div>'+addForm('bug');
  if(!bugs.length)h+='<div class="empty">No open bugs.</div>';
  else{for(const st of S.stages){const inSt=bugs.filter(b=>b.stage===st);if(!inSt.length)continue;
    h+='<div class="stage-row"><div class="stage-h"><span class="name">'+st.replace('-',' ')+'</span><span class="ct">'+inSt.length+'</span><span class="lane"></span></div><div class="cards">'+inSt.map(card).join('')+'</div></div>';}}
  document.getElementById('v-bugs').innerHTML=h;
}

/* ---- decisions ---- */
function renderDecisions(){
  let h='<div class="hint">Every open decision is here — nothing hidden. Pick an option (click again or ✕ to undo), ask a question if something\\'s missing, then <b>Submit</b>. Tell Claude “go” to ratify + implement.</div>';
  for(const s of S.ballot){
    if(s.kind==='explainer'){h+='<div class="dcard explain"><div class="dttl">'+esc(s.title)+'</div><div class="body">'+s.html+'</div></div>';continue;}
    const rec=s.rec?(/no rec/i.test(s.rec)?'<span class="rec no">NO REC</span>':'<span class="rec">'+s.rec.toUpperCase()+'</span>'):'';
    const opts=s.options.map(o=>'<div class="opt" id="o-'+s.id+'-'+o.key+'" onclick="pick(\\''+s.id+'\\',\\''+o.key+'\\')">'+
      '<div class="opt-h"><span class="dot"></span>Option '+o.key+' — '+esc(o.name)+'</div><div class="body">'+o.html+'</div></div>').join('');
    const qs=S.board.questions.filter(q=>q.decisionId===s.id);
    const qhtml=qs.length?qs.map(q=>'<div class="q">'+esc(q.text)+'<span class="st '+q.status+'">'+q.status+'</span>'+
      (q.answer?'<div class="ans">'+esc(q.answer)+'</div>':'<div class="qa">awaiting Claude — will appear here or update the card</div>')+'</div>').join(''):'';
    h+='<div class="dcard"><div class="did">'+s.id+rec+'</div><div class="dttl">'+esc(s.title)+'</div>'+
      '<div class="body">'+s.intro+'</div><div class="opts">'+opts+'</div>'+
      (s.recommendation?'<div class="body" style="color:var(--dim)"><strong>Recommendation:</strong> '+strip(s.recommendation)+'</div>':'')+
      '<textarea id="c-'+s.id+'" placeholder="Comment (optional)" oninput="comments[\\''+s.id+'\\']=this.value">'+(comments[s.id]||'')+'</textarea>'+
      '<div class="drow"><span class="clr" id="clr-'+s.id+'" onclick="clearPick(\\''+s.id+'\\')">✕ clear selection</span>'+
      '<button class="ghost sm" onclick="ask(\\''+s.id+'\\')">Ask a question</button>'+
      '<button class="ghost sm" onclick="regen(\\''+s.id+'\\',\\''+jq(s.title)+'\\')">↻ Improve examples</button></div>'+
      (qhtml?'<div class="qbox">'+qhtml+'</div>':'')+'</div>';
    if(answers[s.id])setTimeout(()=>markPick(s.id,answers[s.id]),0);
  }
  if(S.openCaptured){h+='<h2>Open — captured, not yet drafted as full cards</h2>'+
    '<div class="parked">'+S.openCaptured+'</div>';}
  if(S.pendingImpl&&S.pendingImpl.length){h+='<h2>Ratified — awaiting your “go” to implement</h2><ul class="parked">'+
    S.pendingImpl.map(p=>'<li><b>'+esc(p.id)+'</b> — '+esc(p.text)+'</li>').join('')+'</ul>';}
  if(S.deferred.length){h+='<h2>Deferred / parked (E3+) — visible, not for decision now</h2><ul class="parked">'+
    S.deferred.map(d=>strip(d)).join('')+'</ul>';}
  document.getElementById('v-decisions').innerHTML=h;
  progress();
}
function strip(html){return (html||'').replace(/^<p>/,'').replace(/<\\/p>$/,'').replace(/^<ul>/,'').replace(/<\\/ul>$/,'');}
function markPick(id,key){const s=S.ballot.find(x=>x.id===id);if(!s)return;for(const o of s.options){const el=document.getElementById('o-'+id+'-'+o.key);if(el)el.classList.toggle('sel',o.key===key);}document.getElementById('clr-'+id)?.classList.add('on');}
function pick(id,key){answers[id]=key;markPick(id,key);progress();}
function clearPick(id){delete answers[id];const s=S.ballot.find(x=>x.id===id);for(const o of s.options)document.getElementById('o-'+id+'-'+o.key)?.classList.remove('sel');document.getElementById('clr-'+id)?.classList.remove('on');progress();}
function progress(){const dec=S.ballot.filter(x=>x.kind==='decision');document.getElementById('prog').innerHTML='<b>'+Object.keys(answers).length+'</b> of '+dec.length+' decided';}
async function ask(id){const t=prompt('What do you want to know about '+id+'? (e.g. "what are the tradeoffs?")');if(t&&t.trim()){const j=await api('/api/ask',{decisionId:id,text:t});if(j.ok){await load();toast('Question saved — Claude will answer on this card.');}}}
async function regen(id,title){const j=await api('/api/regen',{id,title});if(j.ok)toast('Queued — Claude will improve this card\\'s examples.');}
async function submitBallot(){
  const dec=S.ballot.filter(x=>x.kind==='decision');
  const results=dec.map(s=>({id:s.id,title:s.title,choice:answers[s.id]||'',comment:comments[s.id]||''}));
  const btn=document.getElementById('submit');btn.disabled=true;btn.textContent='Saving…';
  const j=await api('/api/submit',{results});
  toast(j.ok?'Saved to '+j.path+' — tell Claude “go” to ratify + implement.':'Error');
  btn.textContent='Submitted ✓';setTimeout(()=>{btn.textContent='Submit decisions';btn.disabled=false;},2200);
}

/* ---- scratch ---- */
function renderScratch(){
  const v=document.getElementById('v-scratch');
  if(v.dataset.init)return; v.dataset.init='1';
  v.innerHTML='<div class="hint">A free scratch pad — anything goes. Saved to board.json; persists across restarts.</div>'+
    '<textarea id="scratch" placeholder="Notes, half-thoughts, paste anything…">'+esc(S.board.scratch)+'</textarea>'+
    '<div class="savebar"><button class="sm" onclick="saveScratch()">Save</button><span class="s" id="scratch-s"></span></div>';
}
async function saveScratch(){const t=document.getElementById('scratch').value;const j=await api('/api/scratch',{text:t});if(j.ok){document.getElementById('scratch-s').textContent='saved '+new Date().toLocaleTimeString();S.board.scratch=t;}}

load();
window.addEventListener('hashchange',()=>{const h=location.hash.slice(1);if(h&&h!==active){active=h;render();}});
</script></body></html>`;
}

// ---- console status --------------------------------------------------------

function status() {
  const s = buildState();
  const dec = s.ballot.filter((x) => x.kind === "decision");
  const cards = s.board.cards;
  const line = "─".repeat(64);
  out(`${C.b}Jet pipeline${C.rst}  ${C.dim}scratch → todo → review → plan → plan-review → implementing → done${C.rst}`);
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
  out(`${C.cyn}DECISIONS${C.rst}  ${dec.length} open · ${s.deferred.length} parked/deferred`);
  dec.forEach((d) => out(`  • ${C.b}${d.id}${C.rst} ${truncate(d.title, 52)}  ${/no rec/i.test(d.rec) ? C.yel + "NO REC" : C.grn + (d.rec || "").toUpperCase()}${C.rst}`));
  out(line);
  out(`${C.grn}▸ node tools/pipeline/pipeline.mjs serve --open${C.rst}`);
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
feed docs/spec/decision-ballots.md per the house rule>

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
