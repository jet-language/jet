#!/usr/bin/env node
import { promises as fs } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const harnessDir = path.dirname(fileURLToPath(import.meta.url));
const repoDir = path.resolve(harnessDir, "../..");
const statusPath = path.join(repoDir, "gauntlet/status.json");
const VERDICT_ORDER = Object.freeze({ loss: 0, unmeasured: 1, parity: 2, win: 3 });
const SUMMARY_KEYS = Object.freeze([
  "cells", "win", "parity", "loss", "unmeasured", "unmeasured_allowed", "unmeasured_required",
  "metric_win", "metric_parity", "metric_loss", "metric_unmeasured", "metric_not_applicable",
]);

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const asObject = (value) => isObject(value) ? value : {};
const asArray = (value) => Array.isArray(value) ? value : [];
const finiteNumber = (value) => typeof value === "number" && Number.isFinite(value) ? value : null;
const countValue = (value, fallback) => Number.isInteger(value) && value >= 0 ? value : fallback;

function normalizeReportPath(value) {
  if (value == null) return null;
  const text = String(value);
  if (!path.isAbsolute(text)) return text.replaceAll(path.sep, "/");
  const relative = path.relative(repoDir, path.resolve(text));
  if (relative && !relative.startsWith("..") && !path.isAbsolute(relative)) return relative.split(path.sep).join("/");
  return text.replaceAll(path.sep, "/");
}

function scopeOf(report) {
  const value = report?.options?.scope ?? report?.publication?.scope ?? report?.scope ?? null;
  if (value === "full") return "full_matrix";
  if (value === "partial") return "partial_entry";
  return value;
}

function metricSourceFor(tier, metric) {
  const record = asObject(tier);
  if (isObject(record.metrics?.[metric])) return record.metrics[metric];
  if (isObject(record[metric])) return record[metric];
  return record;
}

function projectTier(tier, fallbackVerdict = null) {
  const record = asObject(tier);
  const verdict = record.verdict ?? fallbackVerdict ?? null;
  return {
    status: record.status ?? (verdict == null ? "unmeasured" : "measured"),
    jet: finiteNumber(record.jet),
    peer: finiteNumber(record.peer),
    ratio: finiteNumber(record.ratio),
    verdict,
  };
}

function projectPeer(peer, primaryMetric) {
  const record = asObject(peer);
  const comparison = asObject(record.metric_comparisons?.[primaryMetric] ?? record.comparison);
  const sourceTiers = asObject(comparison.tiers ?? record.tiers);
  const fallbackVerdicts = asObject(record.tier_verdicts);
  const tierNames = [...new Set([...Object.keys(sourceTiers), ...Object.keys(fallbackVerdicts)])];
  const tiers = Object.fromEntries(tierNames.map((tier) => [
    tier,
    projectTier(metricSourceFor(sourceTiers[tier], primaryMetric), fallbackVerdicts[tier] ?? null),
  ]));
  return {
    peer: record.language ?? record.name ?? record.peer ?? null,
    verdict: record.verdict ?? comparison.verdict ?? null,
    tiers,
  };
}

function rawRecord(result) {
  const entry = isObject(result?.entry) ? result.entry : {};
  const comparisons = asObject(result?.comparisons);
  const firstComparison = Object.values(comparisons).find((item) => isObject(item));
  const primaryMetric = firstComparison?.primary_metric ?? null;
  const peers = Object.entries(comparisons).map(([language, comparison]) => {
    const item = asObject(comparison);
    const metric = item.primary_metric ?? primaryMetric;
    const tiers = Object.fromEntries(Object.entries(asObject(item.tiers)).map(([tier, value]) => [
      tier,
      { metrics: { [metric]: metricSourceFor(value, metric) } },
    ]));
    const metricComparison = asObject(item.metrics);
    return {
      language,
      verdict: metricComparison[metric]?.verdict ?? item.verdicts?.aot ?? null,
      metric_comparisons: { [metric]: { verdict: metricComparison[metric]?.verdict ?? null, tiers } },
    };
  });
  return {
    entry: entry.name ?? (typeof result?.entry === "string" ? result.entry : null),
    mode: entry.mode ?? result?.mode ?? null,
    primary_metric: primaryMetric,
    verdict: null,
    peers,
    metric_failures: result?.metric_failures ?? [],
    loss_owners: result?.loss_owners ?? [],
  };
}

