// Tower — client. Vanilla JS, no framework.
let S = null;
let VIEW = 'decisions';     // decisions lead — they're what's blocked on the owner
let openCard = null;        // card id in the modal
let focusIds = null;        // [decisionId] when focus mode is open
let focusIdx = 0;
let dispatchFilter = '';
const pick = {};            // decisionId -> tentative option key

const $ = (s, r = document) => r.querySelector(s);
const api = async (route, payload) => {
  const r = await fetch('/api/' + route, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(payload || {}) });
  const j = await r.json().catch(() => ({}));
  if (!r.ok || j.ok === false) throw new Error(j.error || `request failed: ${route}`);
  if (j.state) { S = j.state; render(); }
  return j.result;
};
const load = async () => {
  S = await (await fetch('/api/state')).json(); render();
  const qs = new URLSearchParams(location.search);
  const open = qs.get('open');
  if (open && cardById(open)) showDetail(open);
  if (qs.get('focus')) {
    const ids = S.decisions.filter(d => d.status !== 'ratified').map(d => d.id);
    const at = Math.max(0, ids.indexOf(qs.get('focus')));
    if (ids.length) enterFocus(ids, at);
  }
  if (qs.get('legend')) openLegend();
};

const esc = (s) => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const md = (s) => esc(s).replace(/`([^`]+)`/g, '<code>$1</code>').replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
  .split(/\n{2,}/).map(p => `<p>${p.replace(/\n/g, '<br>')}</p>`).join('');
// tiny multi-language syntax highlighter — tokenizes, then escapes each token
const HL_KW = new Set(('fn func function def let var const val mut return yield if elif else match switch case when default for while loop do in of as is import use mod module package from pub priv private public protected internal static struct enum trait impl interface type class extends implements where async await comptime defer go chan select new self this super sizeof typeof null nil none None true false True False and or not break continue throw try catch finally with lambda then begin end macro derive emit').split(' '));
function hl(src) {
  const re = /(\/\/[^\n]*|\/\*[\s\S]*?\*\/|#\s[^\n]*)|([#@][A-Za-z_]\w*)|("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)|(0[xX][0-9a-fA-F_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)|([A-Za-z_$]\w*)|(\s+)|([\s\S])/g;
  let m, out = '';
  while ((m = re.exec(src))) {
    if (m[1]) out += `<span class="hl-c">${esc(m[1])}</span>`;
    else if (m[2]) out += `<span class="hl-t">${esc(m[2])}</span>`;          // #Marker / @attr
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
const phaseLabel = (id) => (S.phases.find(p => p.id === id) || {}).label || id;
const epochOf = (id) => S.epochs.find(e => e.id === id);
const ticket = (c) => '#' + (c.num ?? String(c.id).replace(/\D/g, ''));
const el = (h) => { const t = document.createElement('template'); t.innerHTML = h.trim(); return t.content.firstElementChild; };
const cardById = (id) => S.cards.find(c => c.id === id);

// ---- card --------------------------------------------------------------
function card(c) {
  const who = c.lane.who === 'owner' ? 'lane-owner' : c.lane.who === 'agent' ? 'lane-agent' : 'lane-none';
  const node = el(`
    <button class="card p-${c.phase} who-${c.lane.who || 'none'}" data-card="${c.id}">
      <div class="card__top">
        <span class="card__id">${ticket(c)}</span>
        <span class="card__sq sq-${c.priority}">${c.priority}</span>
        <span class="card__kind">${esc(c.kind)}</span>
        ${c.openQ ? `<span class="card__q">✎ ${c.openQ}</span>` : ''}
      </div>
      <h3 class="card__title">${esc(c.title)}</h3>
      <div class="card__foot">
        <span class="card__lane ${who}"><span class="pip"></span>${esc(c.lane.label)}</span>
      </div>
    </button>`);
  node.addEventListener('click', () => showDetail(c.id));
  return node;
}
const grid = (cards) => { const g = el('<div class="cardgrid"></div>'); cards.forEach(c => g.appendChild(card(c))); return g; };

// a group is open iff its default XOR the owner has toggled it (durable)
const isOpen = (key, def) => { const t = (S.meta.ui.toggled || []).includes(key); return def ? !t : t; };

// collapsible group; body built lazily only when open (done defaults closed)
function group(key, meta, buildBody) {
  const open = isOpen(key, meta.def ?? false);
  const g = el(`<section class="group ${open ? 'group--open' : ''}">
      <button class="group__head"><span class="group__chev">▸</span>
        ${meta.bar ? `<span class="group__bar" style="background:${meta.bar}"></span>` : ''}
        <span class="group__name">${esc(meta.name)}</span>
        ${meta.blurb ? `<span class="group__blurb">${esc(meta.blurb)}</span>` : ''}
        <span class="group__n">${meta.n}</span></button>
      <div class="group__body"></div></section>`);
  g.querySelector('.group__head').addEventListener('click', () => api('ui/toggle', { key }));
  if (open) buildBody($('.group__body', g));
  return g;
}

// collapsible board section (epoch / sidequests / frozen) with a rich header
function trackSection(key, def, o, buildBody) {
  const open = isOpen(key, def);
  const sec = el(`<section class="track ${o.off ? 'track--off' : ''} ${open ? 'is-open' : ''}">
      <button class="track__head">
        <span class="track__chev">▸</span>
        ${o.num ? `<span class="track__num">${esc(o.num)}</span>` : ''}
        <span class="track__name">${esc(o.name)}</span>
        ${o.status ? `<span class="track__status ${o.statusClass || ''}">${esc(o.status)}</span>` : ''}
        <span class="track__count">${esc(o.count)}</span>
      </button>
      <div class="track__body"></div></section>`);
  $('.track__head', sec).addEventListener('click', () => api('ui/toggle', { key }));
  if (open) buildBody($('.track__body', sec));
  return sec;
}

// ---- decisions (default) -----------------------------------------------
const DECISION_GROUPS = [
  ['syntax', 'Syntax', 'surface choices'],
  ['runtime', 'Runtime', 'tasks, I/O, build targets'],
  ['web-ui', 'Web / UI', 'reactivity, layout, rendering'],
  ['stdlib', 'Stdlib', 'core libraries'],
  ['tooling', 'Tooling', 'fmt, docs, semantic tools'],
  ['safety', 'Safety', 'effects, crypto, correctness'],
  ['research', 'Research', 'far-horizon ideas'],
  ['other', 'Other', 'needs triage'],
];

function decisionGroupOf(d) {
  const id = d.group || 'other';
  return DECISION_GROUPS.find(([k]) => k === id) ? id : 'other';
}

function viewDecisions() {
  const v = $('#view');
  const open = S.decisions.filter(d => d.status !== 'ratified');
  const decided = S.decisions.filter(d => d.status === 'ratified');
  const toActivate = S.cards.filter(c => c.lane.lane === 'activate');
  v.innerHTML = `<div class="view__head">
      <h1 class="view__title">Decisions</h1>
      <span class="view__sub"><b>${open.length}</b> awaiting you · recorded on the card, never out of sync</span>
      <div class="view__actions">${open.length ? `<button class="btn btn--red" id="focus-btn">Focus mode →</button>` : ''}</div>
    </div>`;

  if (toActivate.length) {
    const strip = el(`<div class="activate-strip"><div class="activate-strip__h">Also waiting on you — ${toActivate.length} new card${toActivate.length > 1 ? 's' : ''} to greenlight (start work)</div></div>`);
    toActivate.sort(byPrio).forEach(c => {
      const row = el(`<div class="activate-row">
          <span class="activate-row__id">${ticket(c)}</span><span class="card__sq sq-${c.priority}">${c.priority}</span>
          <span class="activate-row__t">${esc(c.title)}</span>
          <button class="btn btn--sm" data-open>Open</button>
          <button class="btn btn--red btn--sm" data-act>Greenlight</button></div>`);
      $('[data-open]', row).addEventListener('click', () => showDetail(c.id));
      $('[data-act]', row).addEventListener('click', () => api('card/activate', { id: c.id }));
      strip.appendChild(row);
    });
    v.appendChild(strip);
  }

  const feed = el('<div class="decfeed"></div>');
  if (!open.length) feed.appendChild(emptyState('✓', 'All decisions made — nothing is waiting on you.'));
  if (open.length) {
    DECISION_GROUPS.forEach(([key, name, blurb]) => {
      const ds = open.filter(d => decisionGroupOf(d) === key);
      if (!ds.length) return;
      const g = group('dec-group:' + key, { name, blurb, n: ds.length }, (body) => {
        ds.forEach(d => body.appendChild(decisionCard(d)));
      });
      feed.appendChild(g);
    });
  }
  if (decided.length) {
    const g = group('dec-done', { name: 'Decided', blurb: 'history', n: decided.length }, (body) => {
      decided.slice().reverse().forEach(d => body.appendChild(decisionCard(d)));
    });
    feed.appendChild(g);
  }
  v.appendChild(feed);
  if (open.length) $('#focus-btn').addEventListener('click', () => focusAll(open[0].id));
}

// compact queue row — the rich deciding happens in focus mode
function decisionCard(d) {
  const c = cardById(d.cardId);
  const decided = d.status === 'ratified';
  const groupName = (DECISION_GROUPS.find(([k]) => k === decisionGroupOf(d)) || [null, 'Other'])[1];
  const meta = `${groupName} · ${(d.options || []).length} options · ${(d.comparisons || []).length} language comparisons${c && c.openQ ? ` · ${c.openQ} open question${c.openQ > 1 ? 's' : ''}` : ''}`;
  const node = el(`<article class="deccard ${decided ? 'deccard--decided' : ''}">
      <div class="deccard__head">
        <span class="deccard__id">${esc(d.id)}</span>
        <span class="deccard__for">card <b>${c ? ticket(c) : '—'}</b> · ${c ? esc(c.title).slice(0, 46) : 'unlinked'}</span>
        ${decided ? `<span class="deccard__outcome">✓ ${esc(d.outcome)} · ${esc(d.ratifiedAt || '')}</span>` : d.rec ? `<span class="deccard__rec">rec ${esc(d.rec)}</span>` : ''}
      </div>
      <h2 class="deccard__title">${esc(d.title)}</h2>
      ${d.gist ? `<p class="deccard__gist">${esc(d.gist)}</p>` : ''}
      <div class="deccard__foot">
        <span style="font-family:var(--mono);font-size:11px;color:var(--text-faint)">${meta}</span>
        ${decided ? `<button class="btn btn--ghost btn--sm" style="margin-left:auto" data-reopen>Reopen</button>`
      : `<button class="btn btn--red btn--sm" style="margin-left:auto" data-focus>Decide →</button>`}
      </div>
    </article>`);
  $('[data-focus]', node)?.addEventListener('click', () => focusAll(d.id));
  $('[data-reopen]', node)?.addEventListener('click', () => api('clearance/reopen', { decisionId: d.id }));
  return node;
}

// ---- focus mode (v1 layout: dots · facet tabs · options) ---------------
let focusFacet = null, askOpen = false;
function focusAll(startId) {
  const ids = S.decisions.filter(d => d.status !== 'ratified').map(d => d.id);
  let idx = ids.indexOf(startId);
  if (idx < 0) idx = 0;
  if (!ids.length) return;
  enterFocus(ids, idx);
}
function enterFocus(ids, idx) { focusIds = ids; focusIdx = idx; focusFacet = null; askOpen = false; renderFocus(); }
function exitFocus() { focusIds = null; $('#focus').hidden = true; render(); }
function focusGo(delta) { focusIdx = Math.max(0, Math.min(focusIds.length - 1, focusIdx + delta)); focusFacet = null; askOpen = false; renderFocus(); }
const optName = (d, key) => { const o = (d.options || []).find(x => x.key === key); return o ? o.name : ''; };
function availFacets(d) {
  const f = [];
  if (d.story) f.push(['story', 'Story']);
  if (d.explainer) f.push(['why', 'Why it matters']);
  if (d.inWild) f.push(['wild', 'In the wild']);
  if ((d.comparisons || []).length) f.push(['langs', 'Other languages']);
  return f;
}
function facetBody(d, fk) {
  if (fk === 'story') return `<p>${esc(d.story)}</p>`;
  if (fk === 'why') return `<p>${esc(d.explainer)}</p>`;
  if (fk === 'wild') return codeBlock(d.inWild);
  if (fk === 'langs') return (d.comparisons || []).map(c => `<div class="cmp"><div class="cmp__head"><span class="cmp__lang">${esc(c.lang)}</span><span class="cmp__note">${esc(c.note || '')}</span></div>${codeBlock(c.code || '')}</div>`).join('');
  return '';
}
function renderFocus() {
  const f = $('#focus');
  focusIds = focusIds.filter(id => {
    const x = S.decisions.find(d => d.id === id);
    return x && x.status !== 'ratified';
  });
  if (!focusIds.length) return exitFocus();
  focusIdx = Math.max(0, Math.min(focusIdx, focusIds.length - 1));
  const d = S.decisions.find(x => x.id === focusIds[focusIdx]);
  if (!d) return exitFocus();
  const c = cardById(d.cardId);
  const chosen = pick[d.id] ?? d.outcome ?? null;
  const pickedIds = focusIds.filter(id => pick[id]);
  const facets = availFacets(d);
  if (!focusFacet || !facets.some(([fk]) => fk === focusFacet)) focusFacet = facets.length ? facets[0][0] : null;
  const dots = focusIds.map((id, i) => {
    return `<span class="dot${i === focusIdx ? ' cur' : ''}${pick[id] ? ' picked' : ''}" data-i="${i}" title="${esc(id)}"></span>`;
  }).join('');
  const qs = c ? c.questions : [];

  f.innerHTML = `
    <div class="focustop">
      <span class="focustop__ctr">Decision <b>${focusIdx + 1}</b> / ${focusIds.length}</span>
      <div class="dots" id="f-dots">${dots}</div>
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
      <div class="optslabel">Choose one</div>
      <div class="opts" id="f-opts"></div>
      ${d.rec ? `<div class="recline"><b>Recommendation:</b> Option ${esc(d.rec)}${optName(d, d.rec) ? ' — ' + esc(optName(d, d.rec)) : ''}.${d.recommendation ? ' ' + esc(d.recommendation) : ''}</div>` : ''}
      <textarea class="fcomment" id="f-comment" placeholder="Comment (optional) — recorded with your decision">${esc(d.comment || '')}</textarea>
      <div class="deck-actions">
        ${chosen ? `<button class="btn btn--ghost btn--sm" id="f-clear">✕ Clear choice</button>` : ''}
        <button class="btn btn--ghost btn--sm" id="f-ask">${askOpen ? 'Close' : '✎ Ask a question'}</button>
      </div>
      ${askOpen ? `<div class="askbox"><div class="askbox__h">Ask the agent — saved to the card, no decision recorded</div>
        <textarea id="f-askt" placeholder="e.g. add a comparison to Elixir, or rework option B around streaming…"></textarea>
        <button class="btn btn--amber btn--sm" id="f-asksend">Send to agent</button></div>` : ''}
      ${qs.length ? `<div class="fqs">${qs.map(q => `<div class="qrow"><div class="qrow__top"><span class="qrow__by ${q.by === 'agent' ? 'agent' : ''}">${esc(q.by)}</span><span class="qrow__kind">${esc(q.kind)}</span><span class="qrow__status ${q.status}">${esc(q.status)}</span></div><div class="qrow__text">${esc(q.text)}</div>${q.answer ? `<div class="qrow__ans"><b>agent</b> ${esc(q.answer)}</div>` : ''}</div>`).join('')}</div>` : ''}
    </div></div>
    <div class="focusnav"><div class="focusnav__inner">
      <span class="focusnav__kbd"><b>1–9</b> pick · <b>←/→</b> move · <b>Enter</b> record selected · <b>Esc</b> close</span>
      <span class="focusnav__spacer"></span>
      <button class="btn btn--red" id="f-record" ${pickedIds.length ? '' : 'disabled'}>${recordLabel(pickedIds)}</button>
    </div></div>`;

  const opts = $('#f-opts', f);
  (d.options || []).forEach((o, idx) => {
    const node = el(`<div class="opt${chosen === o.key ? ' sel' : ''}">
        <button class="opt__h"><span class="opt__num">${idx + 1}</span><span class="opt__name">Option ${esc(o.key)} — ${esc(o.name)}</span>
          ${o.key === d.rec ? '<span class="opt__rec">recommended</span>' : ''}<span class="opt__check">✓ chosen</span></button>
        ${o.detail ? `<div class="opt__detail">${esc(o.detail)}</div>` : ''}
        ${o.code ? `<div class="opt__code">${codeBlock(o.code)}</div>` : ''}</div>`);
    $('.opt__h', node).addEventListener('click', () => { if (pick[d.id] === o.key) delete pick[d.id]; else pick[d.id] = o.key; updateFocusChoice(); });
    opts.appendChild(node);
  });

  $('#f-dots', f).querySelectorAll('.dot').forEach(dot => dot.addEventListener('click', () => { focusIdx = +dot.dataset.i; focusFacet = null; askOpen = false; renderFocus(); }));
  // facet switch updates ONLY the tabs + the panel — no full re-render
  $('#f-facets', f)?.querySelectorAll('.facet').forEach(b => b.addEventListener('click', () => {
    focusFacet = b.dataset.fk;
    $('#f-facets', f).querySelectorAll('.facet').forEach(x => x.classList.toggle('on', x.dataset.fk === focusFacet));
    $('#f-facetbody', f).innerHTML = facetBody(d, focusFacet);
  }));
  $('#f-prev', f).onclick = () => focusGo(-1);
  $('#f-next', f).onclick = () => focusGo(1);
  $('#f-close', f).onclick = exitFocus;
  $('#f-clear', f)?.addEventListener('click', () => { delete pick[d.id]; updateFocusChoice(); });
  $('#f-ask', f).onclick = () => { askOpen = !askOpen; renderFocus(); };
  $('#f-asksend', f)?.addEventListener('click', async () => {
    const t = $('#f-askt', f).value.trim(); if (!t || !c) return; askOpen = false;
    await api('question/add', { cardId: c.id, text: t, kind: 'question' });
  });
  $('#f-record', f)?.addEventListener('click', recordFocusBatch);
  f.hidden = false;
}
function recordLabel(ids) {
  if (!ids.length) return 'Record decisions';
  if (ids.length === 1) return 'Record decision · ' + ids[0] + ' = ' + pick[ids[0]];
  return 'Record decisions · ' + ids.length;
}
// update just the selection state (option highlight, record/clear buttons) — no re-render
function updateFocusChoice() {
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
    clr.addEventListener('click', () => { delete pick[d.id]; updateFocusChoice(); });
    actions.prepend(clr);
  } else if (!chosen && clr) clr.remove();
}
async function recordFocusBatch() {
  const pickedIds = focusIds.filter(id => pick[id]);
  if (!pickedIds.length) return;
  const currentId = focusIds[focusIdx];
  const currentComment = $('#f-comment')?.value.trim();
  const decisions = pickedIds.map(id => ({
    decisionId: id,
    outcome: pick[id],
    comment: id === currentId && currentComment ? currentComment : undefined,
  }));
  try {
    await api('clearance/batch', { decisions });
  } catch {
    // A running Tower server may predate the batch endpoint while serving the
    // updated JS from disk. Fall back to the long-standing single-decision route.
    for (const decision of decisions) await api('clearance', decision);
  }
  pickedIds.forEach(id => delete pick[id]);
  // Drop ratified decisions from the focus set so they vanish from the dot bar immediately.
  // Keep the cursor near the current decision instead of jumping back to the first dot.
  const prevIdx = focusIds.indexOf(currentId);
  focusIds = focusIds.filter(id => { const x = S.decisions.find(x => x.id === id); return x && x.status !== 'ratified'; });
  focusIdx = Math.max(0, Math.min(prevIdx < 0 ? focusIdx : prevIdx, focusIds.length - 1));
  focusFacet = null; askOpen = false;
  if (focusIds.length === 0) { exitFocus(); return; }
  renderFocus();
}

// ---- dispatch (agent-ready) --------------------------------------------
const AGENT_LANES = [
  { lane: 'plan', name: 'Plan', blurb: 'Build a thorough plan + raise the decisions it needs' },
  { lane: 'implement', name: 'Implement', blurb: 'Plan vetted, decisions cleared — build it' },
  { lane: 'building', name: 'Building', blurb: 'In progress — continue' },
  { lane: 'verify', name: 'Verify', blurb: 'Claimed done — verify 100%, then close/remove' },
];
function viewDispatch() {
  const v = $('#view');
  v.innerHTML = `<div class="view__head">
      <h1 class="view__title">Agent</h1>
      <span class="view__sub"><b>${S.counts.agentReady}</b> ready for an agent · point one here, or filter to an epoch or topic</span>
    </div>
    <div class="dispatch__filter"><input id="disp-filter" placeholder="Filter by text, epoch (e3), topic…" value="${esc(dispatchFilter)}"></div>
    <div id="disp-groups"></div>`;
  const input = $('#disp-filter');
  input.addEventListener('input', () => { dispatchFilter = input.value; renderDispatchGroups($('#disp-groups')); });
  renderDispatchGroups($('#disp-groups'));
}
function renderDispatchGroups(box) {
  box.innerHTML = '';
  const q = dispatchFilter.trim().toLowerCase();
  const match = (c) => !q || [c.title, c.plan, c.body, 'e' + (epochOf(c.epoch)?.num), c.track, c.kind].join(' ').toLowerCase().includes(q);

  const openQ = S.questions.filter(x => x.status === 'open');
  if (openQ.length && !q) {
    const sec = el(`<div style="margin-bottom:26px"><div class="lanehead"><span class="lanehead__name">Questions to answer</span>
        <span class="lanehead__who lane-owner">from owner</span><span class="lanehead__blurb">update the ballot or reply on the card</span>
        <span class="lanehead__n">${openQ.length}</span></div></div>`);
    openQ.forEach(qq => {
      const c = cardById(qq.cardId);
      const row = el(`<div class="qrow"><div class="qrow__top"><span class="qrow__by">owner</span><span class="qrow__kind">${esc(qq.kind)}</span>
          <span class="qrow__status">open</span></div>
          <div class="qrow__text">${esc(qq.text)}</div>
          <div class="qrow__ans"><b>card ${c ? ticket(c) : '—'}</b> · ${c ? esc(c.title).slice(0, 60) : ''} — <a href="#" data-open style="color:var(--red-bright)">open</a></div></div>`);
      $('[data-open]', row).addEventListener('click', (e) => { e.preventDefault(); showDetail(qq.cardId); });
      sec.appendChild(row);
    });
    box.appendChild(sec);
  }

  let any = false;
  for (const L of AGENT_LANES) {
    const cs = S.cards.filter(c => c.lane.lane === L.lane && match(c)).sort(byPrio);
    if (!cs.length) continue;
    any = true;
    const sec = el(`<div style="margin-bottom:26px">
        <div class="lanehead"><span class="lanehead__name">${L.name}</span><span class="lanehead__who lane-agent">agent</span>
          <span class="lanehead__blurb">${esc(L.blurb)}</span><span class="lanehead__n">${cs.length}</span></div></div>`);
    sec.appendChild(grid(cs));
    box.appendChild(sec);
  }
  const blocked = S.cards.filter(c => c.lane.lane === 'blocked' && match(c)).sort(byPrio);
  if (blocked.length) {
    const sec = el(`<div style="margin-bottom:26px"><div class="lanehead"><span class="lanehead__name">Blocked</span>
        <span class="lanehead__who lane-none">waiting</span><span class="lanehead__blurb">blocked by another card</span>
        <span class="lanehead__n">${blocked.length}</span></div></div>`);
    sec.appendChild(grid(blocked)); box.appendChild(sec); any = true;
  }
  if (!any) box.appendChild(emptyState('▸', q ? 'Nothing matches that filter.' : 'Nothing queued for agents right now.'));
}

const withClass = (node, cls) => { node.classList.add(cls); return node; };

// ---- board (epochs are the groupings; cards are the work) --------------
function viewBoard() {
  const v = $('#view');
  const tracked = S.cards.filter(c => c.phase !== 'done');
  const onPlan = tracked.filter(c => c.track === 'epoch' && c.phase !== 'frozen').length;
  const offPlan = tracked.filter(c => c.track === 'sidequest' && c.phase !== 'frozen').length;
  const frozenN = tracked.filter(c => c.phase === 'frozen').length;
  const total = onPlan + offPlan || 1;
  v.innerHTML = `<div class="view__head"><h1 class="view__title">Board</h1>
      <span class="view__sub">cards grouped by epoch · manage everything here</span>
      <div class="view__actions"><button class="btn" id="legend-btn">Key</button><button class="btn btn--red" id="new-card">+ New card</button></div></div>`;
  v.appendChild(el(`<div class="ratio">
      <div class="ratio__cell"><div class="ratio__k">On epochs</div><div class="ratio__v on">${onPlan}</div><div class="ratio__note">active on a planned epoch</div></div>
      <div class="ratio__cell"><div class="ratio__k">Sidequests</div><div class="ratio__v off">${offPlan}</div><div class="ratio__note">active off-plan work</div></div>
      <div class="ratio__cell"><div class="ratio__k">Frozen</div><div class="ratio__v">${frozenN}</div><div class="ratio__note">parked, tracked</div></div>
      <div class="ratio__cell"><div class="ratio__k">Balance</div>
        <div class="ratio__meter"><i class="on" style="width:${onPlan / total * 100}%"></i><i class="off" style="width:${offPlan / total * 100}%"></i></div>
        <div class="ratio__note">${Math.round(onPlan / total * 100)}% on the epoch plan</div></div></div>`));

  // FROZEN — first thing, collapsed by default
  const frozen = S.cards.filter(c => c.phase === 'frozen').sort(byPrio);
  v.appendChild(trackSection('board-frozen', false, { name: 'Frozen', count: `${frozen.length} parked` }, (body) => {
    body.appendChild(el(`<div class="track__goal">Parked on purpose. Agents never touch these until you greenlight one.</div>`));
    body.appendChild(withClass(grid(frozen), 'track__cards'));
  }));

  // SIDEQUESTS — before epochs, open by default
  const sqActive = S.cards.filter(c => c.track === 'sidequest' && c.phase !== 'done' && c.phase !== 'frozen').sort(byPhaseThenPrio);
  const sqDone = S.cards.filter(c => c.track === 'sidequest' && c.phase === 'done');
  v.appendChild(trackSection('board-sidequests', true, { name: 'Sidequests · off-plan work', off: true, count: `${sqActive.length} active` }, (body) => {
    body.appendChild(el(`<div class="track__goal">Real work, not on an epoch. Keep this short to stay on the plan.</div>`));
    if (sqActive.length) body.appendChild(withClass(grid(sqActive), 'track__cards'));
    else body.appendChild(el(`<div class="track__empty">none active — all sidequests are frozen or done</div>`));
    if (sqDone.length) body.appendChild(withClass(group('sqdone', { name: 'Done', blurb: 'verified', bar: 'var(--s-done)', n: sqDone.length }, (b) => b.appendChild(grid(sqDone.sort(byPrio)))), 'track__done'));
  }));

  // EPOCHS — each collapsible, open by default
  for (const e of [...S.epochs].sort((a, b) => a.order - b.order)) {
    if (e.status === 'arrived') continue;
    const all = S.cards.filter(c => c.epoch === e.id && c.track === 'epoch');
    const active = all.filter(c => c.phase !== 'done' && c.phase !== 'frozen').sort(byPhaseThenPrio);
    const doneCards = all.filter(c => c.phase === 'done');
    const pct = all.length ? Math.round(doneCards.length / all.length * 100) : 0;
    v.appendChild(trackSection('epoch:' + e.id, true,
      { num: 'Epoch ' + e.num, name: e.name, status: e.status, statusClass: e.status === 'active' ? 'active' : '', count: `${doneCards.length}/${all.length} done · ${pct}%` },
      (body) => {
        body.appendChild(el(`<div class="track__goal">${esc(e.goal)}</div>`));
        body.appendChild(el(`<div class="track__prog"><div class="track__bar"><i style="width:${pct}%"></i></div><span class="track__pct">${active.length} active</span></div>`));
        if (active.length) body.appendChild(withClass(grid(active), 'track__cards'));
        else body.appendChild(el(`<div class="track__empty">no active cards — all done or frozen</div>`));
        if (doneCards.length) body.appendChild(withClass(group('epdone:' + e.id, { name: 'Done', blurb: 'verified', bar: 'var(--s-done)', n: doneCards.length }, (b) => b.appendChild(grid(doneCards.sort(byPrio)))), 'track__done'));
      }));
  }

  $('#new-card').addEventListener('click', newCard);
  $('#legend-btn').addEventListener('click', openLegend);
}

// ---- legend / key ------------------------------------------------------
const STAGE_HELP = [
  ['triage', 'Triage', 'Captured — needs your go-ahead to start (owner)'],
  ['deciding', 'Deciding', 'Blocked on a decision (owner)'],
  ['planning', 'Planning', 'Agent builds a plan + raises decisions'],
  ['ready', 'Ready', 'Decided + vetted — agent implements'],
  ['building', 'Building', 'Implementation in progress'],
  ['verify', 'Verify', 'Claimed done — verify 100%, then close'],
  ['done', 'Done', 'Verified — hidden in collapsed groups'],
  ['frozen', 'Frozen', 'Parked — untouched until you greenlight it'],
];
function openLegend() {
  const m = $('#detail');
  m.innerHTML = `<div class="modal__panel"><div class="modal__bar"><span class="modal__id">KEY</span>
      <span class="card__kind" style="color:var(--text-dim)">what the colors mean</span>
      <button class="modal__x" title="Close (Esc)">×</button></div>
    <div class="modal__body">
      <div class="modal__h">Left bar on a card = its stage</div>
      <div class="legend">${STAGE_HELP.map(([id, name, desc]) => `<div class="legend__row"><span class="legend__bar" style="background:var(--s-${id})"></span><span class="legend__name">${name}</span><span class="legend__desc">${esc(desc)}</span></div>`).join('')}</div>
      <div class="modal__h">Glowing red = it needs you</div>
      <p class="modal__prose">A card <b style="color:var(--red-bri)">glows red</b> when the next move is yours — a decision to record or a new card to greenlight. The same red drives the “blocked on you” pill and the focus-mode dots. Green means resolved (a decided/recommended option).</p>
      <div class="modal__h">Priority chips</div>
      <div class="legend">
        <div class="legend__row"><span class="card__sq sq-P0">P0</span><span class="legend__desc">Urgent — red (glows)</span></div>
        <div class="legend__row"><span class="card__sq sq-P1">P1</span><span class="legend__desc">High — amber</span></div>
        <div class="legend__row"><span class="card__sq sq-P2">P2</span><span class="legend__desc">Normal — muted</span></div>
        <div class="legend__row"><span class="card__sq sq-P3">P3</span><span class="legend__desc">Low — faint</span></div>
      </div>
      <p class="modal__prose" style="margin-top:8px">Only P0/P1 carry colour so the urgent ones pop; P2/P3 stay quiet on purpose.</p>
      <div class="modal__h">Action tag (bottom-right of a card)</div>
      <p class="modal__prose"><span class="card__lane lane-owner" style="font-family:var(--mono);font-size:11px">● needs you</span> · <span class="card__lane lane-agent" style="font-family:var(--mono);font-size:11px">● an agent's</span> · <span class="card__lane lane-none" style="font-family:var(--mono);font-size:11px">● waiting/blocked</span>. It names the exact next move (e.g. “Greenlight to start”, “Ready to implement”, “2 decisions to make”).</p>
    </div></div>`;
  $('.modal__x', m).addEventListener('click', closeDetail);
  m.onclick = (e) => { if (e.target === m) closeDetail(); };
  m.hidden = false; $('#scrim').hidden = false;
}

// ---- ideas -------------------------------------------------------------
function viewIdeas() {
  const v = $('#view');
  const open = S.binder.filter(b => b.status !== 'tagged');
  v.innerHTML = `<div class="view__head"><h1 class="view__title">Ideas</h1>
      <span class="view__sub"><b>${open.length}</b> to triage · add one as a tracked card</span></div>`;
  const wrap = el('<div class="binder"></div>');
  const add = el(`<div class="binder__add"><input placeholder="Capture an idea — it waits here until you add it as a card…"><button class="btn btn--red">Capture</button></div>`);
  const fire = async () => { const i = $('input', add); const t = i.value.trim(); if (!t) return; i.value = ''; await api('binder/add', { text: t }); };
  $('button', add).addEventListener('click', fire);
  $('input', add).addEventListener('keydown', e => { if (e.key === 'Enter') fire(); });
  wrap.appendChild(add);
  const levels = [...new Set(open.map(b => b.level))].sort((a, b) => (a ?? 9) - (b ?? 9));
  for (const lv of levels) {
    const g = el(`<div class="binlevel"><div class="binlevel__h">${lv == null ? 'Unsorted' : 'Level ' + lv}</div></div>`);
    open.filter(b => b.level === lv).forEach(b => {
      const row = el(`<div class="contact"><span class="contact__dot">◦</span>
          <div class="contact__body"><div class="contact__text">${esc(b.text)}</div>${b.note ? `<div class="contact__note">${esc(b.note)}</div>` : ''}</div>
          <div class="contact__tags">${(b.tags || []).map(t => `<span class="tag">${esc(t)}</span>`).join('')}</div>
          <button class="btn btn--red btn--sm">Add as card</button><button class="btn btn--ghost btn--sm">Dismiss</button></div>`);
      const [addBtn, delBtn] = row.querySelectorAll('button');
      addBtn.addEventListener('click', async () => { const c = await api('binder/promote', { id: b.id }); if (c) showDetail(c.id); });
      delBtn.addEventListener('click', () => api('binder/delete', { id: b.id }));
      g.appendChild(row);
    });
    wrap.appendChild(g);
  }
  if (!open.length) wrap.appendChild(emptyState('▤', 'No ideas yet — capture one above.'));
  v.appendChild(wrap);
}

const emptyState = (g, t) => el(`<div class="empty-view"><div class="empty-view__g">${g}</div><div class="empty-view__t">${esc(t)}</div></div>`);

// ---- card modal --------------------------------------------------------
function showDetail(id) {
  openCard = id;
  const c = cardById(id);
  if (!c) return closeDetail();
  const m = $('#detail');
  const sel = (k, opts, cur) => `<select data-fld="${k}">${opts.map(o => `<option value="${o}" ${o === cur ? 'selected' : ''}>${o}</option>`).join('')}</select>`;
  const cta = c.phase === 'triage'
    ? `<button class="btn btn--red" id="cta-activate">Greenlight — start work</button>`
    : c.phase === 'frozen'
    ? `<button class="btn btn--red" id="cta-activate">Unfreeze — start work</button>`
    : c.phase === 'verify' ? `<button class="btn btn--red" id="cta-done">Mark verified — close</button>` : '';
  m.innerHTML = `<div class="modal__panel">
    <div class="modal__bar"><span class="modal__id">${ticket(c)}</span><span class="card__sq sq-${c.priority}">${c.priority}</span>
      <span class="card__kind" style="color:var(--text-dim)">${esc(c.kind)} · ${phaseLabel(c.phase)} · <span class="card__lane ${c.lane.who === 'owner' ? 'lane-owner' : c.lane.who === 'agent' ? 'lane-agent' : 'lane-none'}" style="border:0;background:none;padding:0">${esc(c.lane.label)}</span></span>
      <button class="modal__x" title="Close (Esc)">×</button></div>
    <div class="modal__body">
      <h2 class="modal__title" contenteditable="plaintext-only" data-fld="title">${esc(c.title)}</h2>
      ${cta ? `<div class="modal__cta">${cta}</div>` : ''}
      <div class="fields">
        <div class="fld"><div class="fld__k">Stage</div><select data-fld="phase">${S.phases.map(p => `<option value="${p.id}" ${p.id === c.phase ? 'selected' : ''}>${p.label}</option>`).join('')}</select></div>
        <div class="fld"><div class="fld__k">Track</div>${sel('track', ['epoch', 'sidequest'], c.track)}</div>
        <div class="fld"><div class="fld__k">Epoch</div><select data-fld="epoch"><option value="" ${!c.epoch ? 'selected' : ''}>—</option>${S.epochs.map(e => `<option value="${e.id}" ${e.id === c.epoch ? 'selected' : ''}>E${e.num} ${esc(e.name)}</option>`).join('')}</select></div>
        <div class="fld"><div class="fld__k">Priority</div>${sel('priority', ['P0', 'P1', 'P2', 'P3'], c.priority)}</div>
        <div class="fld"><div class="fld__k">Kind</div>${sel('kind', ['task', 'feature', 'idea', 'bug'], c.kind)}</div>
        <div class="fld"><div class="fld__k">Plan</div><input class="fld__v" data-fld="plan" value="${esc(c.plan || '')}" placeholder="—"></div>
      </div>
      <div class="modal__h">Description</div>
      <div class="modal__prose" contenteditable="plaintext-only" data-fld="body">${md(c.body)}</div>
      <div class="modal__h">Decisions</div><div id="modal-decisions"></div>
      <div class="modal__h">Notes & questions for agents</div><div id="modal-q"></div>
      <div class="modal__h">Log</div>
      <ul class="log">${c.log.map(l => `<li><time>${esc(l.at)}</time><span>${esc(l.text)}</span></li>`).join('') || '<li><span>No entries.</span></li>'}</ul>
      <div class="modal__danger"><button class="btn btn--danger btn--sm" id="del-card">Delete card</button></div>
    </div></div>`;

  // decisions inline
  const dd = $('#modal-decisions', m);
  if (!c.decisions.length) dd.appendChild(el(`<p class="modal__prose">No decision needed for this card.</p>`));
  c.decisions.forEach(de => {
    const box = el(`<div class="modal__decision">
        <div class="modal__dhead"><span class="modal__did">${esc(de.id)}</span>
          <span class="card__lane ${de.status === 'ratified' ? 'lane-agent' : 'lane-owner'}">${de.status === 'ratified' ? '✓ ' + esc(de.outcome) : 'to decide'}</span></div>
        <div class="modal__prose" style="font-size:13px">${esc(de.title)}</div><div class="modal__opts"></div></div>`);
    const row = $('.modal__opts', box);
    (de.options || []).forEach(o => {
      const b = el(`<button class="modal__opt ${de.outcome === o.key ? 'win' : ''}">${esc(o.key)} · ${esc(o.name)}</button>`);
      b.addEventListener('click', () => api('clearance', { decisionId: de.id, outcome: o.key }));
      row.appendChild(b);
    });
    dd.appendChild(box);
  });

  // notes & questions
  const qb = $('#modal-q', m);
  c.questions.forEach(q => {
    const row = el(`<div class="qrow"><div class="qrow__top"><span class="qrow__by ${q.by === 'agent' ? 'agent' : ''}">${esc(q.by)}</span>
        <span class="qrow__kind">${esc(q.kind)}</span><span class="qrow__status ${q.status}">${esc(q.status)}</span></div>
        <div class="qrow__text">${esc(q.text)}</div>
        ${q.answer ? `<div class="qrow__ans"><b>agent</b> ${esc(q.answer)}</div>` : ''}</div>`);
    qb.appendChild(row);
  });
  const qadd = el(`<div class="qadd"><input placeholder="Leave a note or question for an agent…"><button class="btn btn--red btn--sm">Post</button></div>`);
  const post = async () => { const i = $('input', qadd); const t = i.value.trim(); if (!t) return; i.value = ''; await api('question/add', { cardId: id, text: t, kind: 'question' }); };
  $('button', qadd).addEventListener('click', post);
  $('input', qadd).addEventListener('keydown', e => { if (e.key === 'Enter') post(); });
  qb.appendChild(qadd);

  m.querySelectorAll('[data-fld]').forEach(node => {
    const k = node.dataset.fld;
    if (node.tagName === 'SELECT' || node.tagName === 'INPUT') node.addEventListener('change', () => commit(id, k, node.value));
    else node.addEventListener('blur', () => commit(id, k, node.innerText.trim()));
  });
  $('.modal__x', m).addEventListener('click', closeDetail);
  $('#del-card', m).addEventListener('click', async () => { await api('card/delete', { id }); closeDetail(); });
  $('#cta-activate', m)?.addEventListener('click', () => api('card/activate', { id }));
  $('#cta-done', m)?.addEventListener('click', () => api('card/update', { id, phase: 'done', logEntry: 'Verified — closed.' }));
  m.onclick = (e) => { if (e.target === m) closeDetail(); };
  m.hidden = false; $('#scrim').hidden = false;
}
const commit = (id, k, v) => api('card/update', { id, [k]: k === 'plan' && v === '' ? null : v });
function closeDetail() { openCard = null; $('#detail').hidden = true; $('#scrim').hidden = true; }
async function newCard() { const c = await api('card/add', { title: 'New card', phase: 'triage', track: 'epoch', epoch: S.meta.currentEpoch }); if (c) showDetail(c.id); }

// ---- sorting -----------------------------------------------------------
const PR = { P0: 0, P1: 1, P2: 2, P3: 3 };
const byPrio = (a, b) => PR[a.priority] - PR[b.priority] || (a.num ?? 0) - (b.num ?? 0);
const PHO = { deciding: 0, planning: 1, ready: 2, building: 3, verify: 4, triage: 5, done: 6, frozen: 7 };
const byPhaseThenPrio = (a, b) => (PHO[a.phase] - PHO[b.phase]) || byPrio(a, b);

// ---- chrome ------------------------------------------------------------
const VIEWS = [
  { id: 'decisions', ico: '◆', name: 'Decisions', count: () => S.counts.decide, alert: true },
  { id: 'dispatch', ico: '▸', name: 'Agent', count: () => S.counts.agentReady },
  { id: 'board', ico: '▦', name: 'Board', count: () => S.cards.filter(c => c.phase !== 'done' && c.phase !== 'frozen').length },
  { id: 'ideas', ico: '▤', name: 'Ideas', count: () => S.counts.binder },
];
const RENDER = { decisions: viewDecisions, dispatch: viewDispatch, board: viewBoard, ideas: viewIdeas };

function renderChrome() {
  const rail = $('#scope');
  rail.querySelectorAll('.nav').forEach(n => n.remove());
  const foot = $('#foot');
  VIEWS.forEach(vw => {
    const n = vw.count();
    const b = el(`<button class="nav" aria-current="${VIEW === vw.id}" data-view="${vw.id}">
        <span class="nav__ico">${vw.ico}</span><span class="nav__n">${vw.name}</span>
        <span class="nav__count ${vw.alert && n ? 'alert' : ''}">${n}</span></button>`);
    b.addEventListener('click', () => { location.hash = vw.id; });
    rail.insertBefore(b, foot);
  });
  $('#feed').innerHTML = `<b>Epoch ${epochOf(S.meta.currentEpoch)?.num ?? '—'}</b> · <b>${S.cards.length}</b> cards · <b>${S.counts.agentReady}</b> agent-ready`;
  const fy = S.counts.forYou;
  const pill = $('#pill');
  pill.className = 'topbar__pill' + (fy ? '' : ' clear');
  pill.innerHTML = fy ? `<span class="beat"></span> ${S.counts.decide} to decide · ${S.counts.activate} to activate` : '✓ nothing blocked on you';
  pill.onclick = () => { location.hash = 'decisions'; };
}

function render() {
  if (!S) return;
  renderChrome();
  (RENDER[VIEW] || viewDecisions)();
  if (focusIds) renderFocus();
  if (openCard) {
    const editing = $('#detail').contains(document.activeElement);
    if (cardById(openCard)) { if (!editing) showDetail(openCard); } else closeDetail();
  }
}

document.addEventListener('keydown', (e) => {
  if (focusIds) {
    if (e.key === 'Escape') { e.preventDefault(); return exitFocus(); }
    if (/INPUT|TEXTAREA/.test(document.activeElement?.tagName)) return;  // typing a comment/question
    const d = S.decisions.find(x => x.id === focusIds[focusIdx]);
    if (e.key === 'ArrowLeft') return focusGo(-1);
    if (e.key === 'ArrowRight') return focusGo(1);
    if (e.key === 'Enter') { if (focusIds.some(id => pick[id])) recordFocusBatch(); else focusGo(1); return; }
    const n = parseInt(e.key, 10);
    if (n >= 1 && n <= 9 && d && d.status !== 'ratified' && d.options && d.options[n - 1]) { pick[d.id] = d.options[n - 1].key; updateFocusChoice(); }
    return;
  }
  if (e.key === 'Escape') return closeDetail();
  if (openCard || /input|textarea|select/i.test(document.activeElement?.tagName) || document.activeElement?.isContentEditable) return;
  const i = ['1', '2', '3', '4', '5', '6'].indexOf(e.key);
  if (i >= 0 && VIEWS[i]) location.hash = VIEWS[i].id;
});
$('#scrim').addEventListener('click', closeDetail);

const syncHash = () => { const h = location.hash.slice(1); if (RENDER[h]) VIEW = h; if (S) render(); };
window.addEventListener('hashchange', syncHash);
if (RENDER[location.hash.slice(1)]) VIEW = location.hash.slice(1);
load();
