const VERDICTS = new Set(['win', 'parity', 'loss', 'unmeasured']);
const TIERS = ['aot', 'run', 'dev', 'cold', 'warm'];
const PEER_ORDER = [
  'rust', 'python', 'c', 'zig', 'node', 'go', 'java', 'kotlin', 'swift',
  'typescript', 'javascript', 'ruby', 'php', 'lua', 'jet-expert',
];
const UNMEASURED = 'unmeasured';
const GAUNTLET_STATES = new Set(['win', 'parity', 'loss', 'unmeasured', 'n/a']);

const isObject = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const object = (value) => isObject(value) ? value : {};
const array = (value) => Array.isArray(value) ? value : [];
const finite = (value) => typeof value === 'number' && Number.isFinite(value);
const verdict = (value) => VERDICTS.has(value) ? value : null;
const esc = (value) => String(value ?? 'n/a').replace(/[&<>"']/g, (char) => ({
  '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
}[char]));

function peerName(peer) {
  return peer?.peer ?? peer?.language ?? peer?.name ?? null;
}

function tierStatus(item, peer) {
  if (!item) return UNMEASURED;
  if (item.status === 'not_applicable') return 'n/a';
  if (finite(item.ratio)) return ratioVerdict(item.ratio, peer);
  return verdict(item.verdict) ?? UNMEASURED;
}

function selectTier(tiers, names, peer, includeMissing = false) {
  const rank = { unmeasured: 0, loss: 1, parity: 2, win: 3, 'n/a': 4 };
  const candidates = names.map((name, index) => {
    const item = isObject(tiers[name]) ? tiers[name] : null;
    if (!item && !includeMissing) return null;
    return { name, index, item: item ?? {}, state: tierStatus(item, peer) };
  }).filter(Boolean);
  candidates.sort((left, right) => (rank[left.state] ?? 0) - (rank[right.state] ?? 0) || left.index - right.index);
  return candidates[0] ?? null;
}

function metricValue(peer, metric) {
  const comparison = object(peer?.metric_comparisons?.[metric] ?? peer?.metrics?.[metric]);
  const tiers = object(comparison.tiers);
  const declared = Object.keys(tiers);
  const required = array(peer?.required_tiers);
  const names = required.length
    ? [...new Set(required)]
    : [...TIERS.filter((name) => declared.includes(name)), ...declared.filter((name) => !TIERS.includes(name))];
  const selected = selectTier(tiers, names, peerName(peer), required.length > 0);
  if (selected) return { tier: selected.name, values: selected.item, comparison };
  if (finite(comparison.ratio) || comparison.status === 'measured') {
    return { tier: null, values: comparison, comparison };
  }
  return { tier: null, values: {}, comparison };
}

function ratioVerdict(ratio, peer) {
  if (!finite(ratio)) return UNMEASURED;
  if (String(peer ?? '').replace(/-expert$/, '') === 'rust') {
    if (ratio < 1) return 'win';
    if (ratio <= 1.05) return 'parity';
    return 'loss';
  }
  return ratio < 1 ? 'win' : 'loss';
}

function reduce(values) {
  const comparable = values.filter((value) => value !== 'n/a');
  if (!comparable.length) return 'n/a';
  if (comparable.some((value) => value === UNMEASURED || !VERDICTS.has(value))) return UNMEASURED;
  if (comparable.includes('loss')) return 'loss';
  if (comparable.includes('parity')) return 'parity';
  return 'win';
}

function peerMetricVerdict(peer, metric) {
  if (peer?.applicable === false) return 'n/a';
  const selected = metricValue(peer, metric);
  const comparison = selected.comparison;
  const tiers = object(comparison.tiers);
  const required = array(peer?.required_tiers).length ? peer.required_tiers
    : Object.keys(tiers).filter((tier) => ['aot', 'run'].includes(tier));
  const names = required.length ? required : (selected.tier ? [selected.tier] : []);
  if (!names.length) {
    if (comparison.status === 'not_applicable' || comparison.applicability === 'not_applicable') return 'n/a';
    return verdict(comparison.verdict) ?? verdict(peer?.metric_verdicts?.[metric]) ?? verdict(peer?.verdict) ?? UNMEASURED;
  }
  const values = names.map((tier) => {
    const item = object(tiers[tier]);
    if (item.status === 'not_applicable') return 'n/a';
    if (finite(item.ratio)) return ratioVerdict(item.ratio, peerName(peer));
    return verdict(item.verdict) ?? UNMEASURED;
  });
  return reduce(values);
}

