import { createRequestGate, selectedSummary } from './monitor-state.mjs';

const $ = selector => document.querySelector(selector);
const STATUS_ORDER = ['running', 'ready', 'pending', 'passed', 'blocked', 'failed'];
const STATUS_COLOR = { passed: '#43c78c', running: '#45b8ca', ready: '#43c78c', pending: '#9d9ca8', blocked: '#ff2e4d', failed: '#ff2e4d', invalid: '#ff2e4d' };
const state = { data: null, run: null, node: null, filter: null, tab: 'brief', query: '', fingerprint: '', timer: null, requests: createRequestGate() };

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined && text !== null) node.textContent = String(text);
  return node;
}

function svg(tag, attrs = {}) {
  const node = document.createElementNS('http://www.w3.org/2000/svg', tag);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
  return node;
}

function formatTime(value, withDate = false) {
  if (!value) return '—';
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) return value;
  return new Intl.DateTimeFormat(undefined, withDate
    ? { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit' }
    : { hour: '2-digit', minute: '2-digit', second: '2-digit' }).format(date);
}

function age(value) {
  if (!value) return 'never';
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) return 'unknown';
  const seconds = Math.max(0, Math.floor((Date.now() - date) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  return `${Math.floor(seconds / 3600)}h ago`;
}

function setLiveStatus(status, label, title) {
  const live = $('#live-state');
  const key = `${status}:${label}`;
  if (live.dataset.state === key) return;
  live.dataset.state = key;
  live.className = `live-state ${status}`;
  live.lastChild.textContent = ` ${label}`;
  live.title = title;
}

function renderFreshness() {
  const run = state.data?.selected;
  if (!run) {
    setLiveStatus('stale', 'No ledger', 'The server is reachable, but no readable Codeflow ledger was found.');
    $('#freshness').textContent = 'no readable run';
  } else if (run.ledger_stale) {
    setLiveStatus('stale', 'Ledger stale', 'The server is reachable, but Codeflow has not updated its ledger for over 30 minutes.');
    $('#freshness').textContent = `event ${age(run.updated_at)}`;
  } else {
    setLiveStatus('online', 'Live · 2s', 'Server and Codeflow ledger are updating.');
    $('#freshness').textContent = run ? `event ${age(run.updated_at)}` : age(state.data?.generated_at);
  }
}

function workflowNode(id) {
  return state.data?.selected?.workflow?.nodes?.find(node => node.id === id);
}

function syncUrl() {
  const url = new URL(location.href);
  state.run ? url.searchParams.set('run', state.run) : url.searchParams.delete('run');
  state.node ? url.searchParams.set('node', state.node) : url.searchParams.delete('node');
  history.replaceState(null, '', url);
}

function chooseRun(id) {
  if (id === state.run) return;
  state.run = id;
  state.node = null;
  state.fingerprint = '';
  syncUrl();
  load(false);
}

function chooseNode(id) {
  state.node = id;
  syncUrl();
  renderGraph();
  renderInspector();
  if (matchMedia('(max-width: 1180px)').matches) {
    document.querySelector('.inspector').scrollIntoView({ behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth', block: 'start' });
  }
}

function renderRuns() {
  const list = $('#run-list');
  list.replaceChildren();
  $('#run-count').textContent = state.data.runs.length;
  $('#workspace').textContent = state.data.workspace;
  for (const run of state.data.runs) {
    const button = el('button', `run-item${run.run_id === state.run ? ' active' : ''}`);
    button.type = 'button';
    button.dataset.status = run.status;
    button.addEventListener('click', () => chooseRun(run.run_id));
    const main = el('span', 'run-item-main');
    main.append(el('strong', '', run.run_id));
    const meta = el('span', 'run-item-meta');
    meta.append(el('span', '', run.status), el('span', '', `${Math.round((run.progress || 0) * 100)}%`));
    const progress = el('span', 'mini-progress');
    const fill = el('i');
    fill.style.width = `${Math.round((run.progress || 0) * 100)}%`;
    progress.append(fill);
    main.append(meta, progress);
    button.append(main);
    list.append(button);
  }
}

function renderOverview() {
  const run = state.data.selected;
  $('#run-name').textContent = run?.run_id || 'No run selected';
  $('#goal').textContent = run?.workflow?.goal || 'No Codeflow runs found';
  const total = Object.values(run?.counts || {}).reduce((sum, count) => sum + count, 0);
  const passed = run?.counts?.passed || 0;
  $('#progress-value').textContent = `${Math.round((run?.progress || 0) * 100)}%`;
  $('#progress-label').textContent = total ? `${passed} of ${total} nodes passed` : 'No nodes loaded';
  const strip = $('#status-strip');
  strip.replaceChildren();
  for (const status of STATUS_ORDER.filter(name => run?.counts?.[name])) {
    const button = el('button', `status-filter${state.filter === status ? ' active' : ''}`);
    button.type = 'button';
    button.dataset.status = status;
    button.style.setProperty('--status-color', STATUS_COLOR[status]);
    button.append(el('span', '', status), el('strong', '', run.counts[status]));
    button.addEventListener('click', () => { state.filter = state.filter === status ? null : status; renderOverview(); renderGraph(); });
    strip.append(button);
  }
  const meta = $('#run-meta');
  meta.replaceChildren();
  if (run) {
    const values = [['status', run.status], ['cycles', `${run.cycles || 0}/${run.workflow.limits.max_cycles}`], ['parallel', run.workflow.limits.max_parallel], ['updated', age(run.updated_at)]];
    for (const [label, value] of values) { const row = el('span', '', label); row.append(el('b', '', value)); meta.append(row); }
  }
}

function nodeDepths(nodes) {
  const byId = new Map(nodes.map(node => [node.id, node]));
  const memo = new Map();
  const depth = (id, visiting = new Set()) => {
    if (memo.has(id)) return memo.get(id);
    if (visiting.has(id)) return 0;
    visiting.add(id);
    const deps = byId.get(id)?.depends_on || [];
    const value = deps.length ? Math.max(...deps.map(dep => depth(dep, visiting))) + 1 : 0;
    memo.set(id, value);
    return value;
  };
  for (const node of nodes) depth(node.id);
  return memo;
}

function renderGraph() {
  const run = state.data?.selected;
  const graph = $('#graph');
  graph.replaceChildren();
  $('#edges').replaceChildren();
  const nodes = run?.workflow?.nodes || [];
  $('#graph-empty').hidden = Boolean(nodes.length);
  if (!nodes.length) return;

  const depths = nodeDepths(nodes);
  const max = Math.max(...depths.values());
  for (let column = 0; column <= max; column += 1) {
    const group = el('div', 'graph-column');
    group.append(el('span', 'graph-column-label', column === 0 ? 'Entry' : `Stage ${column + 1}`));
    for (const node of nodes.filter(item => depths.get(item.id) === column)) {
      const runtime = run.nodes[node.id] || { status: 'pending' };
      const button = el('button', `node ${runtime.status}${state.node === node.id ? ' selected' : ''}`);
      button.type = 'button';
      button.dataset.node = node.id;
      button.dataset.status = runtime.status;
      const searchable = `${node.id} ${node.objective} ${node.kind} ${runtime.worker || ''}`.toLowerCase();
      const dim = (state.filter && runtime.status !== state.filter) || (state.query && !searchable.includes(state.query));
      if (dim) button.classList.add('dimmed');
      const top = el('span', 'node-top');
      top.append(el('span', '', runtime.status), el('i', '', node.kind));
      button.append(top, el('strong', '', node.id), el('small', '', node.objective));
      button.addEventListener('click', () => chooseNode(node.id));
      group.append(button);
    }
    graph.append(group);
  }
  requestAnimationFrame(drawEdges);
}

function drawEdges() {
  const run = state.data?.selected;
  const viewport = $('#graph-viewport');
  const edgeSvg = $('#edges');
  const graph = $('#graph');
  if (!run || !graph.children.length) return;
  const width = graph.scrollWidth;
  const height = graph.scrollHeight;
  edgeSvg.setAttribute('width', width);
  edgeSvg.setAttribute('height', height);
  edgeSvg.setAttribute('viewBox', `0 0 ${width} ${height}`);
  edgeSvg.replaceChildren();
  const rootRect = viewport.getBoundingClientRect();
  for (const node of run.workflow.nodes) {
    const target = graph.querySelector(`[data-node="${CSS.escape(node.id)}"]`);
    if (!target) continue;
    const targetRect = target.getBoundingClientRect();
    for (const dependency of node.depends_on) {
      const source = graph.querySelector(`[data-node="${CSS.escape(dependency)}"]`);
      if (!source) continue;
      const sourceRect = source.getBoundingClientRect();
      const x1 = sourceRect.right - rootRect.left + viewport.scrollLeft;
      const y1 = sourceRect.top + sourceRect.height / 2 - rootRect.top + viewport.scrollTop;
      const x2 = targetRect.left - rootRect.left + viewport.scrollLeft;
      const y2 = targetRect.top + targetRect.height / 2 - rootRect.top + viewport.scrollTop;
      const bend = Math.max(30, (x2 - x1) * .48);
      const targetStatus = run.nodes[node.id]?.status || 'pending';
      const sourceStatus = run.nodes[dependency]?.status || 'pending';
      const className = targetStatus === 'running' ? 'edge running' : sourceStatus === 'passed' ? 'edge passed' : 'edge';
      edgeSvg.append(svg('path', { class: className, d: `M${x1},${y1} C${x1 + bend},${y1} ${x2 - bend},${y2} ${x2},${y2}` }));
    }
  }
}

function eventColor(type) {
  if (/failed|blocked|invalidated|interrupted/.test(type)) return '#ff2e4d';
  if (/passed|complete/.test(type)) return '#43c78c';
  if (/started/.test(type)) return '#45b8ca';
  if (/ready|synced/.test(type)) return '#dfa14f';
  return '#9d9ca8';
}

function renderEvents() {
  const events = [...(state.data?.selected?.events || [])].reverse().filter(event => {
    if (!state.query) return true;
    return `${event.event} ${event.node || ''} ${event.detail || ''}`.toLowerCase().includes(state.query);
  });
  $('#event-count').textContent = events.length;
  const list = $('#events');
  list.replaceChildren();
  for (const event of events.slice(0, 120)) {
    const row = el('div', 'event');
    row.style.setProperty('--event-color', eventColor(event.event));
    const time = el('time', '', formatTime(event.at));
    time.dateTime = event.at;
    row.append(time, el('span', 'event-type', event.event), el('span', 'event-node', event.node || 'run'), el('span', 'event-detail', event.detail || '—'));
    list.append(row);
  }
}

function detailGroup(title, content, color) {
  const group = el('section', 'detail-group');
  if (color) group.style.setProperty('--group-color', color);
  group.append(el('h3', '', title), content);
  return group;
}

function list(values, empty = 'None recorded') {
  const ul = el('ul');
  const items = values?.length ? values : [empty];
  for (const value of items) ul.append(el('li', '', typeof value === 'string' ? value : JSON.stringify(value)));
  return ul;
}

function keyValues(entries) {
  const dl = el('dl', 'kv');
  for (const [key, value] of entries) { dl.append(el('dt', '', key), el('dd', '', value ?? '—')); }
  return dl;
}

function renderBrief(body, node, runtime, report) {
  body.append(
    detailGroup('Objective', el('p', '', node.objective)),
    detailGroup('Runtime', keyValues([
      ['mode', node.mode], ['kind', node.kind], ['worker', runtime.worker], ['attempts', runtime.attempts ?? 0],
      ['started', formatTime(runtime.started_at, true)], ['finished', formatTime(runtime.finished_at, true)], ['sequence', runtime.sequence ?? 0],
    ])),
    detailGroup('Depends on', list(node.depends_on, 'Entry node')),
    detailGroup('Next action', list(report?.next, 'No next action recorded')),
  );
}

function renderProof(body, node, runtime, report) {
  body.append(detailGroup('Acceptance', list(node.acceptance)), detailGroup('Covered criteria', list(runtime.acceptance)));
  if (report?.summary) body.append(detailGroup('Report summary', el('p', '', report.summary)));
  body.append(detailGroup('Evidence', list(report?.evidence), '#43c78c'));
  const checks = el('div');
  for (const check of report?.checks || []) {
    const item = el('div', 'check');
    item.style.setProperty('--group-color', check.status === 'passed' ? '#43c78c' : '#ff2e4d');
    item.append(el('strong', '', `${check.status || 'unknown'} · ${check.command || 'check'}`), el('span', '', check.detail || ''));
    checks.append(item);
  }
  body.append(detailGroup('Checks', checks.childNodes.length ? checks : el('p', '', 'No checks recorded')),
    detailGroup('Findings', list(report?.findings)), detailGroup('Risks', list(report?.risks)));
}

function renderExposure(body, node, runtime, report) {
  body.append(
    detailGroup('Readable paths', list(node.paths)),
    detailGroup('Write scope', list(node.write_scope, node.mode === 'write' ? 'No scope recorded' : 'Read-only node')),
    detailGroup('Forbidden actions', list(node.forbidden)),
    detailGroup('Changed paths', list(report?.changed_paths)),
    detailGroup('Artifacts', list(report?.artifacts)),
    detailGroup('Digests', keyValues(Object.entries(runtime.artifact_digests || {}))),
    detailGroup('Ledger refs', keyValues([['report', runtime.report], ['report SHA', runtime.report_sha256], ['snapshot', runtime.snapshot]])),
  );
}

function renderInspector() {
  const run = state.data?.selected;
  const body = $('#inspector-body');
  body.replaceChildren();
  const node = workflowNode(state.node);
  const runtime = run?.nodes?.[state.node];
  if (!node || !runtime) {
    $('#node-kind').textContent = 'Node inspector';
    $('#node-name').textContent = 'Select a node';
    $('#node-status').textContent = '—';
    $('#node-status').dataset.status = 'pending';
    const empty = el('div', 'empty-inspector');
    empty.append(el('strong', '', 'Follow the signal'), el('span', '', 'Choose a workflow node to inspect its brief, proof, and exposed scope.'));
    body.append(empty);
    return;
  }
  const report = run.reports?.[node.id];
  $('#node-kind').textContent = `${node.kind} · ${node.mode}`;
  $('#node-name').textContent = node.id;
  $('#node-status').textContent = runtime.status;
  $('#node-status').dataset.status = runtime.status;
  if (report?.error) body.append(detailGroup('Report error', el('p', '', report.error), '#ff2e4d'));
  if (state.tab === 'brief') renderBrief(body, node, runtime, report);
  else if (state.tab === 'proof') renderProof(body, node, runtime, report);
  else renderExposure(body, node, runtime, report);
}

function render() {
  const run = state.data.selected;
  state.run = run?.run_id || state.run || state.data.runs[0]?.run_id || null;
  const ids = run?.workflow?.nodes?.map(node => node.id) || [];
  const requestedNode = new URL(location.href).searchParams.get('node');
  if (!state.node || !ids.includes(state.node)) {
    state.node = ids.includes(requestedNode) ? requestedNode : ids.find(id => run.nodes[id]?.status === 'running') || ids.find(id => run.nodes[id]?.status === 'ready') || ids[0] || null;
  }
  syncUrl();
  renderRuns();
  renderOverview();
  renderGraph();
  renderEvents();
  renderInspector();
}

async function load(quiet = true) {
  const request = state.requests.begin();
  if (!quiet) setLiveStatus('', 'Loading', 'Refreshing Codeflow state.');
  try {
    const url = new URL('/api/state', location.origin);
    if (state.run) url.searchParams.set('run', state.run);
    let response = await fetch(url, { cache: 'no-store' });
    if (!state.requests.isCurrent(request)) return;
    if (response.status === 404 && state.run) {
      state.run = null;
      state.node = null;
      state.fingerprint = '';
      syncUrl();
      url.searchParams.delete('run');
      response = await fetch(url, { cache: 'no-store' });
      if (!state.requests.isCurrent(request)) return;
    }
    if (!response.ok) throw new Error((await response.json()).error || `HTTP ${response.status}`);
    const data = await response.json();
    if (!state.requests.isCurrent(request)) return;
    const fingerprint = JSON.stringify({ selected: data.selected, runs: data.runs });
    state.data = data;
    renderFreshness();
    $('#fatal').hidden = true;
    if (fingerprint !== state.fingerprint) { state.fingerprint = fingerprint; render(); }
  } catch (error) {
    if (!state.requests.isCurrent(request)) return;
    setLiveStatus('offline', 'Offline', 'The monitor server could not return state.');
    const fatal = $('#fatal');
    fatal.textContent = `${error.message} Check server and workspace, then refresh.`;
    fatal.hidden = false;
  } finally {
    if (!state.requests.isCurrent(request)) return;
    clearTimeout(state.timer);
    state.timer = setTimeout(() => load(true), state.data?.poll_interval_ms || 2000);
  }
}

$('#refresh').addEventListener('click', () => {
  if (selectedSummary(state.data, state.run)?.ledger_stale) {
    state.run = null;
    state.node = null;
    state.fingerprint = '';
    syncUrl();
  }
  load(false);
});
$('#show-all').addEventListener('click', () => { state.filter = null; state.query = ''; $('#search').value = ''; renderOverview(); renderGraph(); renderEvents(); });
$('#search').addEventListener('input', event => { state.query = event.target.value.trim().toLowerCase(); renderGraph(); renderEvents(); });
const tabs = [...document.querySelectorAll('[role="tab"]')];
function activateTab(tab, focus = false) {
  state.tab = tab.dataset.tab;
  for (const item of tabs) {
    const active = item === tab;
    item.setAttribute('aria-selected', String(active));
    item.tabIndex = active ? 0 : -1;
  }
  if (focus) tab.focus();
  renderInspector();
}
for (const tab of tabs) {
  tab.addEventListener('click', () => {
    activateTab(tab);
  });
  tab.addEventListener('keydown', event => {
    const index = tabs.indexOf(tab);
    const next = event.key === 'ArrowRight' ? tabs[(index + 1) % tabs.length]
      : event.key === 'ArrowLeft' ? tabs[(index - 1 + tabs.length) % tabs.length]
        : event.key === 'Home' ? tabs[0] : event.key === 'End' ? tabs.at(-1) : null;
    if (next) { event.preventDefault(); activateTab(next, true); }
  });
}
activateTab(tabs[0]);
addEventListener('resize', () => requestAnimationFrame(drawEdges));
setInterval(() => { if (state.data) renderFreshness(); }, 1000);

const params = new URL(location.href).searchParams;
state.run = params.get('run');
state.node = params.get('node');
load(false);
