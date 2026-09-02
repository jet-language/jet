const VERDICTS = new Set(['win', 'parity', 'loss', 'unmeasured']);
const TIERS = ['aot', 'run', 'dev', 'cold', 'warm'];
const PEER_ORDER = [
  'rust', 'python', 'c', 'zig', 'node', 'go', 'java', 'kotlin', 'swift',
  'typescript', 'javascript', 'ruby', 'php', 'lua', 'jet-expert',
];
const UNMEASURED = 'unmeasured';

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

function metricValue(peer, metric) {
  const comparison = object(peer?.metric_comparisons?.[metric] ?? peer?.metrics?.[metric]);
  const tiers = object(comparison.tiers);
  const names = [...new Set([
    ...TIERS,
    ...Object.keys(tiers),
  ])];
  const tier = names.find((name) => isObject(tiers[name]) && (
    finite(tiers[name].ratio) || tiers[name].status === 'measured' || verdict(tiers[name].verdict)));
  if (tier) return { tier, values: tiers[tier], comparison };
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
    return verdict(comparison.verdict) ?? UNMEASURED;
  }
  const values = names.map((tier) => {
    const item = object(tiers[tier]);
    if (item.status === 'not_applicable') return 'n/a';
    if (verdict(item.verdict)) return item.verdict;
    return ratioVerdict(item.ratio, peerName(peer));
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
  if (!peer) return {
    peer: peerNameValue,
    verdict: hasPeers ? 'n/a' : UNMEASURED,
    ratio: null,
    tier: null,
    values: {},
    detail: { cell, peer: null, metric: cell?.primary_metric ?? null, values: {} },
  };
  const metric = cell?.primary_metric ?? null;
  const selected = metricValue(peer, metric);
  const values = selected.values ?? {};
  return {
    peer: peerNameValue,
    verdict: peerMetricVerdict(peer, metric),
    ratio: finite(values.ratio) ? values.ratio : null,
    tier: selected.tier,
    values,
    detail: { cell, peer, metric, tier: selected.tier, values },
  };
}

function axisPeer(axis, peerNameValue) {
  const peer = object(axis?.comparisons?.[peerNameValue]);
  const phases = ['cold', 'warm'].map((phase) => ({ phase, value: object(peer[phase]) }));
  const selected = phases.find(({ value }) => finite(value.ratio) || value.status === 'measured') ?? phases[0];
  const values = selected.value;
  const phaseVerdicts = phases.map(({ value }) => {
    if (value.status === 'not_applicable') return 'n/a';
    if (verdict(value.verdict)) return value.verdict;
    return ratioVerdict(value.ratio, peerNameValue);
  });
  const hasComparison = Object.keys(peer).length > 0;
  return {
    peer: peerNameValue,
    verdict: hasComparison ? reduce(phaseVerdicts) : 'n/a',
    ratio: finite(values.ratio) ? values.ratio : null,
    tier: selected.phase,
    values,
    detail: {
      axis,
      peer: hasComparison ? { ...peer, peer: peerNameValue } : null,
      metric: axis?.metric ?? null,
      tier: selected.phase,
      values,
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

export function projectGauntletMatrix(status) {
  const source = object(status);
  const cells = array(source.cells).map(cellRow);
  const axisRows = Object.entries(object(source.axes)).map(([id, axis]) => axisRow(id, axis));
  const columns = sortPeers([
    ...cells.flatMap((row) => row.peerNames),
    ...axisRows.flatMap((row) => row.peerNames),
  ]);
  const groups = [];
  const grouped = new Map();
  for (const row of cells) {
    const id = row.domain || 'Other';
    if (!grouped.has(id)) {
      const group = { id, label: id, kind: 'cells', rows: [] };
      grouped.set(id, group);
      groups.push(group);
    }
    grouped.get(id).rows.push(row);
  }
  groups.sort((left, right) => left.label.localeCompare(right.label));
  if (axisRows.length) groups.push({ id: 'axes', label: 'Axes', kind: 'axes', rows: axisRows });
  return {
    columns,
    groups,
    rows: [...cells, ...axisRows],
    axisRows,
  };
}

const METRIC_UNITS = Object.freeze({
  runtime_wall_seconds: 'seconds',
  runtime_peak_rss_kb: 'kB',
  runtime_first_stdout_seconds: 'seconds',
  cold_build_seconds: 'seconds',
  warm_build_seconds: 'seconds',
  binary_bytes: 'bytes',
  source_bytes: 'bytes',
  reload_latency_ms: 'ms',
  cold_reload_latency_ms: 'ms',
  warm_reload_latency_ms: 'ms',
});

function metricLabel(metric) {
  return metric ? String(metric).replaceAll('_', ' ') : 'n/a';
}

function formatNumber(value) {
  if (!finite(value)) return 'n/a';
  if (Math.abs(value) >= 1000 || (Math.abs(value) > 0 && Math.abs(value) < 0.01)) return value.toExponential(3);
  return value.toFixed(3).replace(/0+$/, '').replace(/\.$/, '');
}
function compactValue(value) {
  if (value == null) return 'n/a';
  if (Array.isArray(value)) return `${value.length} sample${value.length === 1 ? '' : 's'}`;
  if (isObject(value)) {
    const entries = Object.entries(value).map(([key, item]) => `${key}: ${compactValue(item)}`);
    return entries.length ? entries.join(', ') : 'n/a';
  }
  return String(value);
}

function safeJson(value) {
  if (value == null) return 'n/a';
  if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') return String(value);
  try {
    return JSON.stringify(value, (_key, item) => item === undefined ? null : item);
  } catch {
    return 'n/a';
  }
}

function issueText(value) {
  if (!isObject(value)) return safeJson(value);
  const card = value.card ?? value.card_number ?? value.card_id;
  const fields = ['entry', 'peer', 'metric', 'category', 'status', 'reason']
    .filter((key) => value[key] != null)
    .map((key) => `${key}=${value[key]}`);
  const body = fields.join(', ');
  return `${card == null ? '' : `#${card} `}${body || 'issue'}`;
}
function cardLinks(value) {
  const text = Array.isArray(value) ? (value.length ? value.map(issueText).join('; ') : 'n/a') : safeJson(value);
  return esc(text)
    .replace(/#(\d+)/g, '<a href="#card-$1" data-card="$1" class="gauntlet__card-link">#$1</a>')
    .replace(/((?:&quot;)?(?:card|card_number|card_id)(?:&quot;)?\s*:\s*)(\d+)/gi,
      '$1<a href="#card-$2" data-card="$2" class="gauntlet__card-link">#$2</a>');
}

function detailValue(value) {
  return cardLinks(compactValue(value));
}

export function buildGauntletTooltip(detail = {}) {
  const cell = object(detail.cell);
  const axis = object(detail.axis);
  const peer = object(detail.peer);
  const values = object(detail.values);
  const peerLabel = peerName(detail.peer) ?? detail.peer_name ?? 'n/a';
  const metric = detail.metric ?? axis.metric ?? cell.primary_metric ?? null;
  const ratio = finite(values.ratio) ? `${formatNumber(values.ratio)}x` : 'n/a';
  const rustRule = String(peerLabel).replace(/-expert$/, '') === 'rust'
    ? 'Jet / Rust < 1.00 wins; up to 1.05 is parity.'
    : `Jet / ${peerLabel} < 1.00 wins; 1.00 or more loses.`;
  const runId = cell.run_id ?? values.run_id ?? axis.run_id;
  const measuredAt = cell.measured_at ?? values.measured_at ?? axis.measured_at;
  const lines = [
    ['Metric', `${metricLabel(metric)}${METRIC_UNITS[metric] ? ` (${METRIC_UNITS[metric]})` : ''}`],
    ['Jet', finite(values.jet) ? formatNumber(values.jet) : 'n/a'],
    ['Peer', finite(values.peer) ? formatNumber(values.peer) : peerLabel],
    ['Ratio', ratio],
    ['Rule', esc(rustRule)],
    ['Tier', detail.tier ?? 'n/a'],
    ['Mode', cell.mode ?? detail.mode ?? 'n/a'],
    ['Samples', detailValue(values.samples)],
    ['Median', detailValue(values.median)],
    ['p99', detailValue(values.p99)],
    ['RSS', detailValue(values.rss ?? values.peak_rss_kb)],
    ['Run', runId ?? 'n/a'],
    ['Date', measuredAt ?? 'n/a'],
    ['Failures', cardLinks(cell.failures)],
    ['Loss owners', cardLinks(cell.loss_owners)],
  ];
  return `<div class="gauntlet__tooltip" role="tooltip"><strong>${esc(cell.id ?? axis.id ?? 'Gauntlet detail')}</strong><dl>${lines
    .map(([label, value]) => `<div><dt>${esc(label)}</dt><dd>${value}</dd></div>`).join('')}</dl></div>`;
}

export { metricLabel, metricValue, ratioVerdict };