function sortPeers(names) {
  return [...new Set(names.filter(Boolean))].sort((left, right) => {
    const leftRank = PEER_ORDER.indexOf(left);
    const rightRank = PEER_ORDER.indexOf(right);
    if (leftRank >= 0 || rightRank >= 0) {
      return (leftRank < 0 ? PEER_ORDER.length : leftRank) - (rightRank < 0 ? PEER_ORDER.length : rightRank)
        || left.localeCompare(right);
    }
    return left.localeCompare(right);
  });
}

function rowPeer(cell, peerNameValue, hasPeers) {
  const peer = array(cell?.peers).find((item) => peerName(item) === peerNameValue) ?? null;
  const metric = cell?.primary_metric ?? null;
  if (!peer) {
    const state = hasPeers ? 'n/a' : UNMEASURED;
    return {
      peer: peerNameValue, verdict: state, ratio: null, tier: null, values: {},
      detail: { cell, peer: { peer: peerNameValue }, metric, values: {}, verdict: state },
    };
  }
  const selected = metricValue(peer, metric);
  const values = selected.values ?? {};
  const state = peerMetricVerdict(peer, metric);
  return {
    peer: peerNameValue,
    verdict: state,
    ratio: finite(values.ratio) ? values.ratio : null,
    tier: selected.tier,
    values,
    detail: { cell, peer, metric, tier: selected.tier, values, verdict: state },
  };
}

function axisPeer(axis, peerNameValue) {
  const peer = object(axis?.comparisons?.[peerNameValue]);
  const phases = ['cold', 'warm'].map((phase) => ({ phase, value: object(peer[phase]) }));
  const phaseValues = Object.fromEntries(phases.map(({ phase, value }) => [phase, value]));
  const selectedTier = selectTier(phaseValues, ['cold', 'warm'], peerNameValue, true);
  const selected = selectedTier
    ? { phase: selectedTier.name, value: selectedTier.item }
    : phases[0];
  const values = selected.value;
  const phaseVerdicts = phases.map(({ value }) => tierStatus(value, peerNameValue));
  const hasComparison = Object.keys(peer).length > 0;
  const state = hasComparison ? reduce(phaseVerdicts) : 'n/a';
  return {
    peer: peerNameValue,
    verdict: state,
    ratio: finite(values.ratio) ? values.ratio : null,
    tier: selected.phase,
    values,
    detail: {
      axis,
      peer: { ...peer, peer: peerNameValue },
      metric: axis?.metric ?? null,
      tier: selected.phase,
      values,
      verdict: state,
    },
  };
}

function cellRow(cell) {
  const hasPeers = array(cell?.peers).length > 0;
  const names = array(cell?.peers).map(peerName);
  return {
    kind: 'cell',
    id: cell?.id ?? null,
    domain: cell?.domain ?? 'Other',
    entry: cell?.entry ?? null,
    mode: cell?.mode ?? null,
    primary_metric: cell?.primary_metric ?? null,
    verdict: verdict(cell?.verdict) ?? UNMEASURED,
    cell,
    peerNames: names,
    peers: Object.fromEntries(names.map((name) => [name, rowPeer(cell, name, hasPeers)])),
  };
}

function axisRow(id, axis) {
  const names = Object.keys(object(axis?.comparisons));
  const peers = Object.fromEntries(names.map((name) => [name, axisPeer(axis, name)]));
  return {
    kind: 'axis',
    id: `axis:${id}`,
    domain: 'Axes',
    entry: null,
    mode: id,
    primary_metric: axis?.metric ?? null,
    verdict: reduce(Object.values(peers).map((peer) => peer.verdict)),
    axis: { ...axis, id },
    peerNames: names,
    peers,
  };
}

// Matrix order from gauntlet/matrix.json: the table is one fixed grid, so
// rows keep their domain order instead of sorting by verdict or id.
const DOMAIN_ORDER = [
  'text', 'formats', 'numerics', 'files', 'time', 'concurrency', 'cli', 'webfront', 'netserv', 'embedded',
];
const domainRank = (domain) => {
  const index = DOMAIN_ORDER.indexOf(String(domain ?? '').toLowerCase());
  return index < 0 ? DOMAIN_ORDER.length : index;
};

