// Tower client. Vanilla JS, no framework, no build.
// Three views: Now (everything blocked on the owner), Agents (talk to your
// agents), Board (epochs → milestones → cards). The beacon on the left edge
// carries one lit segment per owner-blocking item; clearing them darkens it.

let S = null;                 // projected state from /api/state
let ROSTER = [];              // /api/agents presence
let VIEW = 'now';
let THREAD = null;            // selected agent name in Agents view
let openCard = null;
let focusIds = null, focusIdx = 0, focusFacet = null, askOpen = false, focusCompare = false;
const pick = {};              // decisionId -> tentative option key

const $ = (s, r = document) => r.querySelector(s);
const el = (h) => { const t = document.createElement('template'); t.innerHTML = h.trim(); return t.content.firstElementChild; };
const esc = (s) => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const md = (s) => esc(s).replace(/`([^`]+)`/g, '<code>$1</code>').replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
  .split(/\n{2,}/).map(p => `<p>${p.replace(/\n/g, '<br>')}</p>`).join('');

// ---- tiny generic syntax highlighter (tokenize, then escape per token) ----
const HL_KW = new Set(('fn func function def let var const val mut return yield if elif else match switch case when default for while loop do in of as is import use mod module package from pub priv private public protected internal static struct enum trait impl interface type class extends implements where async await comptime defer go chan select new self this super sizeof typeof null nil none None true false True False and or not break continue throw try catch finally with lambda then begin end macro derive emit').split(' '));
function hl(src) {
  const re = /(\/\/[^\n]*|\/\*[\s\S]*?\*\/|#\s[^\n]*)|([#@][A-Za-z_]\w*)|("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)|(0[xX][0-9a-fA-F_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)|([A-Za-z_$]\w*)|(\s+)|([\s\S])/g;
  let m, out = '';
  while ((m = re.exec(src))) {
    if (m[1]) out += `<span class="hl-c">${esc(m[1])}</span>`;
    else if (m[2]) out += `<span class="hl-t">${esc(m[2])}</span>`;
    else if (m[3]) out += `<span class="hl-s">${esc(m[3])}</span>`;
    else if (m[4]) out += `<span class="hl-n">${esc(m[4])}</span>`;
    else if (m[5]) {
      const w = m[5], next = src[re.lastIndex];
      if (HL_KW.has(w)) out += `<span class="hl-k">${esc(w)}</span>`;
      else if (next === '(') out += `<span class="hl-f">${esc(w)}</span>`;
      else if (/^[A-Z]/.test(w)) out += `<span class="hl-t">${esc(w)}</span>`;
      else out += esc(w);
    } else if (m[6]) out += m[6];
    else out += esc(m[7]);
  }
  return out;
}
const codeBlock = (s) => `<pre class="code">${hl(s || '')}</pre>`;

// ---- api ------------------------------------------------------------------
let toastTimer = null;
function toast(text, err = false) {
  const t = $('#toast');
  t.textContent = text; t.className = 'toast' + (err ? ' toast--err' : ''); t.hidden = false;
  clearTimeout(toastTimer); toastTimer = setTimeout(() => { t.hidden = true; }, err ? 5000 : 2200);
}
// Undoable actions surface a toast with an Undo button; expectRev pins the
// undo to the state the action produced, so it can never revert someone
// else's interleaved write.
const UNDOABLE = { 'card/delete': 'Card deleted', 'clearance': 'Decision recorded', 'clearance/batch': 'Decisions recorded', 'card/activate': 'Greenlit', 'idea/delete': 'Idea dismissed', 'milestone/delete': 'Milestone deleted' };

const api = async (route, payload) => {
  const r = await fetch('/api/' + route, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(payload || {}) });
  const j = await r.json().catch(() => ({}));
  if (!r.ok || j.ok === false) { toast(j.message || `request failed: ${route}`, true); throw new Error(j.message || route); }
  if (j.state) applyState(j.state, { own: true });
  if (UNDOABLE[route] && j.state) undoToast(UNDOABLE[route], j.state.meta.rev);
  return j.result;
};

// ---- live state: SSE first, gentle polling as fallback -----------------------
// The page must NEVER yank the DOM out from under the owner: passive updates
// only re-render when the data actually changed (rev) AND the owner isn't
// mid-read/mid-type. Otherwise they wait in `pending` until it's safe.
let pending = null;
let es = null;

function uiBusy() {
  if (focusIds) return true;                                   // reading/deciding a ballot
  if (openCard) return true;                                   // card modal open
  const a = document.activeElement;
  if (a && (/INPUT|TEXTAREA|SELECT/.test(a.tagName) || a.isContentEditable)) return true;
  const sel = window.getSelection?.();
  if (sel && !sel.isCollapsed) return true;                    // text selected
  return false;
}

function applyState(next, { own = false } = {}) {
  if (!next) return;
  const changed = !S || next.meta.rev !== S.meta.rev;
  if (own) { S = next; renderPreservingScroll(); return; }
  if (!changed) { S = next; renderBeacon(); return; }          // nothing new — cheap refresh only
  if (uiBusy()) {
    pending = next;
    S = next;                    // data is current for actions; DOM stays put
    renderBeacon();              // beacon + pill may update, they hold no focus
    updatePill();
    return;
  }
  S = next; pending = null;
  renderPreservingScroll();
}

function maybeApplyPending() {
  if (pending && !uiBusy()) { pending = null; renderPreservingScroll(); }
}
document.addEventListener('focusout', () => setTimeout(maybeApplyPending, 80));
document.addEventListener('selectionchange', () => { const s = window.getSelection?.(); if (s?.isCollapsed) setTimeout(maybeApplyPending, 300); });

function renderPreservingScroll() {
  const y = window.scrollY;
  const log = $('.thread__log');
  const atBottom = log ? log.scrollHeight - log.scrollTop - log.clientHeight < 60 : false;
  render();
  window.scrollTo(0, y);
  const log2 = $('.thread__log');
  if (log2 && atBottom) log2.scrollTop = log2.scrollHeight;
}

function connectStream() {
  try { es = new EventSource('/api/stream'); } catch { return scheduleFallbackPoll(); }
  es.addEventListener('state', (e) => { try { applyState(JSON.parse(e.data)); } catch { /* bad frame */ } });
  es.onerror = () => { es.close(); es = null; scheduleFallbackPoll(); setTimeout(connectStream, 8000); };
}
let pollTimer = null;
function scheduleFallbackPoll() {
  clearInterval(pollTimer);
  pollTimer = setInterval(async () => {
    if (es || document.hidden) return;
    try { applyState(await (await fetch('/api/state')).json()); } catch { /* offline */ }
  }, 30_000);
}

async function refresh() {
  try { applyState(await (await fetch('/api/state')).json()); } catch { /* offline */ }
}
async function refreshRoster() {
  try { ROSTER = await (await fetch('/api/agents')).json(); } catch { ROSTER = []; }
}

// ---- derived --------------------------------------------------------------
const cardById = (id) => S.cards.find(c => c.id === id);
const ticket = (c) => '#' + (c.num ?? '');
const CFG = () => S.config || {};
const TERM = (k, fb) => ((CFG().terms || {})[k] || fb);
const epochTag = (e) => e ? (e.num != null ? `${TERM('epoch', 'Epoch')} ${e.num}` : e.id) : '';
const openDecisions = () => S.decisions.filter(d => d.status !== 'ratified');
const toActivate = () => S.cards.filter(c => c.lane.lane === 'activate');
const threadsOf = () => {
  const map = new Map();
  for (const m of S.messages) {
    const k = m.from === 'owner' ? m.to : m.from;
    if (!map.has(k)) map.set(k, { agent: k, messages: [], unread: 0 });
    const t = map.get(k);
    t.messages.push(m);
    if (m.to === 'owner' && !m.readAt) t.unread++;
  }
  return map;
};
const unreadThreads = () => [...threadsOf().values()].filter(t => t.unread > 0);

// waiting-time chip: shown once something has sat for 6+ hours
function ageChip(iso) {
  if (!iso) return '';
  const h = (Date.now() - new Date(iso).getTime()) / 3.6e6;
  if (h < 6) return '';
  const label = h < 48 ? Math.round(h) + 'h' : Math.round(h / 24) + 'd';
  return `<span class="agechip ${h > 72 ? 'agechip--hot' : ''}" title="waiting ${label}">${label}</span>`;
}
const ageOf = (it) => it.type === 'msg' ? it.thread.messages.filter(m => m.to === 'owner' && !m.readAt)[0]?.at
  : it.type === 'decision' ? it.decision.created : it.card.created;

// Every owner-blocking item, in the order the beacon + Now view show them.
function duties() {
  const out = [];
  for (const t of unreadThreads()) out.push({ type: 'msg', id: 'msg:' + t.agent, thread: t });
  for (const d of openDecisions()) out.push({ type: 'decision', id: d.id, decision: d });
  for (const c of toActivate()) out.push({ type: 'activate', id: c.id, card: c });
  return out;
}

// ---- beacon -----------------------------------------------------------------
function renderBeacon() {
  const b = $('#beacon');
  const items = duties();
  b.innerHTML = '';
  b.classList.toggle('beacon--clear', !items.length);
  for (const it of items.slice(0, 40)) {
    const h = (Date.now() - new Date(ageOf(it) || Date.now()).getTime()) / 3.6e6;
    const seg = el(`<button class="beacon__seg" style="opacity:${Math.min(1, .55 + h / 96).toFixed(2)}" title="${esc(it.type === 'msg' ? `message from ${it.thread.agent}` : it.type === 'decision' ? it.decision.title : 'greenlight: ' + it.card.title)}"></button>`);
    seg.addEventListener('click', () => jumpTo(it));
    b.appendChild(seg);
  }
}
function jumpTo(it) {
  if (it.type === 'msg') { THREAD = it.thread.agent; go('agents'); }
  else if (it.type === 'decision') focusAll(it.decision.id);
  else { go('now'); }
}

// ---- chrome ------------------------------------------------------------------
const VIEWS = [
  { id: 'now', name: 'Now', count: () => duties().length, alert: true },
  { id: 'agents', name: 'Agents', count: () => S.counts.unreadForOwner, alert: true },
  { id: 'board', name: 'Board', count: () => S.cards.filter(c => c.phase !== 'done' && c.phase !== 'frozen').length },
];
function renderChrome() {
  document.title = `Tower · ${S.meta.project || 'project'}`;
  $('#project-name').textContent = S.meta.project || '';
  const tabs = $('#tabs');
  tabs.innerHTML = '';
  for (const v of VIEWS) {
    const n = v.count();
    const t = el(`<button class="tab" aria-current="${VIEW === v.id}">${v.name}
        <span class="tab__n ${v.alert && n ? 'alert' : ''}">${n}</span></button>`);
    t.addEventListener('click', () => go(v.id));
    tabs.appendChild(t);
  }
  $('#feed').innerHTML = `<b>${S.cards.length}</b> cards · <b>${S.counts.agentReady}</b> agent-ready`;
  updatePill();
}
function updatePill() {
  const fy = duties().length;
  const pill = $('#pill');
  pill.className = 'top__pill' + (fy ? '' : ' clear');
  pill.innerHTML = fy ? `<span class="beat"></span> ${fy} for you` : '✓ tower clear';
  pill.onclick = () => go('now');
}
function undoToast(label, rev) {
  const t = $('#toast');
  t.className = 'toast'; t.hidden = false;
  t.innerHTML = `${esc(label)} <button class="btn btn--sm" style="margin-left:10px" id="undo-btn">Undo</button>`;
  $('#undo-btn', t).addEventListener('click', async () => {
    t.hidden = true;
    await api('undo', { expectRev: rev });
    toast('undone');
  });
  clearTimeout(toastTimer); toastTimer = setTimeout(() => { t.hidden = true; }, 7000);
}

// ---- NOW ----------------------------------------------------------------------
function viewNow() {
  const v = $('#view');
  const items = duties();
  v.innerHTML = `<div class="viewhead"><h1 class="h1">Now</h1>
    <span class="viewhead__sub">${items.length ? `<b>${items.length}</b> waiting on you — clear the beacon` : 'nothing needs you'}</span>
    ${openDecisions().length ? `<div class="viewhead__actions"><button class="btn btn--red" id="focus-all">Decide all →</button></div>` : ''}</div>`;
  $('#focus-all')?.addEventListener('click', () => focusAll(openDecisions()[0].id));

  const dig = digestBlock();
  if (dig) v.appendChild(dig);

  if (!items.length) {
    v.appendChild(el(`<div class="nowclear">
      <div class="nowclear__mark">▲</div>
      <div class="nowclear__t">Tower clear — nothing is blocked on you.</div>
      <div class="nowclear__sub">${S.counts.agentReady} cards agent-ready · agents report here when they need you</div>
    </div>`));
    return;
  }

  const section = (title) => v.appendChild(el(`<div class="nowsection"><span class="nowsection__t">${title}</span><span class="nowsection__rule"></span></div>`));

  const msgs = items.filter(i => i.type === 'msg');
  if (msgs.length) section('Messages from agents');
  for (const it of msgs) v.appendChild(dutyMessage(it.thread));

  const decs = items.filter(i => i.type === 'decision');
  if (decs.length) section('Decisions');
  for (const it of decs) v.appendChild(dutyDecision(it.decision));

  const acts = items.filter(i => i.type === 'activate');
  if (acts.length) section('Greenlights');
  for (const it of acts) v.appendChild(dutyActivate(it.card));
  updateNowSel();
}

// ---- while-you-were-away digest -------------------------------------------
let digestInit = false;
function digestBlock() {
  const cursor = S.meta.digestCursor;
  if (!cursor) {
    // first run ever: set the cursor quietly so tomorrow's digest starts here
    if (!digestInit) { digestInit = true; api('digest/seen', {}).catch(() => {}); }
    return null;
  }
  const evs = S.events.filter(e => e.at > cursor && e.by !== 'owner');
  if (evs.length < 3) return null;
  const count = (a) => evs.filter(e => e.action === a).length;
  const doneCards = [...new Set(S.events.filter(e => e.at > cursor && e.action === 'card.update')
    .map(e => S.cards.find(c => c.id === e.ref)).filter(c => c && c.phase === 'done').map(c => '#' + c.num))];
  const bits = [];
  if (doneCards.length) bits.push(`<b>${doneCards.length}</b> done (${doneCards.slice(0, 6).join(' ')})`);
  const adv = count('card.update');
  if (adv) bits.push(`<b>${adv}</b> card update${adv > 1 ? 's' : ''}`);
  const nd = count('decision.add');
  if (nd) bits.push(`<b>${nd}</b> new ballot${nd > 1 ? 's' : ''}`);
  const qa = count('question.answer');
  if (qa) bits.push(`<b>${qa}</b> question${qa > 1 ? 's' : ''} answered`);
  const nm = count('message.send');
  if (nm) bits.push(`<b>${nm}</b> message${nm > 1 ? 's' : ''}`);
  const who = [...new Set(evs.map(e => e.by).filter(b => b && b !== 'agent' && b !== 'tower'))];
  const since = new Date(cursor).toLocaleString([], { weekday: 'short', hour: '2-digit', minute: '2-digit' });
  const node = el(`<div class="digest">
      <div class="digest__h">Since ${esc(since)}${who.length ? ` · ${who.map(esc).join(', ')}` : ''}</div>
      <div class="digest__b">${bits.join(' · ') || evs.length + ' events'}</div>
      <button class="btn btn--sm" data-seen>Caught up</button>
    </div>`);
  $('[data-seen]', node).addEventListener('click', () => api('digest/seen', {}));
  return node;
}

// ---- j/k keyboard selection on the Now queue --------------------------------
let nowSel = -1;
function updateNowSel() {
  const cards = [...document.querySelectorAll('#view .duty')];
  cards.forEach((n, i) => n.classList.toggle('duty--sel', i === nowSel));
  if (nowSel >= 0 && cards[nowSel]) cards[nowSel].scrollIntoView({ block: 'nearest' });
}
function nowMove(delta) {
  const n = document.querySelectorAll('#view .duty').length;
  if (!n) return;
  nowSel = Math.max(0, Math.min(n - 1, nowSel + delta));
  updateNowSel();
}
function nowActivate() {
  const cards = [...document.querySelectorAll('#view .duty')];
  const node = cards[nowSel];
  if (node?.__primary) node.__primary();
  else node?.click?.();
}

function dutyMessage(t) {
  const last = t.messages.filter(m => m.to === 'owner' && !m.readAt).at(-1);
  const node = el(`<div class="duty duty--msg">
      <div class="duty__top"><span class="duty__kind">Message</span>
        <span class="duty__meta">${esc(t.agent)} · ${t.unread > 1 ? t.unread + ' unread' : new Date(last.at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</span>${ageChip(last.at)}</div>
      <div class="duty__peek">${esc(last.text.slice(0, 600))}${last.text.length > 600 ? '…' : ''}</div>
      <div class="duty__reply"><input placeholder="Reply to ${esc(t.agent)}…"><button class="btn btn--red btn--sm">Send</button></div>
      <div class="duty__actions"><button class="btn btn--ghost btn--sm" data-open>Open thread</button>
        <button class="btn btn--ghost btn--sm" data-read>Mark read</button></div>
    </div>`);
  const input = $('input', node);
  const send = async () => {
    const text = input.value.trim(); if (!text) return;
    await markThreadRead(t.agent, false);
    await api('message/send', { from: 'owner', to: t.agent, text });
    toast(`sent to ${t.agent}`);
  };
  $('button.btn--red', node).addEventListener('click', send);
  input.addEventListener('keydown', e => { if (e.key === 'Enter') send(); });
  $('[data-open]', node).addEventListener('click', () => { THREAD = t.agent; go('agents'); });
  $('[data-read]', node).addEventListener('click', () => markThreadRead(t.agent));
  node.__primary = () => { THREAD = t.agent; go('agents'); };
  return node;
}

function dutyDecision(d) {
  const c = cardById(d.cardId);
  const node = el(`<button class="duty" style="display:block;width:100%;text-align:left">
      <div class="duty__top"><span class="duty__kind">Decide</span>
        <span class="num">${esc(d.id)}</span>
        <span class="duty__meta">card ${c ? ticket(c) : '—'} · ${(d.options || []).length} options${d.rec ? ` · rec ${esc(d.rec)}` : ''}</span>${ageChip(d.created)}</div>
      <h2 class="duty__title">${esc(d.title)}</h2>
      ${d.gist ? `<p class="duty__gist">${esc(d.gist)}</p>` : ''}
      <div class="duty__actions"><span class="btn btn--red btn--sm">Decide →</span></div>
    </button>`);
  node.addEventListener('click', () => focusAll(d.id));
  node.__primary = () => focusAll(d.id);
  return node;
}

function dutyActivate(c) {
  const node = el(`<div class="duty">
      <div class="duty__top"><span class="duty__kind">Greenlight</span>
        <span class="num">${ticket(c)}</span><span class="prio prio-${c.priority}">${c.priority}</span>
        <span class="duty__meta">${esc(c.kind)}${c.phase === 'frozen' ? ' · frozen' : ''}</span>${ageChip(c.created)}</div>
      <h2 class="duty__title">${esc(c.title)}</h2>
      ${c.body ? `<p class="duty__gist">${esc(c.body.slice(0, 180))}${c.body.length > 180 ? '…' : ''}</p>` : ''}
      <div class="duty__actions">
        <button class="btn btn--red btn--sm" data-go>Greenlight — start work</button>
        <button class="btn btn--ghost btn--sm" data-open>Open card</button>
      </div>
    </div>`);
  $('[data-go]', node).addEventListener('click', () => api('card/activate', { id: c.id, by: 'owner' }));
  $('[data-open]', node).addEventListener('click', () => showDetail(c.id));
  node.__primary = () => api('card/activate', { id: c.id, by: 'owner' });
  return node;
}

async function markThreadRead(agent, rerender = true) {
  const ids = S.messages.filter(m => m.to === 'owner' && !m.readAt && (m.from === agent)).map(m => m.id);
  if (!ids.length) return;
  await api('message/mark', { ids, field: 'readAt' });
  if (rerender) render();
}

// ---- AGENTS ---------------------------------------------------------------------
function viewAgents() {
  const v = $('#view');
  const ts = threadsOf();
  const names = new Set([...ROSTER.map(a => a.name), ...ts.keys()]);
  if (THREAD) names.add(THREAD);
  const list = [...names].map(name => ({
    name,
    roster: ROSTER.find(a => a.name === name) || null,
    thread: ts.get(name) || { agent: name, messages: [], unread: 0 },
  })).sort((a, b) => b.thread.unread - a.thread.unread || (b.roster?.online ? 1 : 0) - (a.roster?.online ? 1 : 0) || a.name.localeCompare(b.name));

  if (!THREAD && list.length) THREAD = (list.find(x => x.thread.unread) || list[0]).name;

  v.innerHTML = `<div class="viewhead"><h1 class="h1">Agents</h1>
    <span class="viewhead__sub">talk to any agent from here — replies land in <b>Now</b></span></div>
    <div class="comms"><div class="roster" id="roster"></div><div id="thread-slot"></div></div>`;

  const roster = $('#roster');
  for (const a of list) {
    const on = a.roster?.online;
    const state = on ? (a.roster.state === 'running' ? 'running' : 'listening') : 'offline';
    const row = el(`<button class="agentrow" aria-current="${THREAD === a.name}">
        <span class="presence ${on ? (a.roster.state === 'running' ? 'running' : 'online') : ''}"></span>
        <span><span class="agentrow__name">${esc(a.name)}</span><br>
          <span class="agentrow__sub">${esc(a.roster?.kind || 'agent')} · ${state}${a.roster?.statusText ? ` — ${esc(a.roster.statusText.slice(0, 46))}` : ''}</span></span>
        ${a.thread.unread ? `<span class="agentrow__unread">${a.thread.unread}</span>` : ''}
      </button>`);
    row.addEventListener('click', () => { THREAD = a.name; render(); });
    roster.appendChild(row);
  }
  const add = el(`<div class="roster__add"><input placeholder="agent name…" aria-label="New agent name"><button class="btn btn--sm">Add</button></div>`);
  const fire = () => { const i = $('input', add); const n = i.value.trim(); if (!n) return; i.value = ''; THREAD = n; render(); };
  $('button', add).addEventListener('click', fire);
  $('input', add).addEventListener('keydown', e => { if (e.key === 'Enter') fire(); });
  roster.appendChild(add);

  if (!list.length && !THREAD) {
    $('#thread-slot').appendChild(el(`<div class="empty"><div class="empty__glyph">▸</div>
      <div>No agents yet. An agent appears when it runs<br><code style="font-family:var(--mono);font-size:12px">tower agent listen --name &lt;name&gt;</code><br>or when you add one and send the first message.</div></div>`));
    return;
  }
  $('#thread-slot').appendChild(threadPane(THREAD, ts.get(THREAD)));
  markThreadRead(THREAD, false).then(() => renderBeacon() + renderChrome());
}

function threadPane(name, thread) {
  const a = ROSTER.find(x => x.name === name);
  const on = a?.online;
  const stateTxt = on
    ? (a.state === 'running' ? 'running a turn…' : (a.statusText ? a.statusText : 'online — listening'))
    : (a?.lastSeen ? `offline · last seen ${new Date(a.lastSeen).toLocaleString()}` : 'offline — never connected');
  const pane = el(`<section class="thread">
      <div class="thread__head"><span class="presence ${on ? (a.state === 'running' ? 'running' : 'online') : ''}"></span>
        <span class="thread__name">${esc(name)}</span>
        <span class="chip chip--${esc(a?.kind || 'agent')}">${esc(a?.kind || 'agent')}</span>
        <span class="thread__state">${esc(stateTxt)}</span></div>
      <div class="thread__log" id="log"></div>
      ${!on ? `<div class="invite">
        <div class="invite__t">This agent isn't listening. Messages queue — or connect it:</div>
        <code class="invite__cmd">tower agent listen --name ${esc(name)}${a?.kind && a.kind !== 'agent' ? ` --kind ${esc(a.kind)}` : ''}</code>
        <button class="btn btn--sm" data-copy>Copy</button>
      </div>` : ''}
      <div class="composer">
        <label class="composer__attach" title="Attach a file"><input type="file" hidden>📎</label>
        <textarea rows="1" placeholder="Message ${esc(name)}… (Enter to send, Shift+Enter for newline)"></textarea>
        <button class="btn btn--red" data-send>Send</button>
        ${!on && a?.launchable ? `<button class="btn" data-launch title="Starts a headless ${esc(a.kind)} turn in the project with this message">Send + run</button>` : ''}
      </div>
      ${!on && a?.launchable ? `<div class="composer__hint"><b>offline:</b> Send queues it; Send + run starts a headless ${esc(a?.kind || '')} turn now</div>` : ''}
    </section>`);
  $('[data-copy]', pane)?.addEventListener('click', () => {
    navigator.clipboard?.writeText($('.invite__cmd', pane).textContent).then(() => toast('copied'));
  });
  $('.composer__attach input', pane)?.addEventListener('change', async (e) => {
    const fl = e.target.files[0]; if (!fl) return;
    if (fl.size > 10_000_000) return toast('file too large (10MB max)', true);
    const r = await fetch(`/api/file?name=${encodeURIComponent(fl.name)}&type=${encodeURIComponent(fl.type || 'application/octet-stream')}`, { method: 'POST', body: fl });
    const j = await r.json();
    if (!j.ok) return toast(j.message || 'upload failed', true);
    const caption = $('textarea', pane).value.trim();
    $('textarea', pane).value = '';
    await api('message/send', { from: 'owner', to: name, text: caption || fl.name, file: { id: j.file.id, name: j.file.name, type: j.file.type } });
  });
  const log = $('#log', pane);
  const msgs = thread?.messages || [];
  if (!msgs.length) log.appendChild(el(`<div class="empty" style="padding:30px"><div>No messages yet — say something.</div></div>`));
  for (const m of msgs) {
    const mine = m.from === 'owner';
    log.appendChild(el(`<div class="msg ${mine ? 'msg--owner' : ''} ${mine && !m.deliveredAt ? 'msg--queued' : ''}">
        <div class="msg__meta"><div class="msg__from">${esc(m.from)}</div>
          <div class="msg__at">${new Date(m.at).toLocaleDateString([], { month: 'short', day: 'numeric' })} ${new Date(m.at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</div></div>
        <div class="msg__body">${esc(m.text)}${m.file ? (
          /^image\//.test(m.file.type)
            ? `<a href="/files/${esc(m.file.id)}" target="_blank"><img class="msg__img" src="/files/${esc(m.file.id)}" alt="${esc(m.file.name)}" loading="lazy"></a>`
            : `<a class="msg__file" href="/files/${esc(m.file.id)}" target="_blank" download="${esc(m.file.name)}">📄 ${esc(m.file.name)}</a>`
        ) : ''}${m.cardId && cardById(m.cardId) ? `<a class="msg__card" href="#" data-card="${esc(m.cardId)}">card ${ticket(cardById(m.cardId))}</a>` : ''}</div>
      </div>`));
  }
  log.querySelectorAll('[data-card]').forEach(x => x.addEventListener('click', (e) => { e.preventDefault(); showDetail(x.dataset.card); }));
  queueMicrotask(() => { log.scrollTop = log.scrollHeight; });

  const ta = $('textarea', pane);
  const send = async (launch = false) => {
    const text = ta.value.trim(); if (!text) return;
    ta.value = '';
    if (launch) { await api('agent/launch', { agent: name, text }); toast(`started ${name} — reply lands in this thread`); }
    else { await api('message/send', { from: 'owner', to: name, text }); }
  };
  $('[data-send]', pane).addEventListener('click', () => send(false));
  $('[data-launch]', pane)?.addEventListener('click', () => send(true));
  ta.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(false); }
  });
  return pane;
}

// ---- BOARD ---------------------------------------------------------------------
const isOpen = (key, def) => { const t = (S.meta.ui.toggled || []).includes(key); return def ? !t : t; };
const PR = { P0: 0, P1: 1, P2: 2, P3: 3 };
const byPrio = (a, b) => (PR[a.priority] ?? 9) - (PR[b.priority] ?? 9) || (a.num ?? 0) - (b.num ?? 0);
const PHO = { deciding: 0, planning: 1, ready: 2, building: 3, verify: 4, triage: 5, done: 6, frozen: 7 };
const orderOf = (c) => c.workOrder == null ? Infinity : c.workOrder;
const bySched = (a, b) => (orderOf(a) - orderOf(b)) || (PHO[a.phase] - PHO[b.phase]) || byPrio(a, b);

function cardTile(c) {
  const who = c.lane.who === 'owner' ? 'lane-owner' : c.lane.who === 'agent' ? 'lane-agent' : 'lane-none';
  const node = el(`<button class="card ${c.lane.who === 'owner' ? 'needs-owner' : ''}" style="--stage:var(--s-${c.phase})">
      <div class="card__top">
        ${c.workOrder != null ? `<span class="order">${c.workOrder}</span>` : ''}
        <span class="num">${ticket(c)}</span>
        <span class="prio prio-${c.priority}">${c.priority}</span>
        <span class="card__kind">${esc(c.kind)}</span>
        ${c.openQ ? `<span class="card__q">✎ ${c.openQ}</span>` : ''}
      </div>
      <h3 class="card__title">${esc(c.title)}</h3>
      ${c.assignee ? `<span class="card__claim">⛭ ${esc(c.assignee)}</span>` : ''}
      <span class="card__lane ${who}"><span class="pip"></span>${esc(c.lane.label)}</span>
    </button>`);
  node.addEventListener('click', () => {
    if (c.lane.lane === 'decide' && c.lane.decisions?.length) focusAll(c.lane.decisions[0]);
    else showDetail(c.id);
  });
  return node;
}
const grid = (cards) => { const g = el('<div class="grid"></div>'); cards.forEach(c => g.appendChild(cardTile(c))); return g; };

function collapsible(key, def, headHTML, extraClass, buildBody) {
  const open = isOpen(key, def);
  const sec = el(`<section class="epoch ${extraClass} ${open ? 'is-open' : ''}">
      <button class="epoch__head"><span class="epoch__chev">▸</span>${headHTML}</button>
      <div class="epoch__body" ${open ? '' : 'hidden'}></div></section>`);
  $('.epoch__head', sec).addEventListener('click', () => api('ui/toggle', { key }));
  if (open) buildBody($('.epoch__body', sec));
  return sec;
}

function subgroup(key, name, n, buildBody) {
  const open = isOpen(key, false);
  const sec = el(`<div class="subgroup">
      <button class="subgroup__head"><span class="epoch__chev" style="${open ? 'transform:rotate(90deg)' : ''}">▸</span> ${esc(name)} <span class="subgroup__n">${n}</span></button>
      <div ${open ? '' : 'hidden'}></div></div>`);
  $('.subgroup__head', sec).addEventListener('click', () => api('ui/toggle', { key }));
  if (open) buildBody(sec.lastElementChild);
  return sec;
}

function milestoneStrip(ms) {
  const wrap = el(`<div class="miles"><div class="miles__h">${esc(TERM('milestones', 'Milestones'))}</div></div>`);
  for (const m of ms) {
    const p = m.progress || { done: 0, total: 0 };
    const pct = p.total ? Math.round(p.done / p.total * 100) : 0;
    const met = m.status === 'met';
    const row = el(`<div class="mile ${met ? 'mile--met' : ''}" title="${esc(m.goal || '')}">
        <span class="mile__dot">${met ? '✓' : '◇'}</span>
        <span class="mile__t">${esc(m.title)}</span>
        <span class="mile__goal">${esc(m.goal || '')}</span>
        <span class="mile__bar"><i style="width:${met ? 100 : pct}%"></i></span>
        <span class="mile__n">${met ? 'met' : `${p.done}/${p.total}`}</span>
        ${met ? '' : `<button class="btn btn--ghost btn--sm" data-met>Met</button>`}
      </div>`);
    $('[data-met]', row)?.addEventListener('click', () => api('milestone/update', { id: m.id, status: 'met', by: 'owner' }));
    wrap.appendChild(row);
  }
  return wrap;
}

function viewBoard() {
  const v = $('#view');
  v.innerHTML = `<div class="viewhead"><h1 class="h1">Board</h1>
      <span class="viewhead__sub">${esc(TERM('epochs', 'epochs'))} → ${esc(TERM('milestones', 'milestones').toLowerCase())} → cards</span>
      <div class="viewhead__actions"><button class="btn btn--red" id="new-card">+ New card</button></div></div>
    <div class="capture"><input id="idea-input" placeholder="Capture an idea — it waits in Ideas until you make it a card…">
      <button class="btn" id="idea-btn">Capture</button></div>`;
  $('#new-card').addEventListener('click', async () => {
    const c = await api('card/add', { title: 'New card', phase: 'triage', epoch: S.meta.currentEpoch, by: 'owner' });
    if (c) showDetail(c.id);
  });
  const fire = async () => {
    const i = $('#idea-input'); const t = i.value.trim(); if (!t) return; i.value = '';
    await api('idea/add', { text: t, by: 'owner' }); toast('captured');
  };
  $('#idea-btn').addEventListener('click', fire);
  $('#idea-input').addEventListener('keydown', e => { if (e.key === 'Enter') fire(); });

  // Ideas bay
  const ideas = S.ideas.filter(b => b.status !== 'tagged');
  if (ideas.length) {
    v.appendChild(collapsible('board-ideas', true,
      `<span class="epoch__tag" style="color:var(--amber)">Ideas</span><span class="epoch__name">to triage</span><span class="epoch__count">${ideas.length}</span>`,
      'epoch--off', (body) => {
        const wrap = el('<div class="ideas"></div>');
        for (const b of ideas) {
          const row = el(`<div class="idea"><span class="idea__t">${esc(b.text)}${b.note ? ` <span class="idea__note">— ${esc(b.note)}</span>` : ''}</span>
              <button class="btn btn--red btn--sm">Make card</button><button class="btn btn--ghost btn--sm">Dismiss</button></div>`);
          const [mk, dl] = row.querySelectorAll('button');
          mk.addEventListener('click', async () => { const c = await api('idea/promote', { id: b.id, by: 'owner' }); if (c) showDetail(c.id); });
          dl.addEventListener('click', () => api('idea/delete', { id: b.id, by: 'owner' }));
          wrap.appendChild(row);
        }
        body.appendChild(wrap);
      }));
  }

  // Sidequests
  const sqActive = S.cards.filter(c => c.track === 'sidequest' && !['done', 'frozen'].includes(c.phase)).sort(bySched);
  const sqDone = S.cards.filter(c => c.track === 'sidequest' && c.phase === 'done').sort(byPrio);
  if (sqActive.length || sqDone.length) {
    v.appendChild(collapsible('board-sidequests', true,
      `<span class="epoch__tag" style="color:var(--frost)">${esc(TERM('sidequest', 'Sidequests'))}</span><span class="epoch__name">off-plan work</span><span class="epoch__count">${sqActive.length} active</span>`,
      'epoch--off', (body) => {
        if (sqActive.length) body.appendChild(grid(sqActive));
        else body.appendChild(el(`<p class="epoch__goal">none active</p>`));
        if (sqDone.length) body.appendChild(subgroup('sq-done', 'Done', sqDone.length, (b) => b.appendChild(grid(sqDone))));
      }));
  }

  // Epochs
  const sorted = [...S.epochs].sort((a, b) => (a.order ?? a.num ?? 999) - (b.order ?? b.num ?? 999));
  for (const e of sorted) {
    if (['arrived', 'done'].includes(e.status)) continue;
    const all = S.cards.filter(c => c.epoch === e.id && c.track !== 'sidequest');
    const active = all.filter(c => !['done', 'frozen'].includes(c.phase)).sort(bySched);
    const doneCards = all.filter(c => c.phase === 'done').sort(bySched);
    const pct = all.length ? Math.round(doneCards.length / all.length * 100) : 0;
    const ms = S.milestones.filter(m => m.epochId === e.id);
    v.appendChild(collapsible('epoch:' + e.id, true,
      `<span class="epoch__tag">${esc(epochTag(e))}</span><span class="epoch__name">${esc(e.name)}</span>
       ${e.status && e.status !== 'open' ? `<span class="epoch__status">${esc(e.status)}</span>` : ''}
       <span class="epoch__count">${doneCards.length}/${all.length} done · ${pct}%</span>`,
      '', (body) => {
        if (e.goal) body.appendChild(el(`<p class="epoch__goal">${esc(e.goal)}</p>`));
        body.appendChild(el(`<div class="epoch__prog"><div class="epoch__bar"><i style="width:${pct}%"></i></div><span class="epoch__pct">${active.length} active</span></div>`));
        if (ms.length) body.appendChild(milestoneStrip(ms));
        if (active.length) body.appendChild(grid(active));
        else body.appendChild(el(`<p class="epoch__goal">no active cards</p>`));
        if (doneCards.length) body.appendChild(subgroup('done:' + e.id, 'Done', doneCards.length, (b) => b.appendChild(grid(doneCards))));
      }));
  }

  // Frozen
  const frozen = S.cards.filter(c => c.phase === 'frozen').sort(byPrio);
  if (frozen.length) {
    v.appendChild(collapsible('board-frozen', false,
      `<span class="epoch__tag" style="color:var(--frost)">Frozen</span><span class="epoch__name">parked on purpose</span><span class="epoch__count">${frozen.length}</span>`,
      'epoch--frozen', (body) => body.appendChild(grid(frozen))));
  }
}

// ---- card modal ------------------------------------------------------------------
function showDetail(id) {
  openCard = id;
  const c = cardById(id);
  if (!c) return closeDetail();
  const m = $('#detail');
  const phaseLabel = (S.phases.find(p => p.id === c.phase) || {}).label || c.phase;
  const sel = (k, opts, cur) => `<select data-fld="${k}">${opts.map(o => `<option value="${esc(o)}" ${o === cur ? 'selected' : ''}>${esc(o)}</option>`).join('')}</select>`;
  const cta = c.phase === 'triage' ? `<button class="btn btn--red" id="cta-activate">Greenlight — start work</button>`
    : c.phase === 'frozen' ? `<button class="btn btn--red" id="cta-activate">Unfreeze — start work</button>`
    : c.phase === 'verify' ? `<button class="btn btn--red" id="cta-done">Mark verified — close</button>` : '';
  m.innerHTML = `<div class="modal__panel">
    <div class="modal__bar">
      ${c.workOrder != null ? `<span class="order">${c.workOrder}</span>` : ''}
      <span class="num" style="color:var(--ink)">${ticket(c)}</span>
      <span class="prio prio-${c.priority}">${c.priority}</span>
      <span class="card__kind">${esc(c.kind)} · ${esc(phaseLabel)}</span>
      <span class="card__lane ${c.lane.who === 'owner' ? 'lane-owner' : c.lane.who === 'agent' ? 'lane-agent' : 'lane-none'}"><span class="pip"></span>${esc(c.lane.label)}</span>
      <button class="modal__x" title="Close (Esc)">×</button></div>
    <div class="modal__body">
      <h2 class="modal__title" contenteditable="plaintext-only" data-fld="title">${esc(c.title)}</h2>
      ${cta ? `<div class="modal__cta">${cta}</div>` : ''}
      <div class="fields">
        <div class="fld"><div class="fld__k">Stage</div><select data-fld="phase">${S.phases.map(p => `<option value="${p.id}" ${p.id === c.phase ? 'selected' : ''}>${p.label}</option>`).join('')}</select></div>
        <div class="fld"><div class="fld__k">Track</div>${sel('track', CFG().tracks || ['epoch', 'sidequest'], c.track)}</div>
        <div class="fld"><div class="fld__k">${esc(TERM('epoch', 'Epoch'))}</div><select data-fld="epoch"><option value="">—</option>${S.epochs.map(e => `<option value="${e.id}" ${e.id === c.epoch ? 'selected' : ''}>${esc(e.name)}</option>`).join('')}</select></div>
        <div class="fld"><div class="fld__k">${esc(TERM('milestone', 'Milestone'))}</div><select data-fld="milestoneId"><option value="">—</option>${S.milestones.filter(x => !c.epoch || x.epochId === c.epoch).map(x => `<option value="${x.id}" ${x.id === c.milestoneId ? 'selected' : ''}>${esc(x.title)}</option>`).join('')}</select></div>
        <div class="fld"><div class="fld__k">Priority</div>${sel('priority', CFG().priorities || ['P0', 'P1', 'P2', 'P3'], c.priority)}</div>
        <div class="fld"><div class="fld__k">Kind</div>${sel('kind', CFG().kinds || ['task', 'feature', 'idea', 'bug'], c.kind)}</div>
        <div class="fld"><div class="fld__k">Assignee</div><input data-fld="assignee" value="${esc(c.assignee || '')}" placeholder="—"></div>
        <div class="fld"><div class="fld__k">Work order</div><input data-fld="workOrder" type="number" min="1" value="${c.workOrder ?? ''}" placeholder="—"></div>
      </div>
      <div class="fld" style="margin-bottom:16px"><div class="fld__k">Plan</div><input data-fld="plan" value="${esc(c.plan || '')}" placeholder="— (agents fill this in the plan lane)"></div>
      <div class="modal__h">Description</div>
      <div class="prose" contenteditable="plaintext-only" data-fld="body">${md(c.body)}</div>
      <div class="modal__h">Decisions</div><div id="m-decisions"></div>
      <div class="modal__h">Notes &amp; questions</div><div id="m-q"></div>
      <div class="modal__h">Log</div>
      <ul class="log">${c.log.map(l => `<li><time>${esc(l.at)}</time>${l.by ? `<span class="by">${esc(l.by)}</span>` : ''}<span>${esc(l.text)}</span></li>`).join('') || '<li><span>No entries.</span></li>'}</ul>
      <div class="modal__danger"><button class="btn btn--danger btn--sm" id="del-card">Delete card</button></div>
    </div></div>`;

  const dd = $('#m-decisions', m);
  if (!c.decisions.length) dd.appendChild(el(`<p class="prose">No decisions on this card.</p>`));
  for (const de of c.decisions) {
    const box = el(`<div class="decrow">
        <div class="decrow__head"><span class="decrow__id">${esc(de.id)}</span>
          <span class="card__lane ${de.status === 'ratified' ? 'lane-agent' : 'lane-owner'}">${de.status === 'ratified' ? '✓ ' + esc(de.outcome) : 'to decide'}</span>
          ${de.status === 'ratified'
            ? `<button class="btn btn--ghost btn--sm" style="margin-left:auto" data-reopen>Reopen</button>`
            : `<button class="btn btn--red btn--sm" style="margin-left:auto" data-focus>Decide</button>`}</div>
        <div class="prose" style="font-size:13px">${esc(de.title)}</div>
        <div class="decrow__opts">${(de.options || []).map(o => `<button class="opt-pill ${de.outcome === o.key ? 'win' : ''}" data-opt="${esc(o.key)}">${esc(o.key)} · ${esc(o.name)}</button>`).join('')}</div>
      </div>`);
    $('[data-focus]', box)?.addEventListener('click', () => { closeDetail(); focusAll(de.id); });
    $('[data-reopen]', box)?.addEventListener('click', () => api('clearance/reopen', { decisionId: de.id, by: 'owner' }));
    box.querySelectorAll('[data-opt]').forEach(b => b.addEventListener('click', () => api('clearance', { decisionId: de.id, outcome: b.dataset.opt, by: 'owner' })));
    dd.appendChild(box);
  }

  const qb = $('#m-q', m);
  for (const q of c.questions) {
    qb.appendChild(el(`<div class="qrow"><div class="qrow__top"><span class="${q.by === 'owner' ? 'qrow__by--owner' : ''}">${esc(q.by)}</span>
        <span>${esc(q.kind)}</span><span>${esc(q.status)}</span>${q.decisionId ? `<span>${esc(q.decisionId)}</span>` : ''}</div>
        <div class="qrow__text">${esc(q.text)}</div>
        ${q.answer ? `<div class="qrow__ans"><b>${esc(q.answeredBy || 'agent')}</b> ${esc(q.answer)}</div>` : ''}</div>`));
  }
  const qadd = el(`<div class="qadd"><input placeholder="Leave a note or question for an agent…"><button class="btn btn--red btn--sm">Post</button></div>`);
  const post = async () => { const i = $('input', qadd); const t = i.value.trim(); if (!t) return; i.value = ''; await api('question/add', { cardId: id, text: t, kind: 'question', by: 'owner' }); };
  $('button', qadd).addEventListener('click', post);
  $('input', qadd).addEventListener('keydown', e => { if (e.key === 'Enter') post(); });
  qb.appendChild(qadd);

  m.querySelectorAll('[data-fld]').forEach(node => {
    const k = node.dataset.fld;
    if (node.tagName === 'SELECT' || node.tagName === 'INPUT') node.addEventListener('change', () => commit(id, k, node.value));
    else node.addEventListener('blur', () => commit(id, k, node.innerText.trim()));
  });
  $('.modal__x', m).addEventListener('click', closeDetail);
  $('#del-card', m).addEventListener('click', async () => { await api('card/delete', { id, by: 'owner' }); closeDetail(); });
  $('#cta-activate', m)?.addEventListener('click', () => api('card/activate', { id, by: 'owner' }));
  $('#cta-done', m)?.addEventListener('click', () => api('card/update', { id, phase: 'done', logEntry: 'Verified — closed.', by: 'owner' }));
  m.onclick = (e) => { if (e.target === m) closeDetail(); };
  m.hidden = false; $('#scrim').hidden = false;
}
const commit = (id, k, v) => {
  let val = v;
  if (['plan', 'milestoneId', 'assignee', 'epoch'].includes(k) && v === '') val = null;
  else if (k === 'workOrder') val = v === '' ? null : Number(v);
  return api('card/update', { id, [k]: val, by: 'owner' });
};
function closeDetail() { openCard = null; $('#detail').hidden = true; $('#scrim').hidden = true; }

// ---- focus mode --------------------------------------------------------------------
function focusAll(startId) {
  const ids = openDecisions().map(d => d.id);
  if (!ids.length) return;
  focusIds = ids; focusIdx = Math.max(0, ids.indexOf(startId)); focusFacet = null; askOpen = false;
  renderFocus();
}
function exitFocus() { focusIds = null; $('#focus').hidden = true; render(); }
function focusGo(delta) { focusIdx = Math.max(0, Math.min(focusIds.length - 1, focusIdx + delta)); focusFacet = null; askOpen = false; renderFocus(); }
const optName = (d, key) => ((d.options || []).find(x => x.key === key) || {}).name || '';
function availFacets(d) {
  const f = [];
  if (d.story) f.push(['story', 'Story']);
  if (d.explainer) f.push(['why', 'Why it matters']);
  if (d.inWild) f.push(['wild', 'In the wild']);
  if ((d.comparisons || []).length) f.push(['langs', 'Elsewhere']);
  if (d.detail) f.push(['detail', 'Q&A']);
  return f;
}
function facetBody(d, fk) {
  if (fk === 'story') return `<p>${esc(d.story)}</p>`;
  if (fk === 'why') return `<p>${esc(d.explainer)}</p>`;
  if (fk === 'wild') return codeBlock(d.inWild);
  if (fk === 'langs') return (d.comparisons || []).map(c => `<div class="cmp"><div class="cmp__head"><span class="cmp__lang">${esc(c.lang)}</span><span class="cmp__note">${esc(c.note || '')}</span></div>${codeBlock(c.code || '')}</div>`).join('');
  if (fk === 'detail') return `<p>${esc(d.detail).replace(/\n/g, '<br>')}</p>`;
  return '';
}
function recordLabel(ids) {
  if (!ids.length) return 'Record decisions';
  if (ids.length === 1) return `Record · ${ids[0]} = ${pick[ids[0]]}`;
  return `Record ${ids.length} decisions`;
}
function renderFocus() {
  const f = $('#focus');
  focusIds = (focusIds || []).filter(id => S.decisions.some(d => d.id === id && d.status !== 'ratified'));
  if (!focusIds.length) return exitFocus();
  focusIdx = Math.max(0, Math.min(focusIdx, focusIds.length - 1));
  const d = S.decisions.find(x => x.id === focusIds[focusIdx]);
  if (!d) return exitFocus();
  const c = cardById(d.cardId);
  const chosen = pick[d.id] ?? null;
  const pickedIds = focusIds.filter(id => pick[id]);
  const facets = availFacets(d);
  if (!focusFacet || !facets.some(([fk]) => fk === focusFacet)) focusFacet = facets.length ? facets[0][0] : null;
  const qs = c ? c.questions.filter(q => q.decisionId === d.id) : [];

  f.innerHTML = `
    <div class="focustop">
      <span class="focustop__ctr">${focusIdx + 1} / ${focusIds.length}</span>
      <div class="dots" id="f-dots">${focusIds.map((id, i) => `<button class="dot${i === focusIdx ? ' cur' : ''}${pick[id] ? ' picked' : ''}" data-i="${i}" title="${esc(id)}"></button>`).join('')}</div>
      <div class="focus__nav">
        <button class="btn btn--sm" id="f-prev" ${focusIdx === 0 ? 'disabled' : ''}>←</button>
        <button class="btn btn--sm" id="f-next" ${focusIdx === focusIds.length - 1 ? 'disabled' : ''}>→</button>
        <button class="btn btn--sm" id="f-close">Esc</button>
      </div>
    </div>
    <div class="focusscroll"><div class="fdeck">
      <div class="fdeck__head"><span class="fdeck__id">${esc(d.id)}</span>
        <span class="fdeck__for">card ${c ? ticket(c) : '—'}${c ? ' · ' + esc(c.title) : ''}</span>
        ${d.rec ? `<span class="fdeck__rec">rec ${esc(d.rec)}</span>` : ''}</div>
      <div class="fdeck__gist">${esc(d.gist || d.title)}</div>
      ${d.gist ? `<div class="fdeck__title">${esc(d.title)}</div>` : ''}
      ${facets.length ? `<div class="facets" id="f-facets">${facets.map(([fk, l]) => `<button class="facet${fk === focusFacet ? ' on' : ''}" data-fk="${fk}">${esc(l)}</button>`).join('')}</div><div class="facetbody" id="f-facetbody">${facetBody(d, focusFacet)}</div>` : ''}
      <div class="optslabel">Choose one
        ${(d.options || []).length >= 2 ? `<button class="btn btn--ghost btn--sm" id="f-compare" style="margin-left:10px;text-transform:none;letter-spacing:0">${focusCompare ? '☰ Stack' : '⇆ Compare'}</button>` : ''}</div>
      <div class="opts ${focusCompare ? 'opts--compare' : ''}" id="f-opts"></div>
      ${d.rec ? `<div class="recline"><b>Recommendation:</b> ${esc(d.rec)}${optName(d, d.rec) ? ' — ' + esc(optName(d, d.rec)) : ''}</div>` : ''}
      <textarea class="fcomment" id="f-comment" placeholder="Comment (optional) — recorded with your decision">${esc(d.comment || '')}</textarea>
      <div class="deck-actions">
        ${chosen ? `<button class="btn btn--ghost btn--sm" id="f-clear">✕ Clear choice</button>` : ''}
        <button class="btn btn--ghost btn--sm" id="f-ask">${askOpen ? 'Close' : '✎ Ask a question'}</button>
      </div>
      ${askOpen ? `<div class="askbox"><div class="askbox__h">Ask the agents about <b>${esc(d.id)}</b> — saved to this ballot, no decision recorded</div>
        <textarea id="f-askt" placeholder="e.g. add a comparison, or rework option B around streaming…"></textarea>
        <button class="btn btn--amber btn--sm" id="f-asksend">Send to agents</button></div>` : ''}
      ${qs.length ? `<div class="fqs">${qs.map(q => `<div class="qrow"><div class="qrow__top"><span class="${q.by === 'owner' ? 'qrow__by--owner' : ''}">${esc(q.by)}</span><span>${esc(q.kind)}</span><span>${esc(q.status)}</span></div><div class="qrow__text">${esc(q.text)}</div>${q.answer ? `<div class="qrow__ans"><b>${esc(q.answeredBy || 'agent')}</b> ${esc(q.answer)}</div>` : ''}</div>`).join('')}</div>` : ''}
    </div></div>
    <div class="focusnav"><div class="focusnav__inner">
      <span class="focusnav__kbd"><b>1–9</b> pick · <b>←/→</b> move · <b>Enter</b> record · <b>Esc</b> close</span>
      <span class="focusnav__spacer"></span>
      <button class="btn btn--red" id="f-record" ${pickedIds.length ? '' : 'disabled'}>${recordLabel(pickedIds)}</button>
    </div></div>`;

  const opts = $('#f-opts', f);
  (d.options || []).forEach((o, idx) => {
    const node = el(`<div class="opt${chosen === o.key ? ' sel' : ''}">
        <button class="opt__h"><span class="opt__num">${idx + 1}</span><span class="opt__name">${esc(o.key)} — ${esc(o.name)}</span>
          ${o.key === d.rec ? '<span class="opt__rec">recommended</span>' : ''}<span class="opt__check">✓ chosen</span></button>
        ${o.detail ? `<div class="opt__detail">${esc(o.detail)}</div>` : ''}
        ${o.code ? `<div class="opt__code">${codeBlock(o.code)}</div>` : ''}</div>`);
    $('.opt__h', node).addEventListener('click', () => { if (pick[d.id] === o.key) delete pick[d.id]; else pick[d.id] = o.key; updateChoice(); });
    opts.appendChild(node);
  });

  $('#f-dots', f).querySelectorAll('.dot').forEach(dot => dot.addEventListener('click', () => { focusIdx = +dot.dataset.i; focusFacet = null; askOpen = false; renderFocus(); }));
  $('#f-facets', f)?.querySelectorAll('.facet').forEach(b => b.addEventListener('click', () => {
    focusFacet = b.dataset.fk;
    $('#f-facets', f).querySelectorAll('.facet').forEach(x => x.classList.toggle('on', x.dataset.fk === focusFacet));
    $('#f-facetbody', f).innerHTML = facetBody(d, focusFacet);
  }));
  $('#f-prev', f).onclick = () => focusGo(-1);
  $('#f-next', f).onclick = () => focusGo(1);
  $('#f-close', f).onclick = exitFocus;
  $('#f-compare', f)?.addEventListener('click', () => { focusCompare = !focusCompare; renderFocus(); });
  $('#f-clear', f)?.addEventListener('click', () => { delete pick[d.id]; updateChoice(); });
  $('#f-ask', f).onclick = () => { askOpen = !askOpen; renderFocus(); };
  $('#f-asksend', f)?.addEventListener('click', async () => {
    const t = $('#f-askt', f).value.trim(); if (!t || !c) return; askOpen = false;
    await api('question/add', { cardId: c.id, decisionId: d.id, text: t, kind: 'question', by: 'owner' });
  });
  $('#f-record', f)?.addEventListener('click', recordBatch);
  f.hidden = false;
}
function updateChoice() {
  const f = $('#focus'); const d = S.decisions.find(x => x.id === focusIds[focusIdx]); if (!d) return;
  const chosen = pick[d.id] ?? null;
  const pickedIds = focusIds.filter(id => pick[id]);
  $('#f-opts', f).querySelectorAll('.opt').forEach((node, i) => node.classList.toggle('sel', (d.options[i] || {}).key === chosen));
  $('#f-dots', f).querySelectorAll('.dot').forEach((dot, i) => dot.classList.toggle('picked', !!pick[focusIds[i]]));
  const rec = $('#f-record', f);
  if (rec) { rec.disabled = !pickedIds.length; rec.textContent = recordLabel(pickedIds); }
  const actions = $('.deck-actions', f); let clr = $('#f-clear', f);
  if (chosen && !clr && actions) {
    clr = el(`<button class="btn btn--ghost btn--sm" id="f-clear">✕ Clear choice</button>`);
    clr.addEventListener('click', () => { delete pick[d.id]; updateChoice(); });
    actions.prepend(clr);
  } else if (!chosen && clr) clr.remove();
}
async function recordBatch() {
  const pickedIds = focusIds.filter(id => pick[id]);
  if (!pickedIds.length) return;
  const currentId = focusIds[focusIdx];
  const comment = $('#f-comment')?.value.trim();
  await api('clearance/batch', { by: 'owner', decisions: pickedIds.map(id => ({ decisionId: id, outcome: pick[id], comment: id === currentId && comment ? comment : undefined })) });
  pickedIds.forEach(id => delete pick[id]);
  const prevIdx = focusIds.indexOf(currentId);
  focusIds = focusIds.filter(id => S.decisions.some(x => x.id === id && x.status !== 'ratified'));
  focusIdx = Math.max(0, Math.min(prevIdx < 0 ? focusIdx : prevIdx, focusIds.length - 1));
  focusFacet = null; askOpen = false;
  toast(`recorded ${pickedIds.length} decision${pickedIds.length > 1 ? 's' : ''}`);
  if (!focusIds.length) return exitFocus();
  renderFocus();
}

// ---- render + routing -----------------------------------------------------------
const RENDER = { now: viewNow, agents: viewAgents, board: viewBoard };
function render() {
  if (!S) return;
  renderBeacon();
  renderChrome();
  try { (RENDER[VIEW] || viewNow)(); }
  catch (err) {
    console.error(err);
    $('#view').appendChild(el(`<div class="empty"><div class="empty__glyph">✕</div><div>Render error: ${esc(err?.message || err)}</div></div>`));
  }
  if (focusIds) renderFocus();
  if (openCard) {
    const editing = $('#detail').contains(document.activeElement);
    if (cardById(openCard)) { if (!editing) showDetail(openCard); } else closeDetail();
  }
}
function go(view) { VIEW = view; location.hash = view; render(); }

document.addEventListener('keydown', (e) => {
  if (focusIds) {
    if (e.key === 'Escape') { e.preventDefault(); return exitFocus(); }
    if (/INPUT|TEXTAREA/.test(document.activeElement?.tagName)) return;
    const d = S.decisions.find(x => x.id === focusIds[focusIdx]);
    if (e.key === 'ArrowLeft') return focusGo(-1);
    if (e.key === 'ArrowRight') return focusGo(1);
    if (e.key === 'Enter') { if (focusIds.some(id => pick[id])) recordBatch(); else focusGo(1); return; }
    const n = parseInt(e.key, 10);
    if (n >= 1 && n <= 9 && d && d.options && d.options[n - 1]) { pick[d.id] = d.options[n - 1].key; updateChoice(); }
    return;
  }
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); return openPalette(); }
  if (paletteOpen) return;   // palette handles its own keys
  if (e.key === 'Escape') return closeDetail();
  if (openCard || /input|textarea|select/i.test(document.activeElement?.tagName) || document.activeElement?.isContentEditable) return;
  if (VIEW === 'now') {
    if (e.key === 'j' || e.key === 'ArrowDown') { e.preventDefault(); return nowMove(1); }
    if (e.key === 'k' || e.key === 'ArrowUp') { e.preventDefault(); return nowMove(-1); }
    if (e.key === 'Enter' && nowSel >= 0) { e.preventDefault(); return nowActivate(); }
  }
  const i = ['1', '2', '3'].indexOf(e.key);
  if (i >= 0) go(VIEWS[i].id);
});

// ---- command palette (⌘K / Ctrl-K) -------------------------------------------
let paletteOpen = false;
function paletteItems(q) {
  const needle = q.toLowerCase();
  const hit = (s) => s.toLowerCase().includes(needle);
  const items = [];
  for (const v of VIEWS) if (!q || hit(v.name)) items.push({ label: `view · ${v.name}`, act: () => go(v.id) });
  for (const a of ROSTER) if (!q || hit(a.name)) items.push({ label: `agent · ${a.name} (${a.online ? 'online' : 'offline'})`, act: () => { THREAD = a.name; go('agents'); } });
  for (const d of openDecisions()) if (!q || hit(d.id + ' ' + d.title)) items.push({ label: `decide · ${d.id} — ${d.title.slice(0, 60)}`, act: () => focusAll(d.id) });
  for (const c of S.cards) {
    if (c.phase === 'done' && q.length < 2) continue;
    if (!q || hit('#' + c.num + ' ' + c.title)) items.push({ label: `card · #${c.num} ${c.title.slice(0, 60)} (${c.phase})`, act: () => showDetail(c.id) });
  }
  return items.slice(0, 12);
}
function openPalette() {
  if (paletteOpen) return;
  paletteOpen = true;
  let sel = 0;
  const box = el(`<div class="palette" role="dialog" aria-label="Jump to">
      <input class="palette__in" placeholder="Jump to card, ballot, agent, view…" aria-label="Search">
      <div class="palette__list"></div></div>`);
  const scrim = el('<div class="scrim"></div>');
  document.body.append(scrim, box);
  const input = $('.palette__in', box);
  const list = $('.palette__list', box);
  let items = [];
  const paint = () => {
    items = paletteItems(input.value.trim());
    sel = Math.min(sel, Math.max(0, items.length - 1));
    list.innerHTML = items.map((it, i) => `<button class="palette__item ${i === sel ? 'sel' : ''}">${esc(it.label)}</button>`).join('') || '<div class="palette__empty">no matches</div>';
    list.querySelectorAll('.palette__item').forEach((n, i) => n.addEventListener('click', () => pick(i)));
  };
  const close = () => { paletteOpen = false; box.remove(); scrim.remove(); };
  const pick = (i) => { const it = items[i]; close(); it?.act(); };
  input.addEventListener('input', () => { sel = 0; paint(); });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') { e.preventDefault(); close(); }
    else if (e.key === 'ArrowDown') { e.preventDefault(); sel = Math.min(items.length - 1, sel + 1); paint(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); sel = Math.max(0, sel - 1); paint(); }
    else if (e.key === 'Enter') { e.preventDefault(); pick(sel); }
  });
  scrim.addEventListener('click', close);
  paint(); input.focus();
}
$('#scrim').addEventListener('click', closeDetail);
window.addEventListener('hashchange', () => { const h = location.hash.slice(1); if (RENDER[h]) { VIEW = h; render(); } });

