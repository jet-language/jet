"use strict";
let S = null;
const TABS = [["board", "Board", "▤"], ["decisions", "Decisions", "◈"], ["proposals", "Proposals", "✎"], ["scratch", "Scratch", "✦"]];
const answers = {}, comments = {};
const sec = {};                              // collapse state by key
const filter = { q: "", type: "all" };
let active = location.hash.slice(1) || "board";

// focus mode state
let focusIdx = 0, focusFacet = null;

const esc = (s) => (s || "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const attr = (s) => esc(s).replace(/"/g, "&quot;");
const key = (s) => (s || "").replace(/[^a-z0-9]+/gi, "_");
const $ = (id) => document.getElementById(id);
function toast(m) { const t = $("toast"); t.textContent = m; t.classList.add("on"); clearTimeout(t._); t._ = setTimeout(() => t.classList.remove("on"), 3400); }
async function api(url, body) { const r = await fetch(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) }); return r.json(); }
async function load() { S = await (await fetch("/api/state")).json(); render(); }
const decisions = () => S.ballot.filter((x) => x.kind === "decision");
const stageLabel = (st) => (S.stageLabels && S.stageLabels[st]) || st.replace(/-/g, " ");
const priorityLabel = (p) => (S.priorityLabels && S.priorityLabels[p]) || p || "P2";

function render() {
  if (!TABS.some((t) => t[0] === active)) active = "board";
  const cards = S.board.cards;
  const counts = { board: cards.length, decisions: decisions().length, proposals: (S.proposals || []).length + (S.ideas || []).length, scratch: 0 };
  $("tabs").innerHTML = TABS.map(([id, label, glyph]) =>
    `<div class="tab${id === active ? " on" : ""}" onclick="go('${id}')"><span class="tg">${glyph}</span><span class="tl">${label}</span>` +
    (counts[id] ? `<span class="n">${counts[id]}</span>` : "") + `</div>`).join("");
  TABS.forEach(([id]) => $("v-" + id).classList.toggle("on", id === active));
  $("bar").classList.toggle("on", active === "decisions" && !$("focusback").classList.contains("on"));
  $("rat").textContent = S.ratified || "—";
  $("last").textContent = S.lastSubmit || "—";
  $("ingest-stat").style.display = S.ingestCount ? "inline" : "none";
  $("ingestn").textContent = S.ingestCount || 0;
  renderRibbon(cards); renderBoard(); renderDecisions(); renderProposals(); renderScratch();
}
function go(id) { active = id; location.hash = id; render(); window.scrollTo(0, 0); }

/* ---- pipeline ribbon ---- */
function renderRibbon(cards) {
  $("ribbon").innerHTML = '<span class="elev-label">elevation</span><div class="elev-scale">' + S.stages.map((st, i) => {
    const n = cards.filter((c) => c.stage === st).length;
    const cls = st === "done" ? "done" : (n > 0 && st !== "frozen" ? "hot" : "");
    return `<button class="seg ${cls}" title="jump to ${stageLabel(st)}" onclick="jumpStage('${st}')">` +
      `<span class="tick"></span><span class="lvl">${String(i + 1).padStart(2, "0")}</span>` +
      `<span class="sc">${n}</span><span class="sg">${esc(stageLabel(st))}</span></button>`;
  }).join("") + "</div>";
}
function jumpStage(st) { if (active !== "board") go("board"); sec["stage_" + key(st)] = true; render(); setTimeout(() => { const el = $("sec-stage_" + key(st)); if (el) el.scrollIntoView({ behavior: "smooth", block: "start" }); }, 30); }

/* ---- collapsible section shell ---- */
function section(skey, name, countHtml, preview, inner, extraCls) {
  const open = !!sec[skey];
  return `<div class="sec${open ? " open" : ""}${extraCls ? " " + extraCls : ""}" id="sec-${skey}">` +
    `<div class="sechead" onclick="toggleSec('${skey}')"><span class="caret">&#9656;</span>` +
    `<span class="sname">${esc(name)}</span>${countHtml}<span class="preview">${preview}</span></div>` +
    `<div class="secbody">${inner}</div></div>`;
}
function toggleSec(k) { sec[k] = !sec[k]; const el = $("sec-" + k); if (el) el.classList.toggle("open", sec[k]); }

