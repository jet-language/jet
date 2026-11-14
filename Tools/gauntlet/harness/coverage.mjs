#!/usr/bin/env node
// The peer-floor loss census over the coverage corpus (#6756).
//
//   node Tools/gauntlet/harness/coverage.mjs --gauntlet REPORT --scriptbench DIR [--out FILE] [--require-empty]
//
// The coverage corpus is every gauntlet program (the measurement manifest's
// corpus.entry_names, each with its peer twins under its entry directory)
// and every ScriptBench workload (Tools/perf/scriptbench/manifest.json, each
// with its peer.* twins), so a fix that only helps a measured cell is caught
// by the programs that use the same feature differently. REPORT is a
// gauntlet run (Tools/gauntlet/results/<date>.json from run.mjs) and DIR a
// ScriptBench --out directory (its cells.tsv). For every program, matched
// peer and metric (the program's time metric and peak memory) the census
// takes the release-binary reading: the gauntlet's aot tier, ScriptBench's
// jet-bin tier. A ratio Jet/peer of 1.00 or more is a loss; a program, peer
// or metric with no reading is uncovered. Losses are grouped by the card
// that owns them (the manifest's loss_owners, the workload's cards).
//
// It prints one line per loss and per uncovered reading, and the totals.
// The exit is 1 when the corpus is short or malformed or any reading is
// uncovered, and with --require-empty also on any loss.
import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { LANGUAGE_SOURCE_DIRS, LOSS_OWNER_CATEGORY_BY_METRIC } from "./run.mjs";

const repoDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const MINIMUM_PROGRAMS = 30;
const MEMORY_METRIC = "runtime_peak_rss_kb";
const GAUNTLET_TIER = "aot";
const SCRIPTBENCH_TIER = "jet-bin";
const SCRIPTBENCH_METRICS = Object.freeze({ wall: "runtime_wall_seconds", rss: MEMORY_METRIC });

// The corpus under `repo`: one row per program with its matched peers, the
// directory holding each peer's twin (run.mjs's: LuaJIT runs the Lua twin),
// and its loss owners.
async function loadCorpus(repo = repoDir) {
  const manifest = JSON.parse(await fs.readFile(path.join(repo, "Tools/gauntlet/measurement-manifest.json"), "utf8"));
  const bench = JSON.parse(await fs.readFile(path.join(repo, "Tools/perf/scriptbench/manifest.json"), "utf8"));
  const programs = [];
  for (const name of manifest.corpus?.entry_names ?? []) {
    const dir = path.join(repo, "Tools/gauntlet/entries", name);
    const entry = JSON.parse(await fs.readFile(path.join(dir, "entry.json"), "utf8"));
    const peers = (entry.languages ?? []).filter((language) => language !== "jet" && language !== "jet-expert" && !entry.non_applicable?.[language]);
    programs.push({
      source: "gauntlet",
      name,
      key: name,
      cells: entry.cells ?? [],
      peers,
      twins: Object.fromEntries(peers.map((peer) => [peer, path.join(dir, LANGUAGE_SOURCE_DIRS[peer] ?? peer)])),
      owners: manifest.loss_owners?.[name] ?? {},
    });
  }
  const peerFiles = Object.fromEntries((bench.peers ?? []).map((peer) => [peer.name, peer.file]));
  for (const workload of bench.workloads ?? []) {
    const dir = path.join(repo, "Tools/perf/scriptbench", workload.dir);
    programs.push({
      source: "scriptbench",
      name: workload.dir,
      key: workload.id,
      cells: [],
      peers: [...(workload.peers ?? [])],
      twins: Object.fromEntries((workload.peers ?? []).map((peer) => [peer, peerFiles[peer] ? path.join(dir, peerFiles[peer]) : null])),
      owners: workload.cards ?? [],
    });
  }
  return programs;
}

// Why `programs` is not a coverage corpus: too few programs, a repeated
// one, or one without a peer twin on disk.
async function corpusIssues(programs) {
  const issues = [];
  if (programs.length < MINIMUM_PROGRAMS) issues.push(`the corpus has ${programs.length} programs, fewer than ${MINIMUM_PROGRAMS}`);
  const seen = new Set();
  for (const program of programs) {
    const id = `${program.source}/${program.name}`;
    if (seen.has(id)) issues.push(`${id} is listed twice`);
    seen.add(id);
    if (program.peers.length === 0) issues.push(`${id} has no peer twin`);
    for (const peer of program.peers) {
      const twin = program.twins[peer];
      if (!twin || !(await fs.stat(twin).then(() => true, () => false))) issues.push(`${id}: the ${peer} twin is missing`);
    }
  }
  return issues;
}

// ScriptBench's cells.tsv as rows keyed by its header.
function parseCells(text) {
  const [header, ...lines] = text.split("\n").filter((line) => line !== "");
  const columns = (header ?? "").split("\t");
  return lines.map((line) => Object.fromEntries(line.split("\t").map((value, index) => [columns[index], value])));
}

function ownerOf(program, metric) {
  if (program.source === "scriptbench") return program.owners.length ? program.owners.join(",") : "unowned";
  const card = program.owners[LOSS_OWNER_CATEGORY_BY_METRIC[metric]];
  return Number.isInteger(card) ? `#${card}` : "unowned";
}