// live updates: SSE stream (fallback: slow poll); roster refresh only while
// on Agents and only when it won't disturb the owner
connectStream();
setInterval(async () => {
  if (VIEW !== 'agents' || uiBusy() || document.hidden) return;
  await refreshRoster(); renderPreservingScroll();
}, 15000);
document.addEventListener('visibilitychange', () => { if (!document.hidden) { refresh(); } });

// PWA: service worker + push
if ('serviceWorker' in navigator) navigator.serviceWorker.register('/sw.js').catch(() => {});
async function enablePush() {
  try {
    const reg = await navigator.serviceWorker.ready;
    const perm = await Notification.requestPermission();
    if (perm !== 'granted') return toast('notifications not allowed', true);
    const { key } = await (await fetch('/api/push/key')).json();
    const raw = Uint8Array.from(atob(key.replace(/-/g, '+').replace(/_/g, '/')), c => c.charCodeAt(0));
    const sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: raw });
    const r = await fetch('/api/push/subscribe', { method: 'POST', body: JSON.stringify({ subscription: sub.toJSON() }) });
    if ((await r.json()).ok) { toast('push enabled on this device'); render(); }
  } catch (e) { toast('push failed: ' + (e.message || e), true); }
}
window.__towerEnablePush = enablePush;
(async () => {
  if (!('serviceWorker' in navigator) || !('PushManager' in window)) return;
  const bell = $('#bell');
  bell.addEventListener('click', enablePush);
  try {
    const reg = await navigator.serviceWorker.ready;
    const sub = await reg.pushManager.getSubscription();
    bell.hidden = !!sub;
  } catch { bell.hidden = false; }
})();

if (RENDER[location.hash.slice(1)]) VIEW = location.hash.slice(1);
// top-level awaits: the window load event waits for the first full render
await refreshRoster();
await refresh();
// deep links: ?focus=<decisionId> opens focus mode, ?open=<cardId|#n> a card
const qs = new URLSearchParams(location.search);
if (qs.get('focus') && S) focusAll(qs.get('focus'));
if (qs.get('open') && S) { const c = S.cards.find(x => x.id === qs.get('open') || '#' + x.num === qs.get('open')); if (c) showDetail(c.id); }