/* ================= BOARD ================= */
function renderBoard() {
  let h = '<div class="hint">File a task, idea, or bug, then move it down the pipeline. Click any <b>title, description, or note</b> to edit; use the dropdowns to change <b>type</b> or jump <b>stage</b>. Each card shows its <b>computed status</b>.</div>';
  h += worklistPanel();
  h += ingestPanel();
  h += fileForm();
  h += filterBar();
  h += '<div id="board-stages"></div>';
  $("v-board").innerHTML = h;
  renderStages();
}

function worklistPanel() {
  const wl = S.worklist || [];
  if (!wl.length) return "";
  const open = sec.worklist;
  const rows = wl.map((w) =>
    `<div class="wl"><span class="gate ${w.auto ? "auto" : "gated"}">${w.auto ? "auto" : "gated"}</span>` +
    `<span class="ord">${w.workOrder ? "#" + esc(String(w.workOrder)) : "—"}</span>` +
    `<span class="prio ${esc(w.priority || "P2")}">${esc(w.priority || "P2")}</span>` +
    `<span class="wid">${esc(w.id)}</span><span class="wact">${esc(w.text)}</span>` +
    `<span class="wttl">${esc(w.title)}</span></div>`).join("");
  const inner = `<div class="wl-rows">${rows}</div>` +
    `<div class="wl-note"><b>auto</b> = I proceed (build plans / draft decisions) without waiting · ` +
    `<b style="color:var(--amber)">gated</b> = say <b style="color:var(--green)">“go”</b> and I implement.</div>`;
  const cnt = `<span class="count">${wl.length}</span>`;
  return `<div class="sec${open ? " open" : ""} worklist" id="sec-worklist"><div class="sechead" onclick="toggleSec('worklist')">` +
    `<span class="caret">&#9656;</span><span class="sname">Ready for Claude</span>${cnt}` +
    `<span class="preview">${esc(wl.slice(0, 3).map((w) => w.id).join(" · "))}${wl.length > 3 ? " +" + (wl.length - 3) : ""}</span></div>` +
    `<div class="secbody">${inner}</div></div>`;
}

function ingestPanel() {
  return '<div class="panel filebox"><span class="lbl">Ingest a file or notes ' +
    '<span class="n">' + (S.ingestCount || 0) + '</span></span>' +
    '<div class="r"><input class="grow" id="ing-src" placeholder="File path to digest, e.g. docs/notes/ideas.md (optional)">' +
    '<button class="sm" onclick="ingest()">Hand to Claude</button></div>' +
    '<textarea id="ing-note" placeholder="…or paste text / a note about what to look for. I read it, extract candidate ideas / features / syntax, and file them as frozen cards for you to triage."></textarea></div>';
}
async function ingest() {
  const src = $("ing-src"), note = $("ing-note");
  if (!src.value.trim() && !note.value.trim()) { toast("Add a file path or some text."); return; }
  const j = await api("/api/ingest", { source: src.value, note: note.value });
  if (j.ok) { src.value = ""; note.value = ""; await load(); toast("Queued for digest — I'll file candidate cards."); }
  else toast(j.error || "Failed.");
}

function fileForm() {
  return '<div class="panel filebox"><span class="lbl">File new work</span><div class="r">' +
    '<input class="grow" id="add-ttl" placeholder="Task, idea, or bug…">' +
    '<select class="sel" id="add-type"><option value="task">task</option><option value="idea">idea</option><option value="bug">bug</option></select>' +
    '<select class="sel" id="add-stage">' + S.stages.map((s) => `<option value="${s}"${s === "frozen" ? " selected" : ""}>${esc(stageLabel(s))}</option>`).join("") + "</select>" +
    '<select class="sel" id="add-priority">' + (S.priorities || ["P0", "P1", "P2", "P3"]).map((p) => `<option value="${p}"${p === "P2" ? " selected" : ""}>${esc(priorityLabel(p))}</option>`).join("") + "</select>" +
    '<input class="orderin" id="add-order" type="number" min="1" placeholder="order">' +
    '<button class="sm" onclick="addCard()">File</button></div>' +
    '<textarea id="add-body" placeholder="Details (optional)"></textarea></div>';
}
async function addCard() {
  const t = $("add-ttl"); if (!t.value.trim()) return;
  const j = await api("/api/card/add", {
    type: $("add-type").value,
    title: t.value,
    body: $("add-body").value,
    stage: $("add-stage").value,
    priority: $("add-priority").value,
    workOrder: $("add-order").value,
  });
  if (j.ok) { t.value = ""; $("add-body").value = ""; await load(); toast("Filed."); }
}