function rawRecordsByCell(report) {
  const byCell = new Map();
  for (const result of asArray(report?.entries)) {
    const record = rawRecord(result);
    const entry = isObject(result?.entry) ? result.entry : {};
    for (const id of asArray(entry.cells)) {
      const rows = byCell.get(id) ?? [];
      rows.push(record);
      byCell.set(id, rows);
    }
  }
  return byCell;
}

function projectCells(report, primaryMetricByMode) {
  const scoreboard = asObject(report?.scoreboard);
  const rawByCell = rawRecordsByCell(report);
  const declaredCells = asArray(scoreboard.cells).length ? scoreboard.cells : asArray(report?.cells);
  const cells = declaredCells.map((cell) => {
    const source = asObject(cell);
    const records = asArray(source.entries).length ? asArray(source.entries) : (rawByCell.get(source.id) ?? []);
    const record = records[0] ?? null;
    const entry = record?.entry ?? null;
    const mode = record?.mode ?? null;
    const primaryMetric = record?.primary_metric ?? primaryMetricByMode[mode] ?? null;
    const peers = records.flatMap((item) => asArray(item.peers).map((peer) => projectPeer(peer, primaryMetric)));
    const verdict = record?.verdict ?? source.verdict ?? "unmeasured";
    const failures = records.flatMap((item) => asArray(item.failures ?? item.metric_failures));
    const lossOwners = records.flatMap((item) => asArray(item.loss_owners));
    if (!records.length) {
      failures.push(...asArray(source.failures ?? source.metric_failures));
      lossOwners.push(...asArray(source.loss_owners));
    }
    return {
      id: source.id ?? null,
      domain: source.domain ?? null,
      kind: source.kind ?? null,
      entry,
      mode,
      primary_metric: primaryMetric,
      verdict,
      peers,
      failures,
      loss_owners: lossOwners,
    };
  });
  return cells.sort((left, right) => {
    const rank = (value) => VERDICT_ORDER[value] ?? VERDICT_ORDER.unmeasured;
    return rank(left.verdict) - rank(right.verdict) || String(left.id ?? "").localeCompare(String(right.id ?? ""));
  });
}

function projectSummary(report, cells) {
  const source = asObject(report?.scoreboard?.summary);
  const verdicts = cells.map((cell) => cell.verdict);
  const fallback = {
    cells: cells.length,
    win: verdicts.filter((value) => value === "win").length,
    parity: verdicts.filter((value) => value === "parity").length,
    loss: verdicts.filter((value) => value === "loss").length,
    unmeasured: verdicts.filter((value) => value === "unmeasured").length,
    unmeasured_allowed: 0,
    unmeasured_required: verdicts.filter((value) => value === "unmeasured").length,
    metric_win: 0,
    metric_parity: 0,
    metric_loss: 0,
    metric_unmeasured: 0,
    metric_not_applicable: 0,
  };
  return Object.fromEntries(SUMMARY_KEYS.map((key) => [key, countValue(source[key], fallback[key])]));
}

function projectAxisComparison(value) {
  const record = asObject(value);
  return {
    verdict: record.verdict ?? null,
    cold: projectTier(record.cold),
    warm: projectTier(record.warm),
  };
}

function projectAxisMetrics(value) {
  return Object.fromEntries(Object.entries(asObject(value)).map(([id, metric]) => {
    const record = asObject(metric);
    return [id, {
      cold_reload_latency_ms: finiteNumber(record.cold_reload_latency_ms),
      warm_reload_latency_ms: finiteNumber(record.warm_reload_latency_ms),
    }];
  }));
}

