// ---- Gauntlet standings ------------------------------------------------------
// The Gauntlet page shows the gauntlet table (Tools/gauntlet/harness/table.mjs):
// its rows as a `standings` section (Tools/gauntlet/harness/standings.mjs)
// over the grid the server plans from the entries' manifests (Tower
// app/gauntlet-standings.mjs), filled in as the harness measures. Every comparison entry is a
// row of the grid and Jet, every standings peer and the hand-written asm
// ceiling are its columns. A measured cell shows its values at a glance (run
// wall · peak RSS, then cycles · size) and is grey until Jet is measured, then
// green where Jet beats it and red where Jet loses; an asm ceiling some peer
// beats is flagged red, a defect in the reference. Any other cell is not
// measured yet or the reason it cannot be (no program, not applicable, no
// ceiling reference, a failed run). A click opens the cell's detail: every
// metric with Jet's value and how far Jet is off it, the best peer and the
// asm ceiling. Below the grid, a measured run draws one bar chart per metric on
// startup, run wall, run cycles, peak RSS and size: every peer's measured
// value is a bar Jet must beat, the asm ceiling a marked bar, and Jet a bar
// once a run measured it; until then the entry reads "Jet pending". Once Jet
// is measured each peer bar carries Jet/peer: below 1.00 passes the peer
// floor, at most 1.00 meets the ceiling, and the gap is ratio − 1. The
// Gauntlet page and `tower gauntlet standings` render this one projection.
const isObject = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const object = (value) => isObject(value) ? value : {};
const array = (value) => Array.isArray(value) ? value : [];
const finite = (value) => typeof value === 'number' && Number.isFinite(value);
const esc = (value) => String(value ?? 'n/a').replace(/[&<>"']/g, (char) => ({
  '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
}[char]));

// Every number is three significant figures in a plain unit: no exponents,
// and the values one chart compares share one unit so they read directly.
const SCALES = Object.freeze({
  seconds: [['s', 1], ['ms', 1e-3], ['µs', 1e-6]],
  ms: [['s', 1e3], ['ms', 1]],
  kb: [['GB', 1e6], ['MB', 1e3], ['kB', 1]],
  bytes: [['GB', 1e9], ['MB', 1e6], ['kB', 1e3], ['B', 1]],
  count: [['', 1]],
});
const sig3 = (value) => Number(value.toPrecision(3)).toLocaleString('en-US', { maximumFractionDigits: 12 });

function formatIn(value, [suffix, factor]) {
  if (!finite(value)) return '—';
  return `${sig3(value / factor)}${suffix ? ` ${suffix}` : ''}`;
}