function filterBar() {
  const types = ["all", "task", "idea", "bug"];
  return '<div class="filterbar"><input class="grow" id="filter-q" placeholder="Filter by name…" value="' + attr(filter.q) + '" oninput="filter.q=this.value;renderStages()">' +
    '<select class="sel" id="filter-type" onchange="filter.type=this.value;renderStages()">' +
    types.map((t) => `<option value="${t}"${filter.type === t ? " selected" : ""}>${t === "all" ? "all types" : t}</option>`).join("") +
    "</select></div>";
}
function matchFilter(c) {
  if (filter.type !== "all" && c.type !== filter.type) return false;
  if (filter.q) { const q = filter.q.toLowerCase(); if (!((c.title || "").toLowerCase().includes(q) || (c.body || "").toLowerCase().includes(q))) return false; }
  return true;
}
function renderStages() {
  const cards = S.board.cards.filter(matchFilter);
  let h = "";
  for (const st of S.stages) {
    const inSt = cards.filter((c) => c.stage === st);
    const inner = inSt.length ? '<div class="grid">' + inSt.map(card).join("") + "</div>" : '<div class="empty">— empty —</div>';
    const prev = inSt.length ? esc(inSt.slice(0, 2).map((c) => c.title).join(" · ") + (inSt.length > 2 ? " +" + (inSt.length - 2) : "")) : "";
    h += section("stage_" + key(st), stageLabel(st), `<span class="count">${inSt.length}</span>`, prev, inner);
  }
  const el = $("board-stages"); if (el) el.innerHTML = h;
}

function findCard(id) { return S.board.cards.find((c) => c.id === id); }
function decIndexById(id) { return decisions().findIndex((d) => d.id === id); }

function statusChips(c) {
  const st = c.status || {};
  let h = `<span class="chip ${st.tone || ""}">${esc(st.label || "")}</span>`;
  if (st.action && !st.owner) h += `<span class="chip ${st.auto ? "auto" : "gated"}">${st.auto ? "auto" : "gated"}</span>`;
  if (st.owner && (st.blockedBy || []).length) {
    h += '<span class="blockedby">' + st.blockedBy.map((id) => {
      const i = decIndexById(id);
      return i >= 0 ? `<a onclick="enterFocus(${i})" title="open this decision">${esc(id)}</a>` : `<a title="decision not drafted">${esc(id)}</a>`;
    }).join("") + "</span>";
  }
  return `<div class="statusrow">${h}</div>`;
}

