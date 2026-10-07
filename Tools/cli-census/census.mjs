#!/usr/bin/env node
// Command-frequency census for `jet help` (#3724).
//
// For every command row in Compiler/JetCli/Source/Cli/Commands.jet, counts the files in
// the repository's teaching and test text (Docs/, Examples/, tests/, Tools/;
// tracked plus untracked files that git does not ignore) that invoke
// `jet <command>`. A file counts once per command, so one generated evidence
// dump cannot outweigh the guides and tests. Rows are ranked most-used first,
// ties broken A-Z, and each row's `frequency_rank` is written in place.
// `jet help` lists commands in that order; `jet help --sort az|za` lists them
// alphabetically.
//
// usage: node Tools/cli-census/census.mjs [--check]
//   (no flag)  rewrite the ranks in Commands.jet and its frozen CLI.rs copy, print the ranking
//   --check    print the ranking; exit 1 when either table holds different ranks
//
// Output is deterministic: files are read in sorted order and the ranking is
// a pure function of their text.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const SCAN_ROOTS = ["Docs", "Examples", "tests", "Tools"];
// Excluded: the board's own data, this census, and tests/cli/, whose help,
// man, and completion snapshots are renderings of the registry itself and
// would feed the census its own output.
const EXCLUDED_PREFIXES = ["Tools/tower/.tower/", "Tools/cli-census/", "tests/cli/"];
// The Jet table is the source (D-JETCLI-HOST1=A); the Rust host keeps a
// frozen copy that `node Compiler/Bootstrap/check-command-table.mjs` holds
// equal, so both carry the same ranks.
const TABLES = [
  {
    path: join(ROOT, "Compiler/JetCli/Source/Cli/Commands.jet"),
    open: "pub JET_CLI_COMMANDS :: prep { [JetCLICommandSpec]{\n",
    close: "\n} }\n",
    rowPattern: /^ {4}JetCLICommandSpec\{\n[\s\S]*?^ {4}\},?$/gm,
  },
  {
    path: join(ROOT, "crates/jet-cli/src/CLI.rs"),
    open: "pub const COMMANDS: &[CommandSpec] = &[\n",
    close: "\n];\n",
    rowPattern: /^ {4}CommandSpec \{\n[\s\S]*?^ {4}\},?$/gm,
  },
];

const check = process.argv.includes("--check");
const unknown = process.argv.slice(2).filter((arg) => arg !== "--check");
if (unknown.length > 0) {
  console.error(`census: unknown argument ${unknown[0]}; usage: census.mjs [--check]`);
  process.exit(64);
}

// One row per top-level command block. A row names itself with a string
// literal or, in CLI.rs, with a `&str` constant (`name: JOBS_COMMAND`).
function readTable(table) {
  const source = readFileSync(table.path, "utf8");
  const open = source.indexOf(table.open);
  if (open < 0) throw new Error(`census: command table not found in ${table.path}`);
  const bodyStart = open + table.open.length;
  const bodyEnd = source.indexOf(table.close, bodyStart);
  if (bodyEnd < 0) throw new Error(`census: command table in ${table.path} has no end`);
  const body = source.slice(bodyStart, bodyEnd);
  const rows = [...body.matchAll(table.rowPattern)].map((match) => {
    const text = match[0];
    const field = /^ {8}name: (?:"([^"]+)"|([A-Z][A-Z0-9_]*)),$/m.exec(text);
    const name = field?.[1] ?? (field && new RegExp(`^pub const ${field[2]}: &str = "([^"]+)";$`, "m").exec(source)?.[1]);
    const rank = /^ {8}frequency_rank: (\d+),$/m.exec(text)?.[1];
    if (!name || rank === undefined) {
      throw new Error(`census: command row lacks name or frequency_rank:\n${text}`);
    }
    return { text, index: match.index, name, rank: Number(rank) };
  });
  return { ...table, source, bodyStart, bodyEnd, body, rows };
}

const tables = TABLES.map(readTable);
const names = new Set(tables[0].rows.map((row) => row.name));

const files = execFileSync(
  "git",
  ["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", ...SCAN_ROOTS],
  { cwd: ROOT, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
)
  .split("\0")
  .filter((path) => path && !EXCLUDED_PREFIXES.some((prefix) => path.startsWith(prefix)));
const uniqueFiles = [...new Set(files)].sort();

// `jet` as its own word (a path such as `target/debug/jet` counts), then the
// command word. The captured word is compared whole, so `jet test-compare`
// never counts as `jet test`.
const invocation = /(?<![\w.-])jet[ \t]+([a-z][a-z0-9+-]*)/g;
const counts = new Map([...names].map((name) => [name, 0]));
for (const path of uniqueFiles) {
  let bytes;
  try {
    bytes = readFileSync(join(ROOT, path));
  } catch {
    continue; // listed but deleted in the working tree
  }
  if (bytes.includes(0)) continue; // binary
  const invoked = new Set();
  for (const match of bytes.toString("utf8").matchAll(invocation)) {
    if (counts.has(match[1])) invoked.add(match[1]);
  }
  for (const name of invoked) counts.set(name, counts.get(name) + 1);
}

const ranked = [...counts.entries()].sort(
  ([leftName, left], [rightName, right]) =>
    right - left || (leftName < rightName ? -1 : leftName > rightName ? 1 : 0),
);
const rankOf = new Map(ranked.map(([name], index) => [name, index + 1]));

for (const [name, count] of ranked) {
  console.log(`${String(rankOf.get(name)).padStart(3)} ${String(count).padStart(6)} ${name}`);
}

const stale = tables.flatMap((table) => table.rows.filter((row) => row.rank !== rankOf.get(row.name)).map((row) => ({ table, row })));
if (check) {
  if (stale.length > 0) {
    for (const { table, row } of stale) {
      console.error(`census: ${row.name} has frequency_rank ${row.rank} in ${table.path.slice(ROOT.length + 1)}, census says ${rankOf.get(row.name)}`);
    }
    console.error("census: run `node Tools/cli-census/census.mjs` to rewrite the ranks");
    process.exit(1);
  }
  process.exit(0);
}

for (const table of tables) {
  if (!table.rows.some((row) => row.rank !== rankOf.get(row.name))) continue;
  let rewritten = "";
  let cursor = 0;
  for (const row of table.rows) {
    rewritten += table.body.slice(cursor, row.index);
    rewritten += row.text.replace(
      /^( {8}frequency_rank: )\d+,$/m,
      `$1${rankOf.get(row.name)},`,
    );
    cursor = row.index + row.text.length;
  }
  rewritten += table.body.slice(cursor);
  writeFileSync(table.path, table.source.slice(0, table.bodyStart) + rewritten + table.source.slice(table.bodyEnd));
}
