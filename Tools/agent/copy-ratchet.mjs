#!/usr/bin/env node
// #4318 copy ratchet over emitted Rust (stage-zero.rs or any MIRRust output).
//
// Counts the clone sites inside each emitted Jet function body (`fn __jet_*`
// at column 0, up to its closing `}` at column 0), by kind:
//   fact:<kind>  a `.clone()/*copy:<kind>*/` site: MIRRust `value_copy` (a MIR
//                `Copy{fact}`) or `value_owned` (`unbacked`), tagged when the
//                emitter runs with JET_COPY_TAGS=1
//   untagged     any other `.clone()` in a Jet function body
// Bitwise copies of Copy carriers print `(*slot)`, not `.clone()`, and are free.
// Runtime and prelude code outside `fn __jet_*` bodies is not counted.
//
// Gate: no kind's total may rise above the baseline. Use --write to record a
// new baseline (after an emission whose tagging changed, e.g. the first
// JET_COPY_TAGS=1 stage-zero).

import { createReadStream, existsSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const DEFAULT_BASELINE = join(REPO_ROOT, "Tools/agent/copy-ratchet.baseline.json");
const FUNCTION_HEAD = /^(?:pub(?:\([^)]*\))? )?fn (__jet_\w+)/;
const TAGGED = /\.clone\(\)\/\*copy:([a-z_]+)\*\//g;
const CLONE = /\.clone\(\)/g;

function usage() {
  return [
    "usage: copy-ratchet.mjs <emitted.rs> [--baseline PATH] [--write] [--top N]",
    "",
    "Count clone sites per emitted Jet function by copy kind and compare the",
    "per-kind totals with the baseline (they may only fall). --write records",
    "the current counts as the baseline.",
  ].join("\n");
}

function parseArgs(argv) {
  const options = { input: null, baseline: DEFAULT_BASELINE, write: false, top: 10 };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--write") {
      options.write = true;
    } else if (arg === "--baseline") {
      const value = argv[++index];
      if (!value) throw new Error("--baseline needs a path");
      options.baseline = resolve(value);
    } else if (arg === "--top") {
      const value = Number(argv[++index]);
      if (!Number.isInteger(value) || value < 0) throw new Error("--top needs a count");
      options.top = value;
    } else if (arg === "--help") {
      console.log(usage());
      process.exit(0);
    } else if (arg.startsWith("--") || options.input) {
      throw new Error(`unknown argument ${arg}\n\n${usage()}`);
    } else {
      options.input = resolve(arg);
    }
  }
  if (!options.input) throw new Error(`missing emitted Rust path\n\n${usage()}`);
  return options;
}

function bump(counts, kind, by) {
  if (by > 0) counts[kind] = (counts[kind] ?? 0) + by;
}

function sorted(object) {
  return Object.fromEntries(Object.entries(object).sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0)));
}

async function scan(path) {
  const totals = {};
  const functions = {};
  let current = null;
  const lines = createInterface({ input: createReadStream(path, { encoding: "utf8" }), crlfDelay: Infinity });
  for await (const line of lines) {
    if (current === null) {
      const head = FUNCTION_HEAD.exec(line);
      if (!head) continue;
      current = head[1];
      // A one-line body opens and closes on its head line.
      if (line.endsWith("}") && !line.endsWith("{}")) current = null;
      if (current === null) continue;
    } else if (line.startsWith("}")) {
      current = null;
      continue;
    }
    const all = line.match(CLONE)?.length ?? 0;
    if (all === 0) continue;
    const counts = (functions[current] ??= {});
    let tagged = 0;
    for (const match of line.matchAll(TAGGED)) {
      tagged += 1;
      bump(counts, `fact:${match[1]}`, 1);
      bump(totals, `fact:${match[1]}`, 1);
    }
    bump(counts, "untagged", all - tagged);
    bump(totals, "untagged", all - tagged);
  }
  for (const name of Object.keys(functions)) functions[name] = sorted(functions[name]);
  return { totals: sorted(totals), functions: sorted(functions) };
}

function total(counts) {
  return Object.values(counts).reduce((sum, count) => sum + count, 0);
}

function topFunctions(functions, limit) {
  return Object.entries(functions)
    .map(([name, counts]) => [name, total(counts)])
    .sort((left, right) => right[1] - left[1] || (left[0] < right[0] ? -1 : 1))
    .slice(0, limit);
}

function growth(current, baseline) {
  const kinds = new Set([...Object.keys(current.totals), ...Object.keys(baseline.totals)]);
  const rows = [];
  for (const kind of [...kinds].sort()) {
    const now = current.totals[kind] ?? 0;
    const before = baseline.totals[kind] ?? 0;
    if (now > before) rows.push({ kind, before, now });
  }
  return rows;
}

function functionGrowth(current, baseline, limit) {
  const rows = [];
  for (const [name, counts] of Object.entries(current.functions)) {
    const before = baseline.functions[name] ?? {};
    for (const [kind, now] of Object.entries(counts)) {
      const was = before[kind] ?? 0;
      if (now > was) rows.push({ name, kind, was, now });
    }
  }
  return rows.sort((left, right) => right.now - right.was - (left.now - left.was)).slice(0, limit);
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const current = await scan(options.input);
  const summary = Object.entries(current.totals).map(([kind, count]) => `${kind}=${count}`).join(" ") || "none";
  console.log(`copy ratchet: ${basename(options.input)}: ${Object.keys(current.functions).length} functions with clones; ${summary}`);
  for (const [name, count] of topFunctions(current.functions, options.top)) console.log(`  ${count}\t${name}`);

  if (options.write) {
    // One function per line keeps baseline diffs reviewable.
    const functions = Object.entries(current.functions).map(([name, counts]) => `  ${JSON.stringify(name)}: ${JSON.stringify(counts)}`);
    const text = [
      "{",
      ` "version": 1,`,
      ` "source": ${JSON.stringify(basename(options.input))},`,
      ` "totals": ${JSON.stringify(current.totals)},`,
      ` "functions": {`,
      functions.join(",\n"),
      " }",
      "}",
      "",
    ].join("\n");
    writeFileSync(options.baseline, text);
    console.log(`copy ratchet: baseline written to ${options.baseline}`);
    return;
  }
  if (!existsSync(options.baseline)) throw new Error(`no baseline at ${options.baseline}; run with --write`);
  const baseline = JSON.parse(readFileSync(options.baseline, "utf8"));
  const grown = growth(current, baseline);
  if (grown.length > 0) {
    console.error("copy ratchet: clone counts rose above the baseline:");
    for (const row of grown) console.error(`  ${row.kind}: ${row.before} -> ${row.now}`);
    for (const row of functionGrowth(current, baseline, options.top)) console.error(`  ${row.name} ${row.kind}: ${row.was} -> ${row.now}`);
    process.exitCode = 1;
    return;
  }
  console.log("copy ratchet: no kind rose above the baseline");
}

main().catch((error) => {
  console.error(`copy ratchet: ${error.message}`);
  process.exitCode = 1;
});