function card(c) {
  const i = S.stages.indexOf(c.stage);
  const back = i > 0 ? S.stages[i - 1] : null, fwd = i < S.stages.length - 1 ? S.stages[i + 1] : null;
  const planLink = c.plan ? `<a class="plan" title="open plan" onclick="openDoc('sidequest','${attr(c.plan)}')">▤ ${esc(c.plan)}</a>` : "";
  const typeSel = `<select class="typesel" title="change type" onchange="changeType('${c.id}',this.value)">` +
    ["task", "idea", "bug"].map((t) => `<option${c.type === t ? " selected" : ""}>${t}</option>`).join("") + "</select>";
  const prioritySel = `<select class="priosel ${esc(c.priority || "P2")}" title="change priority" onchange="changePriority('${c.id}',this.value)">` +
    (S.priorities || ["P0", "P1", "P2", "P3"]).map((p) => `<option value="${p}"${(c.priority || "P2") === p ? " selected" : ""}>${esc(p)}</option>`).join("") + "</select>";
  const stageSel = `<select class="stagesel" title="jump to stage" onchange="moveCard('${c.id}',this.value)">` +
    S.stages.map((s) => `<option value="${s}"${s === c.stage ? " selected" : ""}>${esc(stageLabel(s))}</option>`).join("") + "</select>";
  const notes = (c.notes && c.notes.length) ? '<div class="notes">' + c.notes.map((n, idx) =>
    `<div class="note">•&nbsp;<span class="nt" contenteditable="true" spellcheck="false" data-id="${c.id}" data-i="${idx}" data-orig="${attr(n.t)}" onblur="saveNote(this)" onkeydown="fieldKey(event,this)">${esc(n.t)}</span>` +
    `<span class="at">${esc(n.at)}</span><span class="ndel" title="delete note" onclick="delNote('${c.id}',${idx})">✕</span></div>`).join("") + "</div>" : "";
  return `<div class="card ${c.type}"><div class="edge"></div><div class="body">` +
    `<div class="top"><span class="cid">${esc(c.id)}</span>${c.workOrder ? `<span class="ord">#${esc(String(c.workOrder))}</span>` : ""}${prioritySel}${typeSel}</div>` +
    `<span class="ttl" contenteditable="true" spellcheck="false" data-id="${c.id}" data-orig="${attr(c.title)}" onblur="saveField(this,'title')" onkeydown="fieldKey(event,this)">${esc(c.title)}</span>` +
    `<div class="bd" contenteditable="true" spellcheck="false" data-ph="${c.type === "bug" ? "Describe the defect…" : "Add a description…"}" data-id="${c.id}" data-orig="${attr(c.body || "")}" onblur="saveField(this,'body')" onkeydown="fieldKey(event,this)">${esc(c.body || "")}</div>` +
    statusChips(c) +
    `<div class="meta"><div class="nav">` +
    `<button onclick="moveCard('${c.id}','${back || ""}')" ${back ? "" : "disabled"} title="${back ? stageLabel(back) : ""}">◀</button>` +
    stageSel +
    `<button onclick="moveCard('${c.id}','${fwd || ""}')" ${fwd ? "" : "disabled"} title="${fwd ? stageLabel(fwd) : ""}">▶</button></div>` +
    `<input class="orderedit" type="number" min="1" placeholder="order" value="${c.workOrder || ""}" title="recommended work order" onchange="changeWorkOrder('${c.id}',this.value)">` +
    planLink + `<span class="note-in" onclick="addNote('${c.id}')">+note</span>` +
    `<span class="x" title="delete" onclick="delCard('${c.id}')">✕</span></div>${notes}</div></div>`;
}
async function moveCard(id, stage) { if (!stage) return; const c = findCard(id); if (c) c.stage = stage; await api("/api/card/update", { id, stage }); await load(); }
async function changeType(id, type) { const c = findCard(id); if (c) c.type = type; await api("/api/card/update", { id, type }); await load(); }
async function changePriority(id, priority) { const c = findCard(id); if (c) c.priority = priority; await api("/api/card/update", { id, priority }); await load(); }
async function changeWorkOrder(id, workOrder) { const c = findCard(id); if (c) c.workOrder = workOrder ? Number(workOrder) : null; await api("/api/card/update", { id, workOrder }); await load(); }
async function delCard(id) { if (confirm("Delete this card?")) { await api("/api/card/delete", { id }); await load(); toast("Deleted."); } }
async function addNote(id) { const n = prompt("Add a note:"); if (n && n.trim()) { await api("/api/card/update", { id, note: n }); await load(); } }
async function saveField(el, field) {
  const id = el.dataset.id, val = el.innerText.trim(), orig = el.dataset.orig || "";
  if (val === orig) return;
  if (field === "title" && !val) { el.innerText = orig; return; }
  const j = await api("/api/card/update", { id, [field]: val });
  if (j.ok) { el.dataset.orig = val; const c = findCard(id); if (c) c[field] = val; toast("Saved."); }
  else { el.innerText = orig; toast("Save failed."); }
}
async function saveNote(el) {
  const id = el.dataset.id, i = +el.dataset.i, val = el.innerText.trim(), orig = el.dataset.orig || "";
  if (val === orig) return;
  const c = findCard(id); if (!c) return;
  const notes = (c.notes || []).map((n, idx) => idx === i ? { t: val, at: n.at } : n).filter((n) => n.t);
  const j = await api("/api/card/update", { id, notes });
  if (j.ok) { c.notes = notes; el.dataset.orig = val; if (!val) await load(); toast("Saved."); }
  else { el.innerText = orig; toast("Save failed."); }
}
async function delNote(id, i) {
  const c = findCard(id); if (!c) return;
  if (!confirm("Delete this note?")) return;
  const notes = (c.notes || []).filter((_, idx) => idx !== i);
  const j = await api("/api/card/update", { id, notes });
  if (j.ok) { c.notes = notes; await load(); toast("Note deleted."); }
}
function fieldKey(e, el) {
  if (e.key === "Enter" && !e.shiftKey && el.classList.contains("ttl")) { e.preventDefault(); el.blur(); }
  else if (e.key === "Escape") { el.innerText = el.dataset.orig || ""; el.blur(); }
}

