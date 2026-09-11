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
const RATIO_TIERS = Object.freeze(["aot", "run"]);
const STATUS_RATIO_VERDICTS = Object.freeze({
  rust: { win: "<1", parity: "<=1.05", loss: ">1.05" },
  non_rust: { win: "<1", parity: null, loss: ">=1" },
});
const STATUS_TIER_POLICY_BY_MODE = Object.freeze({
  batch: ["aot", "run"],
  "batch-steps": ["aot", "run"],
  service: ["aot", "run"],
  web: ["aot", "run"],
  "web-app": ["aot"],
});
const STATUS_METRICS_BY_MODE = Object.freeze({
  batch: ["runtime_wall_seconds", "runtime_peak_rss_kb", "runtime_first_stdout_seconds", "cold_build_seconds", "warm_build_seconds", "binary_bytes", "loc", "source_bytes", "tokens", "source_tokens"],
  "batch-steps": ["runtime_wall_seconds", "runtime_peak_rss_kb", "runtime_first_stdout_seconds", "cold_build_seconds", "warm_build_seconds", "binary_bytes", "loc", "source_bytes", "tokens", "source_tokens"],
  service: ["service_latency_ms_p50", "service_latency_ms_p99", "service_startup_seconds", "runtime_peak_rss_kb", "cold_build_seconds", "warm_build_seconds", "binary_bytes", "loc", "source_bytes", "tokens", "source_tokens"],
  web: ["runtime_first_stdout_seconds", "runtime_wall_seconds", "runtime_peak_rss_kb", "cold_build_seconds", "warm_build_seconds", "binary_bytes", "loc", "source_bytes", "tokens", "source_tokens"],
  "web-app": ["runtime_first_stdout_seconds", "runtime_wall_seconds", "runtime_peak_rss_kb", "cold_build_seconds", "warm_build_seconds", "binary_bytes", "loc", "source_bytes", "tokens", "source_tokens"],
});
const STATUS_AOT_ONLY_METRICS = new Set(["cold_build_seconds", "warm_build_seconds", "binary_bytes"]);
const STATUS_POLICY_FAILURE_STATUSES = Object.freeze(["missing", "wrong", "unavailable", "uncovered", "mismatched", "inconclusive"]);
const STATUS_POLICY_TERRITORIES = Object.freeze({
  foundations: Object.freeze([
    { id: "numerics", required: true, cells: ["numerics.float-kernel", "numerics.fft", "numerics.tensor-map", "numerics.int-kernel"], metrics: "all" },
    { id: "text", required: true, cells: ["text.kernel", "text.regex-kernel", "text.regex-find-all-large", "text.report-cli", "text.script"], metrics: "all" },
    { id: "files", required: true, cells: ["files.script", "files.orchestration"], metrics: "all" },
    { id: "concurrency", required: true, cells: ["concurrency.app", "concurrency.service"], metrics: "all" },
    { id: "networking", required: true, cells: ["netserv.client", "netserv.service"], metrics: "all" },
    { id: "build_time", required: true, cells: "all", metrics: ["cold_build_seconds", "warm_build_seconds"] },
    { id: "run_time", required: true, cells: "all", metrics: "runtime" },
  ]),
  critical_areas: Object.freeze([
    { id: "web", required: true, cells: ["webfront.widget", "webfront.app"], metrics: "all" },
    { id: "games", required: false, activation: "first_party_battery", cells: [], metrics: "all" },
    { id: "cli_and_scripts", required: true, cells: ["cli.app", "text.report-cli", "text.script", "formats.csv-cli", "files.script"], metrics: "all" },
    { id: "data_analysis", required: true, cells: ["formats.csv-cli", "numerics.script", "numerics.notebook"], metrics: "all" },
    { id: "backend_services", required: true, cells: ["concurrency.service", "netserv.service"], metrics: "all" },
    { id: "ai_ml_applications", required: false, activation: "first_party_battery", cells: [], metrics: "all" },
    { id: "gui_applications", required: false, activation: "first_party_battery", cells: [], metrics: "all" },
    { id: "embedded", required: true, cells: ["embedded.kernel", "embedded.data"], metrics: "all" },
  ]),
});
const DETAIL_KEYS = Object.freeze(["reason", "unit", "applicability", "evidence", "stats"]);
const STAMP_KEYS = Object.freeze(["measured_at", "measured_iso", "run_id", "source_file"]);

const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const asObject = (value) => isObject(value) ? value : {};
const asArray = (value) => Array.isArray(value) ? value : [];
const finiteNumber = (value) => typeof value === "number" && Number.isFinite(value) ? value : null;
const countValue = (value, fallback) => Number.isInteger(value) && value >= 0 ? value : fallback;
const recognizedVerdict = (value) => ["win", "parity", "loss", "unmeasured", "not_applicable"].includes(value);

function normalizeReportPath(value) {
  if (value == null) return null;
  const text = String(value);
  if (!path.isAbsolute(text)) return text.replaceAll(path.sep, "/");
  const relative = path.relative(repoDir, path.resolve(text));
  if (relative && !relative.startsWith("..") && !path.isAbsolute(relative)) return relative.split(path.sep).join("/");
  return text.replaceAll(path.sep, "/");
}

function reportStamp(report, reportPath = null) {
  const sourceFile = normalizeReportPath(reportPath);
  const generated = typeof report?.generated === "string" ? report.generated : null;
  const pathDate = String(sourceFile ?? "").match(/(?:^|\/)(\d{4}-\d{2}-\d{2})(?:-[^/]*)?\.json$/)?.[1] ?? null;
  const date = generated?.match(/^(\d{4}-\d{2}-\d{2})/)?.[1] ?? pathDate;
  const runId = report?.run_id == null ? "" : String(report.run_id);
  return {
    date: date ?? null,
    generated,
    runId: runId || null,
    sourceFile,
    order: `${date ?? ""}\u0000${runId}\u0000${generated ?? ""}\u0000${sourceFile ?? ""}`,
  };
}

function compareStamps(left, right) {
  return String(left?.order ?? "").localeCompare(String(right?.order ?? ""));
}

