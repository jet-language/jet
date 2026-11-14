const VERDICTS = new Set(['win', 'loss', 'unmeasured']);
const TIERS = ['aot', 'run', 'dev', 'cold', 'warm'];
const PEER_ORDER = [
  'rust', 'c', 'zig', 'go', 'python', 'js',
  'java', 'kotlin', 'swift', 'typescript', 'ruby', 'php', 'lua',
];
const UNMEASURED = 'unmeasured';
const GAUNTLET_STATES = new Set(['win', 'loss', 'unmeasured', 'n/a']);
const AOT_PEERS = new Set(['rust', 'c', 'zig', 'go']);
const DYNAMIC_PEERS = new Set(['python', 'js']);
// Old receipts retain their original rail identity; the chart groups languages.
const languageName = (name) => name === 'node' || name === 'javascript' ? 'js' : name;
const comparisonTiers = (name) => AOT_PEERS.has(name) ? ['aot']
  : DYNAMIC_PEERS.has(name) ? ['run', 'dev'] : null;

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

function tierStatus(item) {
  if (!item) return UNMEASURED;
  if (item.status === 'not_applicable') return 'n/a';
  if (item.status && !['measured', 'ok'].includes(item.status)) return UNMEASURED;
  if (finite(item.ratio)) return ratioVerdict(item.ratio);
  return verdict(item.verdict) ?? UNMEASURED;
}

function selectTier(tiers, names, includeMissing = false) {
  const rank = { unmeasured: 0, loss: 1, win: 2, 'n/a': 3 };
  const candidates = names.map((name, index) => {
    const item = isObject(tiers[name]) ? tiers[name] : null;
    if (!item && !includeMissing) return null;
    return { name, index, item: item ?? {}, state: tierStatus(item) };
  }).filter(Boolean);
  candidates.sort((left, right) => (rank[left.state] ?? 0) - (rank[right.state] ?? 0) || left.index - right.index);
  return candidates[0] ?? null;
}

function metricValue(peer, metric, requiredTiers = null) {
  const comparison = object(peer?.metric_comparisons?.[metric] ?? peer?.metrics?.[metric]);
  const tiers = object(comparison.tiers);
  const declared = Object.keys(tiers);
  const required = requiredTiers ?? array(peer?.required_tiers);
  const names = required.length
    ? [...new Set(required)]
    : [...TIERS.filter((name) => declared.includes(name)), ...declared.filter((name) => !TIERS.includes(name))];
  const selected = selectTier(tiers, names, required.length > 0);
  if (selected) return { tier: selected.name, values: selected.item, comparison };
  if (finite(comparison.ratio) || comparison.status === 'measured') {
    return { tier: null, values: comparison, comparison };
  }
  return { tier: null, values: {}, comparison };
}

// Every peer, Rust included, shares one comparator: Jet/peer below 1 wins.
function ratioVerdict(ratio) {
  if (!finite(ratio)) return UNMEASURED;
  return ratio < 1 ? 'win' : 'loss';
}

function reduce(values) {
  const comparable = values.filter((value) => value !== 'n/a');
  if (!comparable.length) return 'n/a';
  if (comparable.some((value) => value === UNMEASURED || !VERDICTS.has(value))) return UNMEASURED;
  if (comparable.includes('loss')) return 'loss';
  return 'win';
}

