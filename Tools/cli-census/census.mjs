#!/usr/bin/env node
// Command-frequency census for `jet help` (#3724).
//
// For every `COMMANDS` row in crates/jet-cli/src/CLI.rs, counts the files in
// the repository's teaching and test text (Docs/, Examples/, tests/, Tools/;
// tracked plus untracked files that git does not ignore) that invoke
// `jet <command>`. A file counts once per command, so one generated evidence
// dump cannot outweigh the guides and tests. Rows are ranked most-used first,
// ties broken A-Z, and each row's `frequency_rank` is written in place.
// `jet help` lists commands in that order; `jet help --sort az|za` lists them
// alphabetically.
//
// usage: node Tools/cli-census/census.mjs [--check]
//   (no flag)  rewrite the ranks in CLI.rs and print the ranking
//   --check    print the ranking; exit 1 when CLI.rs holds different ranks
//
// Output is deterministic: files are read in sorted order and the ranking is
// a pure function of their text.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const CLI_RS = join(ROOT, "crates/jet-cli/src/CLI.rs");
const SCAN_ROOTS = ["Docs", "Examples", "tests", "Tools"];
// Excluded: the board's own data, this census, and tests/cli/, whose help,
// man, and completion snapshots are renderings of the registry itself and
// would feed the census its own output.
const EXCLUDED_PREFIXES = ["Tools/tower/.tower/", "Tools/cli-census/", "tests/cli/"];
const COMMANDS_OPEN = "pub const COMMANDS: &[CommandSpec] = &[\n";
const COMMANDS_CLOSE = "\n];\n";

const check = process.argv.includes("--check");
const unknown = process.argv.slice(2).filter((arg) => arg !== "--check");
if (unknown.length > 0) {
  console.error(`census: unknown argument ${unknown[0]}; usage: census.mjs [--check]`);
  process.exit(64);
}

const source = readFileSync(CLI_RS, "utf8");
const open = source.indexOf(COMMANDS_OPEN);
if (open < 0) throw new Error("census: COMMANDS table not found in CLI.rs");
const bodyStart = open + COMMANDS_OPEN.length;
const bodyEnd = source.indexOf(COMMANDS_CLOSE, bodyStart);
if (bodyEnd < 0) throw new Error("census: COMMANDS table has no closing `];`");
const body = source.slice(bodyStart, bodyEnd);

// A row names itself with a string literal or with a `&str` constant declared
// in CLI.rs (`name: JOBS_COMMAND`).
function rowName(text) {
  const field = /^ {8}name: (?:"([^"]+)"|([A-Z][A-Z0-9_]*)),$/m.exec(text);
  if (!field) return undefined;
  if (field[1]) return field[1];
  return new RegExp(`^pub const ${field[2]}: &str = "([^"]+)";$`, "m").exec(source)?.[1];
}

// One row per top-level `    CommandSpec {` … `    },` block.
const rowPattern = /^ {4}CommandSpec \{\n[\s\S]*?^ {4}\},?$/gm;
const rows = [...body.matchAll(rowPattern)].map((match) => {
  const text = match[0];
  const name = rowName(text);
  const rank = /^ {8}frequency_rank: (\d+),$/m.exec(text)?.[1];
  if (!name || rank === undefined) {
    throw new Error(`census: COMMANDS row lacks name or frequency_rank:\n${text}`);
  }
  return { text, index: match.index, name, rank: Number(rank) };
});
const names = new Set(rows.map((row) => row.name));

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

const stale = rows.filter((row) => row.rank !== rankOf.get(row.name));
if (check) {
  if (stale.length > 0) {
    for (const row of stale) {
      console.error(`census: ${row.name} has frequency_rank ${row.rank}, census says ${rankOf.get(row.name)}`);
    }
    console.error("census: run `node Tools/cli-census/census.mjs` to rewrite the ranks");
    process.exit(1);
  }
  process.exit(0);
}

if (stale.length > 0) {
  let rewritten = "";
  let cursor = 0;
  for (const row of rows) {
    rewritten += body.slice(cursor, row.index);
    rewritten += row.text.replace(
      /^( {8}frequency_rank: )\d+,$/m,
      `$1${rankOf.get(row.name)},`,
    );
    cursor = row.index + row.text.length;
  }
  rewritten += body.slice(cursor);
  writeFileSync(CLI_RS, source.slice(0, bodyStart) + rewritten + source.slice(bodyEnd));
}