/* ================= DECISIONS — overview ================= */
function renderDecisions() {
  const dec = decisions();
  const done = dec.filter((d) => answers[d.id]).length;
  let h = '<div class="ballotbar"><div class="meter"><div class="mtop"><span>Decisions</span><span><b>' + done + "</b> / " + dec.length + " decided</span></div>" +
    '<div class="track"><i style="width:' + (dec.length ? Math.round(done / dec.length * 100) : 0) + '%"></i></div></div>' +
    (dec.length ? '<button class="bigfocus" onclick="enterFocus(' + Math.max(0, dec.findIndex((d) => !answers[d.id])) + ')">▶ Focus mode — decide one at a time</button>' : "") +
    "</div>";
  h += '<div class="hint">Open <b>Focus mode</b> for a clean, one-at-a-time deck (keyboard-driven), or skim the groups below and click a row to jump straight into it.</div>';

  // Group the decisions. Global (ungrouped) explainer prose — the file preamble —
  // is dropped; per-group intro prose is tucked inside its group (collapsed by
  // default), so the page is decisions, not preamble.
  const groups = [], byG = {};
  for (const s of S.ballot) {
    if (s.kind === "explainer" && !s.group) continue;                 // drop the file preamble
    if (!["decision", "open", "explainer"].includes(s.kind)) continue;
    const g = s.group || "Other";
    if (!byG[g]) { byG[g] = []; groups.push(g); }
    byG[g].push(s);
  }
  for (const g of groups) {
    const items = byG[g];
    const ids = items.filter((s) => s.kind === "decision").map((s) => s.id);
    if (!ids.length && !items.some((s) => s.kind === "open")) continue; // skip explainer-only groups
    const gdone = ids.filter((id) => answers[id]).length;
    const cnt = '<span class="count' + (ids.length && gdone === ids.length ? " ok" : "") + '">' + gdone + " / " + ids.length + "</span>";
    const inner = items.map((s) =>
      s.kind === "decision" ? miniRow(s) :
      s.kind === "open" ? openRow(s) :
      '<div class="gintro">' + s.html + "</div>").join("");
    h += section("grp_" + key(g), g.replace(/\s*—\s*board card.*/, ""), cnt, esc(ids.join("  ")), inner, "dgroup");
  }
  $("v-decisions").innerHTML = h;
}
function miniRow(s) {
  const i = decIndexById(s.id);
  const rec = s.rec ? (/no rec/i.test(s.rec) ? '<span class="rec no">no rec</span>' : '<span class="rec">rec ' + esc(s.rec.replace(/^rec\s+/i, "").toUpperCase()) + "</span>") : "";
  return '<div class="drow-mini' + (answers[s.id] ? " decided" : "") + '" onclick="enterFocus(' + i + ')">' +
    '<span class="did">' + esc(s.id) + '</span><span class="dt">' + esc(s.title) + "</span>" +
    (answers[s.id] ? '<span class="tick">✓ ' + esc(answers[s.id]) + "</span>" : "") + rec + "</div>";
}
function openRow(s) {
  return '<div class="drow-mini" onclick="ask(\'' + (s.id || "") + '\')"><span class="did">' + esc(s.id || "—") + '</span>' +
    '<span class="dt">' + (s.html || "").replace(/<[^>]+>/g, "").slice(0, 90) + '</span><span class="rec no">open</span></div>';
}

