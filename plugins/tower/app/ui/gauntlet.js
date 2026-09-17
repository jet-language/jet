const VERDICTS = new Set(['win', 'parity', 'loss', 'unmeasured']);
const TIERS = ['aot', 'run', 'dev', 'cold', 'warm'];
const PEER_ORDER = [
  'rust', 'c', 'zig', 'go', 'python', 'js',
  'java', 'kotlin', 'swift', 'typescript', 'ruby', 'php', 'lua',
];
const UNMEASURED = 'unmeasured';
const GAUNTLET_STATES = new Set(['win', 'parity', 'loss', 'unmeasured', 'n/a']);
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

function tierStatus(item, peer) {
  if (!item) return UNMEASURED;
  if (item.status === 'not_applicable') return 'n/a';
  if (item.status && !['measured', 'ok'].includes(item.status)) return UNMEASURED;
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

function metricValue(peer, metric, requiredTiers = null) {
  const comparison = object(peer?.metric_comparisons?.[metric] ?? peer?.metrics?.[metric]);
  const tiers = object(comparison.tiers);
  const declared = Object.keys(tiers);
  const required = requiredTiers ?? array(peer?.required_tiers);
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
  const values = names.map((tier) => tierStatus(tiers[tier], peerName(peer)));
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
    const state = peer.applicable === false ? 'n/a' : tierStatus(selected.comparison.tiers?.[tier], peerNameValue);
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

// Matrix order from gauntlet/matrix.json: the table is one fixed grid, so
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
  const summary = { win: 0, parity: 0, loss: 0, unmeasured: 0 };
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