// One reading of a gauntlet program: the aot comparison with `peer` on `metric`.
function gauntletReading(result, peer, metric) {
  const comparison = result?.comparisons?.[peer];
  if (!comparison) return { reason: result ? `no ${peer} comparison` : "the program is not in the gauntlet report" };
  if (comparison.status === "not_applicable" && comparison.basis === "declared-non-applicability") return { not_applicable: true };
  const row = comparison.tiers?.[GAUNTLET_TIER]?.metrics?.[metric];
  if (row?.status === "not_applicable") return { not_applicable: true };
  if (typeof row?.ratio === "number" && Number.isFinite(row.ratio) && row.ratio > 0) return { jet: row.jet, peer: row.peer, ratio: row.ratio, lost: row.ratio >= 1 };
  return { reason: row?.reason ?? `no ${GAUNTLET_TIER} ${metric} reading` };
}

// One reading of a ScriptBench workload: its jet-bin cell for `peer` on
// `metric`. Bench judges the unrounded ratio, so its verdict decides; a peer
// that overran the timeout is a win on wall with no ratio.
function scriptbenchReading(rows, program, peer, metric) {
  const wanted = Object.entries(SCRIPTBENCH_METRICS).find(([, name]) => name === metric)[0];
  const row = rows.find((cell) => cell.workload === program.key && cell.tier === SCRIPTBENCH_TIER && cell.peer === peer && cell.metric === wanted);
  if (!row) return { reason: `no ${SCRIPTBENCH_TIER} ${wanted} cell` };
  const ratio = Number.parseFloat(row.ratio);
  const lost = row.verdict !== "win";
  if (lost && !(ratio > 0)) return { reason: `${SCRIPTBENCH_TIER} ${wanted} cell has no ratio` };
  return { jet: Number(row.jet), peer: Number(row.peer_value), ratio: ratio > 0 ? ratio : null, lost };
}

// The census: every loss and every uncovered reading of `programs`, from a
// gauntlet report and ScriptBench cells, with the losses grouped by owner.
function buildCensus(programs, report, cells) {
  const results = new Map((report?.entries ?? []).map((result) => [result.entry?.name, result]));
  const losses = [];
  const uncovered = [];
  let measured = 0;
  for (const program of programs) {
    const result = results.get(program.name);
    for (const peer of program.peers) {
      const time = program.source === "gauntlet"
        ? (result?.comparisons?.[peer]?.primary_metric ?? "runtime_wall_seconds")
        : SCRIPTBENCH_METRICS.wall;
      for (const metric of [time, MEMORY_METRIC]) {
        const reading = program.source === "gauntlet"
          ? gauntletReading(result, peer, metric)
          : scriptbenchReading(cells, program, peer, metric);
        if (reading.not_applicable) continue;
        const row = { source: program.source, program: program.name, cells: program.cells, peer, metric };
        if (reading.reason) {
          uncovered.push({ ...row, reason: reading.reason });
          continue;
        }
        measured += 1;
        if (!reading.lost) continue;
        losses.push({ ...row, jet: reading.jet, peer_value: reading.peer, ratio: reading.ratio, owner: ownerOf(program, metric) });
      }
    }
  }
  const roots = new Map();
  for (const loss of losses) {
    if (!roots.has(loss.owner)) roots.set(loss.owner, []);
    roots.get(loss.owner).push(loss);
  }
  return {
    programs: programs.length,
    sources: Object.fromEntries(["gauntlet", "scriptbench"].map((source) => [source, programs.filter((program) => program.source === source).length])),
    measured,
    losses,
    uncovered,
    roots: [...roots].sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0).map(([owner, rows]) => ({ owner, losses: rows })),
  };
}

function parseArgs(argv) {
  const options = { gauntlet: null, scriptbench: null, out: null, requireEmpty: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--require-empty") options.requireEmpty = true;
    else if (arg === "--gauntlet" || arg === "--scriptbench" || arg === "--out") {
      const value = argv[index + 1];
      if (!value) throw new Error(`${arg} needs a value`);
      options[arg.slice(2)] = value;
      index += 1;
    } else throw new Error(`unknown argument ${arg}`);
  }
  if (!options.gauntlet || !options.scriptbench) throw new Error("usage: coverage.mjs --gauntlet REPORT --scriptbench DIR [--out FILE] [--require-empty]");
  return options;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const programs = await loadCorpus();
  const issues = await corpusIssues(programs);
  const report = JSON.parse(await fs.readFile(options.gauntlet, "utf8"));
  const cells = parseCells(await fs.readFile(path.join(options.scriptbench, "cells.tsv"), "utf8"));
  const census = buildCensus(programs, report, cells);
  for (const issue of issues) console.log(`corpus\t${issue}`);
  for (const root of census.roots) {
    for (const loss of root.losses) {
      console.log(`loss\t${root.owner}\t${loss.source}/${loss.program}\t${loss.peer}\t${loss.metric}\t${loss.jet}\t${loss.peer_value}\t${loss.ratio.toFixed(3)}`);
    }
  }
  for (const row of census.uncovered) console.log(`uncovered\t${row.source}/${row.program}\t${row.peer}\t${row.metric}\t${row.reason}`);
  console.log(`coverage: ${census.programs} programs (${census.sources.gauntlet} gauntlet, ${census.sources.scriptbench} scriptbench), ${census.measured} readings, ${census.losses.length} losses in ${census.roots.length} roots, ${census.uncovered.length} uncovered`);
  if (options.out) await fs.writeFile(options.out, `${JSON.stringify({ contract: "gauntlet-coverage-census-v1", generated: new Date().toISOString(), gauntlet_report: options.gauntlet, scriptbench: options.scriptbench, corpus_issues: issues, ...census }, null, 2)}\n`);
  if (issues.length || census.uncovered.length || (options.requireEmpty && census.losses.length)) process.exitCode = 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(`coverage: ${error.message}`);
    process.exitCode = 1;
  });
}

export { MINIMUM_PROGRAMS, buildCensus, corpusIssues, loadCorpus, parseCells };
