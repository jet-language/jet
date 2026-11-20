// ---- Gauntlet standings ------------------------------------------------------
// The Gauntlet page shows the gauntlet table (Tools/Gauntlet/Harness/table.mjs):
// its rows as a `standings` section (Tools/Gauntlet/Harness/standings.mjs)
// over the grid the server plans from the entries' manifests (Tower
// App/gauntlet-standings.mjs), filled in as the harness measures. A switch
// picks the Jet the page judges: Compiled (`jet build`, against the compiled
// peers and the asm ceiling) or Scripting (`jet run`, against the
// interpreted peers). For that mode the page tallies Jet's wins and losses
// on compile time, startup, run time and memory, then shows one line per
// entry: Jet's values, then a cell per peer with one bar per metric. Every
// bar is Jet/peer on the same log scale, centered at even: green to the left
// where Jet wins, red to the right where it loses, with the factor in words.
// A cell that measured nothing says why (not measured yet, no program, not
// applicable, no ceiling reference, a failed run). A click opens the cell's
// detail: every metric (cycles and size too) with Jet's value and how far
// Jet is off it, the best peer and the asm ceiling. The Gauntlet page and
// `tower gauntlet standings` render this one projection.
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
const GRID_PEERS = Object.freeze(['rust', 'c', 'zig', 'go', 'python', 'node', 'bun', 'lua', 'luajit']);
const CEILING = 'asm';
const STANDING_PEER_LABELS = Object.freeze({
  jet: 'Jet', rust: 'Rust', c: 'C', zig: 'Zig', go: 'Go', python: 'Python', bun: 'Bun', node: 'Node', lua: 'Lua', luajit: 'LuaJIT', asm: 'asm ceiling',
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

// A grid cell's tone: until Jet is measured every measured cell is gray; then
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
    const tone = !jetMeasured || !judged ? 'gray' : tally.fail || tally.miss ? 'fail' : 'pass';
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

// One metric's values for an entry: every measured peer and the asm ceiling,
// and each Jet mode's once measured (each row's cells carry the Jet it is
// judged against), smallest (best) first. A measured row without this
// metric's value is listed with its reason.
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

// Sorts a metric's values best (smallest) first, each with its display text.
function scaleBars(bars) {
  bars.sort((left, right) => left.value - right.value);
  for (const bar of bars) bar.display = standingValue(bar.unit, bar.value);
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

const STANDING_BARS = 'lower is better on every metric; every bar is Jet / peer on one log scale centered at even: left (green) Jet wins, right (red) Jet loses';
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

// How Jet stands against `value` on `metric`, lower being better: Jet/value
// and the same in words. Null until both are measured.
const VERSUS_WORDS = Object.freeze({
  compile: ['faster', 'slower'], startup: ['faster', 'slower'], wall: ['faster', 'slower'], cycles: ['fewer cycles', 'more cycles'],
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

// What the page answers for every peer, per Jet mode: compile time, startup,
// run time and memory (cycles and size are in a cell's detail). `jet run`
// compiles as it starts and its peers have no build step, so in that mode
// startup includes the compile and there is no compile line.
export const HEADLINE = Object.freeze({
  build: Object.freeze([['compile', 'compile'], ['startup', 'startup'], ['wall', 'run time'], ['peak_rss', 'memory']]),
  run: Object.freeze([['startup', 'startup + compile'], ['wall', 'run time'], ['peak_rss', 'memory']]),
});
const SHORT_WORDS = Object.freeze({ peak_rss: ['less', 'more'], size: ['smaller', 'larger'], cycles: ['fewer', 'more'] });

// Every ratio bar on the page shares one scale: log2 of Jet/peer from the
// center line (even), half the track at RATIO_SPAN×, so a bar's length means
// the same factor in every cell. Wins grow left in green, losses right in
// red; a ratio past the span fills its half and is marked clipped.
export const RATIO_SPAN = 16;
export function ratioBar(ratio) {
  if (!finite(ratio) || ratio <= 0) return null;
  const doublings = Math.log2(ratio);
  const span = Math.log2(RATIO_SPAN);
  return {
    side: ratio < 1 ? 'win' : ratio > 1 ? 'loss' : 'even',
    width: Math.round(Math.min(Math.abs(doublings), span) / span * 1000) / 10,
    clipped: Math.abs(doublings) > span,
  };
}

// Jet/peer in words: "2.1× faster", "1.4× more".
export function ratioWords(metric, ratio) {
  if (!finite(ratio) || ratio <= 0) return null;
  if (ratio === 1) return 'even';
  const [better, worse] = SHORT_WORDS[metric] ?? VERSUS_WORDS[metric] ?? ['better', 'worse'];
  return ratio < 1 ? `${sig3(1 / ratio)}× ${better}` : `${sig3(ratio)}× ${worse}`;
}

// The headline lines a view can show in `mode`: a results file written
// before a metric existed has no cells for it, so it gets no line.
const headline = (view, mode) => HEADLINE[mode].filter(([id]) => view.metrics.some((metric) => metric.id === id));
const modeKind = (mode) => JET_MODES.find((item) => item.mode === mode) ?? JET_MODES[0];
const modeRows = (entry, mode) => entry.rows.filter((row) => row.jetMode === mode);

// Wins and losses per headline metric for one Jet mode, over every entry:
// a peer cell Jet beats is a win and one it does not beat a loss; the asm
// ceiling is met or missed (reported, not gated); every other measurable
// cell is open. The worst loss names its entry, peer and factor.
export function gauntletTally(view, mode) {
  return headline(view, mode).map(([id, label]) => {
    const tally = { metric: id, label, wins: 0, losses: 0, open: 0, met: 0, missed: 0, worst: null };
    for (const entry of view.entries) {
      for (const row of modeRows(entry, mode)) {
        const cell = row.cells.find((item) => item.metric === id);
        if (!cell || row.status === 'not_applicable' || cell.verdict === 'not_applicable') continue;
        if (row.role === 'ceiling') {
          if (cell.verdict === 'meet') tally.met += 1;
          else if (cell.verdict === 'miss') tally.missed += 1;
          continue;
        }
        if (cell.verdict === 'pass') tally.wins += 1;
        else if (cell.verdict === 'fail') {
          tally.losses += 1;
          if (!tally.worst || cell.ratio > tally.worst.ratio) tally.worst = { entry: entry.entry, peer: row.label, ratio: cell.ratio, words: ratioWords(id, cell.ratio) };
        } else if (row.status !== 'uncovered') tally.open += 1;
      }
    }
    return tally;
  });
}

function scoreHtml(view, mode) {
  if (!view.jetMeasured) return '<p class="gscore__pending">Jet pending: no Jet row is measured yet, so there are no wins or losses to show.</p>';
  const tiles = gauntletTally(view, mode).map((tally) => {
    const tone = tally.losses ? 'loss' : tally.wins ? 'win' : 'open';
    const worst = tally.worst ? `<p class="gscore__worst">worst: ${esc(tally.worst.entry)} vs ${esc(tally.worst.peer)}, ${esc(tally.worst.words)}</p>` : '';
    const ceiling = tally.met + tally.missed ? `<p class="gscore__ceil">asm ceiling: ${tally.met} met, ${tally.missed} missed</p>` : '';
    return `<div class="gscore__tile gscore__tile--${tone}"><h3>${esc(tally.label)}</h3>`
      + `<p class="gscore__count"><b class="gscore__win">${tally.wins}</b> won <b class="gscore__loss">${tally.losses}</b> lost${tally.open ? ` <span>${tally.open} open</span>` : ''}</p>${worst}${ceiling}</div>`;
  }).join('');
  return `<div class="gscore">${tiles}</div>`;
}

// The Compiled / Scripting switch: which Jet the page judges, and its peers.
function toggleHtml(view, mode) {
  const names = { build: 'Compiled', run: 'Scripting' };
  return `<div class="gtoggle" role="tablist" aria-label="Jet mode">${view.groups.map((group) => {
    const on = group.mode === mode;
    return `<button type="button" role="tab" class="gtoggle__btn${on ? ' gtoggle__btn--on' : ''}" aria-selected="${on}" data-gview="${group.mode}" title="${esc(group.what)}">`
      + `<b>${esc(names[group.mode] ?? group.label)}</b> <small>${esc(group.mode === 'build' ? 'jet build' : 'jet run')} vs ${esc(group.peers.join(', ') || 'no peer')}</small></button>`;
  }).join('')}</div>`;
}

// The scale's legend: the same ticks every bar is drawn on.
export function legendTicks() {
  const factors = [];
  for (let factor = RATIO_SPAN; factor >= 2; factor /= 2) factors.push(factor);
  const at = (ratio) => {
    const bar = ratioBar(ratio);
    return bar.side === 'even' ? 50 : bar.side === 'win' ? 50 - bar.width / 2 : 50 + bar.width / 2;
  };
  return [...factors.map((factor) => ({ text: `${factor}×`, left: at(1 / factor) })), { text: 'even', left: 50 },
    ...factors.reverse().map((factor) => ({ text: `${factor}×`, left: at(factor) }))];
}
function legendHtml() {
  return `<div class="glegend" aria-hidden="true"><span class="glegend__side glegend__side--win">Jet wins</span>`
    + `<span class="glegend__scale">${legendTicks().map((tick) => `<i style="left:${tick.left}%">${tick.text}</i>`).join('')}</span>`
    + `<span class="glegend__side glegend__side--loss">Jet loses</span>`
    + `<span class="glegend__note">one log scale for every bar on the page: Jet / peer, clipped at ${RATIO_SPAN}×</span></div>`;
}

// One metric line of a peer cell: its bar and Jet/peer in words; before Jet
// is measured, the peer's own value; otherwise why there is none.
function barLine(row, label, cell) {
  const words = cell && cell.ratio !== null ? ratioWords(cell.metric, cell.ratio) : null;
  const bar = cell && cell.ratio !== null ? ratioBar(cell.ratio) : null;
  const ceilingTone = row.role === 'ceiling' ? (cell?.verdict === 'meet' ? 'win' : 'gap') : null;
  const side = bar ? (ceilingTone === 'gap' && bar.side === 'loss' ? 'gap' : bar.side) : 'none';
  const text = words ?? (cell?.peerValue !== null && cell?.peerValue !== undefined ? standingValue(cell.unit, cell.peerValue)
    : cell?.verdict === 'not_applicable' ? 'n/a' : standingWords(cell?.verdict ?? 'unmeasured'));
  const fill = (half) => bar && bar.side === half ? `<span class="gbar__fill${bar.clipped ? ' gbar__fill--clipped' : ''}" style="width:${bar.width}%"></span>` : '';
  return `<li class="gbar gbar--${side}"><span class="gbar__name">${esc(label)}</span>`
    + `<span class="gbar__track"><span class="gbar__half gbar__half--win">${fill('win')}</span><span class="gbar__half gbar__half--loss">${fill('loss')}</span></span>`
    + `<span class="gbar__text">${esc(text)}</span></li>`;
}

function cellTitle(entry, row, lines_) {
  const lines = lines_.map(([id, label]) => {
    const cell = row.cells.find((item) => item.metric === id);
    if (!cell) return null;
    if (cell.ratio !== null) return `${label}: Jet ${cell.jet} vs ${cell.peer} (Jet/peer ${cell.ratio.toFixed(2)}, ${cell.verdict})`;
    return `${label}: ${cell.peerValue !== null ? cell.peer : ''}${cell.reason ? ` ${cell.reason}` : ''}`.trim();
  }).filter(Boolean);
  return `${entry.entry} · ${row.label}\n${lines.join('\n')}\nclick for details`;
}

function peerCell(entry, row, mode, lines) {
  const attrs = `tabindex="0" role="button" data-gentry="${esc(entry.entry)}" data-gmode="${mode}" data-gpeer="${esc(row.peer)}"`;
  if (row.status !== 'ok') {
    const why = row.reason && !row.grid.text.includes(row.reason) && row.reason !== NOT_MEASURED_YET ? ` (${row.reason})` : '';
    return `<td class="gcell gcell--${row.grid.tone}" title="${esc(`${entry.entry} · ${row.label}: ${row.grid.text}${why}`)}" ${attrs}><b>${esc(row.grid.head)}</b>${row.grid.note ? `<small>${esc(row.grid.note)}</small>` : ''}</td>`;
  }
  const flag = row.role === 'ceiling' && entry.beaten.length ? `<small class="gcell__flag">${esc(row.grid.note ?? 'beaten by a peer')}</small>` : '';
  return `<td class="gcell gcell--${row.grid.tone}" title="${esc(cellTitle(entry, row, lines))}" ${attrs}>`
    + `<ul class="gbars">${lines.map(([id, label]) => barLine(row, label, row.cells.find((item) => item.metric === id))).join('')}</ul>${flag}</td>`;
}

// Jet's own cell: its values on the headline metrics, or why there are none.
function jetCell(entry, mode, headlines) {
  const jet = entry.jets[mode];
  const attrs = `tabindex="0" role="button" data-gentry="${esc(entry.entry)}" data-gmode="${mode}" data-gpeer="jet"`;
  if (!jet.measured) {
    const grid = entry.grid.find((item) => item.peer === 'jet' && item.mode === mode);
    return `<td class="gcell gcell__jet gcell--${grid?.tone ?? 'open'}" ${attrs}><b>${esc(grid?.head ?? NOT_MEASURED_YET)}</b>${grid?.note ? `<small>${esc(grid.note)}</small>` : ''}</td>`;
  }
  const cells = modeRows(entry, mode).flatMap((row) => row.cells);
  const lines = headlines.map(([id, label]) => {
    const cell = cells.find((item) => item.metric === id && item.jetValue !== null);
    return `<li><span class="gbar__name">${esc(label)}</span><b>${cell ? esc(standingValue(cell.unit, cell.jetValue)) : '—'}</b></li>`;
  }).join('');
  return `<td class="gcell gcell__jet gcell--jet" title="${esc(`${entry.entry} · ${modeKind(mode).label} · click for details`)}" ${attrs}><ul class="gjet">${lines}</ul></td>`;
}

// The grid for one Jet mode: a line per entry, Jet's cell then one cell per
// peer judged against that Jet, every bar on the one page-wide scale.
function gridHtml(view, mode) {
  const group = view.groups.find((item) => item.mode === mode) ?? view.groups[0];
  const peers = view.peers.filter((peer) => peer.mode === mode);
  const lines = headline(view, mode);
  const line = (entry) => {
    const rows = modeRows(entry, mode);
    return `<tr><th scope="row"><div><strong>${esc(entry.entry)}</strong><small>${esc(entry.mode ?? '')}</small></div></th>${jetCell(entry, mode, lines)}`
      + `${peers.map((peer) => peerCell(entry, rows.find((row) => row.peer === peer.peer), mode, lines)).join('')}</tr>`;
  };
  return `<div class="gauntlet__grid"><table class="gauntlet__table gauntlet__table--${mode}" aria-label="Gauntlet grid, ${esc(group.label)}">
    <thead><tr><th scope="col">entry</th><th scope="col" class="gcell__jet">${esc(group.label)}</th>${peers.map((peer) => `<th scope="col"${peer.peer === CEILING ? ' class="gcol--ceiling"' : ''}>${esc(peer.label)}</th>`).join('')}</tr></thead>
    <tbody>${view.entries.map(line).join('')}</tbody>
  </table></div>
  <section class="gdetail" data-gdetail aria-live="polite" hidden></section>`;
}

// The Gauntlet page below its title: which run it is, the Compiled /
// Scripting switch, the wins and losses per metric for that mode, the scale
// legend and the grid; the rules and size basis fold away beneath.
export function renderGauntletStandings(view, mode = 'build') {
  const shown = view.groups.some((group) => group.mode === mode) ? mode : view.groups[0]?.mode ?? 'build';
  const runLine = `<p class="gauntlet__run">${esc(gauntletRunText(view))}</p>`;
  if (!view.entries.length) {
    return `<section class="standings" aria-label="Gauntlet standings">${runLine}<div class="empty">${view.planned ? 'No comparison entries in Tools/Gauntlet/Entries.' : 'The results file has no standings entries.'}</div></section>`;
  }
  const rules = view.planned ? '' : `<details class="gauntlet__rules"><summary>Rules and size basis</summary>`
    + `<p class="gauntlet__policy">${standingRuleLines(view).map((line) => esc(line)).join(' · ')}</p>`
    + `${view.jetMeasured ? `<p class="gauntlet__policy">${esc(standingSummary(view.summary))}</p>` : ''}`
    + `<p class="gauntlet__policy standings__basis">${sizeBasisLines(view).map((line, index) => index === 0 ? `<strong>${esc(line.trim())}</strong>` : `<span>${esc(line.trim())}</span>`).join('')}</p></details>`;
  return `<section class="standings" aria-label="Gauntlet standings" data-gshown="${shown}">${runLine}${toggleHtml(view, shown)}
    ${view.planned ? '' : scoreHtml(view, shown)}${view.planned ? '' : legendHtml()}
    ${gridHtml(view, shown)}${rules}
  </section>`;
}
