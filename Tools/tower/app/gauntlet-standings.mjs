// The gauntlet run Tower's Gauntlet page shows (#6834). The harness
// (Tools/gauntlet/harness/run.mjs) writes one dated results JSON per run with
// a `standings` section into its checkout's Tools/gauntlet/results, and a copy
// into the machine's shared results directory ($JET_STATE_ROOT/gauntlet/results),
// so a run in any worktree reaches a Tower served from any checkout. The
// Gauntlet page and `tower gauntlet standings` show one run: the newest
// full-matrix run (a whole run, or a --languages run merged into an earlier
// one) from config `gauntletResults` (relative to the project root) and the
// shared directory. A partial run is never shown, and neither is any older
// run. Until a full run exists they show the planned grid instead: every
// comparison entry against Jet, every standings peer and the asm ceiling, from
// the entries' manifests in this checkout, each cell not measured yet or the
// manifest's reason it cannot be (no program, not applicable, no ceiling
// reference). The CLI's --results names one directory or one results file
// instead.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { stateRoot } from '../../agent/roots.mjs';
import { TowerError } from './store.mjs';
import { NOT_MEASURED_YET, STANDINGS_SCHEMA } from './ui/gauntlet.js';

// The checkout this Tower runs from: its gauntlet entries plan the grid.
const repoRoot = fileURLToPath(new URL('../../../', import.meta.url));

function parseResults(file) {
  try {
    return JSON.parse(readFileSync(file, 'utf8'));
  } catch (error) {
    throw new TowerError('E_INVALID', `Gauntlet results could not be read: ${file}: ${error.message}`);
  }
}

const hasStandings = (report) => report?.standings?.schema === STANDINGS_SCHEMA;

// The machine-wide copy of every run's results file (run.mjs writes it).
export const sharedResultsDir = () => join(stateRoot(), 'gauntlet', 'results');

// The locations to search: --results alone when given, else the configured
// directory and the shared one.
export function standingsLocations(root, configured, override) {
  if (override !== undefined) return [resolve(override)];
  if (!configured) throw new TowerError('E_USAGE', 'no gauntlet results location: set config gauntletResults or pass --results');
  if (isAbsolute(configured)) return [configured, sharedResultsDir()];
  if (!root) throw new TowerError('E_NOT_FOUND', 'Gauntlet standings are unavailable: project root is unknown');
  return [resolve(root, configured), sharedResultsDir()];
}

// run.mjs names a whole run `<date>.json` and a merged run
// `<date>-merged-….json`; every other name is a partial or axis run.
const FULL_RUN = /^\d{4}-\d{2}-\d{2}(-merged-.*)?\.json$/;

// Which run a results file is: its date, the commit it measured (run.mjs
// records it from #6834 on) and, for a merged run, the run it merged into.
function runOf(report) {
  const merged = report.merged_from;
  return {
    id: report.run_id ?? null,
    generated: report.generated ?? null,
    commit: report.provenance?.commit ?? null,
    merged_from: merged ? { id: merged.run_id ?? null, generated: merged.generated ?? null, commit: merged.commit ?? null } : null,
  };
}

// One planned entry: Jet and every peer with a program not measured yet, the
// rest with the plan's reason. Its shape is entryStandings' with no values.
function plannedEntry(plan, metrics) {
  const cells = (verdict, reason) => Object.fromEntries(metrics.map((metric) => [metric.id, { jet: null, peer: null, ratio: null, verdict, reason }]));
  const peers = {};
  for (const [peer, item] of Object.entries(plan.peers)) {
    const status = item.plan === 'measure' ? 'not_measured' : item.plan;
    const reason = item.plan === 'measure' ? NOT_MEASURED_YET : item.reason;
    peers[peer] = { role: item.role, status, reason, program: item.program, ...(item.reference ? { reference: item.reference } : {}), metrics: cells(status, reason) };
  }
  return { entry: plan.entry, mode: plan.mode, cells: plan.cells, jet: { status: 'not_measured', reason: NOT_MEASURED_YET }, peers };
}

// The planned grid's standings section, from this checkout's entries, the
// standings policy and the asm-parity references, with nothing measured. The
// harness loads only here, so every other tower command skips it.
export async function plannedStandings() {
  const { CEILING_POLICY, loadEntries, standingsPlans } = await import('../../gauntlet/harness/run.mjs');
  const { buildStandings, STANDINGS_METRICS, standingsPolicy } = await import('../../gauntlet/harness/standings.mjs');
  const read = (file) => JSON.parse(readFileSync(join(repoRoot, file), 'utf8'));
  const policy = standingsPolicy(read('Tools/gauntlet/matrix.json'), read('Tools/gauntlet/measurement-manifest.json'));
  const { loaded } = await loadEntries(join(repoRoot, 'Tools', 'gauntlet', 'entries'), null);
  const plans = await standingsPlans(loaded, policy, read(CEILING_POLICY.references), {});
  return buildStandings(plans.map(({ plan }) => plannedEntry(plan, STANDINGS_METRICS)), policy, { generated: null });
}

// { file, scope, run, standings } from `locations`, a list of results files
// or directories of them; a missing location is skipped. Full runs are read
// newest first (the date in the name, then the file time) and only until one
// has a standings section, so a directory of old reports is not parsed whole.
// With none, the planned grid: { file: null, scope: 'planned', run: null }.
// `file` is relative to `root` when it lies inside it.
export async function readGauntletStandings(root, locations) {
  const shown = (file) => {
    const inside = root ? relative(root, file) : null;
    return inside && !inside.startsWith('..') && !isAbsolute(inside) ? inside : file;
  };
  const payload = (file, report) => ({ file: shown(file), scope: report.options?.scope ?? null, run: runOf(report), standings: report.standings });
  const candidates = [];
  for (const location of locations) {
    let stat;
    try {
      stat = statSync(location);
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
      continue;
    }
    if (!stat.isDirectory()) {
      const report = parseResults(location);
      if (!hasStandings(report)) throw new TowerError('E_INVALID', `${location} has no ${STANDINGS_SCHEMA} standings section`);
      return payload(location, report);
    }
    for (const name of readdirSync(location)) {
      if (!FULL_RUN.test(name)) continue;
      const file = join(location, name);
      candidates.push({ file, name, mtime: statSync(file).mtimeMs });
    }
  }
  candidates.sort((a, b) => b.name.slice(0, 10).localeCompare(a.name.slice(0, 10)) || b.mtime - a.mtime || b.name.localeCompare(a.name));
  for (const candidate of candidates) {
    const report = parseResults(candidate.file);
    if (hasStandings(report)) return payload(candidate.file, report);
  }
  return { file: null, scope: 'planned', run: null, standings: await plannedStandings() };
}
