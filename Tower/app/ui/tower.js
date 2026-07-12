// Tower client. Vanilla JS, no framework, no build.
// Two views: Now (everything blocked on the owner) and Board (epochs →
// milestones → cards). The beacon on the left edge carries one lit segment
// per owner-blocking item; clearing them darkens it.

let S = null;                 // projected state from /api/state
let VIEW = 'now';
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
  let r;
  try {
    r = await fetch('/api/' + route, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(payload || {}) });
  } catch {
    toast('server unreachable — action NOT saved', true);
    throw new Error('offline');
  }
  if (r.status === 401) { showUnlock(); throw new Error('unauthorized'); }
  const j = await r.json().catch(() => ({}));
  if (!r.ok || j.ok === false) { toast(j.message || `request failed: ${route}`, true); throw new Error(j.message || route); }
  if (j.state) applyState(j.state, { own: true });
  if (UNDOABLE[route] && j.state) undoToast(UNDOABLE[route], j.state.meta.rev);
  return j.result;
};

// Auth expired / never set on this device → full-screen unlock, never a
// silent failure.
let unlockShown = false;
function showUnlock() {
  if (unlockShown) return;
  unlockShown = true;
  const box = el(`<div class="unlock" role="dialog" aria-label="Unlock">
      <div class="unlock__card">
        <div class="unlock__mark">TOWER<b>.</b></div>
        <div class="unlock__t">This device isn't unlocked — actions are being rejected.<br>Paste the access key (<code>auth.token</code> in <code>.tower/secrets.json</code>).</div>
        <input class="unlock__in" placeholder="access key" autocomplete="off">
        <button class="btn btn--red" id="unlock-go">Unlock</button>
      </div></div>`);
  document.body.appendChild(box);
  const go = () => {
    const k = $('.unlock__in', box).value.trim();
    if (k) location.href = '/?key=' + encodeURIComponent(k) + location.hash;
  };
  $('#unlock-go', box).addEventListener('click', go);
  $('.unlock__in', box).addEventListener('keydown', e => { if (e.key === 'Enter') go(); });
  $('.unlock__in', box).focus();
}

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
  // server was upgraded/restarted under us → reload for fresh UI code, but
  // only when the owner isn't mid-anything
  if (S?.boot && next.boot && S.boot !== next.boot) {
    if (!uiBusy()) return location.reload();
    pending = next; S = { ...next, boot: S.boot }; return;
  }
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
  render();
  window.scrollTo(0, y);
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
  try {
    const r = await fetch('/api/state');
    if (r.status === 401) return showUnlock();
    applyState(await r.json());
  } catch { /* offline */ }
}

// ---- derived --------------------------------------------------------------
const cardById = (id) => S.cards.find(c => c.id === id);
const ticket = (c) => '#' + (c.num ?? '');
const CFG = () => S.config || {};
const TERM = (k, fb) => ((CFG().terms || {})[k] || fb);
const epochTag = (e) => e ? (e.num != null ? `${TERM('epoch', 'Epoch')} ${e.num}` : e.id) : '';
const openDecisions = () => S.decisions.filter(d => d.status !== 'ratified');
const toActivate = () => S.cards.filter(c => c.lane.lane === 'activate');

// waiting-time chip: shown once something has sat for 6+ hours
function ageChip(iso) {
  if (!iso) return '';
  const h = (Date.now() - new Date(iso).getTime()) / 3.6e6;
  if (h < 6) return '';
  const label = h < 48 ? Math.round(h) + 'h' : Math.round(h / 24) + 'd';
  return `<span class="agechip ${h > 72 ? 'agechip--hot' : ''}" title="waiting ${label}">${label}</span>`;
}
const ageOf = (it) => it.type === 'decision' ? it.decision.created : it.card.created;