/* ================= DECISION FOCUS MODE ================= */
const FACETS = [["why", "Why it matters"], ["code", "In the wild"], ["trade", "Trade-offs"], ["else", "Elsewhere"], ["qa", "Q&A"]];
function facetContent(s, fk) {
  if (fk === "why") return [s.story, s.intro].filter(Boolean).join("\n") || '<p class="muted">No story yet — ask me to add one (↻ improve).</p>';
  if (fk === "code") return s.inWild || '<p class="muted">No in-the-wild example yet.</p>';
  if (fk === "trade") return s.tradeoffs || '<p class="muted">Trade-offs are folded into the story / options above.</p>';
  if (fk === "else") return s.otherLangs || '<p class="muted">No cross-language comparison yet.</p>';
  if (fk === "qa") return s.qa || '<p class="muted">No questions yet — ask one below.</p>';
  return "";
}
function availFacets(s) {
  return FACETS.filter(([fk]) =>
    (fk === "why" && (s.story || s.intro)) || (fk === "code" && s.inWild) ||
    (fk === "trade" && s.tradeoffs) || (fk === "else" && s.otherLangs) || (fk === "qa" && s.qa));
}
function enterFocus(i) {
  const dec = decisions(); if (!dec.length) return;
  focusIdx = Math.max(0, Math.min(i, dec.length - 1));
  focusFacet = null;
  $("focusback").classList.add("on");
  $("bar").classList.remove("on");
  renderDeck();
}
function exitFocus() { $("focusback").classList.remove("on"); render(); }
function focusPrev() { if (focusIdx > 0) { focusIdx--; focusFacet = null; renderDeck(); } }
function focusNext() { const dec = decisions(); if (focusIdx < dec.length - 1) { focusIdx++; focusFacet = null; renderDeck(); } else toast("Last decision — sign & file when ready."); }
function jumpNextUndecided() {
  const dec = decisions(); const next = dec.findIndex((d) => !answers[d.id]);
  if (next < 0) { toast("All decided — sign & file."); return; }
  focusIdx = next; focusFacet = null; renderDeck();
}
function setFacet(fk) { focusFacet = fk; renderDeck(); }
function renderDeck() {
  const dec = decisions(); const s = dec[focusIdx]; if (!s) return;
  $("f-ctr").textContent = focusIdx + 1; $("f-tot").textContent = dec.length;
  $("f-dots").innerHTML = dec.map((d, i) => `<span class="dot${i === focusIdx ? " cur" : ""}${answers[d.id] ? " done" : ""}" title="${esc(d.id)}" onclick="enterFocus(${i})"></span>`).join("");
  $("f-prev").disabled = focusIdx === 0;
  $("f-next").disabled = focusIdx === dec.length - 1;

  const avail = availFacets(s);
  if (!focusFacet || !avail.some(([fk]) => fk === focusFacet)) focusFacet = avail.length ? avail[0][0] : "why";
  const rec = s.rec ? (/no rec/i.test(s.rec) ? '<span class="rec no">no rec</span>' : '<span class="rec">rec ' + esc(s.rec.replace(/^rec\s+/i, "").toUpperCase()) + "</span>") : "";
  const bigline = s.gist || s.title;
  const sub = s.gist ? '<div class="dttl">' + esc(s.title) + "</div>" : "";

  const facetTabs = avail.length ? '<div class="facets">' + avail.map(([fk, label]) =>
    `<span class="facet${fk === focusFacet ? " on" : ""}" onclick="setFacet('${fk}')">${label}</span>`).join("") + "</div>" : "";
  const facetPanel = avail.length ? '<div class="facetbody">' + facetContent(s, focusFacet) + "</div>" : "";

  const opts = (s.options || []).map((o, idx) =>
    `<div class="opt" id="o-${s.id}-${o.key}" onclick="pick('${s.id}','${o.key}')">` +
    `<div class="opt-h"><span class="radio"></span><span class="num">${idx + 1}</span>Option ${esc(o.key)} — ${esc(o.name)}` +
    (o.recommended ? '<span class="recpill">recommended</span>' : "") + "</div>" +
    `<div class="obody">${o.html}</div></div>`).join("");

  const qs = (S.board.questions || []).filter((q) => q.decisionId === s.id);
  const qhtml = qs.length ? '<div class="qbox">' + qs.map((q) => '<div class="q">' + esc(q.text) + '<span class="st ' + q.status + '">' + q.status + "</span>" +
    (q.answer ? '<div class="ans">' + esc(q.answer) + "</div>" : '<div class="qa">awaiting Claude</div>') + "</div>").join("") + "</div>" : "";

  $("f-deck").innerHTML =
    `<div class="dhead"><span class="did">${esc(s.id)}</span>${rec}</div>` +
    `<div class="gist">${esc(bigline)}</div>${sub}` +
    facetTabs + facetPanel +
    (opts ? '<div class="optslabel">Choose one</div><div class="opts">' + opts + "</div>" : "") +
    (s.recommendation ? '<div class="recline"><strong>Recommendation:</strong> ' + stripP(s.recommendation) + "</div>" : "") +
    `<textarea class="comment" placeholder="Comment (optional)" oninput="comments['${s.id}']=this.value">${esc(comments[s.id] || "")}</textarea>` +
    '<div class="deck-actions">' +
    (answers[s.id] ? `<button class="ghost sm" onclick="clearPick('${s.id}')">✕ Clear choice</button>` : "") +
    `<button class="ghost sm" onclick="ask('${s.id}')">Ask a question</button>` +
    `<button class="ghost sm" onclick="regen('${s.id}')">↻ Improve examples</button></div>` + qhtml;

  if (answers[s.id]) markPick(s.id, answers[s.id]);
  $("f-scroll").scrollTop = 0;
}
function stripP(h) { return (h || "").replace(/^<p>/, "").replace(/<\/p>$/, ""); }
function markPick(id, k) {
  const s = decisions().find((x) => x.id === id); if (!s || !s.options) return;
  for (const o of s.options) { const el = $("o-" + id + "-" + o.key); if (el) el.classList.toggle("sel", o.key === k); }
}
function pick(id, k) { answers[id] = k; renderDeck(); }
function clearPick(id) { delete answers[id]; renderDeck(); }