function newerStamp(left, right) {
  return !left || (right && compareStamps(right, left) > 0) ? right : left;
}

function metricSourceFor(tier, metric) {
  const record = asObject(tier);
  if (isObject(record.metrics?.[metric])) return record.metrics[metric];
  if (isObject(record[metric])) return record[metric];
  return record;
}

function copyDetails(record, target) {
  for (const key of [...DETAIL_KEYS, ...STAMP_KEYS]) {
    if (Object.hasOwn(record, key)) target[key] = record[key];
  }
  return target;
}

function projectTier(tier, fallbackVerdict = null, metadata = null) {
  const record = asObject(tier);
  const verdict = recognizedVerdict(record.verdict) ? record.verdict : fallbackVerdict;
  const output = {
    status: record.status ?? (verdict == null ? "unmeasured" : "measured"),
    jet: finiteNumber(record.jet),
    peer: finiteNumber(record.peer),
    ratio: finiteNumber(record.ratio),
    verdict,
  };
  copyDetails(record, output);
  if (metadata) {
    for (const key of STAMP_KEYS) {
      if (metadata[key] != null) output[key] = metadata[key];
    }
  }
  return output;
}

// Sample field behind each runtime metric; other metrics are single values.
const SAMPLE_FIELDS = Object.freeze({
  runtime_wall_seconds: "wall_seconds",
  runtime_peak_rss_kb: "peak_rss_kb",
  runtime_first_stdout_seconds: "time_to_first_stdout_seconds",
});

function sampleStats(runtime, field) {
  const record = asObject(runtime);
  const values = asArray(record.samples)
    .map((sample) => finiteNumber(asObject(sample)[field]))
    .filter((value) => value != null)
    .sort((left, right) => left - right);
  const median = finiteNumber(asObject(record.median)[field]);
  const rss = finiteNumber(asObject(record.median).peak_rss_kb)
    ?? (values.length && field === "peak_rss_kb" ? values[values.length >> 1] : null);
  if (!values.length) {
    if (median == null) return null;
    return { samples: null, best: median, median, mean: median, rss };
  }
  const middle = values.length >> 1;
  return {
    samples: values.length,
    best: values[0],
    median: median ?? (values.length % 2 ? values[middle] : (values[middle - 1] + values[middle]) / 2),
    mean: values.reduce((sum, value) => sum + value, 0) / values.length,
    rss,
  };
}

// Best / median / mean per side for one tier of one metric. The AOT tier is
// the `rows.jet` measurement; run and dev tiers live under `jet_tiers`. Legacy
// reports (no `tiers` block) measured `rows.jet` as their single tier.
function decorateMetricValue(value, result, language, jetLanguage, metric, tier, structured = true) {
  const output = { ...asObject(value) };
  const field = SAMPLE_FIELDS[metric];
  if (!field || Object.hasOwn(output, "stats")) return output;
  const jetRuntime = !structured || tier === "aot"
    ? result?.rows?.[jetLanguage]?.runtime
    : result?.jet_tiers?.[tier]?.runtime;
  const jet = sampleStats(jetRuntime, field);
  const peer = sampleStats(result?.rows?.[language]?.runtime, field);
  if (jet || peer) output.stats = { jet, peer };
  return output;
}

function legacyTierNames(result) {
  const declared = Object.keys(asObject(result?.jet_tiers));
  if (declared.includes("run")) return ["run"];
  if (declared.length) return [declared[0]];
  return ["aot"];
}

function normalizeRawPeer(language, comparison, result) {
  const item = asObject(comparison);
  const structured = isObject(item.metrics) || isObject(item.tiers) || item.primary_metric != null;
  const topMetrics = structured ? asObject(item.metrics) : item;
  const sourceTierNames = structured ? Object.keys(asObject(item.tiers)) : legacyTierNames(result);
  const metricNames = new Set(Object.keys(topMetrics));
  for (const tier of sourceTierNames) {
    const tierMetrics = asObject(item.tiers?.[tier]?.metrics);
    Object.keys(tierMetrics).forEach((metric) => metricNames.add(metric));
  }
  const jetLanguage = item.jet_configuration === "jet-expert" ? "jet-expert" : "jet";
  const metricComparisons = Object.fromEntries([...metricNames].map((metric) => {
    const top = asObject(topMetrics[metric]);
    const tiers = Object.fromEntries(sourceTierNames.map((tier) => {
      const tierRecord = asObject(item.tiers?.[tier]);
      const source = structured
        ? (tierRecord.metrics?.[metric] ?? (tier === "aot" ? top : null))
        : top;
      return [tier, decorateMetricValue(source, result, language, jetLanguage, metric, tier, structured)];
    }));
    const fallback = top.verdict ?? null;
    return [metric, { ...top, tiers, verdict: recognizedVerdict(fallback) ? fallback : null }];
  }));
  const output = {
    language,
    peer: language,
    metric_comparisons: metricComparisons,
    verdict: item.verdict ?? null,
  };
  for (const key of ["applicable", "status", "basis", "jet_configuration", "required_tiers", "tier_policy", "peer_sample", "tier_verdicts", "metric_verdicts", "metric_failures"]) {
    if (Object.hasOwn(item, key)) output[key] = item[key];
  }
  return output;
}