// Every owner-blocking item, in the order the beacon + Now view show them.
function duties() {
  const out = [];
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
    const seg = el(`<button class="beacon__seg" style="opacity:${Math.min(1, .55 + h / 96).toFixed(2)}" title="${esc(it.type === 'decision' ? it.decision.title : 'greenlight: ' + it.card.title)}"></button>`);
    seg.addEventListener('click', () => jumpTo(it));
    b.appendChild(seg);
  }
}
function jumpTo(it) {
  if (it.type === 'decision') focusAll(it.decision.id);
  else { go('now'); }
}

// ---- chrome ------------------------------------------------------------------
const VIEWS = [
  { id: 'now', name: 'Now', count: () => duties().length, alert: true },
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

  const rec = recentlyDecidedBlock();
  if (rec) v.appendChild(rec);

  if (!items.length) {
    v.appendChild(el(`<div class="nowclear">
      <div class="nowclear__mark">▲</div>
      <div class="nowclear__t">Tower clear — nothing is blocked on you.</div>
      <div class="nowclear__sub">${S.counts.agentReady} cards agent-ready · new ballots and greenlights land here</div>
    </div>`));
    return;
  }

  const section = (title) => v.appendChild(el(`<div class="nowsection"><span class="nowsection__t">${title}</span><span class="nowsection__rule"></span></div>`));

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

// ---- #461 walk-back buffer: collapsed, quiet strip of ratified-but-still- --
// live decisions, so a fresh ratification is a one-tap Reopen away.
function recentlyDecidedBlock() {
  const items = S.recentlyDecided || [];
  if (!items.length) return null;
  const days = (S.config || {}).retireAfterDays ?? 3;
  return collapsible('now-recent', false,
    `<span class="epoch__tag" style="color:var(--frost)">Recently decided</span><span class="epoch__name">reversible for ${days} days</span><span class="epoch__count">${items.length}</span>`,
    'epoch--off', (body) => {
      for (const r of items) {
        const c = cardById(r.cardId);
        const row = el(`<div class="idea"><span class="idea__t">${esc(r.title)}
            <span class="idea__note">→ ${esc(r.outcome)}${c ? ' · ' + ticket(c) : ''}${r.comment ? ' — ' + esc(r.comment) : ''}</span></span>
            <button class="btn btn--ghost btn--sm" data-reopen>Reopen</button></div>`);
        $('[data-reopen]', row).addEventListener('click', () => api('clearance/reopen', { decisionId: r.id, by: 'owner' }));
        body.appendChild(row);
      }
    });
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

// #461: archived count per epoch, fetched lazily (once an epoch section is
// opened) and cached in memory — cheap, no localStorage, refetches on reload.
const archivedCountCache = {};
async function archivedCountFor(epochId) {
  if (archivedCountCache[epochId] != null) return archivedCountCache[epochId];
  try {
    const j = await (await fetch(`/api/history?epoch=${encodeURIComponent(epochId)}`)).json();
    archivedCountCache[epochId] = j.count || 0;
  } catch { archivedCountCache[epochId] = 0; }
  return archivedCountCache[epochId];
}

// #464/#merge: Board = Radar's body (roadmap ledger × ops table: per-epoch
// burndown, milestone stalls, sortable/inline-editable/filterable card
// table) fused with the old Board tab's idea-capture + new-card UI. See
// Tower/README.md.
let radarFilterText = '';
const radarSort = {}; // table key -> { col, dir }

function radarMatches(c, needle) {
  if (!needle) return true;
  return ('#' + c.num).includes(needle) || c.title.toLowerCase().includes(needle) || (c.assignee || '').toLowerCase().includes(needle);
}

function viewBoard() {
  const v = $('#view');
  const cardsMode = isOpen('radar-cards', false);
  v.innerHTML = `<div class="viewhead"><h1 class="h1">Board</h1>
      <span class="viewhead__sub">roadmap ledger × ops table</span>
      <div class="viewhead__actions">
        <button class="btn btn--ghost" id="radar-mode" title="Switch between table rows and card tiles">${cardsMode ? '☰ Table view' : '⊞ Card view'}</button>
        <button class="btn btn--red" id="new-card">+ New card</button>
      </div></div>
    <div class="capture"><input id="idea-input" placeholder="Capture an idea — it waits in Ideas until you make it a card…">
      <button class="btn" id="idea-btn">Capture</button></div>
    <div class="capture"><input id="radar-filter" placeholder="Filter by #, title, assignee…" value="${esc(radarFilterText)}"></div>
    <div id="radar-body"></div>`;

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
  $('#radar-mode').addEventListener('click', () => api('ui/toggle', { key: 'radar-cards' }));
  $('#radar-filter').addEventListener('input', (e) => { radarFilterText = e.target.value; renderRadarBody(); });

  // Ideas bay — the triage half of idea-capture, kept from Board so a
  // captured idea has somewhere to be promoted or dismissed.
  const ideas = S.ideas.filter(b => b.status !== 'tagged');
  if (ideas.length) {
    const ideasSec = collapsible('board-ideas', true,
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
      });
    v.insertBefore(ideasSec, $('#radar-body', v));
  }

  renderRadarBody();
}

// One body per section, honoring the card/table mode toggle.
const radarList = (key, cards, cardsMode) => cardsMode ? grid(cards) : opsTable(key, cards);

function renderRadarBody() {
  const body = $('#radar-body');
  if (!body) return;
  const focused = document.activeElement === $('#radar-filter');
  body.innerHTML = '';
  const needle = radarFilterText.trim().toLowerCase();
  const cardsMode = isOpen('radar-cards', false);
  const radar = S.radar || [];

  // Sidequests: their own section — off-plan work, not part of any epoch.
  const sq = S.cards.filter(c => c.track === 'sidequest' && !['done', 'frozen'].includes(c.phase));
  if (sq.length) body.appendChild(radarListSection('radar-sq', TERM('sidequest', 'Sidequests'), 'off-plan work', sq.filter(c => radarMatches(c, needle)), sq.length, cardsMode, true));

  if (!radar.length && !sq.length) {
    body.appendChild(el(`<div class="empty"><div class="empty__glyph">—</div><div>no active epochs</div></div>`));
    return;
  }
  for (const r of radar) body.appendChild(radarEpochSection(r, needle, cardsMode));

  // Frozen: parked on purpose, any track — its own section, collapsed by default.
  const fz = S.cards.filter(c => c.phase === 'frozen');
  if (fz.length) body.appendChild(radarListSection('radar-frozen', 'Frozen', 'parked on purpose', fz.filter(c => radarMatches(c, needle)), fz.length, cardsMode, false));

  if (focused) $('#radar-filter')?.focus();
}

function radarListSection(key, name, sub, shown, total, cardsMode, defOpen) {
  return collapsible(key, defOpen,
    `<span class="epoch__tag" style="color:var(--frost)">${esc(name)}</span><span class="epoch__name">${esc(sub)}</span><span class="epoch__count">${total}</span>`,
    'epoch--off', (body) => {
      body.appendChild(shown.length ? radarList(key, shown, cardsMode) : el(`<p class="epoch__goal">no match</p>`));
    });
}

function radarEpochSection(r, needle, cardsMode) {
  const e = S.epochs.find(x => x.id === r.id);
  return collapsible('radar:' + r.id, true,
    `<span class="epoch__tag">${esc(e ? epochTag(e) : r.id)}</span><span class="epoch__name">${esc(r.name)}</span>
     <span class="epoch__count">${r.done}/${r.done + r.active} done · ${r.pct}%</span>`,
    '', (body) => {
      if (r.goal) body.appendChild(el(`<p class="epoch__goal">${esc(r.goal)}</p>`));
      body.appendChild(radarHead(r));
      if (r.milestones.length) body.appendChild(radarMilestones(r.milestones));

      const ledgerLine = el(`<p class="epoch__goal" style="opacity:.6">ledger: ${r.done} done live</p>`);
      body.appendChild(ledgerLine);
      archivedCountFor(r.id).then(n => { if (n) ledgerLine.textContent = `ledger: ${r.done} done live · ${n} archived`; });

      const active = S.cards.filter(c => c.epoch === r.id && c.track !== 'sidequest' && !['done', 'frozen'].includes(c.phase));
      const activeShown = active.filter(c => radarMatches(c, needle));
      body.appendChild(el(`<div class="radar__subhead">Active</div>`));
      body.appendChild(activeShown.length ? radarList(r.id, activeShown, cardsMode) : el(`<p class="epoch__goal">${active.length ? 'no match' : 'no active cards'}</p>`));
    });
}

function radarHead(r) {
  const wrap = el(`<div class="radar__head"></div>`);
  wrap.appendChild(el(`<div class="epoch__prog"><div class="epoch__bar"><i style="width:${r.pct}%"></i></div><span class="epoch__pct">${r.active} active · ${r.pct}%</span></div>`));
  wrap.appendChild(radarSparkline(r.burndown));
  return wrap;
}

function radarSparkline(days) {
  const max = Math.max(1, ...days.map(d => d.n));
  const todayKey = new Date().toISOString().slice(0, 10);
  const bars = days.map(d => {
    const h = d.n ? Math.max(3, Math.round((d.n / max) * 22)) : 2;
    const cls = 'spark__bar' + (d.day === todayKey ? ' spark__bar--today' : '');
    return `<i class="${cls}" style="height:${h}px" title="${esc(d.day)}: ${d.n} done"></i>`;
  }).join('');
  return el(`<div class="spark" aria-label="30-day burndown">${bars}</div>`);
}

function radarMilestones(ms) {
  const wrap = el(`<div class="miles"><div class="miles__h">${esc(TERM('milestones', 'Milestones'))}</div></div>`);
  for (const m of ms) {
    const pct = m.total ? Math.round(m.done / m.total * 100) : 0;
    const stalled = m.stalledDays != null && m.stalledDays > 5;
    const row = el(`<div class="mile ${m.met ? 'mile--met' : ''}" title="${esc(m.goal || '')}">
        <span class="mile__dot">${m.met ? '✓' : '◇'}</span>
        <span class="mile__t">${esc(m.title)}</span>
        <span class="mile__bar"><i style="width:${m.met ? 100 : pct}%"></i></span>
        <span class="mile__n">${m.met ? 'met' : `${m.done}/${m.total}`}</span>
        ${stalled ? `<span class="agechip agechip--hot" title="no activity in ${m.stalledDays}d">⚠ stalled ${m.stalledDays}d</span>` : ''}
      </div>`);
    wrap.appendChild(row);
  }
  return wrap;
}

const OPS_COLS = [
  { k: 'num', label: '#' },
  { k: 'title', label: 'Title' },
  { k: 'lane', label: 'Lane' },
  { k: 'priority', label: 'Priority' },
  { k: 'workOrder', label: 'Order' },
  { k: 'assignee', label: 'Assignee' },
  { k: 'updated', label: 'Updated' },
];
const opsSortVal = (c, col) => {
  if (col === 'workOrder') return c.workOrder == null ? Infinity : c.workOrder;
  if (col === 'num') return c.num ?? 0;
  if (col === 'priority') return PR[c.priority] ?? 9;
  if (col === 'updated') return c.updated || '';
  if (col === 'lane') return c.lane?.label || '';
  return String(c[col] || '').toLowerCase();
};
function sortOpsRows(key, cards) {
  const st = radarSort[key] || { col: 'workOrder', dir: 'asc' };
  const dir = st.dir === 'asc' ? 1 : -1;
  return [...cards].sort((a, b) => {
    const av = opsSortVal(a, st.col), bv = opsSortVal(b, st.col);
    if (av < bv) return -1 * dir;
    if (av > bv) return 1 * dir;
    return (a.num ?? 0) - (b.num ?? 0);
  });
}
function ageAgo(dateStr) {
  if (!dateStr) return '—';
  const days = Math.floor((Date.now() - new Date(dateStr + 'T00:00:00Z').getTime()) / 86_400_000);
  if (days <= 0) return 'today';
  return `${days}d`;
}

function opsTable(key, cards) {
  const st = radarSort[key] || { col: 'workOrder', dir: 'asc' };
  const sorted = sortOpsRows(key, cards);
  const wrap = el(`<div class="opswrap"><table class="ops"><thead><tr>
      ${OPS_COLS.map(c => `<th data-col="${c.k}">${esc(c.label)}${st.col === c.k ? (st.dir === 'asc' ? ' ▲' : ' ▼') : ''}</th>`).join('')}
    </tr></thead><tbody></tbody></table></div>`);
  wrap.querySelectorAll('th').forEach(th => th.addEventListener('click', () => {
    const col = th.dataset.col;
    const cur = radarSort[key] || { col: 'workOrder', dir: 'asc' };
    radarSort[key] = cur.col === col ? { col, dir: cur.dir === 'asc' ? 'desc' : 'asc' } : { col, dir: 'asc' };
    renderRadarBody();
  }));
  const tbody = $('tbody', wrap);
  for (const c of sorted) tbody.appendChild(opsRow(c));
  return wrap;
}

function opsRow(c) {
  const title = c.title.length > 46 ? c.title.slice(0, 45) + '…' : c.title;
  const who = c.lane.who === 'owner' ? 'lane-owner' : c.lane.who === 'agent' ? 'lane-agent' : 'lane-none';
  const tr = el(`<tr>
      <td class="num">${ticket(c)}</td>
      <td class="ops__title" title="${esc(c.title)}">${esc(title)}</td>
      <td><span class="card__lane ${who}"><span class="pip"></span>${esc(c.lane.label)}</span></td>
      <td class="ops__prio"></td>
      <td class="ops__wo"></td>
      <td class="ops__as"></td>
      <td class="num">${ageAgo(c.updated)}</td>
    </tr>`);

  const prioSel = el(`<select data-fld="priority">${(CFG().priorities || ['P0', 'P1', 'P2', 'P3']).map(p => `<option value="${esc(p)}" ${p === c.priority ? 'selected' : ''}>${esc(p)}</option>`).join('')}</select>`);
  prioSel.addEventListener('click', (ev) => ev.stopPropagation());
  prioSel.addEventListener('change', () => api('card/update', { id: c.id, priority: prioSel.value, by: 'owner' }));
  $('.ops__prio', tr).appendChild(prioSel);

  const woIn = el(`<input data-fld="workOrder" type="number" min="1" value="${c.workOrder ?? ''}" placeholder="—">`);
  woIn.addEventListener('click', (ev) => ev.stopPropagation());
  woIn.addEventListener('change', () => api('card/update', { id: c.id, workOrder: woIn.value === '' ? null : Number(woIn.value), by: 'owner' }));
  $('.ops__wo', tr).appendChild(woIn);

  const asIn = el(`<input data-fld="assignee" value="${esc(c.assignee || '')}" placeholder="—">`);
  asIn.addEventListener('click', (ev) => ev.stopPropagation());
  asIn.addEventListener('change', () => api('card/update', { id: c.id, assignee: asIn.value.trim() === '' ? null : asIn.value.trim(), by: 'owner' }));
  $('.ops__as', tr).appendChild(asIn);

  tr.addEventListener('click', (ev) => { if (!/INPUT|SELECT/.test(ev.target.tagName)) showDetail(c.id); });
  return tr;
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
      <div class="modal__h">Exit criteria <label class="crit__flag"><input type="checkbox" id="needs-acceptance" ${c.needsAcceptance ? 'checked' : ''}> needs owner acceptance</label></div>
      <div id="m-criteria"></div>
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

  const cb = $('#m-criteria', m);
  if (!(c.criteria || []).length) cb.appendChild(el(`<p class="prose">No exit criteria yet.</p>`));
  for (const it of (c.criteria || [])) {
    cb.appendChild(el(`<div class="critrow">
        <div class="critrow__head"><span class="critrow__n">#${it.n}</span>
          <span class="critrow__badge critrow__badge--${it.status}">${esc(it.status)}</span></div>
        <div class="critrow__text">${esc(it.text)}</div>
        ${it.evidence ? `<div class="critrow__ev">${esc(it.evidence)}</div>` : ''}
        ${it.metBy || it.verifiedBy ? `<div class="critrow__by">${it.metBy ? `met: ${esc(it.metBy)}` : ''}${it.verifiedBy ? `  verified: ${esc(it.verifiedBy)}` : ''}</div>` : ''}
      </div>`));
  }
  const critAdd = el(`<div class="qadd"><input placeholder="Add exit criterion…"><button class="btn btn--red btn--sm">Add</button></div>`);
  const postCrit = async () => { const i = $('input', critAdd); const t = i.value.trim(); if (!t) return; i.value = ''; await api('card/criteria-add', { id, text: t, by: 'owner' }); };
  $('button', critAdd).addEventListener('click', postCrit);
  $('input', critAdd).addEventListener('keydown', e => { if (e.key === 'Enter') postCrit(); });
  cb.appendChild(critAdd);
  $('#needs-acceptance', m).addEventListener('change', (e) => api('card/update', { id, needsAcceptance: e.target.checked, by: 'owner' }));

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
  if (d.lesson) f.push(['lesson', 'Learn this first']);
  if (d.story) f.push(['story', 'Story']);
  if (d.explainer) f.push(['why', 'Why it matters']);
  if (d.inWild) f.push(['wild', 'In the wild']);
  if ((d.comparisons || []).length) f.push(['langs', 'Elsewhere']);
  if (d.detail) f.push(['detail', 'Q&A']);
  return f;
}
function facetBody(d, fk) {
  if (fk === 'lesson') return `<p>${esc(d.lesson).replace(/\n/g, '<br>')}</p>`;
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
      ${d.hybrid?.synthesis ? `<div class="hybrid"><b>Hybrid pass — ${esc(d.hybrid.result)}:</b> ${esc(d.hybrid.synthesis)}
        ${(d.hybrid.harvest || []).map(x => `<p><b>From ${esc(x.key)}:</b> ${esc(x.aspect || '')} — ${esc(x.use || '')}</p>`).join('')}</div>` : ''}
      ${d.rec ? `<div class="recline"><b>Recommendation:</b> ${esc(d.rec)}${optName(d, d.rec) ? ' — ' + esc(optName(d, d.rec)) : ''}
        ${d.recommendation?.why ? `<p><b>Why this wins:</b> ${esc(d.recommendation.why)}</p>` : ''}
        ${(d.recommendation?.whyNot || []).map(x => `<p><b>Why not ${esc(x.key)}:</b> ${esc(x.reason || '')}</p>`).join('')}
        ${d.recommendation?.tradeoff ? `<p><b>Accepted tradeoff:</b> ${esc(d.recommendation.tradeoff)}</p>` : ''}</div>` : ''}
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
        ${o.technical ? `<details class="opt__technical"><summary>Technical details</summary><div>${esc(o.technical).replace(/\n/g, '<br>')}</div></details>` : ''}
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
const RENDER = { now: viewNow, board: viewBoard };
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
  const i = ['1', '2'].indexOf(e.key);
  if (i >= 0) go(VIEWS[i].id);
});

// ---- command palette (⌘K / Ctrl-K) -------------------------------------------
let paletteOpen = false;
function paletteItems(q) {
  const needle = q.toLowerCase();
  const hit = (s) => s.toLowerCase().includes(needle);
  const items = [];
  for (const v of VIEWS) if (!q || hit(v.name)) items.push({ label: `view · ${v.name}`, act: () => go(v.id) });
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

// live updates: SSE stream (fallback: slow poll)
connectStream();
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
// top-level await: the window load event waits for the first full render
await refresh();
// deep links: ?focus=<decisionId> opens focus mode, ?open=<cardId|#n> a card
const qs = new URLSearchParams(location.search);
if (qs.get('focus') && S) focusAll(qs.get('focus'));
if (qs.get('open') && S) { const c = S.cards.find(x => x.id === qs.get('open') || '#' + x.num === qs.get('open')); if (c) showDetail(c.id); }