function peerMetricVerdict(peer, metric, requiredTiers = null) {
  if (peer?.applicable === false) return 'n/a';
  const selected = metricValue(peer, metric, requiredTiers);
  const comparison = selected.comparison;
  const tiers = object(comparison.tiers);
  const required = requiredTiers ?? (array(peer?.required_tiers).length ? peer.required_tiers
    : Object.keys(tiers).filter((tier) => ['aot', 'run'].includes(tier)));
  const names = required.length ? required : (selected.tier ? [selected.tier] : []);
  if (!names.length) {
    if (comparison.status === 'not_applicable' || comparison.applicability === 'not_applicable') return 'n/a';
    return verdict(comparison.verdict) ?? verdict(peer?.metric_verdicts?.[metric]) ?? verdict(peer?.verdict) ?? UNMEASURED;
  }
  const values = names.map((tier) => tierStatus(tiers[tier]));
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

function rowPeer(cell, peerNameValue, records) {
  const peer = records.find((item) => peerName(item) === peerNameValue) ?? records[0];
  const metric = cell?.primary_metric ?? null;
  const requiredTiers = comparisonTiers(peerNameValue);
  const selected = metricValue(peer, metric, requiredTiers);
  const values = selected.values ?? {};
  const state = peerMetricVerdict(peer, metric, requiredTiers);
  const detail = { cell, peer, records, peer_name: peerNameValue, metric, tier: selected.tier, values, verdict: state, requiredTiers };
  const samples = requiredTiers?.map((tier) => {
    const item = object(selected.comparison.tiers?.[tier]);
    const state = peer.applicable === false ? 'n/a' : tierStatus(selected.comparison.tiers?.[tier]);
    return { tier, values: item, verdict: state, ratio: state === 'unmeasured' || state === 'n/a' ? null : item.ratio,
      detail: { ...detail, tier, values: item, verdict: state } };
  });
  return {
    peer: peerNameValue,
    verdict: state,
    ratio: state === UNMEASURED || state === 'n/a' ? null : (finite(values.ratio) ? values.ratio : null),
    tier: selected.tier,
    values,
    samples,
    detail,
  };
}

function axisPeer(axis, peerNameValue) {
  const peer = object(axis?.comparisons?.[peerNameValue]);
  const phases = ['cold', 'warm'].map((phase) => ({ phase, value: object(peer[phase]) }));
  const phaseValues = Object.fromEntries(phases.map(({ phase, value }) => [phase, value]));
  const selectedTier = selectTier(phaseValues, ['cold', 'warm'], true);
  const selected = selectedTier
    ? { phase: selectedTier.name, value: selectedTier.item }
    : phases[0];
  const values = selected.value;
  const phaseVerdicts = phases.map(({ value }) => tierStatus(value));
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
  const records = new Map();
  for (const peer of array(cell?.peers)) {
    const name = languageName(peerName(peer));
    if (!name || name.endsWith('-expert')) continue;
    if (!records.has(name)) records.set(name, []);
    records.get(name).push(peer);
  }
  const names = [...records.keys()];
  const peers = Object.fromEntries(names.map((name) => [name, rowPeer(cell, name, records.get(name))]));
  const required = names.filter((name) => comparisonTiers(name));
  return {
    kind: 'cell',
    id: cell?.id ?? null,
    domain: cell?.domain ?? 'Other',
    entry: cell?.entry ?? null,
    mode: cell?.mode ?? null,
    primary_metric: cell?.primary_metric ?? null,
    verdict: required.length ? reduce(required.map((name) => peers[name].verdict)) : UNMEASURED,
    cell,
    peerNames: names,
    peers,
  };
}

function axisRow(id, axis) {
  const recordedNames = Object.keys(object(axis?.comparisons));
  const names = id === 'live_reload'
    ? [...new Set(['bun', recordedNames.includes('node') ? 'node' : 'nodemon', 'vite', ...recordedNames])]
    : recordedNames;
  const peers = Object.fromEntries(names.map((name) => {
    const projected = axisPeer(axis, name);
    projected.detail.reference = id === 'live_reload' && !['node', 'nodemon', 'bun', 'vite'].includes(name);
    if (id === 'live_reload' && !projected.detail.reference && !axis?.comparisons?.[name]) {
      projected.verdict = UNMEASURED;
      projected.detail.verdict = UNMEASURED;
    }
    return [name, projected];
  }));
  return {
    kind: 'axis',
    id: `axis:${id}`,
    domain: 'Axes',
    entry: null,
    mode: id,
    primary_metric: axis?.metric ?? null,
    verdict: reduce(Object.values(peers).filter((peer) => !peer.detail.reference).map((peer) => peer.verdict)),
    axis: { ...axis, id },
    peerNames: names,
    peers,
  };
}

// Matrix order from Tools/gauntlet/matrix.json: the table is one fixed grid, so
// rows keep their domain order instead of sorting by verdict or id.
const DOMAIN_ORDER = [
  'text', 'formats', 'numerics', 'files', 'time', 'concurrency', 'cli', 'webfront', 'netserv', 'embedded',
];
const domainRank = (domain) => {
  const index = DOMAIN_ORDER.indexOf(String(domain ?? '').toLowerCase());
  return index < 0 ? DOMAIN_ORDER.length : index;
};

// Execution groups select samples from immutable receipts, not their old
// aggregate verdicts. Tool workflows keep a separate column set.
export function projectGauntletMatrix(status) {
  const source = object(status);
  const cells = array(source.cells).map(cellRow).sort((left, right) =>
    domainRank(left.domain) - domainRank(right.domain) || String(left.id).localeCompare(String(right.id)));
  const axisRows = Object.entries(object(source.axes)).map(([id, axis]) => axisRow(id, axis));
  const names = sortPeers(cells.flatMap((row) => row.peerNames));
  const groups = [
    { label: 'AOT', columns: names.filter((name) => AOT_PEERS.has(name)) },
    { label: 'Run / dev', columns: names.filter((name) => DYNAMIC_PEERS.has(name)) },
    { label: 'Reference', columns: names.filter((name) => !comparisonTiers(name)) },
  ].filter((group) => group.columns.length);
  const columns = groups.flatMap((group) => group.columns);
  const axisColumns = sortPeers(axisRows.flatMap((row) => row.peerNames));
  const rows = [...cells, ...axisRows];
  const summary = { win: 0, loss: 0, unmeasured: 0 };
  for (const row of rows) if (row.verdict in summary) summary[row.verdict]++;
  return { columns, groups, axisColumns, rows, cellRows: cells, axisRows, summary };
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
const tierState = tierStatus;

function tierRows(detail, peerLabel) {
  const records = detail.axis ? [detail.peer] : (detail.records ?? [detail.peer]);
  return records.flatMap((record) => {
    const tiers = detail.axis
      ? Object.fromEntries(['cold', 'warm'].map((phase) => [phase, object(record?.[phase])]))
      : object(object(record?.metric_comparisons?.[detail.metric] ?? record?.metrics?.[detail.metric]).tiers);
    const declared = [...new Set([...Object.keys(tiers), ...array(detail.requiredTiers)])];
    const names = [...TIER_ORDER.filter((name) => declared.includes(name)), ...declared.filter((name) => !TIER_ORDER.includes(name))];
    return names.map((name) => {
      const item = object(tiers[name]);
      const pair = formatPair(detail.metric, item.jet, item.peer);
      const reference = detail.reference || (detail.requiredTiers && !detail.requiredTiers.includes(name)) || record !== detail.peer;
      const label = `${records.length > 1 ? `${peerName(record)} · ` : ''}${name}${reference ? ' · reference' : ''}`;
      return { name: label, state: tierState(tiers[name], peerLabel), pair, ratio: formatRatio(item.ratio),
        selected: record === detail.peer && name === detail.tier };
    });
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
  const peerLabel = detail.peer_name ?? peerName(detail.peer) ?? 'peer';
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
    ${peerName(detail.peer) === 'node' ? '<div class="gtip__foot">JavaScript · Node runtime</div>' : ''}
    ${peerLabel === 'nodemon' ? '<div class="gtip__foot">Node workflow · nodemon watcher</div>' : ''}
    ${detail.reference ? '<div class="gtip__foot">Non-blocking reference comparison</div>' : ''}
    ${tierTable}${statTable}
    <div class="gtip__foot"><span>${esc(stamp)}</span>${runId ? `<span>run ${esc(runId)}</span>` : ''}</div>
  </div>`;
}

export { metricLabel, metricValue, ratioVerdict };

// ---- standings --------------------------------------------------------------
// A results file's `standings` section (Tools/gauntlet/harness/standings.mjs):
// every comparison entry against every peer and the hand-written asm ceiling
// on startup, run wall, run cycles, peak RSS and size. Every peer's measured
// value is a bar Jet must beat, the asm ceiling a marked bar, and Jet a bar
// once a run measured it; until then the entry reads "Jet pending". Once Jet
// is measured each peer bar carries Jet/peer: below 1.00 passes the peer
// floor, at most 1.00 meets the ceiling, and the gap is ratio − 1. A row or
// cell with no value keeps its status and reason. The Gauntlet page and
// `tower gauntlet standings` render this one projection.
export const STANDINGS_SCHEMA = 'jet.gauntlet.standings.v1';
const STANDING_PEER_LABELS = Object.freeze({
  rust: 'Rust', c: 'C', zig: 'Zig', go: 'Go', python: 'Python', bun: 'Bun', node: 'Node', vite: 'Vite', lua: 'Lua', luajit: 'LuaJIT', asm: 'asm ceiling',
});
const STANDING_UNIT_SCALES = Object.freeze({
  s: SCALES.seconds,
  ms: SCALES.ms,
  kB: SCALES.kb,
  bytes: SCALES.bytes,
  cycles: [['G', 1e9], ['M', 1e6], ['k', 1e3], ['', 1]],
});
const standingPeerLabel = (peer) => STANDING_PEER_LABELS[peer] ?? String(peer);
const standingWords = (value) => String(value).replaceAll('_', ' ');
const count = (value) => finite(value) ? value : 0;

// One unit for a set of values: the largest step that keeps the smallest
// positive value at least 1.
function standingFormatter(unit, values) {
  const scale = STANDING_UNIT_SCALES[unit] ?? SCALES.count;
  const floor = Math.min(...values.filter((value) => finite(value) && value > 0));
  const chosen = Number.isFinite(floor) ? scale.find(([, factor]) => floor / factor >= 1) ?? scale.at(-1) : scale.at(-1);
  const suffix = unit === 'cycles' ? `${chosen[0]} cycles`.trim() : chosen[0];
  return (value) => formatIn(value, [suffix, chosen[1]]);
}
const standingValue = (unit, value) => standingFormatter(unit, [value])(value);

// One side of a size cell's basis (#6834 c6); a dynamically linked binary
// also names the shared libraries it is charged for.
function sizeBasisText(side, basis, detail) {
  const libraries = array(object(detail).libraries);
  if (!basis || libraries.length === 0) return `${side} ${basis ?? '?'}`;
  const bytes = libraries.reduce((total, library) => total + (finite(library?.bytes) ? library.bytes : 0), 0);
  return `${side} ${basis}: ${standingValue('bytes', object(detail).file_bytes)} binary + ${standingValue('bytes', bytes)} in ${libraries.length} shared ${libraries.length === 1 ? 'library' : 'libraries'}`;
}

function standingRatioText(role, ratio, verdict) {
  if (role !== 'ceiling') return `${ratio.toFixed(2)} ${verdict}`;
  const gap = ratio - 1;
  return `${ratio.toFixed(2)} ${verdict} ${gap > 0 ? '+' : gap < 0 ? '−' : '±'}${Math.round(Math.abs(gap) * 100)}%`;
}

function standingCell(role, metric, cell) {
  const source = object(cell);
  const verdict = typeof source.verdict === 'string' ? source.verdict : 'unmeasured';
  const ratio = finite(source.ratio) ? source.ratio : null;
  const jetValue = finite(source.jet) && source.jet > 0 ? source.jet : null;
  const peerValue = finite(source.peer) && source.peer > 0 ? source.peer : null;
  const format = standingFormatter(source.unit, [jetValue, peerValue]);
  const jet = format(jetValue);
  const peer = format(peerValue);
  const jetBasis = metric === 'size' ? source.jet_basis ?? null : null;
  const peerBasis = metric === 'size' ? source.peer_basis ?? null : null;
  const basis = metric === 'size' && (jetBasis || peerBasis)
    ? `${sizeBasisText('Jet', jetBasis, source.jet_detail)}, ${sizeBasisText('peer', peerBasis, source.detail)}` : null;
  return {
    metric,
    verdict,
    ratio,
    gap: role === 'ceiling' && ratio !== null ? ratio - 1 : null,
    unit: source.unit ?? null,
    jetValue,
    peerValue,
    jet,
    peer,
    reason: source.reason ?? null,
    basis,
    jetBasis,
    peerBasis,
    text: ratio !== null ? standingRatioText(role, ratio, verdict) : standingWords(verdict),
    detail: ratio !== null
      ? `Jet ${jet} / ${peer}${basis ? ` (${basis})` : ''}`
      : peerValue !== null ? `${peer}${peerBasis ? ` (${peerBasis})` : ''}` : source.reason ?? standingWords(verdict),
  };
}

function standingRow(peer, item, metrics) {
  const source = object(item);
  const role = source.role === 'ceiling' ? 'ceiling' : 'peer';
  const status = typeof source.status === 'string' ? source.status : 'failed';
  const cells = metrics.map((metric) => standingCell(role, metric.id, object(source.metrics)[metric.id]));
  return {
    peer,
    label: standingPeerLabel(peer),
    role,
    status,
    reason: item ? source.reason ?? null : 'the results file has no row for this peer',
    reference: source.reference ?? null,
    cells,
  };
}

// One metric's bars for an entry: every measured peer and the asm ceiling,
// and Jet once measured, smallest (best) first. Bar length is log scale, so
// a thousandfold spread between asm and an interpreter stays readable. A
// measured row without this metric's value is listed with its reason.
function standingPanel(metric, rows) {
  const bars = [];
  const missing = [];
  let jet = null;
  for (const row of rows) {
    if (row.status !== 'ok') continue;
    const cell = row.cells.find((item) => item.metric === metric.id);
    if (jet === null && cell.jetValue !== null) jet = { value: cell.jetValue, unit: cell.unit, basis: cell.jetBasis };
    if (cell.peerValue === null) {
      missing.push({ peer: row.peer, label: row.label, role: row.role, status: cell.verdict, reason: cell.reason ?? 'no value recorded' });
      continue;
    }
    bars.push({ peer: row.peer, label: row.label, role: row.role, value: cell.peerValue, unit: cell.unit, basis: cell.peerBasis, ratio: cell.ratio, verdict: cell.ratio !== null ? cell.verdict : null, text: cell.ratio !== null ? cell.text : null, detail: cell.detail });
  }
  if (jet) bars.push({ peer: 'jet', label: 'Jet', role: 'jet', value: jet.value, unit: jet.unit, basis: jet.basis, ratio: null, verdict: null, text: null, detail: null });
  bars.sort((left, right) => left.value - right.value);
  const logs = bars.map((bar) => Math.log(bar.value));
  const low = Math.min(...logs);
  const high = Math.max(...logs);
  for (const bar of bars) {
    bar.display = standingValue(bar.unit, bar.value);
    bar.width = high > low ? Math.round((8 + 92 * (Math.log(bar.value) - low) / (high - low)) * 10) / 10 : 100;
  }
  return { metric: metric.id, label: metric.label, what: metric.what, jet: jet !== null, bars, missing };
}

// `payload` is { file, scope, standings } as the server and the CLI read it
// from a results file; null when it holds no standings section.
export function projectGauntletStandings(payload) {
  const section = object(payload?.standings);
  if (section.schema !== STANDINGS_SCHEMA) return null;
  const metrics = array(section.metrics).filter(isObject).map((metric) => ({ id: metric.id, label: metric.label ?? standingWords(metric.id), what: metric.what ?? null }));
  const order = [...array(section.peers), ...(section.ceiling ? [section.ceiling] : [])];
  const entries = array(section.entries).filter(isObject).map((entry) => {
    const rows = order.map((peer) => standingRow(peer, object(entry.peers)[peer] ?? null, metrics));
    const panels = metrics.map((metric) => standingPanel(metric, rows));
    const jet = object(entry.jet);
    return {
      entry: entry.entry,
      mode: entry.mode ?? null,
      jet: { status: jet.status ?? null, reason: jet.reason ?? null, measured: panels.some((panel) => panel.jet) },
      rows,
      panels,
    };
  });
  const summary = object(section.summary);
  const floor = object(summary.floor);
  const ceiling = object(summary.ceiling);
  return {
    file: payload.file ?? null,
    scope: payload.scope ?? null,
    generated: section.generated ?? null,
    peers: order.map((peer) => ({ peer, label: standingPeerLabel(peer) })),
    metrics,
    entries,
    jetMeasured: entries.some((entry) => entry.jet.measured),
    summary: {
      floor: { pass: count(floor.pass), fail: count(floor.fail), other: count(floor.other) },
      ceiling: { meet: count(ceiling.meet), miss: count(ceiling.miss), other: count(ceiling.other) },
    },
    losses: { floor: array(object(section.losses).floor), ceiling: array(object(section.losses).ceiling) },
    sizeBasis: Object.entries(object(section.size_basis)).map(([basis, what]) => ({ basis, what: String(what) })),
  };
}

const standingHeading = (view) => [
  view.file ? `results ${view.file}` : null,
  view.scope ? standingWords(view.scope) : null,
  view.generated ? `generated ${view.generated}` : null,
].filter(Boolean).join(' · ');
const STANDING_BARS = 'every peer value is a bar Jet must beat (lower is better); the asm ceiling is the marked bar';
const STANDING_RULE = 'ratio is Jet / peer: the peer floor passes below 1.00; the asm ceiling is met at 1.00 or below, gap = ratio − 1';
const STANDING_PENDING = 'Jet pending: no Jet row is measured yet, so no ratio or verdict is shown';
const standingRuleLines = (view) => [STANDING_BARS, view.jetMeasured ? STANDING_RULE : STANDING_PENDING];
const standingSummary = ({ floor, ceiling }) =>
  `floor ${floor.pass} pass, ${floor.fail} fail, ${floor.other} other · ceiling ${ceiling.meet} meet, ${ceiling.miss} miss, ${ceiling.other} other (other = uncovered, not applicable, unavailable, failed, mismatched or unmeasured)`;
const SIZE_BASIS_MISSING = 'size basis: not stated by this results file (it predates the #6834 c6 size basis)';
const sizeBasisLines = (view) => view.sizeBasis.length
  ? ['size basis (each size value names its basis):', ...view.sizeBasis.map((item) => `  ${item.basis}: ${item.what}`)]
  : [SIZE_BASIS_MISSING];
// Why an entry has no Jet bars: pending, plus the row's own status when Jet
// was run and did not count. A results file written before not_measured
// records a Jet row the run left out as failed, "not measured: …".
const jetPendingText = (jet) => jet.status && jet.status !== 'ok' && jet.status !== 'not_measured' && !/^not measured/.test(jet.reason ?? '')
  ? `Jet pending (${standingWords(jet.status)}: ${jet.reason ?? 'no reason recorded'})` : 'Jet pending';

// The plain-text table `tower gauntlet standings` prints: one block per entry,
// a line for Jet and one per peer, one column per metric. A cell is the
// value, and Jet/peer with its verdict once Jet is measured. A row that
// measured nothing names its status and reason once instead of five times;
// a measured row's missing value is a note with its reason.
export function gauntletStandingsText(view) {
  const lines = [`Gauntlet standings · ${standingHeading(view)}`, ...standingRuleLines(view), ...(view.jetMeasured ? [standingSummary(view.summary)] : []), ...sizeBasisLines(view)];
  const labelWidth = Math.max(4, ...view.peers.map((peer) => peer.label.length));
  const cellText = (cell) => cell.peerValue === null ? '—' : `${standingValue(cell.unit, cell.peerValue)}${cell.ratio === null ? '' : ` ${cell.text}`}`;
  const jetText = (panel) => panel.bars.find((bar) => bar.role === 'jet')?.display ?? '—';
  const widths = view.metrics.map((metric, index) => Math.max(metric.label.length,
    ...view.entries.flatMap((entry) => [
      ...(entry.jet.measured ? [jetText(entry.panels[index]).length] : []),
      ...entry.rows.filter((row) => row.status === 'ok').map((row) => cellText(row.cells[index]).length),
    ])));
  const line = (label, cells) => `  ${label.padEnd(labelWidth)}  ${cells.map((text, index) => text.padEnd(widths[index])).join('  ')}`.trimEnd();
  for (const entry of view.entries) {
    lines.push('', `${entry.entry} (${entry.mode ?? 'mode?'})${entry.jet.measured ? '' : ` · ${jetPendingText(entry.jet)}`}`);
    lines.push(line('peer', view.metrics.map((metric) => metric.label)));
    if (entry.jet.measured) lines.push(line('Jet', entry.panels.map(jetText)));
    const notes = [];
    for (const row of entry.rows) {
      if (row.status !== 'ok') {
        lines.push(`  ${row.label.padEnd(labelWidth)}  ${standingWords(row.status)}: ${row.reason ?? 'no reason recorded'}`);
        continue;
      }
      lines.push(line(row.label, row.cells.map(cellText)));
      for (const cell of row.cells) {
        if (cell.peerValue === null) notes.push(`  · ${row.label} ${cell.metric}: ${standingWords(cell.verdict)}: ${cell.reason ?? 'no value recorded'}`);
      }
    }
    const bases = entry.rows.filter((row) => row.status === 'ok').map((row) => [row.label, row.cells.find((cell) => cell.metric === 'size')?.peerBasis]).filter(([, basis]) => basis);
    const jetBasis = entry.rows.flatMap((row) => row.cells).find((cell) => cell.jetBasis && cell.jetValue !== null)?.jetBasis;
    if (bases.length) lines.push(`  size basis: ${[jetBasis ? `Jet ${jetBasis}` : null, ...bases.map(([label, basis]) => `${label} ${basis}`)].filter(Boolean).join(' · ')}`);
    lines.push(...notes);
  }
  return lines.join('\n');
}

const standingTone = (verdict) => ({ pass: 'pass', meet: 'pass', fail: 'fail', miss: 'fail', not_applicable: 'na' }[verdict] ?? 'open');

function standingBar(entry, bar) {
  const tone = bar.verdict ? ` standings__bar--${standingTone(bar.verdict)}` : '';
  const title = bar.role === 'jet' ? `${entry} · Jet ${bar.display}${bar.basis ? ` (${bar.basis})` : ''}`
    : `${entry} · ${bar.label} ${bar.detail ?? bar.display}${bar.text ? ` · ${bar.text}` : ''}`;
  return `<li class="standings__bar standings__bar--${bar.role}${tone}" title="${esc(title)}"><span class="standings__name">${esc(bar.label)}</span>`
    + `<span class="standings__track"><span class="standings__fill" style="width:${bar.width}%"></span></span>`
    + `<span class="standings__value">${esc(bar.display)}${bar.basis && bar.role !== 'jet' ? `<em>${esc(bar.basis)}</em>` : ''}</span>`
    + `${bar.text ? `<b class="standings__ratio">${esc(bar.text)}</b>` : ''}</li>`;
}

function standingPanelHtml(entry, panel) {
  const pending = panel.jet ? '' : `<li class="standings__bar standings__bar--pending"><span class="standings__name">Jet</span><span class="standings__note">${esc(jetPendingText(entry.jet))}</span></li>`;
  const missing = panel.missing.map((item) => `<li class="standings__bar standings__bar--missing"><span class="standings__name">${esc(item.label)}</span><span class="standings__note"><b>${esc(standingWords(item.status))}</b> ${esc(item.reason)}</span></li>`).join('');
  return `<figure class="standings__panel"><figcaption title="${esc(panel.what ?? panel.label)}">${esc(panel.label)}</figcaption>`
    + `<ol class="standings__bars">${panel.bars.map((bar) => standingBar(entry.entry, bar)).join('')}${pending}${missing}</ol></figure>`;
}

// The standings section of the Gauntlet page: one block per entry, one bar
// chart per metric, and the rows that measured nothing listed with their
// status and reason below the charts.
export function renderGauntletStandings(view) {
  const pills = view.jetMeasured
    ? [
      ['floor pass', view.summary.floor.pass, 'pass'], ['floor fail', view.summary.floor.fail, 'fail'],
      ['ceiling meet', view.summary.ceiling.meet, 'pass'], ['ceiling miss', view.summary.ceiling.miss, 'fail'],
      ['other', view.summary.floor.other + view.summary.ceiling.other, 'open'],
    ].map(([label, value, tone]) => `<span class="gauntlet__pill standings__pill--${tone}">${esc(label)} <b>${esc(value)}</b></span>`).join('')
    : `<span class="gauntlet__pill standings__pill--open">Jet <b>pending</b></span>`;
  const entries = view.entries.map((entry) => {
    const blocked = entry.rows.filter((row) => row.status !== 'ok');
    const blockedList = blocked.length
      ? `<ul class="standings__blocked">${blocked.map((row) => `<li class="standings__blocked--${standingTone(row.status)}"><b>${esc(row.label)}</b> <span>${esc(standingWords(row.status))}: ${esc(row.reason ?? 'no reason recorded')}</span></li>`).join('')}</ul>` : '';
    const jetNote = entry.jet.measured ? '' : ` · ${jetPendingText(entry.jet)}`;
    return `<article class="standings__entry"><h3>${esc(entry.entry)} <small>${esc(entry.mode ?? '')}${esc(jetNote)}</small></h3>`
      + `<div class="standings__panels">${entry.panels.map((panel) => standingPanelHtml(entry, panel)).join('')}</div>${blockedList}</article>`;
  }).join('');
  return `<section class="standings" aria-label="Gauntlet standings">
    <div class="standings__head"><h2 class="standings__title">Standings: the bars Jet must beat</h2><div class="gauntlet__summary">${pills}</div></div>
    <p class="gauntlet__policy">${standingRuleLines(view).map((line) => esc(line)).join(' · ')}<span>${esc(standingHeading(view))}</span></p>
    <p class="gauntlet__policy standings__basis">${sizeBasisLines(view).map((line, index) => index === 0 ? `<strong>${esc(line.trim())}</strong>` : `<span>${esc(line.trim())}</span>`).join('')}</p>
    ${entries || '<div class="empty">The results file has no standings entries.</div>'}
  </section>`;
}