// Language rails are the matrix columns; axis comparisons (live reload versus
// vite, nodemon, …) compare tools, so they carry their own column set and sit
// in a labelled band under the cells.
export function projectGauntletMatrix(status) {
  const source = object(status);
  const cells = array(source.cells).map(cellRow).sort((left, right) =>
    domainRank(left.domain) - domainRank(right.domain) || String(left.id).localeCompare(String(right.id)));
  const axisRows = Object.entries(object(source.axes)).map(([id, axis]) => axisRow(id, axis));
  const columns = sortPeers(cells.flatMap((row) => row.peerNames));
  const axisColumns = sortPeers(axisRows.flatMap((row) => row.peerNames));
  return { columns, axisColumns, rows: [...cells, ...axisRows], cellRows: cells, axisRows };
}

// ---- formatting -------------------------------------------------------------
// Every number is three significant figures in a plain unit: no exponents,
// and both sides of a comparison share one unit so they read directly.
const SCALES = Object.freeze({
  seconds: [['s', 1], ['ms', 1e-3], ['µs', 1e-6]],
  ms: [['s', 1e3], ['ms', 1]],
  kb: [['GB', 1e6], ['MB', 1e3], ['kB', 1]],
  bytes: [['GB', 1e9], ['MB', 1e6], ['kB', 1e3], ['B', 1]],
  count: [['', 1]],
});
const METRIC_SCALE = Object.freeze({
  runtime_wall_seconds: 'seconds',
  runtime_first_stdout_seconds: 'seconds',
  cold_build_seconds: 'seconds',
  warm_build_seconds: 'seconds',
  runtime_peak_rss_kb: 'kb',
  binary_bytes: 'bytes',
  source_bytes: 'bytes',
  reload_latency_ms: 'ms',
  cold_reload_latency_ms: 'ms',
  warm_reload_latency_ms: 'ms',
  service_latency_ms_p50: 'ms',
  service_latency_ms_p99: 'ms',
  service_startup_seconds: 'seconds',
});
const METRIC_NAMES = Object.freeze({
  runtime_wall_seconds: 'wall time',
  runtime_first_stdout_seconds: 'first output',
  runtime_peak_rss_kb: 'peak memory',
  cold_build_seconds: 'cold build',
  warm_build_seconds: 'warm build',
  binary_bytes: 'binary size',
  source_bytes: 'source size',
  reload_latency_ms: 'reload latency',
  service_latency_ms_p50: 'latency p50',
  service_latency_ms_p99: 'latency p99',
  memory_safety_findings: 'safety findings',
});

const sig3 = (value) => Number(value.toPrecision(3)).toLocaleString('en-US', { maximumFractionDigits: 12 });

// Pick one unit so the smallest non-zero value shows as at least 1.
function unitFor(kind, values) {
  const scale = SCALES[kind] ?? SCALES.count;
  const floor = Math.min(...values.filter((value) => finite(value) && value > 0).map(Math.abs));
  if (!Number.isFinite(floor)) return scale[scale.length - 1];
  return scale.find(([, factor]) => floor / factor >= 1) ?? scale[scale.length - 1];
}

function formatIn(value, [suffix, factor]) {
  if (!finite(value)) return '—';
  return `${sig3(value / factor)}${suffix ? ` ${suffix}` : ''}`;
}

// Format both sides of a comparison in one shared unit.
export function formatPair(metric, jet, peer) {
  const unit = unitFor(METRIC_SCALE[metric] ?? 'count', [jet, peer]);
  return { jet: formatIn(jet, unit), peer: formatIn(peer, unit), unit: unit[0] };
}

export function formatRatio(value) {
  return finite(value) ? `${sig3(value)}×` : '—';
}

function metricLabel(metric) {
  return METRIC_NAMES[metric] ?? (metric ? String(metric).replaceAll('_', ' ') : 'n/a');
}

