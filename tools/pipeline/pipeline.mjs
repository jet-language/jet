#!/usr/bin/env node
// jet pipeline — a tiny devops view + dashboard over the task pipeline:
//   inbox  ->  plan  ->  ballot  ->  ratified  ->  implemented
// No dependencies; pure node + the markdown the team already keeps.
//
// Usage:
//   node tools/pipeline/pipeline.mjs               # status (console, default)
//   node tools/pipeline/pipeline.mjs status
//   node tools/pipeline/pipeline.mjs serve [port]  # dashboard + decision ballot in the browser
//   node tools/pipeline/pipeline.mjs new <slug> "Title"   # scaffold a sidequest plan
//
// The dashboard renders the ballot *from* docs/spec/decision-ballots.md (one
// source of truth — no duplicated card data). Submit writes the owner's
// decisions to docs/spec/ballot-results.md so there is no copy/paste; ratifying
// those decisions and implementing the plans stays a Claude step (on the owner's
// word). "Improve examples" queues a request in tools/pipeline/regen-queue.md.

import { readFileSync, writeFileSync, readdirSync, existsSync, appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";
import { createServer } from "node:http";
import { spawn } from "node:child_process";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const P = {
  inbox: join(ROOT, "docs/plans/owner-todo.md"),
  sidequests: join(ROOT, "docs/plans/sidequests"),
  ballotMd: join(ROOT, "docs/spec/decision-ballots.md"),
  ratified: join(ROOT, "docs/spec/syntax-decisions.md"),
  results: join(ROOT, "docs/spec/ballot-results.md"),
  regenQueue: join(ROOT, "tools/pipeline/regen-queue.md"),
};

const read = (p) => (existsSync(p) ? readFileSync(p, "utf8") : "");
const C = { dim: "\x1b[2m", b: "\x1b[1m", grn: "\x1b[32m", yel: "\x1b[33m", cyn: "\x1b[36m", rst: "\x1b[0m" };

// ---- readers ---------------------------------------------------------------

// Inbox: the actionable "## Next Tasks" bullets, plus a count of "## Considerations".
function readInbox() {
  const md = read(P.inbox);
  const section = (name) => {
    const m = md.match(new RegExp(`^##\\s+${name}\\s*$([\\s\\S]*?)(?=^##\\s|\\Z)`, "m"));
    return m ? m[1] : "";
  };
  const bullets = (txt) =>
    txt.split("\n").filter((l) => /^\s*-\s+\S/.test(l)).map((l) => l.replace(/^\s*-\s+/, "").trim());
  return {
    nextTasks: bullets(section("Next Tasks")),
    considerations: bullets(section("Considerations")).length,
  };
}

// Plans: every sidequest md (the slug + first heading title + a status line if present).
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
  // Count decision-id tokens in the ratified log (rough, stable enough for a dashboard).
  return new Set([...md.matchAll(/\b([DSU]-?[A-Z]*\d+[A-Z]*)\b/g)].map((m) => m[1])).size;
}

// ---- ballot parser (single source: decision-ballots.md) --------------------

const DECISION_ID = /^(D-[A-Z0-9]+|S\d+-[A-Z]+|S\d+|N\d+|U\d+)$/;

// Parse the "## Next Tasks — open ballots" section into ordered sections:
//   { kind: "explainer", title, html }
//   { kind: "decision", id, title, rec, intro(html), options:[{key,name,html}], recommendation(html) }
function parseBallot(md) {
  const start = md.indexOf("## Next Tasks — open ballots");
  if (start < 0) return [];
  const rest = md.slice(start);
  const end = rest.indexOf("\n## Parked");
  const body = (end >= 0 ? rest.slice(0, end) : rest);

  // Split into blocks on `### ` headers (drop the `## Next Tasks` lead-in prose).
  const blocks = [];
  let cur = null;
  for (const line of body.split("\n")) {
    if (line.startsWith("### ")) {
      if (cur) blocks.push(cur);
      cur = { header: line.slice(4).trim(), lines: [] };
    } else if (cur) {
      cur.lines.push(line);
    }
  }
  if (cur) blocks.push(cur);

  return blocks.map((blk) => {
    const dash = blk.header.indexOf(" — ");
    const maybeId = dash > 0 ? blk.header.slice(0, dash).trim() : "";
    if (dash > 0 && DECISION_ID.test(maybeId)) {
      // Decision card.
      let title = blk.header.slice(dash + 3).trim();
      let rec = "";
      const rm = title.match(/\(([^)]*)\)\s*$/);
      if (rm) { rec = rm[1].trim(); title = title.slice(0, rm.index).trim(); }
      return { kind: "decision", id: maybeId, title, rec, ...splitCard(blk.lines) };
    }
    // Explainer block (e.g. "How Jet builds a value today …").
    return { kind: "explainer", title: blk.header, html: renderMd(blk.lines.join("\n")) };
  });
}