async function ask(id) {
  const t = prompt("What do you want to know about " + id + "?");
  if (t && t.trim()) { const j = await api("/api/ask", { decisionId: id, text: t }); if (j.ok) { S.board.questions.push(j.q); if ($("focusback").classList.contains("on")) renderDeck(); else render(); toast("Question saved — I'll answer on this card."); } }
}
async function regen(id) {
  const s = decisions().find((x) => x.id === id); const title = s ? s.title : id;
  const j = await api("/api/regen", { id, title }); if (j.ok) toast("Queued — I'll improve this card's examples.");
}
async function submitBallot() {
  const dec = decisions();
  const results = dec.map((s) => ({ id: s.id, title: s.title, choice: answers[s.id] || "", comment: comments[s.id] || "" }));
  for (const b of [$("submit"), $("f-submit")]) if (b) { b.disabled = true; b.textContent = "Filing…"; }
  const j = await api("/api/submit", { results });
  if (!j.ok) { toast("Error"); for (const b of [$("submit"), $("f-submit")]) if (b) { b.textContent = "Sign & file"; b.disabled = false; } return; }
  toast("Filed to " + j.path + " — reloading…");
  setTimeout(() => location.reload(), 900);
}

/* keyboard for focus mode */
document.addEventListener("keydown", (e) => {
  if (!$("focusback").classList.contains("on")) return;
  const t = e.target.tagName, editing = t === "INPUT" || t === "TEXTAREA" || e.target.isContentEditable;
  if (e.key === "Escape") { e.preventDefault(); exitFocus(); return; }
  if (editing) return;
  if (e.key === "ArrowRight") { e.preventDefault(); focusNext(); }
  else if (e.key === "ArrowLeft") { e.preventDefault(); focusPrev(); }
  else if (/^[1-9]$/.test(e.key)) {
    const s = decisions()[focusIdx]; const o = s && s.options && s.options[+e.key - 1];
    if (o) { e.preventDefault(); pick(s.id, o.key); }
  } else if (e.key === "Enter") {
    e.preventDefault(); const s = decisions()[focusIdx];
    if (s && answers[s.id]) focusNext(); else toast("Pick an option (1–9) first.");
  }
});