export const STANDINGS_SCHEMA = 'jet.gauntlet.standings.v1';
// A planned cell's reason, and the server's planned grid's (gauntlet-standings.mjs).
export const NOT_MEASURED_YET = 'not measured yet';
// The grid's columns after Jet: every standings peer, then the asm ceiling. A
// results file written before a peer joined the standings has no row for it,
// and its column says so.
const GRID_PEERS = Object.freeze(['rust', 'c', 'zig', 'go', 'python', 'node', 'bun', 'vite', 'lua', 'luajit']);
const CEILING = 'asm';
const STANDING_PEER_LABELS = Object.freeze({
  jet: 'Jet', rust: 'Rust', c: 'C', zig: 'Zig', go: 'Go', python: 'Python', bun: 'Bun', node: 'Node', vite: 'Vite', lua: 'Lua', luajit: 'LuaJIT', asm: 'asm ceiling',
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

// A grid cell: what the row is, in a few words, and the reason under it.
function gridText(head, note) {
  return { head, note: note && note !== NOT_MEASURED_YET ? note : null, text: note && note !== NOT_MEASURED_YET ? `${head}: ${note}` : head };
}

// A measured cell at a glance: run wall and peak RSS on its first line,
// cycles and size on its second. `value(metric)` gives that metric's value.
const GLANCE = Object.freeze([['wall', 'peak_rss'], ['cycles', 'size']]);
const glanceLines = (cells, value) => GLANCE.map((ids) => ids.map((id) => {
  const cell = cells.find((item) => item.metric === id);
  const shown = cell ? value(cell) : null;
  if (shown === null) return null;
  const text = standingValue(cell.unit, shown);
  return cell.unit === 'cycles' ? text.replace(/ ?([kMG]?) cycles$/, '$1 cyc') : text;
}).filter(Boolean).join(' · '));

// Where a measured peer beats the asm ceiling: per metric, the best peer
// value below the ceiling's (size only on the same size basis). The ceiling is
// hand-optimized machine code doing the same work, so any entry here is a
// defect in the reference, never a result.
function ceilingBeaten(rows) {
  const ceiling = rows.find((row) => row.role === 'ceiling' && row.status === 'ok');
  if (!ceiling) return [];
  const beaten = [];
  for (const own of ceiling.cells) {
    if (own.peerValue === null) continue;
    let best = null;
    for (const row of rows) {
      if (row.role === 'ceiling' || row.status !== 'ok') continue;
      const cell = row.cells.find((item) => item.metric === own.metric);
      if (!cell || cell.peerValue === null || cell.peerValue >= own.peerValue) continue;
      if (own.metric === 'size' && cell.peerBasis !== own.peerBasis) continue;
      if (!best || cell.peerValue < best.value) best = { peer: row.peer, label: row.label, value: cell.peerValue };
    }
    if (best) beaten.push({ metric: own.metric, by: best.peer, label: best.label, value: best.value, ceiling: own.peerValue, ratio: best.value / own.peerValue, unit: own.unit });
  }
  return beaten;
}

// A grid cell's tone: until Jet is measured every measured cell is grey; then
// green where Jet beats the row on every judged metric, red where it loses
// one. A ceiling a peer beats is flagged red whether or not Jet is measured.
function rowGrid(row, jetMeasured, beaten) {
  if (row.status === 'ok') {
    const [head, note] = glanceLines(row.cells, (cell) => cell.peerValue);
    if (row.role === 'ceiling' && beaten.length) {
      const worst = beaten.reduce((low, item) => item.ratio < low.ratio ? item : low);
      return { head: head || 'measured', note: `beaten: ${worst.label} ${worst.metric.replace('_', ' ')} ${worst.ratio.toFixed(2)}×`, text: `ceiling beaten by ${beaten.map((item) => `${item.label} on ${standingWords(item.metric)}`).join(', ')}`, tone: 'flag' };
    }
    const tally = {};
    for (const cell of row.cells) if (cell.ratio !== null) tally[cell.verdict] = (tally[cell.verdict] ?? 0) + 1;
    const judged = Object.entries(tally).map(([verdict, n]) => `${n} ${verdict}`).join(' · ');
    const tone = !jetMeasured || !judged ? 'grey' : tally.fail || tally.miss ? 'fail' : 'pass';
    return { head: head || 'measured', note: note || null, text: [head, note, judged].filter(Boolean).join(' · ') || 'measured', tone };
  }
  if (row.status === 'not_measured') return { ...gridText(NOT_MEASURED_YET, row.reason), tone: 'open' };
  if (row.status === 'uncovered') return { ...gridText('uncovered', row.role === 'ceiling' ? 'no ceiling reference' : row.reason), tone: 'na' };
  if (row.status === 'not_applicable') return { ...gridText('not applicable', row.reason), tone: 'na' };
  return { ...gridText(standingWords(row.status), row.reason ?? 'no reason recorded'), tone: 'fail' };
}

// A row of the standings: `inRun` is whether the run's standings list the peer
// at all; a peer they list but the entry lacks is a failed row.
function standingRow(peer, item, metrics, inRun) {
  const source = object(item);
  const role = source.role === 'ceiling' || peer === CEILING ? 'ceiling' : 'peer';
  const label = standingPeerLabel(peer);
  const status = item ? (typeof source.status === 'string' ? source.status : 'failed') : inRun ? 'failed' : 'not_measured';
  const cells = metrics.map((metric) => standingCell(role, metric.id, object(source.metrics)[metric.id]));
  const reason = item ? source.reason ?? null
    : inRun ? 'the results file has no row for this peer' : `this run's standings do not include ${label}`;
  return { peer, label, role, status, reason, reference: source.reference ?? null, program: source.program ?? null, measured: isObject(source.measured) ? source.measured : null, runs: finite(source.runs) ? source.runs : null, cells };
}

function jetGrid(jet, measured, rows) {
  if (measured) {
    const [head, note] = glanceLines(rows.flatMap((row) => row.cells).filter((cell) => cell.jetValue !== null)
      .filter((cell, index, all) => all.findIndex((other) => other.metric === cell.metric) === index), (cell) => cell.jetValue);
    return { head: head || 'measured', note: note || null, text: [head, note].filter(Boolean).join(' · ') || 'measured', tone: 'jet' };
  }
  if (!jet.status || jet.status === 'not_measured') return { ...gridText(NOT_MEASURED_YET, jet.reason), tone: 'open' };
  return { ...gridText(standingWords(jet.status), jet.reason ?? 'no reason recorded'), tone: 'fail' };
}

// One metric's bars for an entry: every measured peer and the asm ceiling,
// and a bar for each Jet mode once measured (each row's cells carry the Jet it
// is judged against), smallest (best) first. Bar length is log scale, so a
// thousandfold spread between asm and an interpreter stays readable. A
// measured row without this metric's value is listed with its reason.
function standingPanel(metric, rows) {
  const bars = [];
  const missing = [];
  const jets = {};
  for (const row of rows) {
    if (row.status !== 'ok') continue;
    const cell = row.cells.find((item) => item.metric === metric.id);
    if (!jets[row.jetMode] && cell.jetValue !== null) jets[row.jetMode] = { value: cell.jetValue, unit: cell.unit, basis: cell.jetBasis };
    if (cell.peerValue === null) {
      missing.push({ peer: row.peer, label: row.label, role: row.role, status: cell.verdict, reason: cell.reason ?? 'no value recorded' });
      continue;
    }
    bars.push({ peer: row.peer, label: row.label, role: row.role, mode: row.jetMode, value: cell.peerValue, unit: cell.unit, basis: cell.peerBasis, ratio: cell.ratio, verdict: cell.ratio !== null ? cell.verdict : null, text: cell.ratio !== null ? cell.text : null, detail: cell.detail });
  }
  for (const { mode, label } of JET_MODES) {
    const jet = jets[mode];
    if (jet) bars.push({ peer: 'jet', label, role: 'jet', mode, value: jet.value, unit: jet.unit, basis: jet.basis, ratio: null, verdict: null, text: null, detail: null });
  }
  return { metric: metric.id, label: metric.label, what: metric.what, jet: Object.keys(jets).length > 0, bars: scaleBars(bars), missing };
}

// Sorts bars best (smallest) first and sizes each on a log scale.
function scaleBars(bars) {
  bars.sort((left, right) => left.value - right.value);
  const logs = bars.map((bar) => Math.log(bar.value));
  const low = Math.min(...logs);
  const high = Math.max(...logs);
  for (const bar of bars) {
    bar.display = standingValue(bar.unit, bar.value);
    bar.width = high > low ? Math.round((8 + 92 * (Math.log(bar.value) - low) / (high - low)) * 10) / 10 : 100;
  }
  return bars;
}

// Jet's two ways to run a program. Each peer is judged against the one that
// matches how the peer runs (the section's `jet_modes`): a peer that ships a
// built program, and the asm ceiling, against the binary `jet build` writes;
// an interpreted peer, which starts from source, against `jet run`.
export const JET_MODES = Object.freeze([
  { mode: 'build', label: 'Jet build', what: 'compiled peers and the asm ceiling ship a built program, so they are judged against the binary jet build writes' },
  { mode: 'run', label: 'Jet run', what: 'interpreted peers start from source, so they are judged against jet run, which compiles and runs in one step' },
]);

// One entry: its rows (each tagged with the Jet mode it is judged against),
// bar panels, its one grid line (each mode's Jet, then that mode's peers) and
// the peers that beat the ceiling.
function projectEntry(entry, metrics, order, listed, modeOf) {
  const rows = order.map((peer) => ({ ...standingRow(peer, object(entry.peers)[peer] ?? null, metrics, listed.includes(peer)), jetMode: modeOf(peer) }));
  const panels = metrics.map((metric) => standingPanel(metric, rows));
  const sources = { build: object(entry.jet), run: object(entry.jet_run) };
  const jets = Object.fromEntries(JET_MODES.map(({ mode }) => {
    const jet = sources[mode];
    const measured = jet.status === 'ok' || rows.some((row) => row.jetMode === mode && row.cells.some((cell) => cell.jetValue !== null));
    return [mode, { status: jet.status ?? null, reason: jet.reason ?? null, measured, provenance: isObject(jet.measured) ? jet.measured : null, runs: finite(jet.runs) ? jet.runs : null }];
  }));
  const beaten = ceilingBeaten(rows);
  for (const row of rows) row.grid = { ...rowGrid(row, jets[row.jetMode].measured, beaten), reason: row.reason };
  const grid = JET_MODES.flatMap(({ mode, label }) => {
    const own = rows.filter((row) => row.jetMode === mode);
    return [
      { peer: 'jet', mode, label, ...jetGrid(jets[mode], jets[mode].measured, own), reason: jets[mode].reason },
      ...own.map((row) => ({ peer: row.peer, mode, label: row.label, ...row.grid })),
    ];
  });
  return { jet: jets.build, jetRun: jets.run, jets, beaten, grid, rows, panels };
}

// `payload` is { file, table, standings } as the server and the CLI read it:
// the table's file, its row counts and its standings section; null when it
// holds no standings section. With no row measured the page is the grid only.
export function projectGauntletStandings(payload) {
  const section = object(payload?.standings);
  if (section.schema !== STANDINGS_SCHEMA) return null;
  const metrics = array(section.metrics).filter(isObject).map((metric) => ({ id: metric.id, label: metric.label ?? standingWords(metric.id), what: metric.what ?? null }));
  const listed = [...array(section.peers), section.ceiling ?? CEILING];
  const order = [...GRID_PEERS, ...listed.filter((peer) => !GRID_PEERS.includes(peer) && peer !== CEILING), CEILING];
  const modes = object(section.jet_modes);
  const modeOf = (peer) => modes[peer] === 'run' ? 'run' : 'build';
  const entries = array(section.entries).filter(isObject).map((entry) => ({
    entry: entry.entry,
    mode: entry.mode ?? null,
    ...projectEntry(entry, metrics, order, listed, modeOf),
  }));
  const summary = object(section.summary);
  const floor = object(summary.floor);
  const ceiling = object(summary.ceiling);
  const table = object(payload.table);
  const rows = count(table.rows);
  const groups = JET_MODES.map((item) => ({ ...item, peers: order.filter((peer) => modeOf(peer) === item.mode) }));
  return {
    file: payload.file ?? null,
    table: { updated: table.updated ?? null, entries: count(table.entries), rows },
    planned: isObject(payload.table) && rows === 0,
    generated: section.generated ?? null,
    groups: groups.map((group) => ({ mode: group.mode, label: group.label, what: group.what, span: group.peers.length + 1, peers: group.peers.map(standingPeerLabel) })),
    columns: groups.flatMap((group) => [{ peer: 'jet', mode: group.mode, label: group.label }, ...group.peers.map((peer) => ({ peer, mode: group.mode, label: standingPeerLabel(peer) }))]),
    peers: order.map((peer) => ({ peer, label: standingPeerLabel(peer), mode: modeOf(peer) })),
    metrics,
    entries,
    jetMeasured: entries.some((entry) => entry.jet.measured || entry.jetRun.measured),
    summary: {
      floor: { pass: count(floor.pass), fail: count(floor.fail), other: count(floor.other) },
      ceiling: { meet: count(ceiling.meet), miss: count(ceiling.miss), other: count(ceiling.other) },
    },
    losses: { floor: array(object(section.losses).floor), ceiling: array(object(section.losses).ceiling) },
    sizeBasis: Object.entries(object(section.size_basis)).map(([basis, what]) => ({ basis, what: String(what) })),
  };
}

// Which table the page shows: its file, how many rows it holds and when it
// last changed; or that nothing is measured yet.
const tableDate = (iso) => typeof iso === 'string' && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(iso) ? `${iso.slice(0, 10)} ${iso.slice(11, 16)} UTC` : iso ?? 'date not recorded';
export function gauntletRunText(view) {
  const file = view.file ? `table ${view.file}` : 'gauntlet table';
  if (view.planned) return `${file}: nothing measured yet, every measurable cell is not measured yet`;
  return `${file} · ${view.table.rows} rows over ${view.table.entries} entries · updated ${tableDate(view.table.updated)}`;
}

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

// Which Jet judges which peers, one line per mode.
const modeLines = (view) => view.groups.map((group) => `${group.label} judges ${group.peers.join(', ') || 'no peer'}: ${group.what}`);

// The plain-text table `tower gauntlet standings` prints. The planned grid is
// one block per entry, a line per column. A run is one block per entry: a
// line for `jet build`'s Jet, then the peers judged against it, then one for
// `jet run`'s and its peers, one column per metric. A peer cell is the value
// and Jet/peer with its verdict. A row that measured nothing names its grid
// cell once instead of five times; a measured row's missing value is a note
// with its reason.
export function gauntletStandingsText(view) {
  const labelWidth = Math.max(9, ...view.columns.map((column) => column.label.length));
  if (view.planned) {
    const lines = [`Gauntlet · ${gauntletRunText(view)}`, ...modeLines(view)];
    for (const entry of view.entries) {
      lines.push('', `${entry.entry} (${entry.mode ?? 'mode?'})`);
      for (const cell of entry.grid) lines.push(`  ${cell.label.padEnd(labelWidth)}  ${cell.text}`);
    }
    return lines.join('\n');
  }
  const lines = [`Gauntlet · ${gauntletRunText(view)}`, ...standingRuleLines(view), ...modeLines(view),
    ...(view.jetMeasured ? [standingSummary(view.summary)] : []),
    ...sizeBasisLines(view)];
  const cellText = (cell) => cell.peerValue === null ? '—' : `${standingValue(cell.unit, cell.peerValue)}${cell.ratio === null ? '' : ` ${cell.text}`}`;
  const jetText = (panel, mode) => panel?.bars.find((bar) => bar.role === 'jet' && bar.mode === mode)?.display ?? '—';
  const widths = view.metrics.map((metric, index) => Math.max(metric.label.length,
    ...view.entries.flatMap((entry) => [
      ...JET_MODES.filter(({ mode }) => entry.jets[mode].measured).map(({ mode }) => jetText(entry.panels[index], mode).length),
      ...entry.rows.filter((row) => row.status === 'ok').map((row) => cellText(row.cells[index]).length),
    ])));
  const line = (label, cells) => `  ${label.padEnd(labelWidth)}  ${cells.map((text, index) => text.padEnd(widths[index])).join('  ')}`.trimEnd();
  for (const entry of view.entries) {
    const pending = JET_MODES.filter(({ mode }) => !entry.jets[mode].measured)
      .map(({ mode, label }) => ` · ${jetPendingText(entry.jets[mode]).replace(/^Jet/, label)}`).join('');
    lines.push('', `${entry.entry} (${entry.mode ?? 'mode?'})${pending}`);
    lines.push(line('peer', view.metrics.map((metric) => metric.label)));
    const notes = [];
    for (const { mode, label } of JET_MODES) {
      if (entry.jets[mode].measured) lines.push(line(label, entry.panels.map((panel) => jetText(panel, mode))));
      for (const row of entry.rows.filter((item) => item.jetMode === mode)) {
        if (row.status !== 'ok') {
          lines.push(`  ${row.label.padEnd(labelWidth)}  ${row.grid.text}`);
          continue;
        }
        lines.push(line(row.label, row.cells.map(cellText)));
        for (const cell of row.cells) {
          if (cell.peerValue === null) notes.push(`  · ${row.label} ${cell.metric}: ${standingWords(cell.verdict)}: ${cell.reason ?? 'no value recorded'}`);
        }
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
  const title = bar.role === 'jet' ? `${entry} · ${bar.label} ${bar.display}${bar.basis ? ` (${bar.basis})` : ''}`
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

// How Jet stands against `value` on `metric`, lower being better: Jet/value
// and the same in words. Null until both are measured.
const VERSUS_WORDS = Object.freeze({
  startup: ['faster', 'slower'], wall: ['faster', 'slower'], cycles: ['fewer cycles', 'more cycles'],
  peak_rss: ['less memory', 'more memory'], size: ['smaller', 'larger'],
});
function jetVersus(metric, jet, value) {
  if (jet === null || value === null || !(value > 0)) return null;
  const ratio = jet / value;
  const [better, worse] = VERSUS_WORDS[metric] ?? ['better', 'worse'];
  if (ratio === 1) return { ratio, words: 'Jet even (1.00×)', tone: 'pass' };
  return ratio < 1
    ? { ratio, words: `Jet ${sig3(1 / ratio)}× ${better} (${ratio.toFixed(2)})`, tone: 'pass' }
    : { ratio, words: `Jet ${sig3(ratio)}× ${worse} (+${sig3((ratio - 1) * 100)}%)`, tone: 'fail' };
}

// Everything one grid cell stands for, for the panel a click opens: the row's
// state and provenance, and per metric its value, the value of the Jet it is
// judged against and how far that Jet is off it, the best peer's and (for
// `jet build`) the asm ceiling's. `peer` is a column ('jet' too); `mode` is
// the clicked Jet column's mode, and a peer's own mode decides otherwise.
export function gauntletDetail(view, entryName, peer, mode = 'build') {
  const entry = view?.entries.find((item) => item.entry === entryName);
  if (!entry) return null;
  const isJet = peer === 'jet';
  const row = isJet ? null : entry.rows.find((item) => item.peer === peer);
  if (!isJet && !row) return null;
  const jetMode = isJet ? mode : row.jetMode;
  const kind = JET_MODES.find((item) => item.mode === jetMode);
  if (!kind) return null;
  const jetRow = entry.jets[jetMode];
  const own = entry.rows.filter((item) => item.jetMode === jetMode);
  const cells = entry.rows.flatMap((item) => item.cells);
  const valueIn = (item, id) => item?.status === 'ok' ? item.cells.find((cell) => cell.metric === id)?.peerValue ?? null : null;
  const jetIn = (id) => own.flatMap((item) => item.cells).find((cell) => cell.metric === id && cell.jetValue !== null)?.jetValue ?? null;
  const peers = own.filter((item) => item.role === 'peer' && item.status === 'ok');
  const ceiling = own.find((item) => item.role === 'ceiling') ?? null;
  const metrics = view.metrics.map((metric) => {
    const unit = cells.find((cell) => cell.metric === metric.id && cell.unit)?.unit ?? null;
    const shown = (value) => value === null ? null : standingValue(unit, value);
    const jet = jetIn(metric.id);
    const value = isJet ? jet : valueIn(row, metric.id);
    const cell = isJet ? null : row.cells.find((item) => item.metric === metric.id);
    const best = peers.map((item) => ({ label: item.label, value: valueIn(item, metric.id) })).filter((item) => item.value !== null)
      .sort((left, right) => left.value - right.value)[0] ?? null;
    const asm = valueIn(ceiling, metric.id);
    return {
      metric: metric.id,
      label: metric.label,
      what: metric.what,
      value: shown(value),
      basis: cell?.peerBasis ?? null,
      missing: value !== null ? null : isJet ? NOT_MEASURED_YET : row.status === 'ok' ? cell?.reason ?? 'no value recorded' : null,
      jet: shown(jet),
      versus: isJet ? null : jetVersus(metric.id, jet, value),
      best: best ? { label: best.label, value: shown(best.value), versus: jetVersus(metric.id, jet, best.value) } : null,
      ceiling: asm === null ? null : { value: shown(asm), versus: jetVersus(metric.id, jet, asm) },
      beaten: row?.role === 'ceiling' ? entry.beaten.find((item) => item.metric === metric.id) ?? null : null,
    };
  });
  return {
    entry: entry.entry,
    mode: entry.mode,
    jetMode,
    jetLabel: kind.label,
    jetWhat: kind.what,
    hasCeiling: ceiling !== null,
    peer,
    label: isJet ? kind.label : row.label,
    role: isJet ? 'jet' : row.role,
    status: isJet ? (jetRow.measured ? 'ok' : jetRow.status ?? 'not_measured') : row.status,
    reason: isJet ? jetRow.reason : row.reason,
    program: row?.program ?? null,
    reference: row?.reference ?? null,
    measured: isJet ? jetRow.provenance : row.measured,
    runs: isJet ? jetRow.runs : row.runs,
    jetMeasured: jetRow.measured,
    metrics,
  };
}

// The panel a grid cell opens: one line per metric with this cell's value,
// its Jet's and how far that Jet is off it, then the best peer and the asm
// ceiling. Jet's own panel compares that Jet with those two.
export function renderGauntletDetail(detail) {
  if (!detail) return '';
  const versus = (item) => item ? `<span class="gdetail__vs gdetail__vs--${item.tone}">${esc(item.words)}</span>` : '';
  const pending = detail.jetMeasured ? '—' : NOT_MEASURED_YET;
  const provenance = detail.measured ? [
    `measured ${tableDate(detail.measured.at)}`,
    detail.runs ? `${detail.runs} timed runs` : null,
    detail.measured.toolchain ? `toolchain ${detail.measured.toolchain}` : null,
    detail.measured.commit ? `commit ${String(detail.measured.commit).slice(0, 12)}` : null,
    detail.measured.run_id ? `run ${detail.measured.run_id}` : null,
  ].filter(Boolean).join(' · ') : null;
  const facts = [
    detail.program ? `program ${detail.program}` : null,
    detail.reference ? `reference ${detail.reference}` : null,
    provenance,
  ].filter(Boolean);
  const status = detail.status === 'ok' ? ''
    : `<p class="gdetail__status gdetail__status--${detail.status === 'not_measured' ? 'open' : 'fail'}">${esc(standingWords(detail.status))}${detail.reason && detail.reason !== NOT_MEASURED_YET ? `: ${esc(detail.reason)}` : ''}</p>`;
  const isJet = detail.role === 'jet';
  const jet = detail.jetLabel;
  const asmHead = detail.hasCeiling ? ['asm ceiling', ...(isJet ? [`${jet} vs asm`] : [])] : [];
  const head = isJet
    ? ['metric', jet, 'best peer', `${jet} vs best peer`, ...asmHead]
    : ['metric', detail.label, jet, `${jet} vs ${detail.label}`, 'best peer', ...asmHead];
  const value = (text, basis, missing) => text === null ? `<span class="gdetail__missing">${esc(missing ?? '—')}</span>` : `${esc(text)}${basis ? `<em>${esc(basis)}</em>` : ''}`;
  const rows = detail.metrics.map((item) => {
    const metric = `<th scope="row" title="${esc(item.what ?? item.label)}">${esc(item.label)}</th>`;
    const best = item.best ? `${esc(item.best.label)} ${esc(item.best.value)}` : '—';
    const asm = item.ceiling ? esc(item.ceiling.value) : '—';
    if (isJet) {
      return `<tr>${metric}<td>${value(item.jet, null, pending)}</td><td>${best}</td><td>${versus(item.best?.versus) || '—'}</td>`
        + `${detail.hasCeiling ? `<td>${asm}</td><td>${versus(item.ceiling?.versus) || '—'}</td>` : ''}</tr>`;
    }
    const flag = item.beaten ? `<span class="gdetail__flag">beaten by ${esc(item.beaten.label)} ${esc(standingValue(item.beaten.unit, item.beaten.value))} (${item.beaten.ratio.toFixed(2)}×): the reference is wrong</span>` : '';
    return `<tr${item.beaten ? ' class="gdetail__row--flag"' : ''}>${metric}<td>${value(item.value, item.basis, item.missing)}${flag}</td>`
      + `<td>${value(item.jet, null, pending)}</td><td>${versus(item.versus) || '—'}</td><td>${best}</td>${detail.hasCeiling ? `<td>${asm}</td>` : ''}</tr>`;
  }).join('');
  return `<header class="gdetail__head"><h3>${esc(detail.entry)} <small>${esc(detail.mode ?? '')}</small> · ${esc(detail.label)}</h3>`
    + `<button type="button" class="gdetail__close" data-gclose aria-label="Close details">×</button></header>${status}`
    + `<p class="gdetail__facts gdetail__mode">Judged against ${esc(jet)}: ${esc(detail.jetWhat)}.</p>`
    + `${facts.length ? `<p class="gdetail__facts">${facts.map(esc).join(' · ')}</p>` : ''}`
    + `<table class="gdetail__table"><thead><tr>${head.map((label) => `<th scope="col">${esc(label)}</th>`).join('')}</tr></thead><tbody>${rows}</tbody></table>`
    + `<p class="gdetail__rule">Lower is better on every metric. Jet must beat every peer (Jet/peer below 1.00) and reach the asm ceiling (Jet/asm at most 1.00).</p>`;
}

// The grid: one line per entry in two column groups, `jet build`'s Jet with
// the compiled peers and the asm ceiling, then `jet run`'s with the
// interpreted peers; every peer cell is coloured by how its group's Jet does
// against it. Every cell names its state, its reason beneath and in its
// title. The title adds the full reason where the cell shortens it (the
// ceiling's).
function gridHtml(view) {
  const title = (entry, item) => `${entry.entry} · ${item.label}: ${item.text}${item.reason && !item.text.includes(item.reason) && item.reason !== NOT_MEASURED_YET ? ` (${item.reason})` : ''} · click for details`;
  const cell = (entry, item) => `<td class="ggrid ggrid--${item.tone}${item.peer === 'jet' ? ` ggrid__jet ggrid__jet--${item.mode}` : ''}" title="${esc(title(entry, item))}" tabindex="0" role="button" data-gentry="${esc(entry.entry)}" data-gmode="${item.mode}" data-gpeer="${esc(item.peer)}"><b>${esc(item.head)}</b>${item.note ? `<small>${esc(item.note)}</small>` : ''}</td>`;
  const line = (entry) => `<tr><th scope="row"><div><strong>${esc(entry.entry)}</strong><small>${esc(entry.mode ?? '')}</small></div></th>${entry.grid.map((item) => cell(entry, item)).join('')}</tr>`;
  return `<div class="gauntlet__grid"><table class="gauntlet__table" aria-label="Gauntlet grid">
    <thead><tr><th scope="col" rowspan="2">entry</th>${view.groups.map((group) => `<th scope="colgroup" colspan="${group.span}" class="ggrid__group ggrid__group--${group.mode}" title="${esc(group.what)}">vs ${esc(group.label)}</th>`).join('')}</tr>
    <tr>${view.columns.map((column) => `<th scope="col"${column.peer === 'jet' ? ` class="ggrid__jet ggrid__jet--${column.mode}"` : ''}>${esc(column.label)}</th>`).join('')}</tr></thead>
    <tbody>${view.entries.map(line).join('')}</tbody>
  </table></div>
  <section class="gdetail" data-gdetail aria-live="polite" hidden></section>`;
}

// The Gauntlet page below its title: which run it is, the grid, and for a
// measured run one block per entry with a bar chart per metric and the rows
// that measured nothing listed with their status and reason.
export function renderGauntletStandings(view) {
  const runLine = `<p class="gauntlet__run">${esc(gauntletRunText(view))}</p>`;
  const modes = `<p class="gauntlet__policy gauntlet__modes">${view.groups.map((group) => `<span><strong>${esc(group.label)}</strong> judges ${esc(group.peers.join(', ') || 'no peer')}: ${esc(group.what)}.</span>`).join(' ')}</p>`;
  if (view.planned) {
    return `<section class="standings" aria-label="Gauntlet standings">${runLine}${modes}
    ${view.entries.length ? gridHtml(view) : '<div class="empty">No comparison entries in Tools/gauntlet/entries.</div>'}
  </section>`;
  }
  const { summary } = view;
  const pills = view.jetMeasured
    ? [
      ['floor pass', summary.floor.pass, 'pass'], ['floor fail', summary.floor.fail, 'fail'],
      ['ceiling meet', summary.ceiling.meet, 'pass'], ['ceiling miss', summary.ceiling.miss, 'fail'],
      ['other', summary.floor.other + summary.ceiling.other, 'open'],
    ].map(([name, value, tone]) => `<span class="gauntlet__pill standings__pill--${tone}">${esc(name)} <b>${esc(value)}</b></span>`).join('')
    : '<span class="gauntlet__pill standings__pill--open">Jet <b>pending</b></span>';
  const entries = view.entries.map((entry) => {
    const blocked = entry.rows.filter((row) => row.status !== 'ok');
    const blockedList = blocked.length
      ? `<ul class="standings__blocked">${blocked.map((row) => `<li class="standings__blocked--${standingTone(row.status)}"><b>${esc(row.label)}</b> <span>${esc(row.grid.text)}</span></li>`).join('')}</ul>` : '';
    const pending = JET_MODES.filter(({ mode }) => !entry.jets[mode].measured)
      .map(({ mode, label }) => ` · ${jetPendingText(entry.jets[mode]).replace(/^Jet/, label)}`).join('');
    return `<article class="standings__entry"><h3>${esc(entry.entry)} <small>${esc(entry.mode ?? '')}${esc(pending)}</small></h3>`
      + `<div class="standings__panels">${entry.panels.map((panel) => standingPanelHtml(entry, panel)).join('')}</div>${blockedList}</article>`;
  }).join('');
  return `<section class="standings" aria-label="Gauntlet standings">${runLine}${modes}
    ${view.entries.length ? gridHtml(view) : ''}
    <div class="standings__head"><h2 class="standings__title">Standings: the bars Jet must beat</h2><div class="gauntlet__summary"><div class="gauntlet__pills">${pills}</div></div></div>
    <p class="gauntlet__policy">${standingRuleLines(view).map((line) => esc(line)).join(' · ')}</p>
    <p class="gauntlet__policy standings__basis">${sizeBasisLines(view).map((line, index) => index === 0 ? `<strong>${esc(line.trim())}</strong>` : `<span>${esc(line.trim())}</span>`).join('')}</p>
    ${entries || '<div class="empty">The results file has no standings entries.</div>'}
  </section>`;
}