function rawRecord(result) {
  const entry = isObject(result?.entry) ? result.entry : {};
  const comparisons = asObject(result?.comparisons);
  const firstComparison = Object.values(comparisons).find((item) => isObject(item));
  const primaryMetric = firstComparison?.primary_metric ?? null;
  const peers = Object.entries(comparisons).map(([language, comparison]) =>
    normalizeRawPeer(language, comparison, result));
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

function normalizeScoreboardRecord(record) {
  const source = asObject(record);
  return {
    entry: source.entry ?? null,
    mode: source.mode ?? null,
    primary_metric: source.primary_metric ?? null,
    verdict: source.verdict ?? source.primary_verdict ?? null,
    peers: asArray(source.peers).map((peer) => ({
      ...peer,
      language: peer.language ?? peer.peer ?? null,
      peer: peer.peer ?? peer.language ?? null,
    })),
    metric_failures: source.metric_failures ?? source.failures ?? [],
    loss_owners: source.loss_owners ?? [],
  };
}

function recordsForCell(source, rawByCell) {
  const scoreboardRecords = asArray(source?.entries).map(normalizeScoreboardRecord);
  const rawRecords = rawByCell.get(source?.id) ?? [];
  if (!rawRecords.length) return scoreboardRecords;
  return rawRecords.map((raw, index) => {
    const scoreboard = scoreboardRecords.find((item) =>
      item.entry === raw.entry && item.mode === raw.mode) ?? scoreboardRecords[index];
    if (!scoreboard) return raw;
    const scoreboardPeers = new Map(scoreboard.peers.map((peer) => [peer.peer ?? peer.language, peer]));
    const peers = raw.peers.length
      ? raw.peers.map((peer) => {
        const supplemental = scoreboardPeers.get(peer.peer ?? peer.language);
        if (!supplemental) return peer;
        return {
          ...peer,
          ...supplemental,
          verdict: supplemental.verdict ?? peer.verdict,
          metric_comparisons: peer.metric_comparisons ?? supplemental.metric_comparisons,
        };
      })
      : scoreboard.peers;
    return {
      ...raw,
      entry: raw.entry ?? scoreboard.entry,
      mode: raw.mode ?? scoreboard.mode,
      primary_metric: raw.primary_metric ?? scoreboard.primary_metric,
      verdict: scoreboard.verdict ?? raw.verdict,
      peers,
      metric_failures: scoreboard.metric_failures.length ? scoreboard.metric_failures : raw.metric_failures,
      loss_owners: scoreboard.loss_owners.length ? scoreboard.loss_owners : raw.loss_owners,
    };
  });
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

function declaredCells(report, rawByCell) {
  const byId = new Map();
  const scoreboardCells = asArray(report?.scoreboard?.cells);
  for (const cell of scoreboardCells) {
    const source = asObject(cell);
    if (source.id == null) continue;
    byId.set(source.id, source);
  }
  for (const [id] of rawByCell) {
    if (!byId.has(id)) byId.set(id, { id });
  }
  return [...byId.values()];
}

function recordPeers(record, primaryMetric) {
  return asArray(record?.peers).map((peer) => projectPeer(peer, primaryMetric));
}

function projectPeer(peer, primaryMetric) {
  const record = asObject(peer);
  const comparisonMap = asObject(record.metric_comparisons);
  const metrics = Object.fromEntries(Object.entries(comparisonMap).map(([metric, value]) => {
    const comparison = asObject(value);
    const sourceTiers = asObject(comparison.tiers ?? record.tiers);
    const fallbackVerdicts = asObject(record.tier_verdicts);
    const tierNames = [...new Set([...Object.keys(sourceTiers), ...Object.keys(fallbackVerdicts)])];
    const tiers = Object.fromEntries(tierNames.map((tier) => [
      tier,
      projectTier(metricSourceFor(sourceTiers[tier], metric), fallbackVerdicts[tier] ?? null),
    ]));
    return [metric, {
      ...comparison,
      verdict: recognizedVerdict(comparison.verdict) ? comparison.verdict : null,
      tiers,
    }];
  }));
  const primary = metrics[primaryMetric];
  const output = {
    peer: record.language ?? record.name ?? record.peer ?? null,
    verdict: recognizedVerdict(record.verdict) ? record.verdict : null,
    tiers: primary?.tiers ?? {},
    metrics,
    metric_comparisons: metrics,
  };
  for (const key of ["language", "applicable", "status", "basis", "jet_configuration", "required_tiers", "tier_policy", "peer_sample", "tier_verdicts", "metric_verdicts", "metric_failures"]) {
    if (Object.hasOwn(record, key)) output[key] = record[key];
  }
  return output;
}

function projectRecord(record, primaryMetricByMode, stamp = null) {
  const source = asObject(record);
  const primaryMetric = source.primary_metric ?? primaryMetricByMode[source.mode] ?? null;
  const peers = recordPeers(source, primaryMetric);
  return {
    entry: source.entry ?? null,
    mode: source.mode ?? null,
    primary_metric: primaryMetric,
    verdict: recognizedVerdict(source.verdict) ? source.verdict : null,
    peers,
    metric_failures: asArray(source.metric_failures ?? source.failures),
    loss_owners: asArray(source.loss_owners),
    stamp,
  };
}

function tierPolicyFor(mode, policyByMode) {
  const value = policyByMode?.[mode];
  if (Array.isArray(value)) return value;
  if (isObject(value)) return Object.entries(value).filter(([, item]) => item?.required).map(([tier]) => tier);
  return ["aot", "run"];
}

function ratioVerdict(ratio, peer) {
  if (!Number.isFinite(ratio)) return "unmeasured";
  if (String(peer ?? "").replace(/-expert$/, "") === "rust") {
    if (ratio < 1) return "win";
    if (ratio <= 1.05) return "parity";
    return "loss";
  }
  return ratio < 1 ? "win" : "loss";
}

function reduceVerdicts(values) {
  const comparable = values.filter((value) => value !== "not_applicable");
  if (!comparable.length) return "not_applicable";
  if (comparable.some((value) => !["win", "parity", "loss"].includes(value))) return "unmeasured";
  if (comparable.includes("loss")) return "loss";
  if (comparable.includes("parity")) return "parity";
  return "win";
}

function isMeasured(value) {
  const jet = finiteNumber(value?.jet);
  const peer = finiteNumber(value?.peer);
  const ratio = finiteNumber(value?.ratio);
  return value?.status === "measured" && jet != null && jet > 0 &&
    peer != null && peer > 0 && ratio != null && ratio > 0 &&
    ratio === jet / peer;
}

function metricVerdict(peer, metric, mode, policyByMode) {
  const comparison = asObject(peer?.metric_comparisons?.[metric] ?? peer?.metrics?.[metric]);
  const tiers = asObject(comparison.tiers);
  const hasExplicitPolicy = Array.isArray(peer?.required_tiers) || isObject(peer?.tier_policy);
  const declared = asArray(peer?.required_tiers).length
    ? peer.required_tiers
    : (hasExplicitPolicy ? tierPolicyFor(mode, policyByMode) : Object.keys(tiers));
  const ratioTiers = declared.filter((tier) => RATIO_TIERS.includes(tier));
  const tierNames = ratioTiers.length ? ratioTiers : Object.keys(tiers);
  if (!tierNames.length) {
    const fallback = peer?.metric_verdicts?.[metric] ?? comparison.verdict;
    return recognizedVerdict(fallback) ? fallback : "unmeasured";
  }
  const values = tierNames.map((tier) => {
    const item = tiers[tier];
    if (!item) return "unmeasured";
    if (item.status === "not_applicable") return "not_applicable";
    if (isMeasured(item)) return ratioVerdict(item.ratio, peer.peer);
    return "unmeasured";
  });
  return reduceVerdicts(values);
}
function statusMatrixMode(cell) {
  const id = String(cell?.id ?? "");
  if (id === "webfront.app") return "web-app";
  if (cell?.kind === "service" || id.startsWith("netserv.") || id === "concurrency.service") return "service";
  if (cell?.kind === "web" || id === "webfront.widget") return "web";
  if (cell?.kind === "cli" || cell?.kind === "scripting") return "batch-steps";
  return "batch";
}

function statusMatrixPeerLanguages(matrix, cell = null) {
  const rails = matrix?.rails ?? {};
  const incumbent = cell?.domain && isObject(rails.incumbents) ? rails.incumbents[cell.domain] : null;
  const peers = [
    ...(Array.isArray(rails.always) ? rails.always : []),
    ...(Array.isArray(rails.perf) ? rails.perf : []),
    ...(typeof incumbent === "string" ? [incumbent] : (cell ? [] : Object.values(isObject(rails.incumbents) ? rails.incumbents : {}))),
  ].filter((language) => typeof language === "string" && language !== "jet" && !language.endsWith("-expert"));
  return [...new Set(peers.length ? peers : ["rust"])].sort();
}

function statusPolicyIssues(matrix) {
  const policy = matrix?.strict_performance_policy;
  if (!isObject(policy)) return ["matrix is missing the strict performance policy"];
  const issues = [];
  if (policy.schema !== "jet.strict-performance-policy.v1" ||
      policy.id !== "AGENTS.md#strict-performance-gate" ||
      policy.version !== 1 ||
      policy.comparison !== "per_cell_and_metric") {
    issues.push("matrix strict performance policy identity is not ratified");
  }
  if (JSON.stringify(policy.comparators) !== JSON.stringify(STATUS_RATIO_VERDICTS)) {
    issues.push("matrix strict performance policy comparator differs from the ratified law");
  }
  if (!Array.isArray(policy.failure_statuses) ||
      JSON.stringify(policy.failure_statuses) !== JSON.stringify(STATUS_POLICY_FAILURE_STATUSES)) {
    issues.push("matrix strict performance policy failure statuses are incomplete");
  }
  for (const category of ["foundations", "critical_areas"]) {
    const expected = STATUS_POLICY_TERRITORIES[category];
    const actual = Array.isArray(policy[category]) ? policy[category] : [];
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
      issues.push(`matrix strict performance policy ${category} do not match the ratified territory`);
    }
  }
  if (policy.first_party_battery_required !== true ||
      policy.historical_receipts !== "immutable_recorded_policy") {
    issues.push("matrix strict performance policy does not preserve the first-party and historical-evidence rules");
  }
  if (typeof policy.text !== "string" ||
      !policy.text.includes("Never average away a loss") ||
      !policy.text.includes("Historical receipts remain immutable")) {
    issues.push("matrix strict performance policy text is missing AGENTS.md law");
  }
  return issues;
}

function statusMetricComparableAtTier(metric, tier) {
  return tier === "aot" || !STATUS_AOT_ONLY_METRICS.has(metric);
}
function statusHasStructuralReason(...records) {
  return records.some((record) => {
    const source = asObject(record);
    return ["reason", "basis", "applicability"].some((key) => {
      const value = source[key];
      return (typeof value === "string" && value.trim().length > 0) ||
        (isObject(value) && Object.keys(value).length > 0);
    });
  });
}

function statusMetricFailures(cellId, peerName, peer, metric, mode, cellFailures = [], cellLossOwners = []) {
  const prefix = `${cellId}/${peerName}/${metric}`;
  if (!peer) return [`${prefix}: missing peer row`];
  if (peer.applicable === false || peer.status === "not_applicable") {
    if (peer.status !== "not_applicable" || !statusHasStructuralReason(peer)) {
      return [`${prefix}: peer not_applicable lacks an explicit structural reason`];
    }
    return [];
  }
  if (peer.status !== "ok" && peer.status !== "measured") {
    return [`${prefix}: peer status ${peer.status ?? "missing"} is not publishable`];
  }
  const comparison = asObject(peer.metric_comparisons?.[metric] ?? peer.metrics?.[metric]);
  const tiers = asObject(comparison.tiers);
  const declared = asArray(peer.required_tiers).length
    ? peer.required_tiers
    : (isObject(peer.tier_policy) ? Object.keys(peer.tier_policy) : (STATUS_TIER_POLICY_BY_MODE[mode] ?? ["aot", "run"]));
  const ratioTiers = [...new Set(declared.filter((tier) => RATIO_TIERS.includes(tier)))];
  const values = [];
  const issues = [];
  for (const tier of ratioTiers) {
    const item = tiers[tier];
    if (!item) {
      issues.push(`${prefix}: ${tier} tier is missing`);
      values.push("unmeasured");
      continue;
    }
    if (!statusMetricComparableAtTier(metric, tier)) {
      if (item.status !== "not_applicable") {
        issues.push(`${prefix}: ${tier} tier must be not_applicable`);
      } else if (!statusHasStructuralReason(item, comparison)) {
        issues.push(`${prefix}: ${tier} not_applicable lacks an explicit structural reason`);
      }
      values.push("not_applicable");
      continue;
    }
    if (!isMeasured(item) || item.verdict !== ratioVerdict(item.ratio, peerName)) {
      issues.push(`${prefix}: ${tier} tier is missing or has an invalid strict ratio`);
      values.push("unmeasured");
      continue;
    }
    values.push(ratioVerdict(item.ratio, peerName));
  }
  const verdict = reduceVerdicts(values);
  if (!["win", "parity", "not_applicable"].includes(verdict)) {
    issues.push(`${prefix}: metric ${verdict}`);
  }
  if (verdict === "loss") {
    const owner = asArray(peer.metric_failures).find((failure) => failure?.metric === metric)?.owner ??
      asArray(cellFailures).flatMap((failure) => asArray(failure?.peers))
        .find((failure) => failure?.metric === metric && (failure?.peer ?? failure?.language) === peerName)?.owner ??
      asArray(cellLossOwners).find((owner) => owner?.metric === metric && (owner?.peer ?? owner?.language) === peerName);
    if (!owner || owner.status !== "live") issues.push(`${prefix}: loss owner is missing or not live`);
  }
  return issues;
}

/**
 * Validate the tracked status projection without trusting its declared
 * verdicts. Every matrix cell, declared rail, comparable metric, and ratio
 * tier gets its own addressable failure.
 */
export function validateStatusGate(status, matrix) {
  const issues = statusPolicyIssues(matrix);
  const matrixCells = Array.isArray(matrix?.cells) ? matrix.cells : [];
  const statusCells = asArray(status?.cells);
  const byId = new Map(statusCells.map((cell) => [cell?.id, cell]));
  const matrixIds = new Set(matrixCells.map((cell) => cell.id));
  if (matrixIds.size !== matrixCells.length) issues.push("matrix declares duplicate cell identities");
  for (const cell of statusCells) {
    if (!matrixIds.has(cell?.id)) issues.push(`${cell?.id ?? "<unknown>"}/peer/metric: status declares unknown matrix cell`);
  }
  for (const matrixCell of matrixCells) {
    const id = matrixCell.id;
    const expectedPeers = statusMatrixPeerLanguages(matrix, matrixCell);
    const cell = byId.get(id);
    if (cell) {
      const declaredPeers = asArray(cell.peers).map((peer) => peer?.peer ?? peer?.language);
      if (new Set(declaredPeers).size !== declaredPeers.length) {
        issues.push(`${id}/peer/metric: status declares duplicate peer rows`);
      }
      for (const peerName of declaredPeers) {
        if (!expectedPeers.includes(peerName)) issues.push(`${id}/${peerName ?? "peer"}/metric: status declares an unknown peer`);
      }
    }
    const mode = cell?.mode ?? statusMatrixMode(matrixCell);
    const metrics = STATUS_METRICS_BY_MODE[mode] ?? STATUS_METRICS_BY_MODE.batch;
    for (const peerName of expectedPeers) {
      const peer = asArray(cell?.peers).find((candidate) => (candidate?.peer ?? candidate?.language) === peerName) ?? null;
      for (const metric of metrics) issues.push(...statusMetricFailures(id, peerName, peer, metric, mode, cell?.failures, cell?.loss_owners));
    }
    if (!cell) continue;
    if (!recognizedVerdict(cell.verdict) || !["win", "parity"].includes(cell.verdict)) {
      issues.push(`${id}/peer/metric: cell ${cell.verdict ?? "unmeasured"}`);
    }
  }
  return [...new Set(issues)];
}


function addCellMetadata(cell, stamp) {
  if (!stamp) return;
  cell.measured_at = stamp.date;
  cell.measured_iso = stamp.generated;
  cell.run_id = stamp.runId;
  cell.source_file = stamp.sourceFile;
}

function projectCells(report, primaryMetricByMode, stamp = null, { recompute = false, tierPolicyByMode = {} } = {}) {
  const rawByCell = rawRecordsByCell(report);
  const cells = declaredCells(report, rawByCell).map((source) => {
    const records = recordsForCell(source, rawByCell);
    const projectedRecords = records.map((record) => projectRecord(record, primaryMetricByMode, stamp));
    const record = projectedRecords[0] ?? null;
    const entry = record?.entry ?? null;
    const mode = record?.mode ?? null;
    const primaryMetric = record?.primary_metric ?? primaryMetricByMode[mode] ?? null;
    const peers = projectedRecords.flatMap((item) => item.peers);
    const metricNames = [...new Set(peers.flatMap((peer) => Object.keys(peer.metric_comparisons ?? {})))];
    const metricVerdicts = Object.fromEntries(metricNames.map((metric) => [
      metric,
      reduceVerdicts(peers.map((peer) => metricVerdict(peer, metric, mode, tierPolicyByMode))),
    ]));
    const verdict = recompute
      ? (primaryMetric && Object.hasOwn(metricVerdicts, primaryMetric)
        ? metricVerdicts[primaryMetric]
        : (metricNames.length ? reduceVerdicts(Object.values(metricVerdicts)) : (record?.verdict ?? source.verdict ?? "unmeasured")))
      : (record?.verdict ?? source.verdict ?? "unmeasured");
    const cell = {
      id: source.id ?? null,
      domain: source.domain ?? null,
      kind: source.kind ?? null,
      entry,
      mode,
      primary_metric: primaryMetric,
      verdict,
      peers,
      metric_verdicts: metricVerdicts,
      failures: projectedRecords.flatMap((item) => item.metric_failures),
      loss_owners: projectedRecords.flatMap((item) => item.loss_owners),
    };
    if (!projectedRecords.length) {
      cell.failures.push(...asArray(source.failures ?? source.metric_failures));
      cell.loss_owners.push(...asArray(source.loss_owners));
    }
    if (stamp && projectedRecords.length && records.some((item) => asArray(item.peers).some((peer) =>
      Object.values(asObject(peer.metric_comparisons)).some((comparison) =>
        Object.values(asObject(comparison?.tiers)).some(isMeasured))))) {
      addCellMetadata(cell, stamp);
    }
    return cell;
  });
  return cells.sort((left, right) => {
    const rank = (value) => VERDICT_ORDER[value] ?? VERDICT_ORDER.unmeasured;
    return rank(left.verdict) - rank(right.verdict) || String(left.id ?? "").localeCompare(String(right.id ?? ""));
  });
}

function projectSummary(report, cells, { recomputeMetrics = false } = {}) {
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
  const metricVerdicts = cells.flatMap((cell) => Object.values(cell.metric_verdicts ?? {}));
  if (recomputeMetrics) {
    for (const verdict of metricVerdicts) {
      if (Object.hasOwn(fallback, `metric_${verdict}`)) fallback[`metric_${verdict}`] += 1;
    }
    return fallback;
  }
  const source = asObject(report?.scoreboard?.summary);
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
    const output = {
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
    };
    if (Object.hasOwn(axis, "measurements")) output.measurements = asArray(axis.measurements);
    return [id, output];
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

function projectOneReport(report, reportPath = null) {
  if (!isObject(report)) throw new TypeError("gauntlet report must be an object");
  const scoreboard = asObject(report.scoreboard);
  const reproducibility = asObject(report.reproducibility);
  const primaryMetricByMode = asObject(scoreboard.primary_metric_by_mode ?? reproducibility.primary_metric_by_mode);
  const scope = scopeOf(report);
  const suppliedPath = isObject(reportPath) ? reportPath.reportPath : reportPath;
  const stamp = reportStamp(report, suppliedPath);
  const cells = projectCells(report, primaryMetricByMode, stamp, {
    recompute: true,
    tierPolicyByMode: reproducibility.tier_policy_by_mode ?? {},
  });
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
      strict_performance_policy: reproducibility.strict_performance_policy ?? scoreboard.strict_performance_policy ?? null,
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

function scopeOf(report) {
  const value = report?.options?.scope ?? report?.publication?.scope ?? report?.scope ?? null;
  if (value === "full") return "full_matrix";
  if (value === "partial") return "partial_entry";
  return value;
}

function normalizeInputReports(input, reportPaths = []) {
  return asArray(input).map((item, index) => {
    const report = isObject(item?.report) ? item.report : item;
    const reportPath = isObject(item?.report) ? item.reportPath : reportPaths[index];
    if (!isObject(report)) throw new TypeError("gauntlet report must be an object");
    const scoreboard = asObject(report.scoreboard);
    const reproducibility = asObject(report.reproducibility);
    return {
      report,
      reportPath,
      stamp: reportStamp(report, reportPath),
      primaryMetricByMode: asObject(scoreboard.primary_metric_by_mode ?? reproducibility.primary_metric_by_mode),
      tierPolicyByMode: reproducibility.tier_policy_by_mode ?? {},
    };
  }).sort((left, right) => compareStamps(left.stamp, right.stamp));
}

function projectedPart(part) {
  const rawByCell = rawRecordsByCell(part.report);
  return {
    ...part,
    cells: declaredCells(part.report, rawByCell).map((source) => ({
      source,
      stamp: part.stamp,
      records: recordsForCell(source, rawByCell).map((record) =>
        projectRecord(record, part.primaryMetricByMode, part.stamp)),
    })),
    axes: projectAxes(part.report),
  };
}

function candidateMetric(peer, metric, tier) {
  const comparison = asObject(peer?.metric_comparisons?.[metric]);
  return comparison.tiers?.[tier] ?? null;
}

function chooseMetricCandidate(candidates) {
  const ordered = [...candidates].sort((left, right) => compareStamps(left.stamp, right.stamp));
  const measured = ordered.filter((item) => isMeasured(item.value));
  if (measured.length) return measured[measured.length - 1];
  return ordered.length ? ordered[ordered.length - 1] : null;
}

function mergeAxisParts(parts) {
  const byId = new Map();
  for (const part of parts) {
    for (const [id, axis] of Object.entries(part.report.axes ?? {})) {
      const state = byId.get(id) ?? { latest: null, parts: [] };
      if (!state.latest || compareStamps(state.latest.stamp, part.stamp) < 0) {
        state.latest = { axis, stamp: part.stamp, projected: part.axes[id] };
      }
      state.parts.push({ axis, stamp: part.stamp, projected: part.axes[id] });
      byId.set(id, state);
    }
  }
  return Object.fromEntries([...byId.entries()].map(([id, state]) => {
    const latest = state.latest?.projected ?? {};
    const peers = new Set(state.parts.flatMap((part) => Object.keys(part.projected?.comparisons ?? {})));
    const comparisons = Object.fromEntries([...peers].sort().map((peer) => {
      const phaseValues = {};
      for (const phase of ["cold", "warm"]) {
        const values = state.parts
          .map((part) => ({ value: part.projected?.comparisons?.[peer]?.[phase], stamp: part.stamp }))
          .filter((item) => item.value);
        const selected = chooseMetricCandidate(values.map((item) => ({ value: item.value, stamp: item.stamp })));
        phaseValues[phase] = selected?.value ?? projectTier(null);
      }
      const latestComparison = [...state.parts]
        .reverse()
        .map((part) => part.projected?.comparisons?.[peer])
        .find(Boolean) ?? {};
      return [peer, {
        ...latestComparison,
        cold: phaseValues.cold,
        warm: phaseValues.warm,
      }];
    }));
    const verdicts = Object.fromEntries(Object.entries(comparisons).map(([peer, comparison]) => [
      peer,
      comparison.verdict ?? null,
    ]));
    const measured = Object.values(comparisons).some((comparison) =>
      [comparison.cold, comparison.warm].some((phase) => phase?.status === "measured"));
    const output = {
      ...latest,
      status: measured ? (latest.status === "complete" ? "complete" : "measured") : (latest.status ?? "unmeasured"),
      comparisons,
      verdicts,
    };
    return [id, output];
  }));
}

function mergeCellParts(parts, cellId, primaryMetricByMode, tierPolicyByMode) {
  const definitions = parts
    .flatMap((part) => part.cells.filter((cell) => cell.source.id === cellId))
    .sort((left, right) => compareStamps(left.stamp, right.stamp));
  const latest = definitions.at(-1)?.source ?? { id: cellId };
  const records = definitions.flatMap((part) => part.records.map((record) => ({ record, stamp: part.stamp })));
  const entryCandidate = records.filter((item) => item.record.entry != null).at(-1)?.record ?? null;
  const mode = entryCandidate?.mode ?? latest.mode ?? null;
  const primaryMetric = entryCandidate?.primary_metric ?? primaryMetricByMode[mode] ?? latest.primary_metric ?? null;
  const peerNames = [...new Set(records.flatMap((item) => item.record.peers.map((peer) => peer.peer).filter(Boolean)))];
  const peers = peerNames.sort().map((peerName) => {
    const peerCandidates = records.flatMap(({ record, stamp }) =>
      record.peers.filter((peer) => peer.peer === peerName).map((peer) => ({ peer, stamp })));
    const latestPeer = peerCandidates.at(-1)?.peer ?? { peer: peerName };
    const metricNames = [...new Set(peerCandidates.flatMap(({ peer }) =>
      Object.keys(peer.metric_comparisons ?? {})))];
    if (primaryMetric && !metricNames.includes(primaryMetric)) metricNames.push(primaryMetric);
    const metrics = Object.fromEntries(metricNames.map((metric) => {
      const tierNames = [...new Set(peerCandidates.flatMap(({ peer }) =>
        Object.keys(peer.metric_comparisons?.[metric]?.tiers ?? {})))];
      const hasExplicitPolicy = Array.isArray(latestPeer.required_tiers) || isObject(latestPeer.tier_policy);
      const required = asArray(latestPeer.required_tiers).length
        ? latestPeer.required_tiers
        : (hasExplicitPolicy
          ? tierPolicyFor(mode, tierPolicyByMode)
          : tierNames.filter((tier) => RATIO_TIERS.includes(tier)));
      for (const tier of required) if (!tierNames.includes(tier)) tierNames.push(tier);
      const tiers = Object.fromEntries(tierNames.map((tier) => {
        const values = peerCandidates
          .map(({ peer, stamp }) => ({ value: candidateMetric(peer, metric, tier), stamp }))
          .filter((item) => item.value);
        const selected = chooseMetricCandidate(values);
        return [tier, selected
          ? projectTier(selected.value, null, {
            measured_at: selected.stamp.date,
            measured_iso: selected.stamp.generated,
            run_id: selected.stamp.runId,
            source_file: selected.stamp.sourceFile,
          })
          : projectTier(null)];
      }));
      const latestComparison = [...peerCandidates]
        .reverse()
        .map(({ peer }) => peer.metric_comparisons?.[metric])
        .find(Boolean) ?? {};
      return [metric, { ...latestComparison, tiers }];
    }));
    const output = projectPeer({
      ...latestPeer,
      language: peerName,
      peer: peerName,
      metric_comparisons: metrics,
    }, primaryMetric);
    output.verdict = metricVerdict(output, primaryMetric, mode, tierPolicyByMode);
    return output;
  });
  const metricNames = [...new Set(peers.flatMap((peer) => Object.keys(peer.metric_comparisons ?? {})))];
  const metricVerdicts = Object.fromEntries(metricNames.map((metric) => [
    metric,
    reduceVerdicts(peers.map((peer) => metricVerdict(peer, metric, mode, tierPolicyByMode))),
  ]));
  const measuredCandidates = records.flatMap(({ record, stamp }) => record.peers.flatMap((peer) =>
    Object.values(peer.metric_comparisons ?? {}).flatMap((comparison) =>
      Object.values(asObject(comparison?.tiers)).filter(isMeasured).map(() => stamp))));
  const measuredStamp = measuredCandidates.reduce((latestStamp, stamp) => newerStamp(latestStamp, stamp), null);
  const cell = {
    id: latest.id ?? cellId,
    domain: latest.domain ?? null,
    kind: latest.kind ?? null,
    entry: entryCandidate?.entry ?? latest.entry ?? null,
    mode,
    primary_metric: primaryMetric,
    verdict: primaryMetric && Object.hasOwn(metricVerdicts, primaryMetric)
      ? metricVerdicts[primaryMetric]
      : (metricNames.length ? reduceVerdicts(Object.values(metricVerdicts)) : "unmeasured"),
    peers,
    metric_verdicts: metricVerdicts,
    failures: records.flatMap(({ record }) => record.metric_failures),
    loss_owners: records.flatMap(({ record }) => record.loss_owners),
  };
  if (measuredStamp) {
    cell.measured_at = measuredStamp.date;
    cell.measured_iso = measuredStamp.generated;
    cell.run_id = measuredStamp.runId;
    cell.source_file = measuredStamp.sourceFile;
  }
  return cell;
}

export function mergeStatus(input, reportPaths = []) {
  const normalized = normalizeInputReports(input, reportPaths).map(projectedPart);
  if (!normalized.length) throw new Error("no gauntlet report files found");
  const primaryMetricByMode = Object.assign({}, ...normalized.map((part) => part.primaryMetricByMode));
  const tierPolicyByMode = Object.assign({}, ...normalized.map((part) => part.tierPolicyByMode));
  const cellIds = [...new Set(normalized.flatMap((part) => part.cells.map((cell) => cell.source.id).filter(Boolean)))];
  const cells = cellIds
    .map((id) => mergeCellParts(normalized, id, primaryMetricByMode, tierPolicyByMode))
    .sort((left, right) => String(left.id ?? "").localeCompare(String(right.id ?? "")));
  const metricVerdicts = cells.flatMap((cell) => Object.values(cell.metric_verdicts ?? {}));
  const summary = {
    cells: cells.length,
    win: cells.filter((cell) => cell.verdict === "win").length,
    parity: cells.filter((cell) => cell.verdict === "parity").length,
    loss: cells.filter((cell) => cell.verdict === "loss").length,
    unmeasured: cells.filter((cell) => cell.verdict === "unmeasured").length,
    unmeasured_allowed: 0,
    unmeasured_required: cells.filter((cell) => cell.verdict === "unmeasured").length,
    metric_win: metricVerdicts.filter((value) => value === "win").length,
    metric_parity: metricVerdicts.filter((value) => value === "parity").length,
    metric_loss: metricVerdicts.filter((value) => value === "loss").length,
    metric_unmeasured: metricVerdicts.filter((value) => value === "unmeasured").length,
    metric_not_applicable: metricVerdicts.filter((value) => value === "not_applicable").length,
  };
  const latest = normalized.at(-1);
  const runIds = [...new Set(normalized.map((part) => part.stamp.runId).filter(Boolean))];
  const files = [...new Set(normalized.map((part) => part.stamp.sourceFile).filter(Boolean))];
  const latestReport = latest.report;
  const historicalReceipts = normalized.map((part) => ({
    run_id: part.stamp.runId,
    generated: part.stamp.generated,
    source_file: part.stamp.sourceFile,
    strict_performance_policy: part.report.reproducibility?.strict_performance_policy ??
      part.report.scoreboard?.strict_performance_policy ?? null,
  }));
  return {
    contract: "gauntlet-status-v1",
    generated: latestReport.generated ?? null,
    source: {
      report_path: latest.stamp.sourceFile,
      report_contract: latestReport.contract ?? null,
      run_id: latest.stamp.runId,
      run_ids: runIds,
      files,
      scope: "merge",
    },
    policy: {
      primary_metric_by_mode: primaryMetricByMode,
      verdict_policy: asObject(latestReport.scoreboard?.verdict_policy ?? latestReport.reproducibility?.ratio_verdicts),
      tier_policy_by_mode: tierPolicyByMode,
      strict_performance_policy: latestReport.reproducibility?.strict_performance_policy ??
        latestReport.scoreboard?.strict_performance_policy ?? null,
      historical_receipts: historicalReceipts,
    },
    summary,
    cells,
    axes: mergeAxisParts(normalized),
    publication: {
      scope: "merge",
      status: "incomplete",
      complete: false,
      blockers: [],
    },
    provenance: {
      matrix_sha256: latestReport.provenance?.matrix_sha256 ?? null,
      measurement_manifest_sha256: latestReport.provenance?.measurement_manifest_sha256 ?? null,
      corpus_tree_sha256: latestReport.provenance?.corpus_tree_sha256 ?? null,
      jet_binary_sha256: latestReport.provenance?.jet_binary_sha256 ?? null,
    },
  };
}

/** Project one dated gauntlet report into the small tracked Tower payload. */
export function projectStatus(report, reportPath = null) {
  return projectOneReport(report, reportPath);
}

function parseArgs(argv) {
  let from = null;
  let merge = null;
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--from" || arg === "--merge") {
      if (i + 1 >= argv.length) throw new Error(`${arg} needs a value`);
      const value = argv[++i];
      if (arg === "--from") from = value;
      else merge = value;
      continue;
    }
    if (arg === "--help" || arg === "-h") {
      console.log("usage: node gauntlet/harness/status.mjs [--merge gauntlet/results] [--from gauntlet/results/YYYY-MM-DD.json]");
      return null;
    }
    throw new Error(`unknown argument: ${arg}`);
  }
  if (from && merge) throw new Error("--from and --merge are mutually exclusive");
  return { from, merge: merge ?? "gauntlet/results" };
}

async function reportFiles(directory) {
  const root = path.resolve(process.cwd(), directory);
  const entries = await fs.readdir(root, { withFileTypes: true });
  return entries.filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .map((entry) => path.join(root, entry.name))
    .sort();
}

export function applyStatusGate(status, matrix) {
  const strictBlockers = validateStatusGate(status, matrix);
  const policy = matrix?.strict_performance_policy ?? null;
  status.policy = {
    ...asObject(status.policy),
    strict_performance_policy: policy,
  };
  const blockers = [...new Set([
    ...asArray(status.publication?.blockers),
    ...strictBlockers,
  ].filter((blocker) => typeof blocker === "string" && blocker.length > 0))];
  status.publication = {
    ...asObject(status.publication),
    status: blockers.length ? "incomplete" : (status.publication?.status ?? "incomplete"),
    complete: status.publication?.complete === true && blockers.length === 0,
    blockers,
  };
  return strictBlockers;
}

async function loadMatrix() {
  return JSON.parse(await fs.readFile(path.join(repoDir, "gauntlet/matrix.json"), "utf8"));
}

async function writeStatusAndReport(status, matrix) {
  const strictBlockers = applyStatusGate(status, matrix);
  await fs.writeFile(statusPath, `${JSON.stringify(status, null, 2)}\n`);
  console.log(`status\t${path.relative(repoDir, statusPath).split(path.sep).join("/")}`);
  console.log(`summary\tcells=${status.summary.cells}\twin=${status.summary.win}\tparity=${status.summary.parity}\tloss=${status.summary.loss}\tunmeasured=${status.summary.unmeasured}`);
  console.log(`strict_gate\t${strictBlockers.length ? "blocked" : "complete"}\tblockers=${strictBlockers.length}`);
  for (const blocker of strictBlockers) console.log(`blocker\t${blocker}`);
  if (strictBlockers.length) process.exitCode = 1;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (!options) return;
  const matrix = await loadMatrix();
  if (options.from) {
    const inputPath = path.resolve(process.cwd(), options.from);
    const report = JSON.parse(await fs.readFile(inputPath, "utf8"));
    const status = projectStatus(report, inputPath);
    await writeStatusAndReport(status, matrix);
    return;
  }
  const files = await reportFiles(options.merge);
  const reports = await Promise.all(files.map(async (file) => ({
    report: JSON.parse(await fs.readFile(file, "utf8")),
    reportPath: file,
  })));
  const status = mergeStatus(reports);
  await writeStatusAndReport(status, matrix);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(`status: ${error.message}`);
    process.exitCode = 1;
  });
}