// Split a decision card body into intro / options / recommendation.
function splitCard(lines) {
  const isOpt = (l) => /^- \*\*Option [A-Za-z0-9] —/.test(l);
  const isRec = (l) => /^\*\*Recommendation:/.test(l);
  const intro = [];
  const options = [];
  let rec = [];
  let mode = "intro";
  let optBuf = null;
  const flushOpt = () => { if (optBuf) { options.push(finishOption(optBuf)); optBuf = null; } };
  for (const line of lines) {
    if (isRec(line)) { flushOpt(); mode = "rec"; rec.push(line); continue; }
    if (isOpt(line)) {
      flushOpt();
      mode = "opt";
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

function finishOption(o) {
  return { key: o.key, name: o.name, html: renderMd(o.lines.join("\n").trim()) };
}

// ---- minimal markdown -> HTML (server-side; client just sets innerHTML) ----

function escapeHtml(s) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

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
    // fenced code (tolerate leading indent from list/option nesting)
    const fence = line.match(/^(\s*)```(\w+)?\s*$/);
    if (fence) {
      flushPara();
      const indent = fence[1].length;
      const lang = fence[2] || "";
      const buf = [];
      i++;
      while (i < lines.length && !/^\s*```/.test(lines[i])) {
        buf.push(lines[i].slice(indent)); // dedent to the fence column
        i++;
      }
      i++; // closing fence
      out.push(`<pre class="code"><code>${highlight(buf.join("\n"), lang)}</code></pre>`);
      continue;
    }
    // table
    if (line.trim().startsWith("|") && i + 1 < lines.length && /^\s*\|?\s*:?-{2,}/.test(lines[i + 1])) {
      flushPara();
      const rows = [];
      while (i < lines.length && lines[i].trim().startsWith("|")) { rows.push(lines[i]); i++; }
      out.push(renderTable(rows));
      continue;
    }
    // list
    if (/^\s*-\s+/.test(line)) {
      flushPara();
      const items = [];
      while (i < lines.length && /^\s*-\s+/.test(lines[i])) {
        items.push(`<li>${inline(lines[i].replace(/^\s*-\s+/, ""))}</li>`);
        i++;
      }
      out.push(`<ul>${items.join("")}</ul>`);
      continue;
    }
    // heading inside body
    const h = line.match(/^(#{2,6})\s+(.+)$/);
    if (h) { flushPara(); out.push(`<h4>${inline(h[2])}</h4>`); i++; continue; }
    // blank
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
  const bodyRows = rows.slice(2).map(cells);
  const th = head.map((c) => `<th>${inline(c)}</th>`).join("");
  const trs = bodyRows
    .map((r) => `<tr>${r.map((c) => `<td>${inline(c)}</td>`).join("")}</tr>`)
    .join("");
  return `<table><thead><tr>${th}</tr></thead><tbody>${trs}</tbody></table>`;
}

// ---- syntax highlighter (multi-language; good enough for a comparison) -----

const KEYWORDS = new Set(
  ("fn struct enum trait impl use module pub return self mut take const if else loop break continue " +
   "for in while match when new init derive let var val pub(crate) error ok value " + // jet-ish + a few neighbours
   "func type var const package import range defer go chan map interface " + // go
   "let mut struct enum impl pub fn trait use mod " + // rust (dup ok)
   "comptime pub const var fn return " + // zig
   "class init func var let struct enum extension protocol guard " + // swift
   "def class return import").split(/\s+/)
);

function highlight(code, _lang) {
  return code.split("\n").map(highlightLine).join("\n");
}

function highlightLine(line) {
  // split off a line comment (// ... ; respect that // inside a string is rare in these snippets)
  let comment = "";
  const cidx = line.indexOf("//");
  let codePart = line;
  if (cidx >= 0) { codePart = line.slice(0, cidx); comment = line.slice(cidx); }
  // tokenize codePart into strings vs non-strings
  let html = "";
  const re = /("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')/g;
  let last = 0; let m;
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
  // s has no strings/comments; escape then wrap keywords, types (Capitalized), numbers.
  return escapeHtml(s)
    .replace(/\b([A-Za-z_][A-Za-z0-9_]*)\b/g, (w) =>
      KEYWORDS.has(w) ? `<span class="k">${w}</span>`
      : /^[A-Z]/.test(w) ? `<span class="t">${w}</span>`
      : w)
    .replace(/\b(\d+\.?\d*)\b/g, '<span class="n">$1</span>');
}

// ---- console status --------------------------------------------------------

function status() {
  const inbox = readInbox();
  const plans = readPlans();
  const ballot = parseBallot(read(P.ballotMd)).filter((s) => s.kind === "decision");

  const line = "─".repeat(64);
  out(`${C.b}Jet task pipeline${C.rst}  ${C.dim}inbox → plan → ballot → ratified → implemented${C.rst}`);
  out(line);

  out(`${C.cyn}INBOX${C.rst}  ${P.inbox.replace(ROOT + "/", "")}`);
  if (inbox.nextTasks.length === 0) out(`  ${C.dim}(no Next Tasks)${C.rst}`);
  inbox.nextTasks.forEach((t) => out(`  • ${truncate(t, 72)}`));
  out(`  ${C.dim}+ ${inbox.considerations} considerations parked${C.rst}`);
  out("");

  out(`${C.cyn}PLANS${C.rst}  ${plans.length} sidequest${plans.length === 1 ? "" : "s"}`);
  plans.forEach((p) => out(`  • ${C.b}${p.slug}${C.rst} ${C.dim}— ${truncate(p.status || p.title, 56)}${C.rst}`));
  out("");

  out(`${C.cyn}BALLOT${C.rst}  ${ballot.length} open decision${ballot.length === 1 ? "" : "s"} awaiting owner`);
  ballot.forEach((d) => {
    const rec = d.rec ? `  ${/no rec/i.test(d.rec) ? C.yel + "NO REC" : C.grn + d.rec.toUpperCase()}${C.rst}` : "";
    out(`  • ${C.b}${d.id}${C.rst} ${truncate(d.title, 50)}${rec}`);
  });
  out("");

  out(`${C.cyn}RATIFIED${C.rst}  ~${ratifiedCount()} decisions logged in syntax-decisions.md`);
  out(line);
  out(
    ballot.length
      ? `${C.yel}▸ ${ballot.length} decision${ballot.length === 1 ? "" : "s"} need your call.${C.rst}  Run: node tools/pipeline/pipeline.mjs serve`
      : `${C.grn}▸ No decisions pending. Plans are clear to implement.${C.rst}`,
  );
}

// ---- serve (dashboard + ballot) --------------------------------------------

function buildState() {
  return {
    inbox: readInbox(),
    plans: readPlans(),
    sections: parseBallot(read(P.ballotMd)),
    ratified: ratifiedCount(),
    lastSubmit: existsSync(P.results) ? (read(P.results).match(/_submitted (.+?)_/) || [, ""])[1] : "",
    regen: existsSync(P.regenQueue)
      ? read(P.regenQueue).split("\n").filter((l) => l.startsWith("- [ ]")).length
      : 0,
  };
}

function writeResults(payload) {
  const when = new Date().toISOString().replace("T", " ").slice(0, 16);
  const lines = [
    "# Owner ballot results",
    "",
    `_submitted ${when}_`,
    "",
    "Decisions captured from the dashboard. Tell Claude **\"go\"** to ratify these",
    "into syntax-decisions.md, strip the cards, and implement the plans.",
    "",
    "## Next Tasks",
    "",
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
    writeFileSync(
      P.regenQueue,
      "# Example-regeneration queue\n\nClaude reviews each checked item against the example criteria " +
        "(human-authored voice, plain language, a user-story scenario, inline cross-language comparison) " +
        "and improves the ballot card before the owner re-reads it.\n\n",
    );
  }
  const when = new Date().toISOString().slice(0, 16).replace("T", " ");
  appendFileSync(P.regenQueue, `- [ ] ${id} — ${title}  _(requested ${when})_\n`);
  return P.regenQueue.replace(ROOT + "/", "");
}

function serve(port) {
  const server = createServer((req, res) => {
    const send = (code, type, body) => { res.writeHead(code, { "content-type": type }); res.end(body); };
    if (req.method === "GET" && (req.url === "/" || req.url.startsWith("/?"))) {
      return send(200, "text/html; charset=utf-8", page());
    }
    if (req.method === "GET" && req.url === "/api/state") {
      return send(200, "application/json", JSON.stringify(buildState()));
    }
    if (req.method === "POST" && (req.url === "/api/submit" || req.url === "/api/regen")) {
      let data = "";
      req.on("data", (c) => (data += c));
      req.on("end", () => {
        try {
          const payload = JSON.parse(data || "{}");
          if (req.url === "/api/submit") {
            const path = writeResults(payload);
            return send(200, "application/json", JSON.stringify({ ok: true, path }));
          }
          const path = queueRegen(payload.id, payload.title);
          return send(200, "application/json", JSON.stringify({ ok: true, path }));
        } catch (e) {
          return send(400, "application/json", JSON.stringify({ ok: false, error: String(e) }));
        }
      });
      return;
    }
    send(404, "text/plain", "not found");
  });
  server.listen(port, "127.0.0.1", () => {
    const url = `http://127.0.0.1:${port}`;
    out(`${C.grn}Jet dashboard${C.rst} → ${C.b}${url}${C.rst}`);
    out(`${C.dim}renders the ballot from docs/spec/decision-ballots.md; submit writes docs/spec/ballot-results.md. Ctrl-C to stop.${C.rst}`);
    if (rest.includes("--open") || rest.includes("-o")) openBrowser(url);
  });
}

// ---- the page (HTML/CSS/client JS as one string) ---------------------------

function page() {
  const state = JSON.stringify(buildState());
  return `<!DOCTYPE html>
<html lang="en"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Jet — Pipeline Dashboard</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{background:#0d1117;color:#e6edf3;font:14px/1.6 ui-monospace,SFMono-Regular,Menlo,monospace;padding:28px 22px 120px;max-width:1100px;margin:0 auto}
h1{font-size:20px;color:#f0f6fc;margin-bottom:2px}
.sub{color:#8b949e;font-size:13px;margin-bottom:24px}
h2{font-size:12px;color:#8b949e;letter-spacing:.09em;text-transform:uppercase;margin:34px 0 14px;border-bottom:1px solid #21262d;padding-bottom:7px}
.pipe{display:flex;gap:8px;flex-wrap:wrap;margin-bottom:8px}
.stage{flex:1;min-width:150px;background:#161b22;border:1px solid #21262d;border-radius:8px;padding:12px 14px}
.stage .n{font-size:22px;font-weight:700;color:#f0f6fc}
.stage .l{font-size:11px;color:#8b949e;text-transform:uppercase;letter-spacing:.06em}
.stage.live{border-color:#388bfd}
.muted{color:#8b949e;font-size:12px}
.muted ul{margin:6px 0 0 16px}
.card{background:#161b22;border:1px solid #21262d;border-radius:8px;padding:20px;margin-bottom:18px}
.explain{background:#0f1620;border:1px solid #1b2433}
.card-id{font-size:11px;color:#58a6ff;font-weight:700;letter-spacing:.04em}
.card-title{font-size:15px;font-weight:700;color:#f0f6fc;margin:4px 0 10px}
.rec{display:inline-block;font-size:10px;font-weight:700;padding:1px 7px;border-radius:20px;background:#10331a;color:#3fb950;border:1px solid #2c7a3f;margin-left:8px;vertical-align:middle}
.rec.no{background:#2d2000;color:#e3b341;border-color:#9e7b1b}
.body p{margin:8px 0}.body ul{margin:8px 0 8px 18px}.body li{margin:3px 0}
.body table{border-collapse:collapse;margin:12px 0;width:100%;font-size:12px}
.body th,.body td{border:1px solid #30363d;padding:6px 9px;text-align:left;vertical-align:top}
.body th{background:#1b2330;color:#f0f6fc}
.opts{display:grid;grid-template-columns:repeat(auto-fit,minmax(300px,1fr));gap:10px;margin:14px 0}
.opt{border:2px solid #21262d;border-radius:7px;padding:12px;cursor:pointer;transition:border-color .12s,background .12s}
.opt:hover{border-color:#388bfd}
.opt.sel{border-color:#58a6ff;background:#0d2044}
.opt-h{display:flex;align-items:center;gap:9px;font-weight:700;color:#f0f6fc;font-size:13px}
.dot{width:15px;height:15px;border-radius:50%;border:2px solid #6b7787;flex-shrink:0}
.opt.sel .dot{border-color:#58a6ff;background:#58a6ff;box-shadow:inset 0 0 0 3px #0d2044}
.opt .body{font-size:12px;color:#adbac7;margin-top:6px}
pre.code{background:#010409;border:1px solid #21262d;border-radius:5px;padding:10px;overflow-x:auto;font-size:11.5px;line-height:1.55;margin:8px 0;white-space:pre;color:#c9d1d9}
code{background:#1b2330;border-radius:4px;padding:0 4px;font-size:.92em}
.body p code,.opt .body code{background:#11202f}
.k{color:#ff7b72}.t{color:#79c0ff}.s{color:#a5d6ff}.c{color:#8b949e;font-style:italic}.n{color:#d2a8ff}
.row{display:flex;align-items:center;gap:12px;margin-top:8px;flex-wrap:wrap}
.clr{font-size:11px;color:#e3b341;cursor:pointer;text-decoration:underline;visibility:hidden}
.clr.on{visibility:visible}
.regen{font-size:11px;color:#58a6ff;cursor:pointer;text-decoration:underline;margin-left:auto}
textarea{width:100%;background:#010409;border:1px solid #30363d;border-radius:5px;color:#e6edf3;font:12px/1.5 ui-monospace,monospace;padding:8px;resize:vertical;min-height:46px;margin-top:8px;outline:none}
textarea:focus{border-color:#388bfd}
.bar{position:fixed;left:0;right:0;bottom:0;background:#0d1117ee;backdrop-filter:blur(6px);border-top:1px solid #21262d;padding:14px 22px;display:flex;align-items:center;gap:16px;max-width:1100px;margin:0 auto}
.bar .p{flex:1;color:#8b949e;font-size:13px}.bar .p b{color:#3fb950}
button{background:#238636;border:1px solid #2ea043;color:#fff;border-radius:6px;padding:9px 18px;font:13px/1 ui-monospace,monospace;font-weight:700;cursor:pointer}
button:hover{background:#2ea043}button:disabled{opacity:.5;cursor:default}
.toast{position:fixed;bottom:74px;left:50%;transform:translateX(-50%);background:#1b2330;border:1px solid #2ea043;color:#e6edf3;padding:10px 16px;border-radius:7px;font-size:12px;opacity:0;transition:opacity .2s;pointer-events:none;max-width:90%}
.toast.on{opacity:1}
</style></head><body>
<h1>Jet — Pipeline Dashboard</h1>
<p class="sub">inbox → plan → ballot → ratified → implemented. Decide below and hit <b>Submit</b> — your choices are saved to <code>docs/spec/ballot-results.md</code>; then tell Claude <b>"go"</b> to ratify &amp; implement.</p>
<div id="dash"></div>
<h2>Open decisions</h2>
<div id="ballot"></div>
<div class="bar"><div class="p" id="prog">0 answered</div>
<button id="submit" onclick="submit()">Submit decisions</button></div>
<div class="toast" id="toast"></div>
<script>
const STATE = ${state};
const answers = {};
const comments = {};

function dash(){
  const s=STATE, dec=s.sections.filter(x=>x.kind==='decision');
  document.getElementById('dash').innerHTML =
    '<div class="pipe">'+
    stage(s.inbox.nextTasks.length,'inbox tasks',false)+
    stage(s.plans.length,'plans',false)+
    stage(dec.length,'open ballots',dec.length>0)+
    stage(s.ratified,'ratified',false)+
    '</div>'+
    '<div class="muted">'+
    (s.lastSubmit?('last submitted '+s.lastSubmit+' → ballot-results.md. '):'')+
    (s.regen?(s.regen+' example-improvement request(s) queued for Claude. '):'')+
    'Inbox: '+(s.inbox.nextTasks.map(t=>esc(t)).join(' · ')||'(none)')+
    ' · +'+s.inbox.considerations+' considerations.</div>';
}
function stage(n,l,live){return '<div class="stage'+(live?' live':'')+'"><div class="n">'+n+'</div><div class="l">'+l+'</div></div>';}
function esc(s){return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');}

function ballot(){
  let html='';
  for(const s of STATE.sections){
    if(s.kind==='explainer'){
      html+='<div class="card explain"><div class="card-title">'+esc(s.title)+'</div><div class="body">'+s.html+'</div></div>';
      continue;
    }
    const recHtml = s.rec ? (/no rec/i.test(s.rec)
        ? '<span class="rec no">NO REC</span>'
        : '<span class="rec">'+s.rec.toUpperCase()+'</span>') : '';
    const opts = s.options.map(o=>
      '<div class="opt" id="o-'+s.id+'-'+o.key+'" onclick="pick(\\''+s.id+'\\',\\''+o.key+'\\')">'+
        '<div class="opt-h"><span class="dot"></span>Option '+o.key+' — '+esc(o.name)+'</div>'+
        '<div class="body">'+o.html+'</div></div>').join('');
    html+='<div class="card"><div class="card-id">'+s.id+recHtml+'</div>'+
      '<div class="card-title">'+esc(s.title)+'</div>'+
      '<div class="body">'+s.intro+'</div>'+
      '<div class="opts">'+opts+'</div>'+
      (s.recommendation?'<div class="body muted"><strong>Recommendation:</strong> '+stripP(s.recommendation)+'</div>':'')+
      '<textarea id="c-'+s.id+'" placeholder="Comment (optional) — e.g. why, or a caveat" oninput="comments[\\''+s.id+'\\']=this.value"></textarea>'+
      '<div class="row"><span class="clr" id="clr-'+s.id+'" onclick="clearPick(\\''+s.id+'\\')">✕ clear selection</span>'+
      '<span class="regen" onclick="regen(\\''+s.id+'\\',\\''+esc(s.title).replace(/\\x27/g,"")+'\\')">↻ improve examples</span></div>'+
      '</div>';
  }
  document.getElementById('ballot').innerHTML=html;
  progress();
}
function stripP(h){return h.replace(/^<p>/,'').replace(/<\\/p>$/,'');}

function pick(id,key){
  answers[id]=key;
  for(const s of STATE.sections) if(s.id===id) for(const o of s.options){
    document.getElementById('o-'+id+'-'+o.key).classList.toggle('sel',o.key===key);
  }
  document.getElementById('clr-'+id).classList.add('on');
  progress();
}
function clearPick(id){
  delete answers[id];
  for(const s of STATE.sections) if(s.id===id) for(const o of s.options)
    document.getElementById('o-'+id+'-'+o.key).classList.remove('sel');
  document.getElementById('clr-'+id).classList.remove('on');
  progress();
}
function progress(){
  const dec=STATE.sections.filter(x=>x.kind==='decision');
  const n=Object.keys(answers).length;
  document.getElementById('prog').innerHTML='<b>'+n+'</b> of '+dec.length+' decided';
}
function toast(msg){const t=document.getElementById('toast');t.textContent=msg;t.classList.add('on');setTimeout(()=>t.classList.remove('on'),3200);}

async function submit(){
  const dec=STATE.sections.filter(x=>x.kind==='decision');
  const results=dec.map(s=>({id:s.id,title:s.title,choice:answers[s.id]||'',comment:comments[s.id]||''}));
  const btn=document.getElementById('submit');btn.disabled=true;btn.textContent='Saving…';
  try{
    const r=await fetch('/api/submit',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({results})});
    const j=await r.json();
    toast(j.ok?('Saved to '+j.path+' — tell Claude "go" to ratify + implement.'):'Error: '+j.error);
    btn.textContent='Submitted ✓';setTimeout(()=>{btn.textContent='Submit decisions';btn.disabled=false;},2500);
  }catch(e){toast('Submit failed: '+e);btn.textContent='Submit decisions';btn.disabled=false;}
}
async function regen(id,title){
  try{
    const r=await fetch('/api/regen',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({id,title})});
    const j=await r.json();
    toast(j.ok?('Queued in '+j.path+' — Claude will improve this card\\'s examples.'):'Error');
  }catch(e){toast('Failed: '+e);}
}
dash();ballot();
</script></body></html>`;
}

// ---- scaffold --------------------------------------------------------------

function scaffold(slug, title) {
  if (!slug) die('usage: pipeline new <slug> "Title"');
  if (!/^[a-z0-9][a-z0-9-]*$/.test(slug)) die(`bad slug "${slug}" — use kebab-case`);
  const file = join(P.sidequests, `${slug}.md`);
  if (existsSync(file)) die(`already exists: ${file.replace(ROOT + "/", "")}`);
  const t = title || slug.replace(/-/g, " ");
  const tmpl = `# ${t}

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
`;
  writeFileSync(file, tmpl);
  out(`${C.grn}created${C.rst} ${file.replace(ROOT + "/", "")}`);
  out(`${C.dim}next: fill it in, then surface its decisions into docs/spec/decision-ballots.md${C.rst}`);
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
  case "serve": serve(Number(rest[0]) || 4173); break;
  case "new": scaffold(rest[0], rest.slice(1).join(" ")); break;
  default: die(`unknown command "${cmd}". commands: status | serve [port] | new <slug> "Title"`);
}
