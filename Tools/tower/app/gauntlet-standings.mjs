// The gauntlet results file Tower's standings come from (#6834). The harness
// (Tools/gauntlet/harness/run.mjs) writes one dated results JSON per run with
// a `standings` section; the Gauntlet page and `tower gauntlet standings`
// read the newest full-matrix run, else the newest partial run. The location
// is config `gauntletResults` (relative to the project root), and the CLI's
// --results names a directory or one results file.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { isAbsolute, join, relative, resolve } from 'node:path';
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

export function standingsLocation(root, configured, override) {
  const path = override ?? configured;
  if (!path) throw new TowerError('E_USAGE', 'no gauntlet results location: set config gauntletResults or pass --results');
  if (isAbsolute(path)) return path;
  if (!root) throw new TowerError('E_NOT_FOUND', 'Gauntlet standings are unavailable: project root is unknown');
  return resolve(root, path);
}

// { file, scope, standings } from `location`, a results file or a directory
// of them. `file` is relative to `root` when it lies inside it.
export function readGauntletStandings(root, location) {
  let stat;
  try {
    stat = statSync(location);
  } catch (error) {
    if (error.code === 'ENOENT') throw new TowerError('E_NOT_FOUND', `Gauntlet standings are unavailable: ${location} does not exist`);
    throw error;
  }
  const shown = (file) => {
    const inside = root ? relative(root, file) : null;
    return inside && !inside.startsWith('..') && !isAbsolute(inside) ? inside : file;
  };
  const payload = (file, report) => ({ file: shown(file), scope: report.options?.scope ?? null, standings: report.standings });
  if (!stat.isDirectory()) {
    const report = parseResults(location);
    if (!hasStandings(report)) throw new TowerError('E_INVALID', `${location} has no ${STANDINGS_SCHEMA} standings section`);
    return payload(location, report);
  }
  let partial = null;
  for (const name of readdirSync(location).filter((item) => item.endsWith('.json')).sort().reverse()) {
    const file = join(location, name);
    const report = parseResults(file);
    if (!hasStandings(report)) continue;
    if (report.options?.scope === 'full_matrix') return payload(file, report);
    partial ??= payload(file, report);
  }
  if (partial) return partial;
  throw new TowerError('E_NOT_FOUND', `Gauntlet standings are unavailable: no results file in ${location} has a standings section`);
}