const DATE_FORMAT = new Intl.DateTimeFormat('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
const TIME_FORMAT = new Intl.DateTimeFormat('en-US', { hour: '2-digit', minute: '2-digit', hour12: false });

export function formatStamp(iso, date) {
  const parsed = iso ? new Date(iso) : null;
  if (parsed && !Number.isNaN(parsed.getTime())) return `${DATE_FORMAT.format(parsed)} · ${TIME_FORMAT.format(parsed)}`;
  const day = date ? new Date(`${date}T00:00:00`) : null;
  if (day && !Number.isNaN(day.getTime())) return DATE_FORMAT.format(day);
  return 'not measured';
}

// ---- detail card ------------------------------------------------------------
const TIER_ORDER = ['aot', 'run', 'dev', 'cold', 'warm'];
const tierState = (item, peer) => {
  if (!item || !Object.keys(item).length) return UNMEASURED;
  if (item.status === 'not_applicable') return 'n/a';
  if (finite(item.ratio)) return ratioVerdict(item.ratio, peer);
  return verdict(item.verdict) ?? UNMEASURED;
};

function tierRows(detail, peerLabel) {
  const tiers = detail.axis
    ? Object.fromEntries(['cold', 'warm'].map((phase) => [phase, object(detail.peer?.[phase])]))
    : object(object(detail.peer?.metric_comparisons?.[detail.metric] ?? detail.peer?.metrics?.[detail.metric]).tiers);
  const names = [...TIER_ORDER.filter((name) => name in tiers), ...Object.keys(tiers).filter((name) => !TIER_ORDER.includes(name))];
  return names.map((name) => {
    const item = object(tiers[name]);
    const pair = formatPair(detail.metric, item.jet, item.peer);
    return { name, state: tierState(item, peerLabel), pair, ratio: formatRatio(item.ratio), selected: name === detail.tier };
  });
}

function statRows(detail) {
  const stats = object(detail.values?.stats);
  const jet = object(stats.jet);
  const peer = object(stats.peer);
  if (!Object.keys(jet).length && !Object.keys(peer).length) return [];
  const rows = [['best', 'best'], ['median', 'median'], ['mean', 'mean']]
    .filter(([key]) => finite(jet[key]) || finite(peer[key]))
    .map(([key, label]) => ({ label, ...formatPair(detail.metric, jet[key], peer[key]) }));
  if (finite(jet.rss) || finite(peer.rss)) rows.push({ label: 'peak memory', ...formatPair('runtime_peak_rss_kb', jet.rss, peer.rss) });
  if (finite(jet.samples) || finite(peer.samples)) {
    rows.push({ label: 'samples', jet: finite(jet.samples) ? String(jet.samples) : '—', peer: finite(peer.samples) ? String(peer.samples) : '—' });
  }
  return rows;
}

export function buildGauntletTooltip(detail = {}) {
  const cell = object(detail.cell);
  const axis = object(detail.axis);
  const values = object(detail.values);
  const peerLabel = peerName(detail.peer) ?? detail.peer_name ?? 'peer';
  const metric = detail.metric ?? axis.metric ?? cell.primary_metric ?? null;
  const state = GAUNTLET_STATES.has(detail.verdict) ? detail.verdict : tierState(values, peerLabel);
  const title = cell.id ?? (axis.id ? `axis · ${String(axis.id).replaceAll('_', ' ')}` : 'Gauntlet detail');
  const subtitle = [cell.entry, `Jet vs ${peerLabel}`, metricLabel(metric)].filter(Boolean).join(' · ');
  const tiers = tierRows({ ...detail, metric }, peerLabel);
  const stats = statRows({ ...detail, metric });
  const stamp = formatStamp(values.measured_iso ?? cell.measured_iso ?? axis.measured_iso, values.measured_at ?? cell.measured_at ?? axis.measured_at);
  const runId = values.run_id ?? cell.run_id ?? axis.run_id;
  const tierTable = tiers.length ? `<table class="gtip__table">
      <thead><tr><th>tier</th><th>Jet</th><th>${esc(peerLabel)}</th><th>ratio</th></tr></thead>
      <tbody>${tiers.map((row) => `<tr class="gtip__tier gtip__tier--${row.state === 'n/a' ? 'na' : row.state}${row.selected ? ' gtip__tier--selected' : ''}">
        <th>${esc(row.name)}</th><td>${esc(row.pair.jet)}</td><td>${esc(row.pair.peer)}</td><td class="gtip__ratio">${esc(row.ratio)}</td></tr>`).join('')}</tbody>
    </table>` : '';
  const statTable = stats.length ? `<table class="gtip__table gtip__table--stats">
      <thead><tr><th>${esc(detail.tier ?? 'samples')} tier</th><th>Jet</th><th>${esc(peerLabel)}</th></tr></thead>
      <tbody>${stats.map((row) => `<tr><th>${esc(row.label)}</th><td>${esc(row.jet)}</td><td>${esc(row.peer)}</td></tr>`).join('')}</tbody>
    </table>` : '';
  return `<div class="gtip" role="tooltip">
    <div class="gtip__head">
      <div><strong>${esc(title)}</strong><small>${esc(subtitle)}</small></div>
      <span class="gtip__state gtip__state--${state === 'n/a' ? 'na' : state}">${esc(state)}</span>
    </div>
    ${tierTable}${statTable}
    <div class="gtip__foot"><span>${esc(stamp)}</span>${runId ? `<span>run ${esc(runId)}</span>` : ''}</div>
  </div>`;
}

export { metricLabel, metricValue, ratioVerdict };
