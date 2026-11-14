// The gauntlet table: the one durable record of every measurement, one row
// per entry and language (Jet, each peer and the asm ceiling). The harness
// adds each entry's rows as soon as that entry is measured, and Tower's
// Gauntlet page shows the table. A peer's numbers do not change while its
// program and toolchain stay the same, so a row is measured once and reused:
// the harness measures a language again only when its program digest or its
// toolchain (the Jet binary for Jet) differs from the row's, or when asked to
// remeasure. The table lives in the machine's state root, beside the run
// reports, so a run in any checkout fills it.
//
//   node Tools/gauntlet/harness/table.mjs import RESULTS.json...
//
// adds the rows of earlier run reports (each row keeps its run's provenance).
import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { stateRoot } from "../../agent/roots.mjs";

export const TABLE_SCHEMA = "jet.gauntlet.table.v1";

export const tablePath = () => path.join(stateRoot(), "gauntlet", "table.json");

const emptyTable = () => ({ schema: TABLE_SCHEMA, updated: null, runtimes: {}, toolchains: {}, rows: {} });

export async function readTable(file = tablePath()) {
  let text;
  try {
    text = await fs.readFile(file, "utf8");
  } catch (error) {
    if (error.code === "ENOENT") return emptyTable();
    throw error;
  }
  const table = JSON.parse(text);
  if (table?.schema !== TABLE_SCHEMA) throw new Error(`${file} is not a ${TABLE_SCHEMA} table`);
  return { ...emptyTable(), ...table };
}

// Written whole to a temporary file and renamed, so a reader never sees half
// a table.
export async function writeTable(table, file = tablePath()) {
  await fs.mkdir(path.dirname(file), { recursive: true });
  const temporary = `${file}.${process.pid}.tmp`;
  await fs.writeFile(temporary, `${JSON.stringify(table, null, 2)}\n`);
  await fs.rename(temporary, file);
}

// Applies `change` to the table as it is on disk now, under a lock file, and
// writes it back: runs in other checkouts write the same table, and a run
// that wrote the copy it read at its start would erase every row they added
// since. A lock older than ten minutes is from a run that died holding it.
// Returns the table as written.
export async function updateTable(change, file = tablePath()) {
  await fs.mkdir(path.dirname(file), { recursive: true });
  const lock = `${file}.lock`;
  for (let waited = 0; ; waited += 50) {
    try {
      await (await fs.open(lock, "wx")).close();
      break;
    } catch (error) {
      if (error.code !== "EEXIST") throw error;
      const stat = await fs.stat(lock).catch(() => null);
      if (stat && Date.now() - stat.mtimeMs > 10 * 60_000) await fs.rm(lock, { force: true });
      else if (waited > 120_000) throw new Error(`${lock} is held for over two minutes`);
      else await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }
  try {
    const table = await readTable(file);
    change(table);
    await writeTable(table, file);
    return table;
  } finally {
    await fs.rm(lock, { force: true });
  }
}

// What a row was measured against: the toolchain's version line, or the Jet
// binary's digest for Jet.
export function toolchainKey(language, toolchains, jetBinarySha256) {
  if (language === "jet" || language.startsWith("jet-")) return jetBinarySha256 ?? null;
  const base = language.replace(/-expert$/, "");
  return toolchains?.[base]?.version ?? null;
}

// The table row for `language` of `entry` when it still holds: measured ok,
// from the same program and the same toolchain.
export function currentRow(table, entry, language, programSha256, toolchain) {
  const row = table.rows?.[entry]?.[language];
  if (!row || row.status !== "ok") return null;
  const measured = row.measured ?? {};
  return measured.program_sha256 === programSha256 && measured.toolchain === toolchain ? row : null;
}

// Adds one entry's measured rows (its declared languages and its
// standings-only peers and ceiling). A row the run did not measure keeps the
// table's earlier row.
export function recordEntry(table, result, { runId, generated, commit, toolchains, jetBinarySha256, programs = {} }) {
  const entry = result.entry?.name;
  if (!entry) return;
  const rows = { ...(result.rows ?? {}), ...(result.standings_rows ?? {}) };
  for (const [language, row] of Object.entries(rows)) {
    if (!row || row.not_measured) continue;
    table.rows[entry] ??= {};
    table.rows[entry][language] = {
      ...row,
      measured: {
        at: generated,
        run_id: runId,
        commit: commit ?? null,
        toolchain: toolchainKey(language, toolchains, jetBinarySha256),
        program_sha256: programs[language] ?? null,
      },
    };
  }
  table.updated = generated;
}

// One entry's rows as the harness result shape entryStandings reads.
export function tableResult(table, entry) {
  const rows = table.rows?.[entry] ?? {};
  return { rows: rows.jet ? { jet: rows.jet } : {}, standings_rows: rows };
}

// Adds every entry row of an earlier run report, with that run's provenance.
export function importReport(table, report) {
  const toolchains = report.provenance?.toolchains ?? {};
  for (const result of report.entries ?? []) {
    recordEntry(table, result, {
      runId: report.run_id ?? null,
      generated: report.generated ?? null,
      commit: report.provenance?.commit ?? null,
      toolchains,
      jetBinarySha256: report.provenance?.jet_binary_sha256 ?? null,
    });
  }
  Object.assign(table.runtimes, report.standings?.runtimes ?? {});
  Object.assign(table.toolchains, toolchains);
}

async function main(args) {
  if (args[0] !== "import" || args.length < 2) {
    console.error("usage: node Tools/gauntlet/harness/table.mjs import RESULTS.json...");
    return 2;
  }
  const reports = await Promise.all(args.slice(1).map(async (file) => JSON.parse(await fs.readFile(file, "utf8"))));
  const table = await updateTable((fresh) => {
    for (const file of reports) importReport(fresh, file);
  });
  const count = Object.values(table.rows).reduce((sum, rows) => sum + Object.keys(rows).length, 0);
  console.log(`table\t${tablePath()}\t${Object.keys(table.rows).length} entries\t${count} rows`);
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = await main(process.argv.slice(2));
}
