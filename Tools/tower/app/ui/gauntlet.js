// ---- Gauntlet standings ------------------------------------------------------
// The Gauntlet page shows the gauntlet table (Tools/gauntlet/harness/table.mjs):
// its rows as a `standings` section (Tools/gauntlet/harness/standings.mjs)
// over the grid the server plans from the entries' manifests (Tower
// app/gauntlet-standings.mjs), filled in as the harness measures. Every comparison entry is a
// row of the grid and Jet, every standings peer and the hand-written asm
// ceiling are its columns; a cell is measured, not measured yet, or the reason
// it cannot be (no program, not applicable, no ceiling reference, a failed
// run). Below the grid, a measured run draws one bar chart per metric on
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

function rowGrid(row) {
  if (row.status === 'ok') {
    const tally = {};
    for (const cell of row.cells) if (cell.ratio !== null) tally[cell.verdict] = (tally[cell.verdict] ?? 0) + 1;
    const judged = Object.entries(tally).map(([verdict, n]) => `${n} ${verdict}`).join(' · ');
    const tone = tally.fail || tally.miss ? 'fail' : judged ? 'pass' : 'ok';
    return { ...gridText(judged || 'measured', null), tone };
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
  const row = { peer, label, role, status, reason, reference: source.reference ?? null, cells };
  return { ...row, grid: { ...rowGrid(row), reason } };
}

function jetGrid(jet, measured) {
  if (measured) return { ...gridText('measured', null), tone: 'ok' };
  if (!jet.status || jet.status === 'not_measured') return { ...gridText(NOT_MEASURED_YET, jet.reason), tone: 'open' };
  return { ...gridText(standingWords(jet.status), jet.reason ?? 'no reason recorded'), tone: 'fail' };
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

// `payload` is { file, table, standings } as the server and the CLI read it:
// the table's file, its row counts and its standings section; null when it
// holds no standings section. With no row measured the page is the grid only.
export function projectGauntletStandings(payload) {
  const section = object(payload?.standings);
  if (section.schema !== STANDINGS_SCHEMA) return null;
  const metrics = array(section.metrics).filter(isObject).map((metric) => ({ id: metric.id, label: metric.label ?? standingWords(metric.id), what: metric.what ?? null }));
  const listed = [...array(section.peers), section.ceiling ?? CEILING];
  const order = [...GRID_PEERS, ...listed.filter((peer) => !GRID_PEERS.includes(peer) && peer !== CEILING), CEILING];
  const entries = array(section.entries).filter(isObject).map((entry) => {
    const rows = order.map((peer) => standingRow(peer, object(entry.peers)[peer] ?? null, metrics, listed.includes(peer)));
    const panels = metrics.map((metric) => standingPanel(metric, rows));
    const jet = object(entry.jet);
    const measured = panels.some((panel) => panel.jet);
    const jetRow = { status: jet.status ?? null, reason: jet.reason ?? null, measured };
    return {
      entry: entry.entry,
      mode: entry.mode ?? null,
      jet: jetRow,
      grid: [{ peer: 'jet', label: 'Jet', ...jetGrid(jetRow, measured), reason: jetRow.reason }, ...rows.map((row) => ({ peer: row.peer, label: row.label, ...row.grid }))],
      rows,
      panels,
    };
  });
  const summary = object(section.summary);
  const floor = object(summary.floor);
  const ceiling = object(summary.ceiling);
  const table = object(payload.table);
  const rows = count(table.rows);
  return {
    file: payload.file ?? null,
    table: { updated: table.updated ?? null, entries: count(table.entries), rows },
    planned: isObject(payload.table) && rows === 0,
    generated: section.generated ?? null,
    columns: [{ peer: 'jet', label: 'Jet' }, ...order.map((peer) => ({ peer, label: standingPeerLabel(peer) }))],
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

// The plain-text table `tower gauntlet standings` prints. The planned grid is
// one block per entry, a line per column. A run is one block per entry, a
// line for Jet and one per peer, one column per metric: a cell is the value,
// and Jet/peer with its verdict once Jet is measured. A row that measured
// nothing names its grid cell once instead of five times; a measured row's
// missing value is a note with its reason.
export function gauntletStandingsText(view) {
  const labelWidth = Math.max(4, ...view.columns.map((column) => column.label.length));
  if (view.planned) {
    const lines = [`Gauntlet · ${gauntletRunText(view)}`];
    for (const entry of view.entries) {
      lines.push('', `${entry.entry} (${entry.mode ?? 'mode?'})`);
      for (const cell of entry.grid) lines.push(`  ${cell.label.padEnd(labelWidth)}  ${cell.text}`);
    }
    return lines.join('\n');
  }
  const lines = [`Gauntlet · ${gauntletRunText(view)}`, ...standingRuleLines(view), ...(view.jetMeasured ? [standingSummary(view.summary)] : []), ...sizeBasisLines(view)];
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
        lines.push(`  ${row.label.padEnd(labelWidth)}  ${row.grid.text}`);
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

// The grid: one row per entry, one column for Jet, each peer and the asm
// ceiling; every cell names its state, its reason beneath and in its title.
// The title adds the full reason where the cell shortens it (the ceiling's).
function gridHtml(view) {
  const title = (entry, item) => `${entry.entry} · ${item.label}: ${item.text}${item.reason && !item.text.includes(item.reason) && item.reason !== NOT_MEASURED_YET ? ` (${item.reason})` : ''}`;
  const cell = (entry, item) => `<td class="ggrid ggrid--${item.tone}" title="${esc(title(entry, item))}"><b>${esc(item.head)}</b>${item.note ? `<small>${esc(item.note)}</small>` : ''}</td>`;
  return `<div class="gauntlet__grid"><table class="gauntlet__table" aria-label="Gauntlet grid">
    <thead><tr><th scope="col">entry</th>${view.columns.map((column) => `<th scope="col">${esc(column.label)}</th>`).join('')}</tr></thead>
    <tbody>${view.entries.map((entry) => `<tr><th scope="row"><div><strong>${esc(entry.entry)}</strong><small>${esc(entry.mode ?? '')}</small></div></th>${entry.grid.map((item) => cell(entry, item)).join('')}</tr>`).join('')}</tbody>
  </table></div>`;
}

// The Gauntlet page below its title: which run it is, the grid, and for a
// measured run one block per entry with a bar chart per metric and the rows
// that measured nothing listed with their status and reason.
export function renderGauntletStandings(view) {
  const runLine = `<p class="gauntlet__run">${esc(gauntletRunText(view))}</p>`;
  if (view.planned) {
    return `<section class="standings" aria-label="Gauntlet standings">${runLine}
    ${view.entries.length ? gridHtml(view) : '<div class="empty">No comparison entries in Tools/gauntlet/entries.</div>'}
  </section>`;
  }
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
      ? `<ul class="standings__blocked">${blocked.map((row) => `<li class="standings__blocked--${standingTone(row.status)}"><b>${esc(row.label)}</b> <span>${esc(row.grid.text)}</span></li>`).join('')}</ul>` : '';
    const jetNote = entry.jet.measured ? '' : ` · ${jetPendingText(entry.jet)}`;
    return `<article class="standings__entry"><h3>${esc(entry.entry)} <small>${esc(entry.mode ?? '')}${esc(jetNote)}</small></h3>`
      + `<div class="standings__panels">${entry.panels.map((panel) => standingPanelHtml(entry, panel)).join('')}</div>${blockedList}</article>`;
  }).join('');
  return `<section class="standings" aria-label="Gauntlet standings">${runLine}
    ${view.entries.length ? gridHtml(view) : ''}
    <div class="standings__head"><h2 class="standings__title">Standings: the bars Jet must beat</h2><div class="gauntlet__summary">${pills}</div></div>
    <p class="gauntlet__policy">${standingRuleLines(view).map((line) => esc(line)).join(' · ')}</p>
    <p class="gauntlet__policy standings__basis">${sizeBasisLines(view).map((line, index) => index === 0 ? `<strong>${esc(line.trim())}</strong>` : `<span>${esc(line.trim())}</span>`).join('')}</p>
    ${entries || '<div class="empty">The results file has no standings entries.</div>'}
  </section>`;
}
