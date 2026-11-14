// The gauntlet results file Tower's standings come from (#6834). The harness
// (Tools/gauntlet/harness/run.mjs) writes one dated results JSON per run with
// a `standings` section into its checkout's Tools/gauntlet/results, and a copy
// into the machine's shared results directory ($JET_STATE_ROOT/gauntlet/results),
// so a run in any worktree reaches a Tower served from any checkout. The
// Gauntlet page and `tower gauntlet standings` read the newest full-matrix run
// (a whole run, or a --languages run merged into an earlier one), else the
// newest partial run, from config `gauntletResults` (relative to the project
// root) and the shared directory. The CLI's --results names one directory or
// one results file instead.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { isAbsolute, join, relative, resolve } from 'node:path';
import { stateRoot } from '../../agent/roots.mjs';
import { TowerError } from './store.mjs';
import { STANDINGS_SCHEMA } from './ui/gauntlet.js';

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

// run.mjs names a whole or merged run `<date>.json` / `<date>-merged-….json`,
// an axis run `<date>-axis-….json` (no standings), and any other partial run
// `<date>-<entry and languages>.json`.
const nameScope = (name) => /^\d{4}-\d{2}-\d{2}(-merged-.*)?\.json$/.test(name) ? 'full'
  : /^\d{4}-\d{2}-\d{2}-axis-/.test(name) ? 'axis' : 'partial';

// { file, scope, standings } from `locations`, a list of results files or
// directories of them; a missing location is skipped unless all are missing.
// Candidates are read newest first (the date in the name, then the file time),
// full runs before partial ones, and only until one has a standings section,
// so a directory of old reports is not parsed whole. `file` is relative to
// `root` when it lies inside it.
export function readGauntletStandings(root, locations) {
  const shown = (file) => {
    const inside = root ? relative(root, file) : null;
    return inside && !inside.startsWith('..') && !isAbsolute(inside) ? inside : file;
  };
  const payload = (file, report) => ({ file: shown(file), scope: report.options?.scope ?? null, standings: report.standings });
  const candidates = [];
  const missing = [];
  for (const location of locations) {
    let stat;
    try {
      stat = statSync(location);
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
      missing.push(location);
      continue;
    }
    if (!stat.isDirectory()) {
      const report = parseResults(location);
      if (!hasStandings(report)) throw new TowerError('E_INVALID', `${location} has no ${STANDINGS_SCHEMA} standings section`);
      return payload(location, report);
    }
    for (const name of readdirSync(location)) {
      if (!name.endsWith('.json') || nameScope(name) === 'axis') continue;
      const file = join(location, name);
      candidates.push({ file, name, scope: nameScope(name), mtime: statSync(file).mtimeMs });
    }
  }
  if (missing.length === locations.length) {
    throw new TowerError('E_NOT_FOUND', `Gauntlet standings are unavailable: ${missing.join(' and ')} ${missing.length === 1 ? 'does' : 'do'} not exist`);
  }
  candidates.sort((a, b) => b.name.slice(0, 10).localeCompare(a.name.slice(0, 10)) || b.mtime - a.mtime || b.name.localeCompare(a.name));
  for (const pass of ['full', 'partial']) {
    for (const candidate of candidates) {
      if (candidate.scope !== pass) continue;
      const report = parseResults(candidate.file);
      if (hasStandings(report)) return payload(candidate.file, report);
    }
  }
  throw new TowerError('E_NOT_FOUND', `Gauntlet standings are unavailable: no results file in ${locations.join(' or ')} has a standings section`);
}