function projectAxes(report) {
  return Object.fromEntries(Object.entries(asObject(report?.axes)).map(([id, value]) => {
    const axis = asObject(value);
    const publication = asObject(axis.publication);
    const comparisons = Object.fromEntries(Object.entries(asObject(axis.comparisons))
      .map(([peer, comparison]) => [peer, projectAxisComparison(comparison)]));
    const declaredVerdicts = asObject(axis.verdicts);
    const verdicts = Object.keys(declaredVerdicts).length
      ? Object.fromEntries(Object.entries(declaredVerdicts).map(([peer, verdict]) => [peer, typeof verdict === "string" ? verdict : null]))
      : Object.fromEntries(Object.entries(comparisons).map(([peer, comparison]) => [peer, comparison.verdict]));
    return [id, {
      status: axis.status ?? "unmeasured",
      schema: axis.schema ?? axis.contract?.schema ?? null,
      metric: axis.metric ?? axis.contract?.metric ?? null,
      metrics: projectAxisMetrics(axis.metrics),
      comparisons,
      verdicts,
      publication: {
        status: publication.status ?? "blocked",
        blockers: asArray(publication.blockers),
      },
    }];
  }));
}

function projectPublication(report, scope) {
  const source = asObject(report?.publication);
  const blockers = [...asArray(source.blockers)];
  const fullScope = scope === "full_matrix";
  const axisScope = typeof scope === "string" && scope.startsWith("axis_");
  if (!fullScope && !axisScope && !blockers.includes("run scope is partial; full matrix publication requires no --entry")) {
    blockers.push("run scope is partial; full matrix publication requires no --entry");
  }
  if (!Object.keys(source).length) blockers.push("publication is missing");
  const complete = (fullScope || axisScope) && source.status === "complete" && source.complete === true && blockers.length === 0;
  return {
    scope: source.scope ?? scope,
    status: complete ? "complete" : "incomplete",
    complete,
    blockers,
  };
}

/** Project one dated gauntlet report into the small tracked Tower payload. */
export function projectStatus(report, reportPath = null) {
  if (!isObject(report)) throw new TypeError("gauntlet report must be an object");
  const scoreboard = asObject(report.scoreboard);
  const reproducibility = asObject(report.reproducibility);
  const primaryMetricByMode = asObject(scoreboard.primary_metric_by_mode ?? reproducibility.primary_metric_by_mode);
  const scope = scopeOf(report);
  const suppliedPath = isObject(reportPath) ? reportPath.reportPath : reportPath;
  const cells = projectCells(report, primaryMetricByMode);
  const publication = projectPublication(report, scope);
  return {
    contract: "gauntlet-status-v1",
    generated: report.generated ?? null,
    source: {
      report_path: normalizeReportPath(suppliedPath ?? report.source?.report_path ?? report.report_path ?? null),
      report_contract: report.contract ?? null,
      run_id: report.run_id ?? null,
      scope,
    },
    policy: {
      primary_metric_by_mode: primaryMetricByMode,
      verdict_policy: scoreboard.verdict_policy ?? reproducibility.ratio_verdicts ?? {},
      tier_policy_by_mode: reproducibility.tier_policy_by_mode ?? {},
    },
    summary: projectSummary(report, cells),
    cells,
    axes: projectAxes(report),
    publication,
    provenance: {
      matrix_sha256: report.provenance?.matrix_sha256 ?? null,
      measurement_manifest_sha256: report.provenance?.measurement_manifest_sha256 ?? null,
      corpus_tree_sha256: report.provenance?.corpus_tree_sha256 ?? null,
      jet_binary_sha256: report.provenance?.jet_binary_sha256 ?? null,
    },
  };
}

function parseArgs(argv) {
  let from = null;
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--from") {
      if (i + 1 >= argv.length) throw new Error("--from needs a value");
      from = argv[++i];
      continue;
    }
    if (arg === "--help" || arg === "-h") {
      console.log("usage: node gauntlet/harness/status.mjs --from gauntlet/results/YYYY-MM-DD.json");
      return null;
    }
    throw new Error(`unknown argument: ${arg}`);
  }
  if (!from) throw new Error("--from is required");
  return { from };
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (!options) return;
  const inputPath = path.resolve(process.cwd(), options.from);
  const report = JSON.parse(await fs.readFile(inputPath, "utf8"));
  const status = projectStatus(report, inputPath);
  await fs.writeFile(statusPath, `${JSON.stringify(status, null, 2)}\n`);
  console.log(`status\t${path.relative(repoDir, statusPath).split(path.sep).join("/")}`);
  console.log(`summary\tcells=${status.summary.cells}\twin=${status.summary.win}\tparity=${status.summary.parity}\tloss=${status.summary.loss}\tunmeasured=${status.summary.unmeasured}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(`status: ${error.message}`);
    process.exitCode = 1;
  });
}
