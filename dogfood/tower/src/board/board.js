(() => {
  'use strict';

  const VIEWS = ['board', 'focus', 'papercuts', 'status'];

  // These ranks mirror the projected lane order. Lane values come only from
  // card.lane.lane; phase is displayed separately and never substitutes for it.
  const WORKFLOW_RANK = {
    verify: 0,
    building: 0,
    implement: 1,
    plan: 2,
    blocked: 3,
    decide: 3,
    done: 4,
  };
  const LANE_RANK = {
    verify: 0,
    building: 1,
    implement: 2,
    plan: 3,
  };
  const STATUS_LANES = ['decide', 'plan', 'implement', 'building', 'verify', 'blocked', 'frozen'];
  const STATUS_LANE_LABELS = {
    decide: 'Decide',
    verify: 'Verify',
    building: 'Building',
    implement: 'Implement',
    plan: 'Plan',
    blocked: 'Blocked',
    frozen: 'Frozen',
    other: 'Other lanes',
  };
  const DEFAULT_PRIORITIES = ['P0', 'P1', 'P2', 'P3'];
  const CARD_SORT_LABELS = {
    projection: 'Lane projection',
    priority: 'Priority',
    updated: 'Recently updated',
    title: 'Title',
    number: 'Card number',
  };
  const PHASE_LABELS = {
    deciding: 'Deciding',
    planning: 'Planning',
    triage: 'Triage',
    ready: 'Ready',
    building: 'Building',
    verify: 'Verify',
    done: 'Done',
    frozen: 'Frozen',
  };

  const app = {
    view: readView(),
    state: null,
    stateLoading: true,
    stateError: null,
    connection: 'Loading snapshot…',
    showClosed: false,
    board: {
      text: '',
      lane: 'all',
      priority: 'all',
      track: 'all',
      sort: 'projection',
      direction: 'asc',
    },
    closed: null,
    closedLoading: false,
    closedError: null,
    historyCounts: new Map(),
    historyLoading: new Set(),
    historyErrors: new Map(),
    focus: {
      ref: null,
      decisionId: null,
      card: null,
      loading: false,
      error: null,
      token: 0,
      returnFocus: null,
      returnFocusRef: null,
      returnView: null,
    },
    dialogOpen: false,
    papercutFilter: 'open',
    status: {
      requested: false,
      loading: false,
      lint: null,
      lintError: null,
      version: null,
      versionError: null,
    },
    stream: null,
    streamStarted: false,
    fallbackTimer: null,
  };

  const byId = (id) => document.getElementById(id);
  const make = (tag, className = '', content = '') => {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (content !== '' && content !== null && content !== undefined) node.textContent = String(content);
    return node;
  };
  const append = (parent, ...children) => {
    children.filter(Boolean).forEach((child) => parent.append(child));
    return parent;
  };
  const clear = (node) => {
    if (node) node.replaceChildren();
    return node;
  };
  const setText = (node, value) => {
    if (node) node.textContent = String(value ?? '');
    return node;
  };
  const setAttrs = (node, attrs = {}) => {
    Object.entries(attrs).forEach(([name, value]) => {
      if (value !== null && value !== undefined) node.setAttribute(name, String(value));
    });
    return node;
  };
  const hasOwn = (value, key) => Boolean(value && typeof value === 'object'
    && Object.prototype.hasOwnProperty.call(value, key));
  const isRecord = (value) => Boolean(value && typeof value === 'object' && !Array.isArray(value));
  const makeButton = (label, className, handler, attrs = {}) => {
    const button = make('button', className, label);
    button.type = 'button';
    setAttrs(button, attrs);
    if (handler) button.addEventListener('click', handler);
    return button;
  };
  const makeFocusButton = (label, className, ref, handler, attrs = {}) => makeButton(
    label,
    className,
    handler,
    { ...attrs, 'data-focus-ref': String(ref) },
  );

  function readView() {
    let value = location.hash.slice(1).split('/')[0] || 'board';
    try { value = decodeURIComponent(value); } catch { value = 'board'; }
    return VIEWS.includes(value) ? value : 'board';
  }

  function currentRevision(state = app.state) {
    return state?.meta?.rev ?? state?.rev ?? null;
  }

  function stringValue(value, fallback = '—') {
    if (value === null || value === undefined || value === '') return fallback;
    if (typeof value === 'object') {
      try { return JSON.stringify(value); } catch { return fallback; }
    }
    return String(value);
  }

  function fieldLabel(key) {
    return String(key)
      .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
      .replace(/[_-]+/g, ' ')
      .replace(/^./, (letter) => letter.toUpperCase());
  }

  function collectionItems(value) {
    if (Array.isArray(value)) return value;
    if (isRecord(value) && Array.isArray(value.items)) return value.items;
    return null;
  }

  function scalarText(value, fallback = '—') {
    if (value === null || value === undefined || value === '') return fallback;
    if (typeof value === 'object') return fallback;
    return String(value);
  }

  function listValue(value) {
    if (Array.isArray(value)) return value;
    if (value && Array.isArray(value.items)) return value.items;
    return [];
  }

  function phaseLabel(phase) {
    if (typeof phase === 'object' && phase) return stringValue(phase.label || phase.id);
    return PHASE_LABELS[phase] || stringValue(phase, 'Unknown phase');
  }

  function laneToken(card) {
    return String(card?.lane?.lane || 'other');
  }

  function laneLabel(card) {
    return card?.lane?.label || laneToken(card);
  }

  function cardTitle(card) {
    return stringValue(card?.title, 'Untitled card');
  }

  function cardRef(card) {
    if (!card) return null;
    if (card.id !== null && card.id !== undefined && card.id !== '') return String(card.id);
    if (card.num !== null && card.num !== undefined && card.num !== '') return `#${card.num}`;
    return null;
  }

  function cardNumber(card) {
    if (card?.num !== null && card?.num !== undefined && card.num !== '') return `#${card.num}`;
    return cardRef(card) || 'card';
  }

  function formatDate(value, options = {}) {
    if (!value) return '—';
    const date = new Date(String(value).length === 10 ? `${value}T00:00:00` : value);
    if (Number.isNaN(date.getTime())) return String(value);
    try {
      return new Intl.DateTimeFormat(undefined, options).format(date);
    } catch {
      return String(value);
    }
  }

  function formatDateTime(value) {
    return formatDate(value, { dateStyle: 'medium', timeStyle: 'short' });
  }

  function priorityOrder() {
    const configured = app.state?.config?.priorities || app.state?.config?.priorityOrder;
    return Array.isArray(configured) && configured.length ? configured.map(String) : DEFAULT_PRIORITIES;
  }

  function priorityRank(card) {
    const rank = priorityOrder().indexOf(String(card?.priority ?? ''));
    return rank < 0 ? priorityOrder().length : rank;
  }

  function workOrderValue(card) {
    if (card?.workOrder === null || card?.workOrder === undefined || card.workOrder === '') return Infinity;
    const number = Number(card.workOrder);
    return Number.isFinite(number) ? number : Infinity;
  }

  function numberValue(value, fallback = Infinity) {
    const number = Number(value);
    return Number.isFinite(number) ? number : fallback;
  }

  function workflowRank(card) {
    return WORKFLOW_RANK[laneToken(card)] ?? 5;
  }

  function compareCards(a, b) {
    const workflow = workflowRank(a) - workflowRank(b);
    if (workflow) return workflow;

    const lane = (LANE_RANK[laneToken(a)] ?? 4) - (LANE_RANK[laneToken(b)] ?? 4);
    if (lane) return lane;

    const order = workOrderValue(a) - workOrderValue(b);
    if (order) return order;

    const priority = priorityRank(a) - priorityRank(b);
    if (priority) return priority;

    return numberValue(a?.num, 0) - numberValue(b?.num, 0);
  }

  function compareText(left, right) {
    return String(left ?? '').localeCompare(String(right ?? ''), undefined, { sensitivity: 'base' });
  }

  function cardSortValue(card, sort) {
    if (sort === 'priority') return priorityRank(card);
    if (sort === 'updated') return String(card?.updated || card?.created || '');
    if (sort === 'title') return cardTitle(card).toLowerCase();
    if (sort === 'number') return numberValue(card?.num, Infinity);
    return null;
  }

  function sortBoardCards(cards) {
    const sort = app.board.sort;
    const direction = app.board.direction === 'desc' ? -1 : 1;
    return [...cards].sort((left, right) => {
      if (sort === 'projection') return compareCards(left, right) * direction;
      const leftValue = cardSortValue(left, sort);
      const rightValue = cardSortValue(right, sort);
      const comparison = typeof leftValue === 'string'
        ? compareText(leftValue, rightValue)
        : leftValue - rightValue;
      return (comparison * direction) || compareCards(left, right);
    });
  }

  function boardSearchText(card) {
    const fields = [
      card?.id,
      card?.num,
      card?.title,
      card?.body,
      card?.summary,
      card?.plan,
      card?.phase,
      card?.track,
      card?.priority,
      laneLabel(card),
      ...listValue(card?.tags),
      ...listValue(card?.refs),
    ];
    return fields.map((value) => scalarText(value, '')).join(' ').toLowerCase();
  }

  function boardCardMatches(card) {
    const needle = app.board.text.trim().toLowerCase();
    if (needle && !boardSearchText(card).includes(needle)) return false;
    if (app.board.lane !== 'all' && laneToken(card) !== app.board.lane) return false;
    if (app.board.priority !== 'all' && String(card?.priority ?? '') !== app.board.priority) return false;
    if (app.board.track !== 'all' && String(card?.track ?? '') !== app.board.track) return false;
    return true;
  }

  function boardFilterActive() {
    return Boolean(app.board.text.trim())
      || app.board.lane !== 'all'
      || app.board.priority !== 'all'
      || app.board.track !== 'all';
  }

  function boardLaneOptions(cards) {
    const lanes = new Set(STATUS_LANES);
    cards.forEach((card) => lanes.add(laneToken(card)));
    return [...lanes].map((lane) => [lane, STATUS_LANE_LABELS[lane] || laneLabel({ lane: { lane } }) || lane]);
  }

  function boardTrackOptions(cards) {
    const tracks = new Set(cards.map((card) => String(card?.track || '')).filter(Boolean));
    return [...tracks].sort(compareText).map((track) => [track, fieldLabel(track)]);
  }

  function boardPriorityOptions(cards) {
    const priorities = new Set(priorityOrder().map(String));
    cards.forEach((card) => {
      if (card?.priority !== null && card?.priority !== undefined && card.priority !== '') priorities.add(String(card.priority));
    });
    return [...priorities].sort((left, right) => {
      const ranks = priorityOrder();
      return (ranks.indexOf(left) < 0 ? ranks.length : ranks.indexOf(left))
        - (ranks.indexOf(right) < 0 ? ranks.length : ranks.indexOf(right));
    }).map((priority) => [priority, priority]);
  }

  function syncBoardSelect(id, options, selected, allLabel) {
    const select = byId(id);
    if (!select) return selected;
    const signature = `${allLabel}|${options.map(([value, label]) => `${value}:${label}`).join('|')}`;
    if (select.dataset.options !== signature) {
      clear(select);
      append(select, make('option', '', allLabel));
      select.lastElementChild.value = 'all';
      options.forEach(([value, label]) => {
        const option = make('option', '', label);
        option.value = value;
        select.append(option);
      });
      select.dataset.options = signature;
    }
    if ([...select.options].some((option) => option.value === selected)) {
      select.value = selected;
      return selected;
    }
    select.value = 'all';
    return 'all';
  }

  function syncBoardControls(cards) {
    app.board.lane = syncBoardSelect('board-filter-lane', boardLaneOptions(cards), app.board.lane, 'All lanes');
    app.board.priority = syncBoardSelect('board-filter-priority', boardPriorityOptions(cards), app.board.priority, 'All priorities');
    app.board.track = syncBoardSelect('board-filter-track', boardTrackOptions(cards), app.board.track, 'All tracks');
    const text = byId('board-filter-text');
    if (text && text.value !== app.board.text) text.value = app.board.text;
    const sort = byId('board-sort');
    if (!CARD_SORT_LABELS[app.board.sort]) app.board.sort = 'projection';
    if (sort) sort.value = app.board.sort;
    const direction = byId('board-sort-direction');
    if (direction) {
      const descending = app.board.direction === 'desc';
      direction.dataset.direction = descending ? 'desc' : 'asc';
      direction.setAttribute('aria-label', `${descending ? 'Use ascending' : 'Use descending'} card sort`);
      setText(direction, descending ? 'Descending' : 'Ascending');
    }
  }

  function renderBoardFilterStatus(total, visible) {
    const status = byId('board-filter-status');
    if (!status) return;
    clear(status);
    const label = boardFilterActive()
      ? `${visible} of ${total} projected cards match`
      : `${total} projected card${total === 1 ? '' : 's'}`;
    append(status, make('span', '', label));
    if (boardFilterActive()) {
      append(status, makeButton('Clear filters', 'button button--quiet board-toolbar__clear', () => {
        app.board.text = '';
        app.board.lane = 'all';
        app.board.priority = 'all';
        app.board.track = 'all';
        renderBoard();
      }));
    }
  }

  function isClosed(card) {
    return laneToken(card) === 'done' || card?.archived === true;
  }

  function identityOf(card) {
    return cardRef(card) || `${card?.num ?? ''}:${card?.title ?? ''}`;
  }

  function snapshotCards() {
    return Array.isArray(app.state?.cards) ? app.state.cards : [];
  }

  function boardCards() {
    const cards = new Map();
    snapshotCards().forEach((card) => {
      if (app.showClosed || !isClosed(card)) cards.set(identityOf(card), card);
    });
    if (app.showClosed) {
      (app.state?.closed?.cards || []).forEach((card) => cards.set(identityOf(card), card));
      (app.closed?.cards || []).forEach((card) => cards.set(identityOf(card), card));
    }
    return [...cards.values()];
  }

  async function getJSON(path) {
    const response = await fetch(path, {
      method: 'GET',
      headers: { Accept: 'application/json' },
      cache: 'no-store',
    });
    let payload = null;
    try { payload = await response.json(); } catch { /* handled below */ }
    if (!response.ok) {
      throw new Error(payload?.message || `GET ${path} failed (${response.status})`);
    }
    return payload;
  }

  function stateBlock(kind, title, detail, actionLabel, action) {
    const block = make('div', `state-block${kind === 'error' ? ' state-block--error' : ''}`);
    append(
      block,
      make('div', 'state-block__glyph', kind === 'error' ? '!' : kind === 'empty' ? '∅' : '…'),
      make('div', 'state-block__title', title),
      make('div', 'state-block__detail', detail),
    );
    if (actionLabel && action) append(block, makeButton(actionLabel, 'button button--quiet', action));
    return block;
  }

  function renderTabs() {
    VIEWS.forEach((view) => {
      const tab = byId(`tab-${view}`);
      const active = app.view === view;
      if (!tab) return;
      tab.classList.toggle('is-active', active);
      tab.setAttribute('aria-selected', String(active));
      tab.tabIndex = active ? 0 : -1;
      const panel = byId(`panel-${view}`);
      if (panel) {
        panel.hidden = !active;
        panel.setAttribute('aria-hidden', String(!active));
      }
    });
  }

  function renderChrome() {
    const loadState = byId('load-state');
    const revision = currentRevision();
    if (app.stateLoading && !app.state) {
      setText(loadState, 'Loading snapshot…');
    } else if (app.stateError && !app.state) {
      setText(loadState, 'Snapshot unavailable · retry below');
    } else if (app.stateLoading) {
      setText(loadState, `Refreshing snapshot${revision === null ? '' : ` · rev ${revision}`}…`);
    } else {
      setText(loadState, `${app.connection}${revision === null ? '' : ` · rev ${revision}`}`);
    }
    if (loadState) loadState.setAttribute('aria-busy', String(app.stateLoading));

    const stamp = byId('source-stamp');
    if (revision === null) setText(stamp, app.state ? 'revision not supplied' : 'Waiting for snapshot');
    else setText(stamp, `snapshot revision ${revision}`);

    const closedToggle = byId('closed-toggle');
    if (closedToggle) {
      closedToggle.setAttribute('aria-pressed', String(app.showClosed));
      setText(closedToggle, app.showClosed ? 'Hide closed' : 'Show closed');
    }

    const filter = byId('papercut-filter');
    if (filter) {
      const showingAll = app.papercutFilter === 'all';
      filter.setAttribute('aria-pressed', String(showingAll));
      setText(filter, showingAll ? 'Open only' : 'Show all');
    }
  }

  function activateView(view, { writeHash = true, focusTab = false } = {}) {
    if (!VIEWS.includes(view)) view = 'board';
    app.view = view;
    if (writeHash && location.hash !== `#${view}`) history.replaceState(null, '', `#${view}`);
    render();
    if (view === 'status' && app.state) ensureDiagnostics();
    if (focusTab) byId(`tab-${view}`)?.focus();
  }

  function invalidateRevisionCaches(previousRevision, nextRevision) {
    if (previousRevision === null || previousRevision === nextRevision) return;
    app.closed = null;
    app.closedError = null;
    app.historyCounts.clear();
    app.historyLoading.clear();
    app.historyErrors.clear();
    app.status = {
      requested: false,
      loading: false,
      lint: null,
      lintError: null,
      version: null,
      versionError: null,
    };
  }

  async function loadSnapshot({ quiet = false } = {}) {
    app.stateLoading = true;
    if (!quiet || !app.state) app.stateError = null;
    render();
    try {
      const next = await getJSON('/api/state');
      const previousRevision = currentRevision();
      app.state = next && typeof next === 'object' ? next : null;
      const nextRevision = currentRevision();
      invalidateRevisionCaches(previousRevision, nextRevision);
      app.stateError = app.state ? null : new Error('The state response was empty.');
      app.stateLoading = false;
      app.connection = 'Snapshot received';
      render();
      if (app.view === 'status') ensureDiagnostics();
      if (app.showClosed) ensureClosed();
    } catch (error) {
      app.stateLoading = false;
      app.stateError = error instanceof Error ? error : new Error(String(error));
      app.connection = 'Snapshot request failed';
      render();
    }
  }

  async function ensureClosed(force = false) {
    if (!app.state || app.closedLoading) return;
    const revision = currentRevision();
    if (!force && app.closed && (app.closed.rev === revision || revision === null)) return;
    app.closedLoading = true;
    app.closedError = null;
    renderBoard();
    try {
      const payload = await getJSON('/api/closed');
      app.closed = payload && typeof payload === 'object' ? payload : { cards: [], counts: {} };
      app.closedLoading = false;
      renderBoard();
    } catch (error) {
      app.closedLoading = false;
      app.closedError = error instanceof Error ? error : new Error(String(error));
      renderBoard();
    }
  }

  async function ensureHistoryCount(epochId) {
    if (!epochId || app.historyLoading.has(epochId) || app.historyCounts.has(epochId) || app.historyErrors.has(epochId)) return;
    app.historyLoading.add(epochId);
    app.historyErrors.delete(epochId);
    try {
      const payload = await getJSON(`/api/history?epoch=${encodeURIComponent(epochId)}&count=1`);
      const count = Number(payload?.count ?? payload?.cards?.length ?? 0);
      app.historyCounts.set(epochId, Number.isFinite(count) ? count : 0);
      app.historyLoading.delete(epochId);
      renderBoard();
    } catch (error) {
      app.historyLoading.delete(epochId);
      app.historyErrors.set(epochId, error instanceof Error ? error : new Error(String(error)));
      renderBoard();
    }
  }

  function epochSourceById(id) {
    const epoch = (app.state?.epochs || []).find((item) => String(item.id) === String(id));
    const radar = (app.state?.radar || []).find((item) => String(item.id) === String(id));
    return { epoch, radar };
  }

  function boardEpochGroups(cards) {
    const byEpoch = new Map();
    cards
      .filter((card) => laneToken(card) !== 'frozen' && card.track !== 'sidequest')
      .forEach((card) => {
        const id = card.epoch === null || card.epoch === undefined || card.epoch === '' ? null : String(card.epoch);
        const group = byEpoch.get(id) || [];
        group.push(card);
        byEpoch.set(id, group);
      });

    const groups = [];
    const seen = new Set();
    const add = (id, source = {}) => {
      const key = id === null || id === undefined ? null : String(id);
      if (seen.has(key)) return;
      seen.add(key);
      const details = key === null ? {} : epochSourceById(key);
      const epoch = details.epoch || {};
      const radar = details.radar || {};
      groups.push({
        id: key,
        name: source.name || radar.name || epoch.name || key || 'Unplaced',
        goal: source.goal || radar.goal || epoch.goal || '',
        current: epoch.status === 'active' || ((app.state?.radar || [])[0]?.id === key),
        cards: byEpoch.get(key) || [],
        radar,
      });
    };

    // radar is already current-first and then ordered by the server.
    (app.state?.radar || []).forEach((radar) => add(radar.id, radar));
    // Keep source order for epochs not present in the active radar projection.
    (app.state?.epochs || []).forEach((epoch) => {
      const key = String(epoch.id);
      if (byEpoch.has(key)) add(key, epoch);
    });
    // Preserve any card group whose epoch metadata is not in the snapshot.
    byEpoch.forEach((group, key) => {
      if (key !== null && !seen.has(key)) add(key, { name: key });
    });
    if (byEpoch.has(null)) add(null, { name: 'Unplaced' });
    return groups;
  }

  function appendMetric(parent, label, value, className = '') {
    const metric = make('div', `metric${className ? ` ${className}` : ''}`);
    append(metric, make('dt', '', label), make('dd', '', value));
    parent.append(metric);
  }

  function overviewSection(title, lede = '', className = '') {
    const section = make('section', `overview-section${className ? ` ${className}` : ''}`);
    append(section, make('h3', '', title));
    if (lede) append(section, make('p', 'overview-section__lede', lede));
    return section;
  }

  function countNumber(value, fallback = 0) {
    const number = Number(value);
    return Number.isFinite(number) ? number : fallback;
  }

  function boundedPercent(value) {
    return Math.max(0, Math.min(100, Math.round(countNumber(value, 0))));
  }

  function progressFor(record) {
    const source = isRecord(record?.progress) ? record.progress : record || {};
    const total = countNumber(source.total, 0);
    const done = Math.max(0, countNumber(source.done, 0));
    const pct = hasOwn(source, 'pct')
      ? boundedPercent(source.pct)
      : total > 0 ? boundedPercent((done / total) * 100) : 0;
    return {
      total,
      done: Math.min(done, total || done),
      pct,
      met: source.met === true,
      reviewReady: source.reviewReady === true,
    };
  }

  function appendProgress(parent, progress, label) {
    const wrap = make('div', 'progress-wrap');
    const track = make('div', 'progress-track');
    setAttrs(track, {
      role: 'progressbar',
      'aria-label': label,
      'aria-valuemin': 0,
      'aria-valuemax': 100,
      'aria-valuenow': progress.pct,
      'aria-valuetext': `${progress.pct}% complete`,
    });
    const fill = make('span', 'progress-track__fill');
    fill.style.width = `${progress.pct}%`;
    append(track, fill);
    append(wrap, track, make('span', 'progress-wrap__label', `${progress.done}/${progress.total} · ${progress.pct}%`));
    append(parent, wrap);
  }

  function renderBoardCounts(parent, cards, visibleCards) {
    const section = overviewSection('Counts', 'A compact read of the projected snapshot.', 'overview-section--counts');
    const rawCounts = isRecord(app.state?.counts) ? app.state.counts : {};
    const stateCards = snapshotCards();
    const metrics = make('div', 'overview-metrics');
    const entries = [
      ['Projected cards', cards.length],
      ['Matching cards', visibleCards.length],
      ['Open cards', cards.filter((card) => !isClosed(card)).length],
      ['Closed in state', stateCards.filter(isClosed).length],
      ['Decisions', listValue(app.state?.decisions).length],
      ['For owner', countNumber(rawCounts.forYou ?? rawCounts.decide, 0)],
      ['Agent ready', countNumber(rawCounts.agentReady, 0)],
      ['Open questions', countNumber(rawCounts.openQuestions, 0)],
      ['Ideas', countNumber(rawCounts.ideas, listValue(app.state?.ideas).length)],
    ];
    entries.forEach(([label, value]) => appendMetric(metrics, label, value));
    const byPhase = isRecord(rawCounts.byPhase) ? rawCounts.byPhase : null;
    if (byPhase && Object.keys(byPhase).length) {
      const phaseCounts = make('div', 'overview-phase-counts');
      append(phaseCounts, make('h4', '', 'Phase counts'));
      Object.entries(byPhase).forEach(([phase, count]) => appendMetric(phaseCounts, phaseLabel(phase), count));
      append(section, phaseCounts);
    }
    append(section, metrics);
    if (!Object.keys(rawCounts).length) append(section, make('p', 'overview-section__note', 'Derived from cards; counts object is not present in this snapshot.'));
    append(parent, section);
  }

  function milestoneTitle(milestone) {
    return scalarText(milestone?.title || milestone?.name || milestone?.id, 'Untitled milestone');
  }

  function milestoneStatus(milestone, progress = progressFor(milestone)) {
    if (progress.met) return 'met';
    if (progress.reviewReady) return 'review-ready';
    return scalarText(milestone?.status, 'open');
  }

  function renderMilestoneRecord(parent, milestone) {
    const item = make('article', 'overview-record milestone-record');
    const progress = progressFor(milestone);
    const head = make('div', 'overview-record__head');
    append(head, make('h4', '', milestoneTitle(milestone)));
    const meta = [
      milestone?.id ? String(milestone.id) : '',
      milestoneStatus(milestone, progress),
      milestone?.archived ? 'archived' : '',
    ].filter(Boolean).join(' · ');
    if (meta) append(head, make('span', 'overview-record__meta', meta));
    append(item, head);
    if (milestone?.goal) append(item, make('p', 'overview-record__copy', milestone.goal));
    appendProgress(item, progress, `${milestoneTitle(milestone)} progress`);
    const criteria = collectionItems(milestone?.criteria);
    if (criteria) append(item, make('p', 'overview-record__note', `${criteria.length} exit criteri${criteria.length === 1 ? 'on' : 'a'} recorded`));
    append(parent, item);
  }

  function renderMilestoneOverview(parent) {
    const section = overviewSection('Milestone progress', 'Done work, review readiness, and verified milestones.', 'overview-section--milestones');
    const milestones = collectionItems(app.state?.milestones);
    if (!milestones) {
      append(section, stateBlock('empty', 'Milestone data unavailable', 'This snapshot does not contain a milestone list.'));
    } else if (!milestones.length) {
      append(section, stateBlock('empty', 'No milestones recorded', 'The snapshot has no milestone rows to progress.'));
    } else {
      const list = make('div', 'overview-record-list');
      milestones.forEach((milestone) => renderMilestoneRecord(list, milestone));
      append(section, list);
    }
    append(parent, section);
  }

  function renderBurndown(parent, points, label) {
    const values = collectionItems(points);
    if (!values) {
      append(parent, stateBlock('empty', 'Burndown unavailable', 'No burndown series is present for this radar row.'));
      return;
    }
    if (!values.length) {
      append(parent, stateBlock('empty', 'No burndown points', 'The snapshot contains no completed-card points for this period.'));
      return;
    }
    const numbers = values.map((point) => countNumber(point?.n ?? point?.count, 0));
    const max = Math.max(...numbers, 1);
    const chart = make('div', 'burndown-chart');
    setAttrs(chart, { role: 'img', 'aria-label': `${label} 30-day burndown` });
    values.forEach((point, index) => {
      const day = scalarText(point?.day, `point ${index + 1}`);
      const number = numbers[index];
      const bar = make('span', 'burndown-chart__bar');
      bar.style.height = `${Math.max(8, Math.round((number / max) * 100))}%`;
      setAttrs(bar, { title: `${day}: ${number} completed`, 'aria-label': `${day}: ${number} completed` });
      append(chart, bar);
    });
    const labels = make('div', 'burndown-chart__labels');
    append(labels,
      make('span', '', scalarText(values[0]?.day, 'start')),
      make('span', '', scalarText(values[values.length - 1]?.day, 'latest')),
    );
    append(parent, chart, labels);
  }

  function radarMilestoneRow(parent, milestone) {
    const progress = progressFor(milestone);
    const row = make('li', 'radar-milestone');
    const title = milestoneTitle(milestone);
    const status = progress.met ? 'met' : progress.reviewReady ? 'review-ready' : scalarText(milestone?.status, 'open');
    const stalled = milestone?.stalledDays === null || milestone?.stalledDays === undefined || milestone?.stalledDays === ''
      ? ''
      : `stalled ${milestone.stalledDays}d`;
    append(row,
      make('span', `radar-milestone__status radar-milestone__status--${statusToken(status)}`, status),
      make('span', 'radar-milestone__title', title),
      make('span', 'radar-milestone__count', `${progress.done}/${progress.total}`),
      stalled ? make('span', 'radar-milestone__stalled', stalled) : null,
    );
    append(parent, row);
  }

  function renderRadarRecord(parent, radar) {
    const item = make('article', 'overview-record radar-record');
    const head = make('div', 'overview-record__head');
    append(head, make('h4', '', scalarText(radar?.name || radar?.title || radar?.id, 'Untitled radar')));
    const pct = boundedPercent(radar?.pct ?? progressFor(radar).pct);
    const meta = [
      radar?.id ? String(radar.id) : '',
      `${pct}% milestones`,
      `${countNumber(radar?.active, 0)} active`,
      `${countNumber(radar?.done, 0)} done`,
    ].filter(Boolean).join(' · ');
    append(head, make('span', 'overview-record__meta', meta));
    append(item, head);
    if (radar?.goal) append(item, make('p', 'overview-record__copy', radar.goal));
    const progress = {
      done: countNumber(radar?.milestonesMet, 0),
      total: countNumber(radar?.milestoneTotal, 0),
      pct,
    };
    appendProgress(item, progress, `${scalarText(radar?.name || radar?.id, 'Radar')} milestone progress`);
    const burndown = make('div', 'radar-record__burndown');
    append(burndown, make('h5', '', '30-day burndown'));
    renderBurndown(burndown, radar?.burndown, scalarText(radar?.name || radar?.id, 'Radar'));
    append(item, burndown);
    const milestones = collectionItems(radar?.milestones);
    if (milestones && milestones.length) {
      const list = make('ul', 'radar-milestone-list');
      milestones.forEach((milestone) => radarMilestoneRow(list, milestone));
      append(item, make('h5', 'radar-record__milestones-title', 'Milestones'), list);
    }
    append(parent, item);
  }

  function renderRadarOverview(parent) {
    const section = overviewSection('Radar & burndown', 'Epoch signal with recent completion activity.', 'overview-section--radar');
    const radar = collectionItems(app.state?.radar);
    if (!radar) {
      append(section, stateBlock('empty', 'Radar data unavailable', 'This snapshot does not contain an epoch radar.'));
    } else if (!radar.length) {
      append(section, stateBlock('empty', 'No radar rows', 'No active or unfinished epochs are in the snapshot.'));
    } else {
      const list = make('div', 'overview-record-list');
      radar.forEach((record) => renderRadarRecord(list, record));
      append(section, list);
    }
    append(parent, section);
  }

  function appendRecordCardLink(parent, cardId) {
    if (cardId === null || cardId === undefined || cardId === '') return;
    const card = findCardSummary(cardId);
    const ref = cardRef(card) || String(cardId);
    append(parent, makeFocusButton(card ? `Open ${cardNumber(card)}` : `Open card ${cardId}`, 'linked-card', ref, () => openFocus(ref, null, true), {
      'aria-label': `Open ${card ? cardNumber(card) : cardId} in read-only focus`,
    }));
  }

  function renderRecentlyDecided(parent) {
    const section = overviewSection('Recently decided', 'Ratified decisions still inside the snapshot’s recent window.', 'overview-section--recent');
    const decisions = collectionItems(app.state?.recentlyDecided);
    if (!decisions) {
      append(section, stateBlock('empty', 'Recent decisions unavailable', 'This snapshot does not contain recently decided items.'));
    } else if (!decisions.length) {
      append(section, stateBlock('empty', 'No recent decisions', 'No ratified decisions fall inside the snapshot window.'));
    } else {
      const list = make('ul', 'recent-list');
      decisions.forEach((decision) => {
        const item = make('li', 'recent-item');
        const head = make('div', 'recent-item__head');
        append(head,
          make('span', 'decision-id', scalarText(decision?.id, 'Decision')),
          make('time', 'recent-item__date', formatDateTime(decision?.ratifiedAt)),
        );
        append(item, head, make('p', 'recent-item__title', scalarText(decision?.title || decision?.gist, 'Untitled decision')));
        if (hasOwn(decision, 'outcome')) append(item, make('p', 'recent-item__copy', `Outcome · ${stringValue(decision.outcome)}`));
        append(item, make('p', 'recent-item__copy', hasOwn(decision, 'comment') ? `Comment · ${stringValue(decision.comment, 'No comment recorded.')}` : 'No comment recorded.'));
        const link = make('p', 'recent-item__link');
        appendRecordCardLink(link, decision?.cardId);
        if (link.childElementCount) append(item, link);
        append(list, item);
      });
      append(section, list);
    }
    append(parent, section);
  }

  function renderIdeas(parent) {
    const section = overviewSection('Ideas', 'Unfiltered idea records, including tagged outcomes.', 'overview-section--ideas');
    const ideas = collectionItems(app.state?.ideas);
    if (!ideas) {
      append(section, stateBlock('empty', 'Ideas unavailable', 'This snapshot does not contain an ideas list.'));
    } else if (!ideas.length) {
      append(section, stateBlock('empty', 'No ideas recorded', 'The snapshot has no idea rows.'));
    } else {
      const list = make('ul', 'idea-list');
      ideas.forEach((idea) => {
        const item = make('li', 'idea-item');
        const head = make('div', 'idea-item__head');
        append(head, make('span', 'idea-item__id', scalarText(idea?.id, 'Idea')));
        if (hasOwn(idea, 'status')) append(head, make('span', `status-chip status-chip--${statusToken(idea.status)}`, scalarText(idea.status, 'unknown')));
        if (idea?.created) append(head, make('time', 'idea-item__date', formatDateTime(idea.created)));
        const text = idea?.text || idea?.title || idea?.body;
        append(item, head, make('p', 'idea-item__text', stringValue(text, 'Idea text not recorded.')));
        if (idea?.note) append(item, make('p', 'idea-item__note', idea.note));
        const tags = listValue(idea?.tags);
        if (tags.length) append(item, make('p', 'idea-item__tags', `Tags · ${tags.map((tag) => scalarText(tag)).join(', ')}`));
        const link = make('p', 'idea-item__link');
        appendRecordCardLink(link, idea?.cardId);
        if (link.childElementCount) append(item, link);
        append(list, item);
      });
      append(section, list);
    }
    append(parent, section);
  }

  function renderBoardOverview(parent, cards, visibleCards) {
    const overview = make('div', 'board-overview');
    renderBoardCounts(overview, cards, visibleCards);
    const grid = make('div', 'board-overview__grid');
    renderMilestoneOverview(grid);
    renderRadarOverview(grid);
    renderRecentlyDecided(grid);
    renderIdeas(grid);
    append(overview, grid);
    append(parent, overview);
  }

  function makeCard(card) {
    const item = make('li');
    const lane = laneToken(card);
    const article = make('article', 'shadow-card');
    article.dataset.lane = lane;
    const top = make('div', 'shadow-card__top');
    if (card.workOrder !== null && card.workOrder !== undefined && card.workOrder !== '') {
      append(top, make('span', 'card__work-order', `order ${card.workOrder}`));
    }
    append(top, make('span', 'card__number', cardNumber(card)));
    append(top, make('span', 'card__phase', phaseLabel(card.phase)));

    const body = make('div', 'card__body');
    const ref = cardRef(card);
    const titleButton = ref
      ? makeFocusButton(cardTitle(card), 'card__title-button', ref, () => openFocus(ref, null, true), {
        'aria-label': `Open read-only focus for ${cardNumber(card)} ${cardTitle(card)}`,
      })
      : make('span', 'card__title-button', cardTitle(card));
    append(body, titleButton);

    const chips = make('div', 'card__chips');
    if (card.priority !== null && card.priority !== undefined && card.priority !== '') {
      append(chips, make('span', 'card-chip card-chip--priority', String(card.priority)));
    }
    if (card.track) append(chips, make('span', 'card-chip', String(card.track)));
    if (isClosed(card)) append(chips, make('span', 'card-chip card-chip--closed', card.archived ? 'archived' : 'closed'));
    if (chips.childElementCount) append(body, chips);

    const summary = card.plan || card.summary;
    if (summary) append(body, make('p', 'card__summary', summary));

    const footer = make('div', 'card__footer');
    append(footer, make('span', 'card__lane', `lane / ${laneLabel(card)}`));
    if (ref) append(footer, makeFocusButton('Read focus →', 'card__open', ref, () => openFocus(ref, null, true), {
      'aria-label': `Read full details for ${cardNumber(card)}`,
    }));
    append(body, footer);
    append(article, top, body);
    append(item, article);
    return item;
  }

  function appendBoardGroup(parent, group, kind = 'epoch') {
    const section = make('section', 'board-group');
    const headingId = `board-group-${groupsRendered++}`;
    section.setAttribute('aria-labelledby', headingId);

    const head = make('div', 'board-group__head');
    const title = make('div');
    const titleRow = make('div', 'board-group__title-row');
    const tagClass = kind === 'sidequest' ? 'group-tag--sidequest' : kind === 'frozen' ? 'group-tag--frozen' : '';
    const tagText = kind === 'sidequest' ? 'OFF-PLAN' : kind === 'frozen' ? 'PARKED' : group.current ? 'CURRENT' : 'EPOCH';
    append(titleRow, make('span', `group-tag${tagClass ? ` ${tagClass}` : ''}`, tagText));
    append(titleRow, make('h3', '', group.name));
    const heading = titleRow.querySelector('h3');
    if (heading) heading.id = headingId;
    append(title, titleRow);
    if (group.goal) append(title, make('p', 'board-group__goal', group.goal));
    append(head, title);

    const metrics = make('dl', 'group-metrics');
    const openCount = group.cards.filter((card) => !isClosed(card)).length;
    const closedCount = group.cards.filter(isClosed).length;
    appendMetric(metrics, 'cards', group.cards.length);
    if (app.showClosed) appendMetric(metrics, 'closed', closedCount, 'metric--archive');
    append(head, metrics);
    append(section, head);

    if (app.showClosed && kind === 'epoch' && group.id) {
      const archive = make('p', 'board-group__archive');
      if (app.historyLoading.has(group.id)) setText(archive, 'archive count · checking…');
      else if (app.historyErrors.has(group.id)) setText(archive, 'archive count · unavailable');
      else if (app.historyCounts.has(group.id)) setText(archive, `archive count · ${app.historyCounts.get(group.id)}`);
      else setText(archive, 'archive count · waiting…');
      append(section, archive);
      ensureHistoryCount(group.id);
    }

    const list = make('ul', 'board-card-list');
    setAttrs(list, { 'aria-label': `${group.name} cards` });
    const sorted = sortBoardCards(group.cards);
    if (!sorted.length) {
      append(section, stateBlock('empty', 'No cards in this group', 'This snapshot has no card projected here.'));
    } else {
      sorted.forEach((card) => list.append(makeCard(card)));
      append(section, list);
    }
    if (openCount === 0 && closedCount > 0 && app.showClosed) {
      // The metric remains useful when a group contains only finished cards.
      section.dataset.finishedOnly = 'true';
    }
    append(parent, section);
  }

  let groupsRendered = 0;

  function renderBoard() {
    const content = byId('board-content');
    if (!content) return;
    clear(content);
    groupsRendered = 0;
    if (!app.state) {
      append(content, app.stateError
        ? stateBlock('error', 'Snapshot unavailable', app.stateError.message, 'Retry GET /api/state', () => loadSnapshot())
        : stateBlock('loading', 'Loading board snapshot', 'Reading the projected board.'));
      return;
    }
    if (app.stateError) {
      const notice = make('div', 'inline-notice inline-notice--error');
      append(notice, make('strong', '', 'Refresh failed'), make('span', '', app.stateError.message));
      append(notice, makeButton('Retry', 'button button--quiet', () => loadSnapshot()));
      append(content, notice);
    }
    if (app.showClosed && app.closedLoading) {
      append(content, make('div', 'inline-notice', 'Loading closed cards from GET /api/closed…'));
    } else if (app.showClosed && app.closedError) {
      const notice = make('div', 'inline-notice inline-notice--error');
      append(notice, make('strong', '', 'Closed cards unavailable'), make('span', '', app.closedError.message));
      append(notice, makeButton('Retry', 'button button--quiet', () => ensureClosed(true)));
      append(content, notice);
    }

    const cards = boardCards();
    syncBoardControls(cards);
    const visibleCards = cards.filter(boardCardMatches);
    renderBoardFilterStatus(cards.length, visibleCards.length);
    renderBoardOverview(content, cards, visibleCards);
    const sidequests = visibleCards.filter((card) => laneToken(card) !== 'frozen' && card.track === 'sidequest');
    const frozen = visibleCards.filter((card) => laneToken(card) === 'frozen');
    const epochs = boardEpochGroups(visibleCards);

    if (!visibleCards.length) {
      append(content, stateBlock('empty', cards.length ? 'No cards match these filters' : 'Board is empty', cards.length
        ? 'Clear one or more client-side filters to see the projected cards.'
        : app.showClosed
          ? 'No active or closed cards are present in this snapshot.'
          : 'No active cards are present in this snapshot. Use Show closed to include finished work.'));
      return;
    }
    if (sidequests.length) appendBoardGroup(content, { name: 'Sidequests', goal: 'Work outside the epoch plan.', cards: sidequests }, 'sidequest');
    epochs.forEach((group) => appendBoardGroup(content, group, 'epoch'));
    if (frozen.length) appendBoardGroup(content, { name: 'Frozen', goal: 'Owner-paused work.', cards: frozen }, 'frozen');
  }

  function findCardSummary(ref) {
    const target = String(ref ?? '').replace(/^#/, '');
    return boardCards().find((card) => String(card.id ?? '').replace(/^#/, '') === target
      || String(card.num ?? '') === target) || null;
  }

  function openDecisionList() {
    return listValue(app.state?.decisions).filter((decision) => decision.status !== 'ratified' && !decision.draft);
  }

  function renderFocusChooser(parent) {
    const chooser = make('div', 'focus-chooser');
    const decisionsSection = make('section', 'focus-chooser__section');
    append(decisionsSection, make('h3', '', 'Decision queue'));
    append(decisionsSection, make('p', '', 'Open a decision to fetch its owning card and inspect the full read-only record.'));
    const decisions = openDecisionList();
    if (!decisions.length) {
      append(decisionsSection, stateBlock('empty', 'No open decisions', 'The snapshot has no unresolved decision rows.'));
    } else {
      const list = make('ol', 'focus-list');
      decisions.forEach((decision, index) => {
        const item = make('li', 'focus-list__item');
        const mark = make('span', 'focus-list__mark', index + 1);
        const card = findCardSummary(decision.cardId);
        const ref = decision.cardId || cardRef(card);
        const label = stringValue(decision.title || decision.gist, 'Untitled decision');
        const choice = ref
          ? makeFocusButton(`${decision.id || 'Decision'} — ${label}`, 'focus-choice', ref, () => openFocus(ref, decision.id, true), {
            'aria-label': `Open ${decision.id || 'decision'} in read-only focus`,
          })
          : make('span', 'focus-choice', `${decision.id || 'Decision'} — ${label}`);
        const meta = make('span', 'focus-choice__meta', `${ref ? cardNumber(card) : 'No owning card'} · ${stringValue(decision.status, 'open')}`);
        append(choice, meta);
        append(item, mark, choice);
        list.append(item);
      });
      append(decisionsSection, list);
    }

    const cardsSection = make('section', 'focus-chooser__section');
    append(cardsSection, make('h3', '', 'Cards in snapshot'));
    append(cardsSection, make('p', '', 'Choose any visible card to request its complete record from /api/card?id=.'));
    const cards = snapshotCards().filter((card) => !isClosed(card));
    if (!cards.length) {
      append(cardsSection, stateBlock('empty', 'No active cards', 'Show closed on Board to include finished records.'));
    } else {
      const list = make('ol', 'focus-list');
      cards.forEach((card, index) => {
        const item = make('li', 'focus-list__item');
        const mark = make('span', 'focus-list__mark', index + 1);
        const ref = cardRef(card);
        const choice = ref
          ? makeFocusButton(`${cardNumber(card)} — ${cardTitle(card)}`, 'focus-choice', ref, () => openFocus(ref, null, true), {
            'aria-label': `Open ${cardNumber(card)} in read-only focus`,
          })
          : make('span', 'focus-choice', `${cardNumber(card)} — ${cardTitle(card)}`);
        append(choice, make('span', 'focus-choice__meta', `${phaseLabel(card.phase)} · lane / ${laneLabel(card)}`));
        append(item, mark, choice);
        list.append(item);
      });
      append(cardsSection, list);
    }
    append(chooser, decisionsSection, cardsSection);
    append(parent, chooser);
  }

  function detailSection(title, className = '') {
    const section = make('section', `detail-section${className ? ` ${className}` : ''}`);
    append(section, make('h3', '', title));
    return section;
  }

  function appendEmptyDetail(parent, message) {
    append(parent, make('p', 'detail-copy detail-copy--muted', message));
  }

  function statusToken(value) {
    return String(value || 'unknown').toLowerCase().replace(/[^a-z0-9_-]/g, '-');
  }

  function renderStructuredValue(value) {
    const wrap = make('div', 'structured-value');
    if (Array.isArray(value)) {
      if (!value.length) append(wrap, make('span', 'structured-scalar structured-scalar--muted', 'Empty list'));
      else {
        const list = make('ul', 'structured-list');
        value.forEach((item) => {
          const row = make('li', 'structured-list__item');
          append(row, renderStructuredValue(item));
          append(list, row);
        });
        append(wrap, list);
      }
    } else if (isRecord(value)) {
      const details = make('dl', 'structured-object');
      Object.entries(value).forEach(([key, item]) => {
        const row = make('div', 'structured-object__row');
        const valueCell = make('dd', 'structured-object__value');
        append(valueCell, renderStructuredValue(item));
        append(row, make('dt', 'structured-object__key', fieldLabel(key)), valueCell);
        append(details, row);
      });
      append(wrap, details);
    } else {
      append(wrap, make('span', 'structured-scalar', scalarText(value)));
    }
    return wrap;
  }

  function appendRecordField(parent, label, value, used, key, className = '') {
    if (key) used.add(key);
    const field = make('div', `structured-field${className ? ` ${className}` : ''}`);
    append(field, make('div', 'structured-field__label', label), renderStructuredValue(value));
    append(parent, field);
    return field;
  }

  function appendObjectResidual(parent, value, used = new Set()) {
    if (!isRecord(value)) return false;
    const entries = Object.entries(value).filter(([key]) => !used.has(key));
    if (!entries.length) return false;
    const details = make('dl', 'structured-object structured-object--residual');
    entries.forEach(([key, item]) => {
      const row = make('div', 'structured-object__row');
      const valueCell = make('dd', 'structured-object__value');
      append(valueCell, renderStructuredValue(item));
      append(row, make('dt', 'structured-object__key', fieldLabel(key)), valueCell);
      append(details, row);
    });
    append(parent, details);
    return true;
  }

  function appendResidualDetails(parent, record, used, title = 'Additional snapshot details') {
    if (!isRecord(record)) return;
    const entries = Object.keys(record).filter((key) => !used.has(key));
    if (!entries.length) return;
    const section = detailSection(title, 'detail-section--residual');
    append(section, make('p', 'detail-copy detail-copy--muted', 'Fields outside the concise view remain visible below.'));
    appendObjectResidual(section, record, used);
    append(parent, section);
  }

  function appendDecisionField(parent, label, decision, key, used, className = '') {
    if (!hasOwn(decision, key)) return false;
    appendRecordField(parent, label, decision[key], used, key, className);
    return true;
  }

  function renderOptionDetail(parent, option, outcome) {
    const item = make('li', 'option-detail');
    if (!isRecord(option)) {
      append(item, renderStructuredValue(option));
      append(parent, item);
      return;
    }
    const used = new Set();
    const key = option.key ?? option.id;
    if (hasOwn(option, 'key')) used.add('key');
    if (hasOwn(option, 'id')) used.add('id');
    if (hasOwn(option, 'name')) used.add('name');
    if (hasOwn(option, 'title')) used.add('title');
    const optionKey = scalarText(key, 'Option');
    const chosen = outcome !== null && outcome !== undefined && String(outcome) === optionKey;
    const top = make('div', 'option-detail__top');
    append(top, make('span', `option-chip${chosen ? ' option-chip--chosen' : ''}`, `${optionKey}${chosen ? ' · chosen' : ''}`));
    if (hasOwn(option, 'name') || hasOwn(option, 'title')) append(top, make('strong', 'option-detail__name', scalarText(option.name ?? option.title)));
    append(item, top);
    if (hasOwn(option, 'detail')) appendRecordField(item, 'Detail', option.detail, used, 'detail');
    if (hasOwn(option, 'technical')) appendRecordField(item, 'Technical', option.technical, used, 'technical');
    appendObjectResidual(item, option, used);
    append(parent, item);
  }

  function renderComparisonDetail(parent, comparison) {
    const item = make('li', 'comparison-item');
    if (!isRecord(comparison)) {
      append(item, renderStructuredValue(comparison));
      append(parent, item);
      return;
    }
    const used = new Set();
    const head = make('div', 'comparison-item__head');
    if (hasOwn(comparison, 'lang')) {
      used.add('lang');
      append(head, make('strong', '', scalarText(comparison.lang, 'Comparison')));
    } else append(head, make('strong', '', 'Comparison'));
    append(item, head);
    if (hasOwn(comparison, 'note')) appendRecordField(item, 'Note', comparison.note, used, 'note');
    if (hasOwn(comparison, 'code')) appendRecordField(item, 'Example', comparison.code, used, 'code', 'structured-field--code');
    appendObjectResidual(item, comparison, used);
    append(parent, item);
  }

  function renderRecommendationDetail(parent, decision, used) {
    if (!hasOwn(decision, 'recommendation')) return;
    used.add('recommendation');
    const block = make('div', 'decision-subsection');
    append(block, make('h5', '', 'Recommendation'));
    if (!isRecord(decision.recommendation)) append(block, renderStructuredValue(decision.recommendation));
    else {
      const recommendation = decision.recommendation;
      const recommendationUsed = new Set();
      if (hasOwn(recommendation, 'why')) appendRecordField(block, 'Why', recommendation.why, recommendationUsed, 'why');
      if (hasOwn(recommendation, 'whyNot')) {
        recommendationUsed.add('whyNot');
        appendRecordField(block, 'Why not', recommendation.whyNot, recommendationUsed, 'whyNot');
      }
      if (hasOwn(recommendation, 'tradeoff')) appendRecordField(block, 'Trade-off', recommendation.tradeoff, recommendationUsed, 'tradeoff');
      appendObjectResidual(block, recommendation, recommendationUsed);
    }
    append(parent, block);
  }

  function renderHybridDetail(parent, decision, used) {
    if (!hasOwn(decision, 'hybrid')) return;
    used.add('hybrid');
    const block = make('div', 'decision-subsection');
    append(block, make('h5', '', 'Hybrid'));
    if (!isRecord(decision.hybrid)) append(block, renderStructuredValue(decision.hybrid));
    else {
      const hybrid = decision.hybrid;
      const hybridUsed = new Set();
      if (hasOwn(hybrid, 'result')) appendRecordField(block, 'Result', hybrid.result, hybridUsed, 'result');
      if (hasOwn(hybrid, 'synthesis')) appendRecordField(block, 'Synthesis', hybrid.synthesis, hybridUsed, 'synthesis');
      if (hasOwn(hybrid, 'harvest')) appendRecordField(block, 'Harvest', hybrid.harvest, hybridUsed, 'harvest');
      appendObjectResidual(block, hybrid, hybridUsed);
    }
    append(parent, block);
  }

  function renderDecisionDetail(parent, decision) {
    const item = make('li', 'decision-item');
    const record = isRecord(decision) ? decision : { value: decision };
    const used = new Set();
    const top = make('div', 'decision-item__top');
    const id = stringValue(record.id, 'Decision');
    const status = stringValue(record.status, 'unknown');
    if (hasOwn(record, 'id')) used.add('id');
    if (hasOwn(record, 'status')) used.add('status');
    append(top, make('span', 'decision-id', id), make('span', `table-label decision-status--${statusToken(status)}`, status));
    if (hasOwn(record, 'outcome')) {
      used.add('outcome');
      append(top, make('span', 'decision-outcome', `Outcome · ${stringValue(record.outcome)}`));
    }
    append(item, top);
    if (hasOwn(record, 'title')) {
      used.add('title');
      append(item, make('p', 'decision-title', stringValue(record.title, 'Untitled decision')));
    } else append(item, make('p', 'decision-title', stringValue(record.gist, 'Untitled decision')));

    appendDecisionField(item, 'Gist', record, 'gist', used);
    appendDecisionField(item, 'Story', record, 'story', used);
    appendDecisionField(item, 'Lesson', record, 'lesson', used);
    appendDecisionField(item, 'Detail', record, 'detail', used);
    appendDecisionField(item, 'In the wild', record, 'inWild', used, 'structured-field--code');
    appendDecisionField(item, 'Comment', record, 'comment', used);
    appendDecisionField(item, 'Check instructions', record, 'checkInstructions', used);
    if (hasOwn(record, 'rec')) appendRecordField(item, 'Recommendation key', record.rec, used, 'rec');
    appendDecisionField(item, 'Trade-off', record, 'tradeoff', used);

    const decisionQuestions = [];
    ['questions', 'messages'].forEach((key) => {
      if (!hasOwn(record, key)) return;
      used.add(key);
      const items = collectionItems(record[key]);
      if (items) decisionQuestions.push(...items);
      else appendRecordField(item, fieldLabel(key), record[key], new Set(), null);
    });
    ['questions', 'messages'].forEach((key) => {
      if (isRecord(record[key])) appendObjectResidual(item, record[key], new Set(['items']));
    });
    if (decisionQuestions.length) {
      const block = make('div', 'decision-subsection');
      append(block, make('h5', '', 'Questions & messages'));
      const list = make('ul', 'detail-list');
      decisionQuestions.forEach((question) => renderQuestionDetail(list, question));
      append(block, list);
      append(item, block);
    }

    const optionKey = hasOwn(record, 'options') ? 'options' : hasOwn(record, 'choices') ? 'choices' : null;
    if (optionKey) {
      used.add(optionKey);
      const options = collectionItems(record[optionKey]);
      const block = make('div', 'decision-subsection');
      append(block, make('h5', '', 'Options'));
      if (!options) append(block, renderStructuredValue(record[optionKey]));
      else if (!options.length) appendEmptyDetail(block, 'No options recorded.');
      else {
        const list = make('ul', 'option-list option-list--stacked');
        options.forEach((option) => renderOptionDetail(list, option, record.outcome));
        append(block, list);
      }
      if (isRecord(record[optionKey])) appendObjectResidual(block, record[optionKey], new Set(['items']));
      append(item, block);
    } else append(item, make('p', 'decision-detail', 'No options recorded.'));

    if (hasOwn(record, 'comparisons')) {
      used.add('comparisons');
      const block = make('div', 'decision-subsection');
      append(block, make('h5', '', 'Comparisons'));
      const comparisons = collectionItems(record.comparisons);
      if (!comparisons) append(block, renderStructuredValue(record.comparisons));
      else if (!comparisons.length) appendEmptyDetail(block, 'No comparisons recorded.');
      else {
        const list = make('ul', 'comparison-list');
        comparisons.forEach((comparison) => renderComparisonDetail(list, comparison));
        append(block, list);
      }
      if (isRecord(record.comparisons)) appendObjectResidual(block, record.comparisons, new Set(['items']));
      append(item, block);
    }
    renderRecommendationDetail(item, record, used);
    renderHybridDetail(item, record, used);

    if (hasOwn(record, 'reviewPasses')) {
      used.add('reviewPasses');
      appendRecordField(item, 'Review passes', record.reviewPasses, new Set(), null, 'structured-field--review-passes');
    }

    if (hasOwn(record, 'log')) {
      used.add('log');
      const log = make('div', 'decision-subsection');
      append(log, make('h5', '', 'Decision log'));
      const entries = collectionItems(record.log);
      if (!entries) append(log, renderStructuredValue(record.log));
      else if (!entries.length) appendEmptyDetail(log, 'No decision log entries recorded.');
      else {
        const list = make('ol', 'log-list');
        entries.forEach((entry) => renderLogDetail(list, entry));
        append(log, list);
      }
      if (isRecord(record.log)) appendObjectResidual(log, record.log, new Set(['items']));
      append(item, log);
    }
    appendResidualDetails(item, record, used, 'Additional decision details');
    append(parent, item);
  }

  function renderCriterionDetail(parent, criterion) {
    const item = make('li', 'criterion-item');
    const record = isRecord(criterion) ? criterion : { text: criterion };
    const used = new Set();
    if (hasOwn(record, 'n')) used.add('n');
    append(item, make('span', 'criterion-number', `#${stringValue(record.n, '?')}`));
    const copy = make('div');
    const status = stringValue(record.status, 'open');
    if (hasOwn(record, 'status')) used.add('status');
    append(copy, make('div', `criterion-status criterion-status--${statusToken(status)}`, status));
    if (hasOwn(record, 'text')) {
      used.add('text');
      append(copy, make('p', 'criterion-text', stringValue(record.text, 'Criterion text not recorded.')));
    } else append(copy, make('p', 'criterion-text', 'Criterion text not recorded.'));
    if (hasOwn(record, 'evidence')) {
      used.add('evidence');
      append(copy, make('p', 'criterion-evidence', `Evidence: ${stringValue(record.evidence, 'No evidence recorded.')}`));
    }
    const by = [hasOwn(record, 'metBy') ? `met by ${stringValue(record.metBy)}` : '', hasOwn(record, 'verifiedBy') ? `verified by ${stringValue(record.verifiedBy)}` : '', hasOwn(record, 'at') ? `at ${stringValue(record.at)}` : '']
      .filter(Boolean).join(' · ');
    ['metBy', 'verifiedBy', 'at'].forEach((key) => { if (hasOwn(record, key)) used.add(key); });
    if (by) append(copy, make('p', 'criterion-by', by));
    appendObjectResidual(copy, record, used);
    append(item, copy);
    append(parent, item);
  }

  function renderQuestionDetail(parent, question) {
    const item = make('li', 'question-item');
    const record = isRecord(question) ? question : { text: question };
    const used = new Set();
    const meta = make('div', 'question-meta');
    ['by', 'kind', 'status', 'at'].forEach((key) => { if (hasOwn(record, key)) used.add(key); });
    append(meta,
      make('span', 'question-meta__author', stringValue(record.by, 'unknown author')),
      make('span', '', stringValue(record.kind, 'question')),
      make('span', '', stringValue(record.status, 'open')),
      record.at ? make('time', '', formatDateTime(record.at)) : null,
    );
    append(item, meta, make('p', 'detail-copy', stringValue(record.text, 'Question text not recorded.')));
    if (hasOwn(record, 'text')) used.add('text');
    if (hasOwn(record, 'answer')) {
      used.add('answer');
      append(item, make('p', 'question-answer', `Answer · ${stringValue(record.answeredBy, 'unknown author')}: ${stringValue(record.answer, 'No answer recorded.')}`));
    }
    if (hasOwn(record, 'answeredBy') && hasOwn(record, 'answer')) used.add('answeredBy');
    appendObjectResidual(item, record, used);
    append(parent, item);
  }

  function renderLogDetail(parent, entry) {
    const item = make('li', 'log-item');
    const record = isRecord(entry) ? entry : { text: entry };
    const used = new Set();
    const meta = make('div');
    if (hasOwn(record, 'at')) used.add('at');
    append(meta, make('time', '', formatDateTime(record.at)));
    if (record?.by) {
      used.add('by');
      append(meta, make('span', 'log-item__who', record.by));
    }
    if (hasOwn(record, 'text')) used.add('text');
    append(item, meta, make('p', 'log-text', stringValue(record.text, 'Log entry has no text.')));
    appendObjectResidual(item, record, used);
    append(parent, item);
  }

  function owningDecisions(card) {
    const linked = collectionItems(card?.decisions) || [];
    const cardId = card?.id;
    const stateDecisions = cardId === null || cardId === undefined || cardId === ''
      ? []
      : listValue(app.state?.decisions).filter((decision) => String(decision?.cardId) === String(cardId));
    const selected = app.focus.decisionId
      ? listValue(app.state?.decisions).filter((decision) => String(decision?.id) === String(app.focus.decisionId))
      : [];
    const records = new Map();
    [...linked, ...stateDecisions, ...selected].forEach((decision, index) => {
      const key = isRecord(decision) && (decision.id || decision.cardId) ? String(decision.id || `${decision.cardId}:${index}`) : `decision:${index}`;
      if (!records.has(key) || (isRecord(decision) && Object.keys(decision).length > Object.keys(records.get(key) || {}).length)) records.set(key, decision);
    });
    return [...records.values()];
  }

  function renderReferenceItems(parent, value) {
    const values = collectionItems(value);
    if (!values) {
      append(parent, renderStructuredValue(value));
      return;
    }
    if (!values.length) appendEmptyDetail(parent, 'Empty list.');
    else {
      const list = make('ul', 'reference-list');
      values.forEach((reference) => {
        const item = make('li', 'reference-list__item');
        if (!isRecord(reference) && reference !== null && reference !== undefined && findCardSummary(reference)) {
          const card = findCardSummary(reference);
          append(item, makeFocusButton(`${cardNumber(card)} — ${cardTitle(card)}`, 'linked-card', cardRef(card), () => openFocus(cardRef(card), null, true), {
            'aria-label': `Open ${cardNumber(card)} in read-only focus`,
          }));
        } else append(item, renderStructuredValue(reference));
        append(list, item);
      });
      append(parent, list);
    }
    if (isRecord(value)) appendObjectResidual(parent, value, new Set(['items']));
  }

  function renderFocusDetail(parent, card, { dialog = false } = {}) {
    const detail = make('div', 'focus-detail');
    const used = new Set();
    if (!dialog) {
      const bar = make('div', 'focus-detail__bar');
      append(bar, makeButton('← Focus index', 'button button--quiet focus-detail__back', () => clearFocus()));
      append(detail, bar);
    }

    const head = make('header', 'focus-card-head');
    const identity = make('div');
    const meta = make('div', 'focus-card-head__meta');
    ['id', 'num', 'title', 'phase', 'priority', 'archived'].forEach((key) => { if (hasOwn(card, key)) used.add(key); });
    append(meta, make('span', 'card__number', cardNumber(card)), make('span', 'card-chip', phaseLabel(card.phase)));
    if (card.id && card.num !== null && card.num !== undefined) append(meta, make('span', 'card-chip', `id ${card.id}`));
    if (card.priority !== null && card.priority !== undefined && card.priority !== '') append(meta, make('span', 'card-chip card-chip--priority', card.priority));
    if (card.archived) append(meta, make('span', 'card-chip card-chip--closed', 'archived'));
    append(identity, meta);
    const title = make('h3', '', cardTitle(card));
    if (dialog) title.id = 'dialog-title';
    append(identity, title);
    append(head, identity, make('div', 'focus-card-head__lane', `lane / ${laneLabel(card)}`));
    append(detail, head);
    append(detail, make('p', 'focus-read-only', 'Read-only card focus. This snapshot exposes record data only; no mutation controls are present.'));

    const metadata = detailSection('Snapshot metadata');
    if (hasOwn(card, 'lane')) {
      used.add('lane');
      appendRecordField(metadata, 'Lane projection', card.lane, new Set(), null);
    }
    ['kind', 'track', 'epoch', 'milestoneId', 'assignee', 'created', 'updated', 'workOrder', 'needsAcceptance', 'parentId', 'clearance', 'openQ'].forEach((key) => {
      if (hasOwn(card, key)) appendRecordField(metadata, fieldLabel(key), card[key], used, key);
    });
    if (card.phase && typeof card.phase === 'object') appendRecordField(metadata, 'Phase', card.phase, new Set(), null);
    if (hasOwn(card, 'priority') && (card.priority === null || card.priority === undefined || card.priority === '')) {
      appendRecordField(metadata, 'Priority', card.priority, new Set(), null);
    }
    if (hasOwn(card, 'archived') && !card.archived) appendRecordField(metadata, 'Archived', card.archived, new Set(), null);
    if (hasOwn(card, 'id') && !card.id) appendRecordField(metadata, 'Id', card.id, new Set(), null);
    append(detail, metadata);

    const body = detailSection('Body', 'detail-section--wide');
    let bodyFound = false;
    ['body', 'description'].forEach((key) => {
      if (hasOwn(card, key)) {
        bodyFound = true;
        appendRecordField(body, fieldLabel(key), card[key], used, key);
      }
    });
    if (!bodyFound) appendEmptyDetail(body, 'No body or description recorded.');
    append(detail, body);

    const plan = detailSection('Plan', 'detail-section--wide');
    if (hasOwn(card, 'plan')) appendRecordField(plan, 'Plan', card.plan, used, 'plan');
    else appendEmptyDetail(plan, 'No plan recorded.');
    append(detail, plan);

    const checks = detailSection('Check steps');
    const checkKeys = ['checkSteps', 'steps', 'checks', 'checklist', 'checkInstructions'];
    let checksFound = false;
    checkKeys.forEach((key) => {
      if (hasOwn(card, key)) {
        checksFound = true;
        appendRecordField(checks, fieldLabel(key), card[key], used, key);
      }
    });
    if (!checksFound) appendEmptyDetail(checks, 'No check steps recorded on this card.');
    append(detail, checks);

    const blockers = detailSection('Blockers');
    if (hasOwn(card, 'blockedBy')) {
      used.add('blockedBy');
      renderReferenceItems(blockers, card.blockedBy);
    } else if (isRecord(card.lane) && hasOwn(card.lane, 'blockers')) {
      renderReferenceItems(blockers, card.lane.blockers);
    } else appendEmptyDetail(blockers, 'No blockers recorded.');
    append(detail, blockers);

    const references = detailSection('References & tags');
    let referenceFound = false;
    if (hasOwn(card, 'refs')) {
      referenceFound = true;
      used.add('refs');
      const refs = make('div', 'structured-field');
      append(refs, make('div', 'structured-field__label', 'References'));
      renderReferenceItems(refs, card.refs);
      append(references, refs);
    }
    if (hasOwn(card, 'tags')) {
      referenceFound = true;
      used.add('tags');
      appendRecordField(references, 'Tags', card.tags, new Set(), null);
    }
    if (!referenceFound) appendEmptyDetail(references, 'No references or tags recorded.');
    append(detail, references);

    const decisions = detailSection('Decisions');
    const decisionList = owningDecisions(card);
    if (hasOwn(card, 'decisions')) used.add('decisions');
    if (!decisionList.length) appendEmptyDetail(decisions, 'No decisions recorded on this card.');
    else {
      const list = make('ul', 'detail-list');
      decisionList.forEach((decision) => renderDecisionDetail(list, decision));
      append(decisions, list);
    }
    if (isRecord(card.decisions)) appendObjectResidual(decisions, card.decisions, new Set(['items']));
    append(detail, decisions);

    const criteria = detailSection('Exit criteria');
    const criterionValue = card.criteria;
    const criterionList = collectionItems(criterionValue);
    if (hasOwn(card, 'criteria')) used.add('criteria');
    if (!criterionList) append(criterionValue === undefined ? make('p', 'detail-copy detail-copy--muted', 'No exit criteria recorded on this card.') : renderStructuredValue(criterionValue));
    else if (!criterionList.length) appendEmptyDetail(criteria, 'No exit criteria recorded on this card.');
    else {
      const list = make('ol', 'detail-list');
      criterionList.forEach((criterion) => renderCriterionDetail(list, criterion));
      append(criteria, list);
    }
    if (isRecord(criterionValue)) appendObjectResidual(criteria, criterionValue, new Set(['items']));
    if (hasOwn(card, 'evidence')) appendRecordField(criteria, 'Evidence', card.evidence, used, 'evidence');
    append(detail, criteria);

    const questions = detailSection('Questions & messages');
    const questionList = [];
    ['questions', 'messages'].forEach((key) => {
      if (!hasOwn(card, key)) return;
      used.add(key);
      const items = collectionItems(card[key]);
      if (items) questionList.push(...items);
      else appendRecordField(questions, fieldLabel(key), card[key], new Set(), null);
    });
    ['questions', 'messages'].forEach((key) => {
      if (isRecord(card[key])) appendObjectResidual(questions, card[key], new Set(['items']));
    });
    if (card.id !== null && card.id !== undefined && card.id !== '') {
      questionList.push(...listValue(app.state?.questions).filter((question) => String(question.cardId) === String(card.id)));
      questionList.push(...listValue(app.state?.messages).filter((message) => String(message.cardId) === String(card.id)));
      const seenQuestions = new Set();
      const uniqueQuestions = questionList.filter((question) => {
        const key = isRecord(question) && question.id
          ? String(question.id)
          : `${stringValue(question?.kind, '')}|${stringValue(question?.at, '')}|${stringValue(question?.text ?? question, '')}`;
        if (seenQuestions.has(key)) return false;
        seenQuestions.add(key);
        return true;
      });
      questionList.splice(0, questionList.length, ...uniqueQuestions);
    }
    if (!questionList.length) appendEmptyDetail(questions, 'No questions or messages recorded on this card.');
    else {
      const list = make('ul', 'detail-list');
      questionList.forEach((question) => renderQuestionDetail(list, question));
      append(questions, list);
    }
    append(detail, questions);

    const log = detailSection('Log');
    const logValue = card.log;
    const logList = collectionItems(logValue);
    if (hasOwn(card, 'log')) used.add('log');
    if (!logList) append(logValue === undefined ? make('p', 'detail-copy detail-copy--muted', 'No log entries recorded on this card.') : renderStructuredValue(logValue));
    else if (!logList.length) appendEmptyDetail(log, 'No log entries recorded on this card.');
    else {
      const list = make('ol', 'log-list');
      logList.forEach((entry) => renderLogDetail(list, entry));
      append(log, list);
    }
    if (isRecord(logValue)) appendObjectResidual(log, logValue, new Set(['items']));
    append(detail, log);
    appendResidualDetails(detail, card, used);
    append(parent, detail);
  }

  function focusErrorBlock(error, includeClose = false) {
    const block = stateBlock('error', 'Card focus unavailable', error?.message || 'The full card could not be loaded.', 'Retry GET /api/card', () => retryFocus());
    if (includeClose) append(block, makeButton('Close focus', 'button button--quiet', () => closeFocusDialog()));
    return block;
  }

  function renderFocusPanel() {
    const content = byId('focus-content');
    if (!content) return;
    clear(content);
    if (!app.state) {
      append(content, app.stateError
        ? stateBlock('error', 'Snapshot unavailable', app.stateError.message, 'Retry GET /api/state', () => loadSnapshot())
        : stateBlock('loading', 'Loading focus index', 'Reading cards and open decisions.'));
      return;
    }
    if (!app.focus.ref) {
      renderFocusChooser(content);
      return;
    }
    if (app.focus.loading) {
      append(content, stateBlock('loading', 'Loading full card', 'Fetching the read-only record from GET /api/card?id=…'));
      return;
    }
    if (app.focus.error || !app.focus.card) {
      append(content, focusErrorBlock(app.focus.error));
      return;
    }
    renderFocusDetail(content, app.focus.card);
  }

  function renderFocusDialog() {
    const content = byId('focus-dialog-content');
    if (!content || !app.dialogOpen) return;
    clear(content);
    const dialog = byId('focus-dialog');
    const bar = make('div', 'dialog-bar');
    append(bar, make('span', 'dialog-bar__label', 'READ-ONLY CARD FOCUS'));
    append(bar, makeButton('Close', 'button button--quiet dialog-close', () => closeFocusDialog(), { 'aria-label': 'Close focus dialog' }));
    append(content, bar);
    if (app.focus.loading) {
      dialog?.removeAttribute('aria-labelledby');
      append(content, stateBlock('loading', 'Loading full card', 'Fetching the read-only record from GET /api/card?id=…'));
    } else if (app.focus.error || !app.focus.card) {
      dialog?.removeAttribute('aria-labelledby');
      append(content, focusErrorBlock(app.focus.error, true));
    } else {
      dialog?.setAttribute('aria-labelledby', 'dialog-title');
      renderFocusDetail(content, app.focus.card, { dialog: true });
    }
    if (app.dialogOpen && dialog && !dialog.contains(document.activeElement)) {
      dialog.querySelector('.dialog-close')?.focus();
    }
  }

  const FOCUSABLE_SELECTOR = [
    'button:not([disabled])',
    'input:not([disabled])',
    'select:not([disabled])',
    'textarea:not([disabled])',
    'a[href]',
    '[tabindex]:not([tabindex="-1"])',
  ].join(',');

  function dialogFocusables(dialog) {
    return [...dialog.querySelectorAll(FOCUSABLE_SELECTOR)].filter((node) => !node.closest('[hidden]'));
  }

  function trapDialogFocus(event) {
    const dialog = byId('focus-dialog');
    if (!dialog) return;
    const focusables = dialogFocusables(dialog);
    if (!focusables.length) {
      event.preventDefault();
      dialog.focus();
      return;
    }
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    if (!dialog.contains(document.activeElement)) {
      event.preventDefault();
      first.focus();
    } else if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function setDialogBackgroundInert(inert) {
    const frame = document.querySelector('.site-frame');
    if (!frame) return;
    if (inert) frame.setAttribute('inert', '');
    else frame.removeAttribute('inert');
  }

  function isVisibleFocusTarget(target) {
    return target?.isConnected && !target.closest('[hidden]') && !target.closest('[inert]');
  }

  function findFocusOpener(ref) {
    if (!ref) return null;
    return [...document.querySelectorAll('[data-focus-ref]')]
      .find((node) => node.dataset.focusRef === String(ref) && isVisibleFocusTarget(node)) || null;
  }

  function showFocusDialog() {
    const dialog = byId('focus-dialog');
    if (!dialog) return;
    app.dialogOpen = true;
    setDialogBackgroundInert(true);
    renderFocusDialog();
    if (typeof dialog.showModal === 'function') {
      if (!dialog.open) {
        try { dialog.showModal(); } catch { dialog.setAttribute('open', ''); }
      }
    } else {
      dialog.setAttribute('open', '');
    }
    dialog.querySelector('.dialog-close')?.focus();
  }

  function closeFocusDialog() {
    const dialog = byId('focus-dialog');
    const { returnFocus, returnFocusRef, returnView } = app.focus;
    const wasOpen = app.dialogOpen || Boolean(dialog?.open);
    app.dialogOpen = false;
    setDialogBackgroundInert(false);
    if (dialog?.open && typeof dialog.close === 'function') dialog.close();
    else dialog?.removeAttribute('open');
    app.focus.returnFocus = null;
    app.focus.returnFocusRef = null;
    app.focus.returnView = null;
    if (!wasOpen) return;
    if (returnView && app.view !== returnView) {
      app.view = returnView;
      if (location.hash !== `#${returnView}`) history.replaceState(null, '', `#${returnView}`);
      render();
    }
    const target = isVisibleFocusTarget(returnFocus) ? returnFocus : findFocusOpener(returnFocusRef);
    if (target && typeof target.focus === 'function') target.focus();
    else byId(`tab-${returnView || app.view}`)?.focus();
  }

  function clearFocus() {
    closeFocusDialog();
    app.focus = {
      ref: null,
      decisionId: null,
      card: null,
      loading: false,
      error: null,
      token: app.focus.token + 1,
      returnFocus: null,
      returnFocusRef: null,
      returnView: null,
    };
    render();
  }

  async function openFocus(ref, decisionId = null, openDialog = false) {
    if (!ref) return;
    const sameFocus = app.dialogOpen && app.focus.ref === String(ref);
    const previousFocus = sameFocus
      ? app.focus.returnFocus
      : document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const token = app.focus.token + 1;
    app.focus = {
      ref: String(ref),
      decisionId: decisionId || null,
      card: null,
      loading: true,
      error: null,
      token,
      returnFocus: previousFocus,
      returnFocusRef: sameFocus ? app.focus.returnFocusRef : previousFocus?.dataset?.focusRef || String(ref),
      returnView: sameFocus ? app.focus.returnView : app.view,
    };
    activateView('focus');
    if (openDialog) showFocusDialog();
    try {
      const payload = await getJSON(`/api/card?id=${encodeURIComponent(String(ref))}`);
      if (app.focus.token !== token) return;
      const card = payload?.card || payload;
      if (!card || typeof card !== 'object') throw new Error('The card response was empty.');
      app.focus.card = card;
      app.focus.loading = false;
      app.focus.error = null;
      renderFocusPanel();
      if (app.dialogOpen) {
        renderFocusDialog();
        byId('focus-dialog')?.querySelector('.dialog-close')?.focus();
      }
    } catch (error) {
      if (app.focus.token !== token) return;
      app.focus.loading = false;
      app.focus.error = error instanceof Error ? error : new Error(String(error));
      renderFocusPanel();
      if (app.dialogOpen) renderFocusDialog();
    }
  }

  function retryFocus() {
    if (app.focus.ref) openFocus(app.focus.ref, app.focus.decisionId, app.dialogOpen);
  }

  function bindBoardControls() {
    const toolbar = byId('board-toolbar');
    if (!toolbar) return;
    toolbar.addEventListener('input', (event) => {
      if (event.target?.id !== 'board-filter-text') return;
      app.board.text = event.target.value;
      renderBoard();
    });
    toolbar.addEventListener('change', (event) => {
      const id = event.target?.id;
      if (id === 'board-filter-lane') app.board.lane = event.target.value;
      else if (id === 'board-filter-priority') app.board.priority = event.target.value;
      else if (id === 'board-filter-track') app.board.track = event.target.value;
      else if (id === 'board-sort') app.board.sort = event.target.value;
      else return;
      renderBoard();
    });
    byId('board-sort-direction')?.addEventListener('click', () => {
      app.board.direction = app.board.direction === 'desc' ? 'asc' : 'desc';
      renderBoard();
    });
  }

  function renderPapercuts() {
    const content = byId('papercut-content');
    if (!content) return;
    clear(content);
    if (!app.state) {
      append(content, app.stateError
        ? stateBlock('error', 'Snapshot unavailable', app.stateError.message, 'Retry GET /api/state', () => loadSnapshot())
        : stateBlock('loading', 'Loading papercuts', 'Reading the friction log.'));
      return;
    }
    const all = listValue(app.state.papercuts).map((papercut, index) => ({ papercut, index }));
    all.sort((a, b) => String(b.papercut.created || '').localeCompare(String(a.papercut.created || '')) || a.index - b.index);
    const open = all.filter(({ papercut }) => papercut.status === 'open');
    const shown = app.papercutFilter === 'open' ? open : all;
    const summary = make('div', 'inline-notice');
    append(summary, make('strong', '', `${open.length} open`), make('span', '', app.papercutFilter === 'open' ? 'Showing unresolved friction.' : `Showing all ${all.length} reports.`));
    append(content, summary);
    if (!shown.length) {
      append(content, stateBlock('empty', app.papercutFilter === 'open' ? 'No open papercuts' : 'No papercuts recorded', app.papercutFilter === 'open'
        ? 'Nothing unresolved is present in this snapshot.'
        : 'The snapshot contains no papercut rows.'));
      return;
    }

    const wrap = make('div', 'papercut-table-wrap');
    const table = make('table', 'data-table');
    append(table, make('caption', '', app.papercutFilter === 'open' ? 'Open papercuts, newest first.' : 'All papercuts, newest first.'));
    const head = make('thead');
    const headerRow = make('tr');
    ['Reported', 'By', 'Friction', 'Status', 'Card'].forEach((label) => append(headerRow, make('th', '', label)));
    append(head, headerRow);
    const body = make('tbody');
    shown.forEach(({ papercut }) => {
      const row = make('tr');
      append(row, make('td', 'papercut-time', formatDateTime(papercut.created)));
      append(row, make('td', 'papercut-author', stringValue(papercut.by, 'unknown')));
      append(row, make('td', 'papercut-text', stringValue(papercut.text, 'No text recorded.')));
      const statusCell = make('td');
      append(statusCell, make('span', `status-chip status-chip--${statusToken(papercut.status)}`, stringValue(papercut.status, 'unknown')));
      append(row, statusCell);
      const cardCell = make('td');
      const linked = findCardSummary(papercut.cardId);
      if (papercut.cardId) {
        const ref = cardRef(linked) || String(papercut.cardId);
        append(cardCell, makeFocusButton(linked ? cardNumber(linked) : String(papercut.cardId), 'linked-card', ref, () => openFocus(ref, null, true), {
          'aria-label': `Open ${linked ? cardNumber(linked) : papercut.cardId} in read-only focus`,
        }));
      } else {
        append(cardCell, make('span', 'papercut-time', '—'));
      }
      append(row, cardCell);
      append(body, row);
    });
    append(table, head, body);
    append(wrap, table);
    append(content, wrap);
  }

  function phaseEntries() {
    const counts = app.state?.counts?.byPhase;
    if (!counts || typeof counts !== 'object' || Array.isArray(counts)) return [];
    const ordered = [];
    const seen = new Set();
    (app.state?.phases || []).forEach((phase) => {
      const id = typeof phase === 'object' ? phase.id : phase;
      if (id !== null && id !== undefined && Object.prototype.hasOwnProperty.call(counts, id)) {
        ordered.push([String(id), counts[id]]);
        seen.add(String(id));
      }
    });
    Object.keys(counts).forEach((id) => {
      if (!seen.has(id)) ordered.push([id, counts[id]]);
    });
    return ordered;
  }

  function renderPhaseSection(parent) {
    const section = make('section', 'status-section');
    append(section, make('h3', '', 'Phase counts'));
    const entries = phaseEntries();
    if (!entries.length) {
      appendEmptyDetail(section, 'counts.byPhase is not present in this snapshot.');
      append(parent, section);
      return;
    }
    const wrap = make('div', 'phase-table-wrap');
    const table = make('table', 'data-table phase-table');
    append(table, make('caption', '', 'Card counts by projected phase.'));
    const head = make('thead');
    const row = make('tr');
    append(row, make('th', '', 'Phase'), make('th', '', 'Count'));
    append(head, row);
    const body = make('tbody');
    entries.forEach(([id, count]) => {
      const phaseRow = make('tr');
      append(phaseRow, make('td', '', phaseLabel(id)), make('td', '', numberValue(count, 0)));
      append(body, phaseRow);
    });
    append(table, head, body);
    append(wrap, table);
    append(section, wrap);
    append(parent, section);
  }

  function statusLaneEntries() {
    const lanes = app.state?.lanes;
    if (lanes && typeof lanes === 'object' && !Array.isArray(lanes)) {
      return Object.entries(lanes)
        .filter(([id]) => id !== 'done')
        .sort(([, a], [, b]) => numberValue(a?.rank) - numberValue(b?.rank))
        .map(([id, lane]) => [id, stringValue(lane?.label, id)]);
    }
    return STATUS_LANES.map((lane) => [lane, STATUS_LANE_LABELS[lane] || lane]);
  }

  function laneCardList(parent, lane, cards, label = STATUS_LANE_LABELS[lane] || lane) {
    const group = make('section', 'lane-group');
    group.dataset.lane = lane;
    const heading = make('div', 'lane-group__head');
    append(heading, make('h4', '', label));
    append(heading, make('span', 'lane-group__count', `${cards.length} card${cards.length === 1 ? '' : 's'}`));
    append(group, heading);
    if (!cards.length) {
      append(group, make('p', 'detail-copy detail-copy--muted', 'No cards projected into this lane.'));
    } else {
      const list = make('ul', 'lane-card-list');
      [...cards].sort(compareCards).forEach((card) => {
        const item = make('li');
        append(item, make('span', 'lane-card-list__number', cardNumber(card)));
        const ref = cardRef(card);
        append(item, ref
          ? makeFocusButton(cardTitle(card), 'focus-choice lane-card-list__title', ref, () => openFocus(ref, null, true), {
            'aria-label': `Open ${cardNumber(card)} in read-only focus`,
          })
          : make('span', 'lane-card-list__title', cardTitle(card)));
        append(list, item);
      });
      append(group, list);
    }
    append(parent, group);
  }

  function renderLaneSection(parent) {
    const section = make('section', 'status-section');
    append(section, make('h3', '', 'Cards by lane'));
    const cards = snapshotCards().filter((card) => !isClosed(card));
    const list = make('div', 'status-lane-list');
    const entries = statusLaneEntries();
    entries.forEach(([lane, label]) => laneCardList(list, lane, cards.filter((card) => laneToken(card) === lane), label));
    const known = new Set(entries.map(([lane]) => lane));
    const other = cards.filter((card) => !known.has(laneToken(card)));
    if (other.length) laneCardList(list, 'other', other);
    append(section, list);
    append(parent, section);
  }

  function lintFindings(payload) {
    if (Array.isArray(payload)) return payload;
    if (Array.isArray(payload?.findings)) return payload.findings;
    return [];
  }

  function renderDiagnostics(parent) {
    const section = make('section', 'status-section');
    append(section, make('h3', '', 'Snapshot checks'));
    if (!app.status.requested) {
      append(section, make('p', 'detail-copy detail-copy--muted', app.view === 'status'
        ? 'Checks begin when this view opens.'
        : 'Open Status to request lint and version data.'));
      append(parent, section);
      return;
    }
    if (app.status.loading && !app.status.lint && !app.status.version) {
      append(section, stateBlock('loading', 'Loading checks', 'Reading GET /api/lint and GET /api/version.'));
      append(parent, section);
      return;
    }

    const lintWrap = make('div');
    append(lintWrap, make('p', 'detail-copy', 'Lint'));
    if (app.status.lintError) {
      append(lintWrap, make('p', 'detail-copy detail-copy--muted', `Unavailable: ${app.status.lintError.message}`));
    } else {
      const findings = lintFindings(app.status.lint);
      if (!findings.length) append(lintWrap, make('p', 'check-good', 'No findings in this snapshot.'));
      else {
        const list = make('ul', 'diagnostic-list');
        findings.forEach((finding) => {
          const item = make('li', 'is-finding');
          append(item, make('span', 'diagnostic-list__meta', `${stringValue(finding.rule, 'finding')} · ${stringValue(finding.ref, 'unscoped')}`));
          append(item, make('span', '', stringValue(finding.msg || finding.detail, 'Finding has no message.')));
          append(list, item);
        });
        append(lintWrap, list);
      }
    }
    append(section, lintWrap);

    const versionWrap = make('div');
    append(versionWrap, make('p', 'detail-copy', 'Immutable snapshot identity'));
    if (app.status.versionError) {
      append(versionWrap, make('p', 'detail-copy detail-copy--muted', `Unavailable: ${app.status.versionError.message}`));
    } else if (app.status.version) {
      const version = app.status.version;
      append(versionWrap, make('p', 'check-good', 'Identity belongs to this immutable snapshot.'));
      const grid = make('dl', 'version-grid');
      const values = [
        ['mode', version.mode],
        ['captured at', version.capturedAt],
        ['revision', version.revision ?? currentRevision()],
        ['identity', version.identity],
      ];
      values.forEach(([label, value]) => {
        const cell = make('div');
        append(cell, make('dt', '', label), make('dd', '', stringValue(value)));
        append(grid, cell);
      });
      append(versionWrap, grid);
    } else append(versionWrap, stateBlock('empty', 'Snapshot identity unavailable', 'The version response contained no identity record.'));
    append(section, versionWrap);
    append(parent, section);
  }

  function ensureDiagnostics() {
    if (!app.state || app.status.requested) return;
    app.status.requested = true;
    app.status.loading = true;
    renderStatus();
    Promise.allSettled([
      getJSON('/api/lint'),
      getJSON('/api/version'),
    ]).then(([lintResult, versionResult]) => {
      if (lintResult.status === 'fulfilled') app.status.lint = lintResult.value;
      else app.status.lintError = lintResult.reason instanceof Error ? lintResult.reason : new Error(String(lintResult.reason));
      if (versionResult.status === 'fulfilled') app.status.version = versionResult.value;
      else app.status.versionError = versionResult.reason instanceof Error ? versionResult.reason : new Error(String(versionResult.reason));
      app.status.loading = false;
      renderStatus();
    });
  }

  function renderStatus() {
    const content = byId('status-content');
    if (!content) return;
    clear(content);
    if (!app.state) {
      append(content, app.stateError
        ? stateBlock('error', 'Snapshot unavailable', app.stateError.message, 'Retry GET /api/state', () => loadSnapshot())
        : stateBlock('loading', 'Loading status', 'Reading the projected board.'));
      return;
    }
    const layout = make('div', 'status-layout');
    const left = make('div', 'status-column');
    const right = make('div', 'status-column');
    renderPhaseSection(left);
    renderDiagnostics(left);
    renderLaneSection(right);
    append(layout, left, right);
    append(content, layout);
  }

  function render() {
    renderChrome();
    renderTabs();
    renderBoard();
    renderFocusPanel();
    renderPapercuts();
    renderStatus();
    if (app.dialogOpen) renderFocusDialog();
  }

  function scheduleFallbackRefresh() {
    clearTimeout(app.fallbackTimer);
    app.fallbackTimer = setTimeout(async () => {
      app.fallbackTimer = null;
      await loadSnapshot({ quiet: true });
      startStream();
    }, 30000);
  }

  function startStream() {
    if (app.stream || app.streamStarted) return;
    app.streamStarted = true;
    if (!('EventSource' in window)) {
      app.streamStarted = false;
      scheduleFallbackRefresh();
      return;
    }
    let source;
    try {
      // The server sends an initial state event. Close after that one event;
      // the fallback refresh keeps this shadow usable without a live socket.
      source = new EventSource('/api/stream');
    } catch {
      app.streamStarted = false;
      scheduleFallbackRefresh();
      return;
    }
    app.stream = source;
    let finished = false;
    const finish = () => {
      if (finished) return;
      finished = true;
      source.close();
      app.stream = null;
      app.streamStarted = false;
    };
    source.addEventListener('state', (event) => {
      try {
        const next = JSON.parse(event.data);
        const previousRevision = currentRevision();
        app.state = next;
        invalidateRevisionCaches(previousRevision, currentRevision());
        app.stateLoading = false;
        app.stateError = null;
        app.connection = 'One-shot stream received';
        render();
      } catch (error) {
        app.stateError = error instanceof Error ? error : new Error(String(error));
        render();
      } finally {
        finish();
        scheduleFallbackRefresh();
      }
    });
    source.onerror = () => {
      finish();
      scheduleFallbackRefresh();
    };
  }

  function handleKeydown(event) {
    const target = event.target;
    const tag = target?.tagName?.toLowerCase();
    if (app.dialogOpen) {
      if (event.key === 'Escape') {
        event.preventDefault();
        closeFocusDialog();
      } else if (event.key === 'Tab') {
        trapDialogFocus(event);
      }
      return;
    }
    if (event.key === 'Escape') {
      if (app.view === 'focus' && app.focus.ref) {
        event.preventDefault();
        clearFocus();
        return;
      }
    }
    if (target?.isContentEditable || tag === 'input' || tag === 'textarea' || tag === 'select') return;

    if (target?.getAttribute?.('role') === 'tab' && (event.key === 'ArrowRight' || event.key === 'ArrowLeft')) {
      event.preventDefault();
      const index = VIEWS.indexOf(target.dataset.view);
      const nextIndex = (index + (event.key === 'ArrowRight' ? 1 : -1) + VIEWS.length) % VIEWS.length;
      activateView(VIEWS[nextIndex], { focusTab: true });
      return;
    }

    if (event.key >= '1' && event.key <= '4') {
      event.preventDefault();
      activateView(VIEWS[Number(event.key) - 1], { focusTab: true });
    }
  }

  function bindEvents() {
    VIEWS.forEach((view) => byId(`tab-${view}`)?.addEventListener('click', () => activateView(view)));
    bindBoardControls();
    byId('refresh-button')?.addEventListener('click', () => loadSnapshot());
    byId('closed-toggle')?.addEventListener('click', () => {
      app.showClosed = !app.showClosed;
      render();
      if (app.showClosed) ensureClosed();
    });
    byId('papercut-filter')?.addEventListener('click', () => {
      app.papercutFilter = app.papercutFilter === 'open' ? 'all' : 'open';
      renderPapercuts();
      renderChrome();
    });
    window.addEventListener('hashchange', () => activateView(readView(), { writeHash: false }));
    document.addEventListener('keydown', handleKeydown);

    const dialog = byId('focus-dialog');
    dialog?.addEventListener('cancel', (event) => {
      event.preventDefault();
      closeFocusDialog();
    });
    dialog?.addEventListener('click', (event) => {
      if (event.target === dialog) closeFocusDialog();
    });
    dialog?.addEventListener('close', () => {
      if (app.dialogOpen) closeFocusDialog();
      else setDialogBackgroundInert(false);
    });
  }

  function init() {
    bindEvents();
    activateView(app.view, { writeHash: false });
    loadSnapshot().finally(startStream);
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init, { once: true });
  else init();
})();
