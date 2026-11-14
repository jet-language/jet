// What Tower's Gauntlet page and `tower gauntlet standings` show: the
// gauntlet table (Tools/gauntlet/harness/table.mjs), the one durable record of
// every measurement, one row per entry and language, which the harness fills
// in as each entry is measured. Every comparison entry gets a cell for Jet,
// every standings peer and the asm ceiling, planned from the entries'
// manifests in this checkout: the table's row where it has one, else not
// measured yet, or the manifest's reason it cannot be measured (no program,
// not applicable, no ceiling reference). The CLI's --table names another
// table file.
import { relative, isAbsolute, join } from 'node:path';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { TowerError } from './store.mjs';

// The checkout this Tower runs from: its gauntlet entries plan the grid.
const repoRoot = fileURLToPath(new URL('../../../', import.meta.url));

// Every entry's plan, from the standings policy and the asm-parity
// references. The harness loads only here, so every other tower command
// skips it.
async function entryPlans() {
  const { CEILING_POLICY, loadEntries, standingsPlans } = await import('../../gauntlet/harness/run.mjs');
  const { standingsPolicy } = await import('../../gauntlet/harness/standings.mjs');
  const read = (file) => JSON.parse(readFileSync(join(repoRoot, file), 'utf8'));
  const policy = standingsPolicy(read('Tools/gauntlet/matrix.json'), read('Tools/gauntlet/measurement-manifest.json'));
  const { loaded } = await loadEntries(join(repoRoot, 'Tools', 'gauntlet', 'entries'), null);
  return { policy, plans: await standingsPlans(loaded, policy, read(CEILING_POLICY.references), {}) };
}

// The standings section for `table`: each entry's plan with the table's rows.
// Each measured cell also names how it was measured (the row's provenance:
// when, which run, commit, toolchain) and how many timed runs it holds.
// Jet is judged twice: `jet build`'s binary is the section's Jet, and the
// same entries judged with `jet run` standing as Jet are its `run` section.
export async function tableStandings(table) {
  const { buildStandings, entryStandings } = await import('../../gauntlet/harness/standings.mjs');
  const { tableResult, JET_RUN } = await import('../../gauntlet/harness/table.mjs');
  const { policy, plans } = await entryPlans();
  const runtimes = table.runtimes ?? {};
  const facts = (row) => row ? { measured: row.measured ?? null, runs: row.runtime?.samples?.length ?? null } : {};
  const judged = (jet) => {
    const standings = buildStandings(
      plans.map(({ plan }) => entryStandings(plan, tableResult(table, plan.entry, jet), { runtimes, pending: true })),
      policy,
      { generated: table.updated ?? null, runtimes },
    );
    for (const entry of standings.entries ?? []) {
      const rows = table.rows?.[entry.entry] ?? {};
      for (const [language, item] of Object.entries(entry.peers ?? {})) Object.assign(item, facts(rows[language]));
      if (entry.jet) Object.assign(entry.jet, facts(rows[jet]));
    }
    return standings;
  };
  const standings = judged('jet');
  const run = judged(JET_RUN);
  standings.run = { summary: run.summary, losses: run.losses, entries: run.entries };
  return standings;
}

// { file, table: { updated, entries, rows }, standings } for the table at
// `file` (the machine's table by default). `file` is shown relative to `root`
// when it lies inside it.
export async function readGauntletStandings(root, file) {
  const { readTable, tablePath } = await import('../../gauntlet/harness/table.mjs');
  const location = file ?? tablePath();
  let table;
  try {
    table = await readTable(location);
  } catch (error) {
    throw new TowerError('E_INVALID', `Gauntlet table could not be read: ${location}: ${error.message}`);
  }
  const inside = root ? relative(root, location) : null;
  const shown = inside && !inside.startsWith('..') && !isAbsolute(inside) ? inside : location;
  const rows = Object.values(table.rows ?? {}).reduce((sum, entry) => sum + Object.keys(entry).length, 0);
  return {
    file: shown,
    table: { updated: table.updated ?? null, entries: Object.keys(table.rows ?? {}).length, rows },
    standings: await tableStandings(table),
  };
}