/* ================= PROPOSALS + IDEAS ================= */
function renderProposals() {
  const ps = S.proposals || [], ideas = S.ideas || [];
  let h = '<div class="hint">Feature <b>proposals</b> and parked <b>ideas</b> — exploratory thinking being shaped. Click any card to read or edit it inline.</div>';
  h += docGrid("Proposals", "proposal", ps);
  h += docGrid("Ideas", "idea", ideas);
  $("v-proposals").innerHTML = h;
}
function docGrid(label, kind, list) {
  const inner = list.length ? '<div class="grid">' + list.map((p) =>
    `<div class="card task pcard" onclick="openDoc('${kind}','${attr(p.slug)}')"><div class="edge"></div><div class="body">` +
    `<div class="top"><span class="ttl">${esc(p.title)}</span></div>` +
    (p.status ? `<div class="bd">${esc(p.status)}</div>` : "") +
    `<div class="meta"><span class="plan">▤ ${esc(p.slug)}.md</span></div></div></div>`).join("") + "</div>"
    : '<div class="empty">— none —</div>';
  return section("doc_" + key(label), label, `<span class="count">${list.length}</span>`, esc(list.slice(0, 2).map((p) => p.title).join(" · ")), inner);
}

/* ================= SCRATCH ================= */
let scratchT = null;
function renderScratch() {
  const v = $("v-scratch"); if (v.dataset.init) return; v.dataset.init = "1";
  v.innerHTML = '<div class="hint">A free scratch pad — anything goes. Autosaves to board.json as you type; persists across restarts.</div>' +
    '<textarea id="scratch" placeholder="Notes, half-thoughts, paste anything…" oninput="scratchChanged()" onblur="saveScratch()">' + esc(S.board.scratch) + "</textarea>" +
    '<div class="savebar"><span class="s" id="scratch-s">saved</span></div>';
}
function scratchChanged() { const s = $("scratch-s"); if (s) s.textContent = "editing…"; clearTimeout(scratchT); scratchT = setTimeout(saveScratch, 1500); }
async function saveScratch() {
  clearTimeout(scratchT); const el = $("scratch"); if (!el) return;
  const t = el.value, s = $("scratch-s");
  if (t === S.board.scratch) { if (s) s.textContent = "saved"; return; }
  const j = await api("/api/scratch", { text: t });
  if (j.ok) { S.board.scratch = t; if (s) s.textContent = "saved " + new Date().toLocaleTimeString(); }
  else if (s) s.textContent = "save failed";
}

/* ================= DOC VIEWER / EDITOR ================= */
let curDoc = null, curDocRaw = "";
async function openDoc(kind, slug) {
  if (!slug) return;
  const j = await api("/api/doc/get", { kind, slug });
  if (!j.ok) { toast("Could not open " + slug); return; }
  curDoc = { kind, slug }; curDocRaw = j.raw;
  $("doc-title").textContent = j.title || slug;
  $("doc-path").textContent = j.path || "";
  $("doc-view").innerHTML = j.html;
  $("doc-edit-area").value = j.raw;
  setDocMode(false);
  $("docback").classList.add("on");
  $("doc-view").scrollTop = 0;
}
function setDocMode(edit) {
  $("doc-view").style.display = edit ? "none" : "block";
  $("doc-edit-area").style.display = edit ? "block" : "none";
  $("doc-save").style.display = edit ? "inline-block" : "none";
  $("doc-cancel").style.display = edit ? "inline-block" : "none";
  $("doc-edit").style.display = edit ? "none" : "inline-block";
}
function toggleEdit() { setDocMode(true); $("doc-edit-area").focus(); }
function cancelEdit() { $("doc-edit-area").value = curDocRaw; setDocMode(false); }
async function saveDoc() {
  if (!curDoc) return;
  const text = $("doc-edit-area").value, b = $("doc-save");
  b.disabled = true; b.textContent = "Saving…";
  const j = await api("/api/doc/save", { kind: curDoc.kind, slug: curDoc.slug, text });
  b.disabled = false; b.textContent = "Save";
  if (!j.ok) { toast("Save failed"); return; }
  curDocRaw = text; $("doc-view").innerHTML = j.html; setDocMode(false); toast("Saved " + (j.path || ""));
}
function closeDoc(e) { if (e && e.target && e.target.id !== "docback") return; $("docback").classList.remove("on"); curDoc = null; }
document.addEventListener("keydown", (e) => { if (e.key === "Escape" && $("docback").classList.contains("on")) closeDoc(); });

load();
window.addEventListener("hashchange", () => { const h = location.hash.slice(1); if (h && h !== active) { active = h; render(); } });
