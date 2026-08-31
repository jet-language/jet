#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync, lstatSync, readFileSync, readdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { isAbsolute, join, relative, resolve } from "node:path";

import {
  canonicalJson,
  readReproBundle,
  sha256 as reproSha256,
} from "./hardening-repro.mjs";
import { readManifestArtifact } from "./hardening-oracle-layer.mjs";
import { verifySignedReceipt } from "./hardening-red-team.mjs";

export const HANDOFF_THRESHOLDS = Object.freeze({
  decision: "D-HARDENING-GATE1=A",
  clean_window_days: 14,
  valid_cases: 10_000_000,
  min_cases_per_eligible_row: 100,
  red_team_lanes: 8,
  red_team_waves: 4,
  red_team_concurrency: 2,
});

const KINDS = Object.freeze(["module_call", "receiver_method", "field", "nominal_type"]);
const TIERS = Object.freeze(["aot", "jet_run", "interpreter"]);
const DONE_PHASES = new Set(["done", "frozen"]);
const JSON_SUFFIX = ".json";
const SHA256_RE = /^sha256:[0-9a-f]{64}$/;

function text(value) {
  return typeof value === "string" && value.length ? value : null;
}

function finiteInteger(value) {
  return Number.isInteger(value) && value >= 0 ? value : null;
}

function finiteNumber(value) {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function pick(object, ...keys) {
  if (!object || typeof object !== "object") return undefined;
  for (const key of keys) if (object[key] !== undefined && object[key] !== null) return object[key];
  return undefined;
}

function sortedUnique(values) {
  return [...new Set(values.filter((value) => value !== null && value !== undefined).map(String))].sort();
}

function clone(value) {
  if (value === undefined) return undefined;
  return JSON.parse(JSON.stringify(value));
}

function hashMatches(value, hashes) {
  if (typeof value !== "string" || !value) return false;
  const actual = value.replace(/^sha256:/, "").toLowerCase();
  return hashes.some((candidate) => typeof candidate === "string" && candidate.replace(/^sha256:/, "").toLowerCase() === actual);
}

function hashBytes(value) {
  return reproSha256(Buffer.isBuffer(value) ? value : Buffer.from(value));
}

function rawHashBytes(value) {
  return createHash("sha256").update(value).digest("hex");
}

function parseDate(value) {
  if (typeof value !== "string" || !value.trim()) return null;
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? null : parsed;
}

function dateKey(value) {
  const date = parseDate(value);
  return date ? date.toISOString().slice(0, 10) : null;
}

function readJsonFile(path) {
  if (!existsSync(path)) return { value: null, error: `missing: ${path}` };
  try {
    return { value: JSON.parse(readFileSync(path, "utf8")), error: null };
  } catch (error) {
    return { value: null, error: `unreadable: ${path}: ${error.message}` };
  }
}

function walkJsonFiles(root) {
  if (!existsSync(root)) return { files: [], errors: [`missing evidence directory: ${root}`] };
  const files = [];
  const errors = [];
  const visit = (directory) => {
    let entries;
    try {
      entries = readdirSync(directory, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name));
    } catch (error) {
      errors.push(`unreadable evidence directory: ${directory}: ${error.message}`);
      return;
    }
    for (const entry of entries) {
      const path = join(directory, entry.name);
      let stat;
      try { stat = lstatSync(path); } catch (error) {
        errors.push(`unreadable evidence entry: ${path}: ${error.message}`);
        continue;
      }
      if (stat.isSymbolicLink()) {
        errors.push(`evidence symlink is not allowed: ${path}`);
      } else if (stat.isDirectory()) {
        visit(path);
      } else if (stat.isFile() && path.endsWith(JSON_SUFFIX)) {
        files.push(path);
      }
    }
  };
  visit(root);
  return { files: files.sort(), errors };
}

function safeRelative(root, path) {
  const absolute = resolve(root, path);
  const inside = relative(root, absolute);
  if (inside.startsWith("..") || isAbsolute(inside)) return null;
  return absolute;
}

function sourceSnapshotCheck(manifest, root) {
  const errors = [];
  const snapshot = manifest?.source_snapshot;
  if (!snapshot || typeof snapshot !== "object" || !Array.isArray(snapshot.files)) {
    return { stale: true, errors: ["manifest source snapshot is missing"] };
  }
  const files = snapshot.files;
  for (const file of files) {
    if (!file || typeof file.path !== "string" || !Number.isInteger(file.bytes) || typeof file.sha256 !== "string") {
      errors.push("manifest source snapshot contains an invalid file");
      continue;
    }
    const path = safeRelative(root, file.path);
    if (!path) {
      errors.push(`manifest source snapshot escapes root: ${file.path}`);
      continue;
    }
    let bytes;
    try { bytes = readFileSync(path); } catch (error) {
      errors.push(`manifest source snapshot file is unreadable: ${file.path}: ${error.message}`);
      continue;
    }
    if (bytes.length !== file.bytes) errors.push(`manifest source snapshot byte mismatch: ${file.path}`);
    if (hashBytes(bytes) !== file.sha256) errors.push(`manifest source snapshot digest mismatch: ${file.path}`);
  }
  if (typeof snapshot.hash !== "string" || !SHA256_RE.test(snapshot.hash)) {
    errors.push("manifest source snapshot hash is missing or invalid");
  } else if (hashBytes(canonicalJson(files)) !== snapshot.hash) {
    errors.push("manifest source snapshot hash does not match files");
  }
  return { stale: errors.length > 0, errors };
}

function manifestPathFor({ root, manifestPath }) {
  if (manifestPath) return resolve(root, manifestPath);
  const configured = process.env.JET_HARDENING_MANIFEST;
  const candidates = configured
    ? [resolve(root, configured)]
    : [
        join(root, ".jet/hardening-manifest.json"),
        join(root, "tests/conformance/manifest.json"),
      ];
  return candidates.find(existsSync) || candidates[0];
}

function loadManifest({ root, manifestPath }) {
  const path = manifestPathFor({ root, manifestPath });
  const loaded = readJsonFile(path);
  if (loaded.error) {
    return {
      path: relative(root, path),
      present: false,
      readable: false,
      hash: null,
      content_digest: null,
      source_snapshot_hash: null,
      manifest: null,
      stale: true,
      errors: [loaded.error],
    };
  }
  let manifest;
  try {
    manifest = readManifestArtifact(path);
  } catch (error) {
    return {
      path: relative(root, path),
      present: true,
      readable: false,
      hash: rawHashBytes(readFileSync(path)),
      content_digest: loaded.value?.content_digest || null,
      source_snapshot_hash: loaded.value?.source_snapshot?.hash || null,
      manifest: null,
      stale: true,
      errors: [error.message],
    };
  }
  const snapshot = sourceSnapshotCheck(manifest, root);
  let raw;
  try { raw = readFileSync(path); } catch (error) {
    return {
      path: relative(root, path),
      present: true,
      readable: false,
      hash: null,
      content_digest: manifest.content_digest || null,
      source_snapshot_hash: manifest.source_snapshot?.hash || null,
      manifest: null,
      stale: true,
      errors: [`manifest became unreadable: ${error.message}`],
    };
  }
  return {
    path: relative(root, path),
    present: true,
    readable: true,
    hash: rawHashBytes(raw),
    content_digest: manifest.content_digest || null,
    source_snapshot_hash: manifest.source_snapshot?.hash || null,
    manifest,
    stale: snapshot.stale,
    errors: snapshot.errors,
  };
}

function targetIdentity({ root, binaryPath, target }) {
  if (target && typeof target === "object") {
    return {
      path: binaryPath || null,
      commit: text(target.commit),
      clean: target.clean === true,
      binary_sha256: text(target.binary_sha256 || target.binarySha256),
      errors: Array.isArray(target.errors) ? [...target.errors] : [],
    };
  }
  const errors = [];
  const status = spawnSync("git", ["-C", root, "status", "--porcelain=v1", "--untracked-files=all"], { encoding: "utf8" });
  const commit = spawnSync("git", ["-C", root, "rev-parse", "HEAD"], { encoding: "utf8" });
  if (status.status !== 0) errors.push(`git status failed: ${String(status.stderr || "").trim() || "unknown error"}`);
  if (commit.status !== 0) errors.push(`git commit identity unavailable: ${String(commit.stderr || "").trim() || "unknown error"}`);
  const binary = binaryPath ? resolve(root, binaryPath) : null;
  let binarySha = null;
  if (binary) {
    try { binarySha = rawHashBytes(readFileSync(binary)); } catch (error) { errors.push(`binary is unreadable: ${binary}: ${error.message}`); }
  }
  return {
    path: binary,
    commit: commit.status === 0 ? String(commit.stdout || "").trim() : null,
    clean: status.status === 0 && !String(status.stdout || "").trim(),
    binary_sha256: binarySha,
    errors,
  };
}

function statusCounts() {
  return Object.fromEntries(["total", "covered", "excluded", "missing", "refused", "stale"].map((key) => [key, 0]));
}

function rowStatus(row, manifestStale) {
  if (manifestStale) return "stale";
  if (row.status === "covered") return "covered";
  if (row.status === "excluded") {
    const exclusion = row.exclusion;
    if (exclusion?.reason && exclusion?.owner && exclusion?.decision) return "excluded";
    return "refused";
  }
  if (row.status === "missing") return "missing";
  return "refused";
}

function conformanceReport(manifestInfo) {
  const byKind = Object.fromEntries(KINDS.map((kind) => [kind, statusCounts()]));
  const byTier = Object.fromEntries(TIERS.map((tier) => [tier, statusCounts()]));
  const totals = statusCounts();
  const missing = [];
  const refused = [];
  const stale = [];
  const exclusions = [];
  const rows = manifestInfo.manifest?.rows || [];
  const errors = [...manifestInfo.errors];
  for (const row of rows) {
    const state = rowStatus(row, manifestInfo.stale);
    const kind = KINDS.includes(row.kind) ? row.kind : null;
    if (!kind) errors.push(`manifest row has unknown kind: ${row.stable_id || "<unknown>"}`);
    totals.total += 1;
    totals[state] += 1;
    if (kind) {
      byKind[kind].total += 1;
      byKind[kind][state] += 1;
    }
    if (state === "missing") missing.push(row.stable_id);
    if (state === "refused") refused.push(row.stable_id);
    if (state === "stale") stale.push(row.stable_id);
    if (row.status === "excluded") {
      const exclusion = row.exclusion;
      exclusions.push({
        stable_id: row.stable_id,
        reason: exclusion?.reason || null,
        owner: exclusion?.owner || null,
        decision: exclusion?.decision || null,
        ratified: Boolean(exclusion?.reason && exclusion?.owner && exclusion?.decision),
      });
      if (!exclusion?.reason || !exclusion?.owner || !exclusion?.decision) {
        errors.push(`excluded row has no owner-ratified reason: ${row.stable_id}`);
      }
    }
    for (const tier of Array.isArray(row.applicable_tiers) ? row.applicable_tiers : []) {
      if (!TIERS.includes(tier)) {
        errors.push(`manifest row has unknown tier: ${row.stable_id || "<unknown>"}:${tier}`);
        continue;
      }
      byTier[tier].total += 1;
      byTier[tier][state] += 1;
    }
  }
  if (!manifestInfo.manifest) errors.push("tagged hardening manifest is unavailable");
  const normalizedErrors = sortedUnique(errors);
  const gate = Boolean(
    manifestInfo.readable
      && !manifestInfo.stale
      && totals.total > 0
      && totals.missing === 0
      && totals.refused === 0
      && totals.stale === 0
      && exclusions.every((row) => row.ratified),
  );
  return {
    status: gate ? "GREEN" : "RED",
    ok: gate,
    totals,
    by_kind: byKind,
    by_tier: byTier,
    tier_totals: byTier,
    missing: missing.sort(),
    refused: refused.sort(),
    stale: stale.sort(),
    exclusions: exclusions.sort((left, right) => left.stable_id.localeCompare(right.stable_id)),
    errors: normalizedErrors,
  };
}

function canonicalRegistryHashes(manifestInfo) {
  return [manifestInfo.hash, manifestInfo.content_digest, manifestInfo.source_snapshot_hash].filter(Boolean);
}

function registryMatches(value, manifestInfo) {
  return hashMatches(value, canonicalRegistryHashes(manifestInfo));
}

function cycleCandidate(value) {
  if (!value || typeof value !== "object" || Array.isArray(value) || value.schema) return false;
  return Boolean(value.run_id && value.commit && (value.started || value.finished || value.date)
    && (value.status || value.oracle || value.property || value.grammar || value.mutation || value.fuzz));
}

function flattenCycleCandidates(value, path, out) {
  if (Array.isArray(value)) {
    for (const item of value) flattenCycleCandidates(item, path, out);
    return;
  }
  if (!value || typeof value !== "object") return;
  if (cycleCandidate(value)) out.push({ value, path });
  for (const key of ["cycles", "runs", "results"]) {
    if (Array.isArray(value[key])) flattenCycleCandidates(value[key], path, out);
  }
}

function findingIsSilent(finding) {
  const classification = String(pick(finding, "classification", "finding_classification", "kind") || "").toLowerCase().replaceAll("_", "-");
  return Boolean(
    finding?.silentWrongData === true
      || finding?.silent_data === true
      || finding?.defaultJetRunDivergence === true
      || finding?.default_jet_run_divergence === true
      || classification.includes("silent")
      || classification.includes("default-jet-run")
      || classification.includes("default-run"),
  );
}

function findingKey(finding, fallback) {
  return String(pick(finding, "finding_id", "findingId", "bundle_digest", "bundleDigest", "id", "stable_surface_id") || fallback);
}

function layerObjects(raw) {
  const layers = [];
  for (const key of ["oracle", "differential", "fuzz", "property", "grammar", "mutation"]) {
    if (raw[key] && typeof raw[key] === "object") layers.push({ key, value: raw[key] });
  }
  if (Array.isArray(raw.layers)) {
    for (const value of raw.layers) if (value && typeof value === "object") layers.push({ key: value.layer || value.name || "layer", value });
  } else if (raw.layers && typeof raw.layers === "object") {
    for (const [key, value] of Object.entries(raw.layers)) if (value && typeof value === "object") layers.push({ key, value });
  }
  return layers;
}

function addNumericMap(target, source) {
  if (!source || typeof source !== "object" || Array.isArray(source)) return;
  for (const [key, value] of Object.entries(source)) {
    const count = finiteInteger(value);
    if (count === null) continue;
    target[key] = (target[key] || 0) + count;
  }
}

function addCaseRows(target, cases) {
  if (!Array.isArray(cases)) return;
  for (const item of cases) {
    if (!item || typeof item !== "object") continue;
    const id = pick(item, "stable_surface_id", "stable_id", "surface_id", "row", "callable");
    if (typeof id !== "string" || !id) continue;
    target[id] = (target[id] || 0) + 1;
  }
}

function addCaseDomains(target, cases) {
  if (!Array.isArray(cases)) return;
  for (const item of cases) {
    if (!item || typeof item !== "object") continue;
    const domain = pick(item, "domain", "domain_id");
    if (typeof domain !== "string" || !domain) continue;
    target[domain] = (target[domain] || 0) + 1;
  }
}

function normalizeCycle(raw, path, manifestInfo, target) {
  const errors = [];
  const layers = layerObjects(raw);
  const registryHash = pick(
    raw,
    "registry_snapshot_hash",
    "registrySnapshotHash",
    "manifest_hash",
    "manifestHash",
  ) || pick(raw.registry_snapshot, "sha256", "hash");
  const configHash = pick(raw, "config_hash", "configHash") || pick(raw.config, "hash");
  const status = String(raw.status || "").toUpperCase();
  const started = raw.started || raw.start || raw.date;
  const finished = raw.finished || raw.end || raw.timestamp || started;
  const date = dateKey(finished || started);
  if (!raw.run_id) errors.push("cycle run_id is missing");
  if (!target.commit || raw.commit !== target.commit) errors.push("cycle target commit does not match checkout");
  if (!target.binary_sha256 || !hashMatches(raw.binary_sha256, [target.binary_sha256])) errors.push("cycle binary identity does not match checkout");
  if (!registryMatches(registryHash, manifestInfo)) errors.push("cycle registry snapshot is stale or missing");
  if (!configHash) errors.push("cycle config hash is missing");
  if (!date) errors.push("cycle finished date is missing or invalid");
  if (status !== "PASS") errors.push(`cycle status is ${status || "missing"}`);
  if (raw.proof?.skipped === true) errors.push("cycle proof was skipped");
  if (Array.isArray(raw.resource_violations) && raw.resource_violations.length) errors.push("cycle exceeded a resource budget");

  const rowCounts = {};
  const mutationRowCounts = {};
  const domains = {};
  const findings = [];
  let validCases = 0;
  let lastSeed = pick(raw, "last_seed", "lastSeed", "seed") || null;
  const summaries = layers.length ? layers : [{ key: "cycle", value: raw }];
  const differentialSummaries = summaries.filter(({ key }) => ["oracle", "differential", "fuzz"].includes(key));
  for (const { key, value } of summaries) {
    const layerStatus = String(value.status || "").toUpperCase();
    if (layerStatus && !["PASS", "DISABLED"].includes(layerStatus)) errors.push(`${key} layer status is ${layerStatus}`);
    if (layerStatus === "DISABLED" && (typeof value.reason !== "string" || !value.reason.trim())) {
      errors.push(`${key} disabled layer reason is missing`);
    }
    const targetRows = key === "mutation" ? mutationRowCounts : rowCounts;
    for (const name of ["row_counts", "per_row_counts", "mutation_counts", "counts_by_surface", "surface_counts"]) addNumericMap(targetRows, value[name]);
    addCaseRows(targetRows, value.cases);
    addCaseRows(targetRows, value.valid_cases);
    for (const name of ["domain_counts", "domain_distribution", "domains"]) addNumericMap(domains, value[name]);
    addCaseDomains(domains, value.cases);
    addCaseDomains(domains, value.valid_cases);
    lastSeed = pick(value, "last_seed", "lastSeed", "seed") || lastSeed;
    if (Array.isArray(value.findings)) findings.push(...value.findings);
    if (Array.isArray(value.silent_findings)) findings.push(...value.silent_findings);
  }
  if (differentialSummaries.length) {
    for (const { key, value } of differentialSummaries) {
      const count = finiteInteger(pick(value, "valid_case_count", "valid_cases", "validCases"));
      if (count !== null) validCases += count;
      else errors.push(`${key} valid case count is missing`);
    }
  } else {
    const count = finiteInteger(pick(raw, "valid_case_count", "valid_cases", "validCases"));
    if (count !== null) validCases = count;
    else errors.push("cycle valid case count is missing");
  }
  if (Array.isArray(raw.findings)) findings.push(...raw.findings);
  const uniqueFindings = new Map();
  findings.forEach((finding, index) => uniqueFindings.set(findingKey(finding, `${path}:${index}`), finding));
  const silentFindings = [...uniqueFindings.values()].filter(findingIsSilent);
  if (findings.length) errors.push(`cycle contains ${findings.length} finding(s)`);
  return {
    path,
    run_id: text(raw.run_id),
    commit: text(raw.commit),
    binary_sha256: text(raw.binary_sha256),
    registry_snapshot_hash: text(registryHash),
    config_hash: text(configHash),
    status,
    started: parseDate(started)?.toISOString() || null,
    finished: parseDate(finished)?.toISOString() || null,
    date,
    valid_cases: validCases,
    row_counts: rowCounts,
    mutation_row_counts: mutationRowCounts,
    domain_distribution: domains,
    findings: [...uniqueFindings.values()],
    silent_findings: silentFindings,
    last_seed: lastSeed,
    errors: sortedUnique(errors),
  };
}

function loadEvidence({ root, evidenceRoot, manifestInfo, target }) {
  const walk = walkJsonFiles(evidenceRoot);
  const cycles = [];
  const repros = [];
  const parseErrors = [...walk.errors];
  const evidencePaths = [];
  const entries = walk.files.flatMap((path) => {
    if (path === resolve(root, manifestInfo.path || "")) return [];
    return [{ path, relativePath: relative(root, path), loaded: readJsonFile(path) }];
  });
  const archivedRunIds = new Set(entries.flatMap(({ path, loaded }) => {
    const parts = relative(root, path).split(/[\\/]/);
    return parts.at(-2) === "cycles" && cycleCandidate(loaded.value) ? [loaded.value.run_id] : [];
  }));
  for (const { path, relativePath, loaded } of entries) {
    const name = path.split(/[\\/]/).pop();
    if (["state.json", "failure.json", "session.json", "receipt.json"].includes(name)) continue;
    evidencePaths.push(relativePath);
    if (loaded.error) {
      parseErrors.push(loaded.error);
      continue;
    }
    const value = loaded.value;
    if (name === "result.json" && archivedRunIds.has(value?.run_id)) continue;
    if (value?.schema === "jet.hardening.repro.v1") {
      try {
        const bundle = readReproBundle(path);
        if (bundle.commit === target.commit && !registryMatches(bundle.registry_snapshot_hash, manifestInfo)) parseErrors.push(`${path}: registry snapshot is stale`);
        repros.push({ path, value: bundle });
      } catch (error) {
        parseErrors.push(`${path}: ${error.message}`);
      }
      continue;
    }
    flattenCycleCandidates(value, path, cycles);
  }
  const normalized = cycles.map(({ value, path }) => normalizeCycle(value, path, manifestInfo, target));
  return { cycles: normalized, repros, errors: sortedUnique(parseErrors), evidence_paths: evidencePaths.sort() };
}

function fuzzReport(evidence, manifestInfo, target) {
  const current = evidence.cycles.filter((cycle) => cycle.errors.length === 0);
  const invalid = evidence.cycles.filter((cycle) => cycle.errors.length > 0);
  const validDates = new Set(current.map((cycle) => cycle.date).filter(Boolean));
  const invalidDates = new Set(invalid.map((cycle) => cycle.date).filter(Boolean));
  const orderedDates = [...validDates].sort();
  const windowDates = [];
  if (orderedDates.length) {
    let cursor = orderedDates[orderedDates.length - 1];
    while (validDates.has(cursor) && !invalidDates.has(cursor)) {
      windowDates.unshift(cursor);
      const previous = new Date(`${cursor}T00:00:00Z`);
      previous.setUTCDate(previous.getUTCDate() - 1);
      cursor = previous.toISOString().slice(0, 10);
    }
  }
  const windowDateSet = new Set(windowDates);
  const windowCycles = current.filter((cycle) => windowDateSet.has(cycle.date));
  const rowCounts = {};
  const domains = {};
  let validCases = 0;
  let lastSeed = null;
  for (const cycle of windowCycles) {
    validCases += cycle.valid_cases;
    addNumericMap(rowCounts, Object.keys(cycle.mutation_row_counts || {}).length ? cycle.mutation_row_counts : cycle.row_counts);
    addNumericMap(domains, cycle.domain_distribution);
    lastSeed = cycle.last_seed || lastSeed;
  }
  for (const repro of evidence.repros) {
    const bundle = repro.value;
    if (bundle.commit === target.commit && registryMatches(bundle.registry_snapshot_hash, manifestInfo) && findingIsSilent(bundle)) {
      invalid.push({ errors: ["silent finding bundle"], date: dateKey(bundle.finished) });
    }
  }
  const eligibleRows = (manifestInfo.manifest?.rows || []).filter((row) => row.status === "covered");
  const perRow = eligibleRows.map((row) => ({
    stable_id: row.stable_id,
    domain: row.domain || null,
    valid_cases: rowCounts[row.stable_id] || 0,
    minimum: HANDOFF_THRESHOLDS.min_cases_per_eligible_row,
    meets_floor: (rowCounts[row.stable_id] || 0) >= HANDOFF_THRESHOLDS.min_cases_per_eligible_row,
  })).sort((left, right) => left.stable_id.localeCompare(right.stable_id));
  const lowest = perRow.length ? Math.min(...perRow.map((row) => row.valid_cases)) : null;
  const silentFindings = current.flatMap((cycle) => cycle.silent_findings || []).length
    + evidence.repros.filter(({ value }) => (
      value.commit === target.commit
      && registryMatches(value.registry_snapshot_hash, manifestInfo)
      && findingIsSilent(value)
    )).length;
  const invalidationCause = evidence.errors[0]
    || invalid.flatMap((cycle) => cycle.errors || [])[0]
    || (manifestInfo.stale ? "manifest source snapshot is stale" : null)
    || (silentFindings ? "silent finding invalidated the clean window" : null);
  const gate = Boolean(
    manifestInfo.readable
      && !manifestInfo.stale
      && evidence.errors.length === 0
      && windowDates.length >= HANDOFF_THRESHOLDS.clean_window_days
      && validCases >= HANDOFF_THRESHOLDS.valid_cases
      && perRow.length > 0
      && perRow.every((row) => row.meets_floor)
      && silentFindings === 0,
  );
  return {
    status: gate ? "GREEN" : "RED",
    ok: gate,
    target_commit: target.commit,
    window: {
      start: windowDates[0] || null,
      end: windowDates[windowDates.length - 1] || null,
      dates: windowDates,
      clean_days: windowDates.length,
    },
    clean_days: windowDates.length,
    valid_cases: validCases,
    valid_case_count: validCases,
    per_row: perRow,
    row_floor: { required: HANDOFF_THRESHOLDS.min_cases_per_eligible_row, lowest },
    lowest_row: lowest,
    domain_distribution: Object.fromEntries(Object.entries(domains).sort(([left], [right]) => left.localeCompare(right))),
    silent_findings: silentFindings,
    last_seed: lastSeed,
    invalidation_cause: invalidationCause,
    evidence: evidence.cycles.map((cycle) => ({ path: cycle.path, status: cycle.errors.length ? "INVALID" : "PASS", errors: cycle.errors })),
    errors: sortedUnique([
      ...evidence.errors,
      ...invalid.flatMap((cycle) => cycle.errors || []),
      ...(manifestInfo.stale ? ["manifest source snapshot is stale"] : []),
    ]),
    threshold: {
      clean_window_days: HANDOFF_THRESHOLDS.clean_window_days,
      valid_cases: HANDOFF_THRESHOLDS.valid_cases,
      min_cases_per_eligible_row: HANDOFF_THRESHOLDS.min_cases_per_eligible_row,
    },
  };
}

function laneNumber(lane) {
  const raw = pick(lane, "lane", "lane_id", "id", "number");
  if (Number.isInteger(raw)) return raw;
  const match = String(raw || "").match(/(?:^|[^0-9])([1-9][0-9]*)(?:$|[^0-9])/);
  return match ? Number(match[1]) : null;
}

function laneComplete(lane) {
  const status = String(pick(lane, "status", "state") || "").toUpperCase();
  return ["PASS", "OK", "COMPLETE", "COMPLETED", "DONE"].includes(status)
    || lane.complete === true
    || lane.completed === true;
}

function loadRedTeam({ evidenceRoot, target, manifestInfo }) {
  const sessionPath = process.env.JET_HARDENING_RED_TEAM_MANIFEST
    ? resolve(evidenceRoot, process.env.JET_HARDENING_RED_TEAM_MANIFEST)
    : join(evidenceRoot, "red-team/session.json");
  const receiptPath = process.env.JET_HARDENING_RED_TEAM_RECEIPT
    ? resolve(evidenceRoot, process.env.JET_HARDENING_RED_TEAM_RECEIPT)
    : join(evidenceRoot, "red-team/receipt.json");
  const sessionLoaded = readJsonFile(sessionPath);
  const receiptLoaded = readJsonFile(receiptPath);
  const errors = [];
  if (sessionLoaded.error) errors.push(sessionLoaded.error);
  if (receiptLoaded.error) errors.push(receiptLoaded.error);
  const session = sessionLoaded.value;
  const receipt = receiptLoaded.value;
  if (session?.schema !== "jet.hardening.red-team.session.v1") errors.push("red-team session schema is missing or invalid");
  if (receipt?.schema !== "jet.hardening.red-team.receipt.v1") errors.push("red-team receipt schema is missing or invalid");
  const sessionIdentity = session?.session || session || {};
  const receiptIdentity = receipt?.session || {};
  const targetCommit = pick(receiptIdentity, "commit", "target_commit", "targetCommit")
    || pick(sessionIdentity, "commit", "target_commit", "targetCommit");
  const sessionRegistry = session?.registry_snapshot || session?.public_surface_snapshot || {};
  const manifestHash = pick(receiptIdentity, "registry_sha256", "registry_snapshot_hash", "manifest_hash", "manifestHash")
    || pick(receipt, "registry_sha256", "public_surface_sha256")
    || pick(sessionIdentity, "registry_sha256", "registry_snapshot_hash", "manifest_hash", "manifestHash")
    || pick(sessionRegistry, "sha256", "hash");
  const binarySha = pick(receiptIdentity, "binary_sha256", "binaryHash")
    || pick(sessionIdentity, "binary_sha256", "binaryHash");
  const sessionCommit = pick(session, "commit") || pick(sessionIdentity, "commit");
  const sessionBinary = pick(session, "binary_sha256") || pick(sessionIdentity, "binary_sha256");
  const sessionRegistryHash = pick(sessionRegistry, "sha256", "hash");
  if (sessionCommit && targetCommit && sessionCommit !== targetCommit) errors.push("red-team receipt and session commits differ");
  if (sessionBinary && binarySha && !hashMatches(sessionBinary, [binarySha])) errors.push("red-team receipt and session binaries differ");
  if (sessionRegistryHash && manifestHash && !hashMatches(sessionRegistryHash, [manifestHash])) errors.push("red-team receipt and session registries differ");
  if (session?.session_id && receipt?.session_id && session.session_id !== receipt.session_id) errors.push("red-team receipt belongs to another session");
  if (!target.commit || targetCommit !== target.commit) errors.push("red-team target commit is stale");
  if (!registryMatches(manifestHash, manifestInfo)) errors.push("red-team manifest is stale");
  if (!target.binary_sha256 || !hashMatches(binarySha, [target.binary_sha256])) errors.push("red-team binary identity is stale");
  const quota = receipt?.quota || session?.quota || {};
  const laneQuota = finiteInteger(pick(quota, "lanes", "lane_count", "contexts"));
  const waveQuota = finiteInteger(pick(quota, "waves", "wave_count"));
  if (laneQuota !== HANDOFF_THRESHOLDS.red_team_lanes) errors.push("red-team lane quota is not ratified 8");
  if (waveQuota !== HANDOFF_THRESHOLDS.red_team_waves) errors.push("red-team wave quota is not ratified 4");
  const lanes = Array.isArray(receipt?.lanes) ? receipt.lanes : [];
  const seenLanes = new Set();
  let validAttempts = 0;
  let duplicates = 0;
  let falsePositives = 0;
  const p0 = new Map();
  for (const lane of lanes) {
    const number = laneNumber(lane);
    if (number === null || seenLanes.has(number)) errors.push("red-team lane identity is missing or duplicated");
    else seenLanes.add(number);
    if (!laneComplete(lane)) errors.push(`red-team lane ${number || "?"} is incomplete`);
    if (lane.semantic_change === true || lane.registry_changed === true) errors.push(`red-team lane ${number || "?"} is stale`);
    if (lane.wave !== undefined && lane.wave !== Math.ceil(number / 2)) errors.push(`red-team lane ${number || "?"} is in the wrong wave`);
    if (lane.fresh_context !== undefined && lane.fresh_context !== true) errors.push(`red-team lane ${number || "?"} is not fresh-context`);
    if (lane.model !== undefined && !String(lane.model).toLowerCase().includes("luna")) errors.push(`red-team lane ${number || "?"} is not a Luna lane`);
    if (lane.reasoning_effort !== undefined && lane.reasoning_effort !== "max") errors.push(`red-team lane ${number || "?"} is not max reasoning`);
    if (lane.known_defects_visible !== undefined && lane.known_defects_visible !== false) errors.push(`red-team lane ${number || "?"} saw known defects`);
    if (lane.stopped_early === true) errors.push(`red-team lane ${number || "?"} stopped early`);
    if (lane.target && (lane.target.commit !== target.commit || !hashMatches(lane.target.binary_sha256, [target.binary_sha256]))) errors.push(`red-team lane ${number || "?"} target is stale`);
    const counts = lane.counts || {};
    const rawAttempts = pick(lane, "valid_attempts", "valid_case_count", "attempt_count")
      ?? pick(counts, "valid_attempts", "valid_cases", "attempts");
    const attempts = finiteInteger(rawAttempts) ?? (Array.isArray(lane.valid_cases) ? lane.valid_cases.length : null);
    if (attempts === null) errors.push(`red-team lane ${number || "?"} has no valid attempt count`);
    else validAttempts += attempts;
    const duplicateCount = finiteInteger(pick(lane, "duplicate_count")) ?? (Array.isArray(lane.duplicates) ? lane.duplicates.length : finiteInteger(lane.duplicates));
    const falsePositiveCount = finiteInteger(pick(lane, "false_positive_count")) ?? (Array.isArray(lane.false_positives) ? lane.false_positives.length : finiteInteger(lane.false_positives));
    duplicates += duplicateCount || 0;
    falsePositives += falsePositiveCount || 0;
    const findings = Array.isArray(lane.findings) ? lane.findings : (Array.isArray(lane.unique_findings) ? lane.unique_findings : []);
    findings.forEach((finding, index) => {
      const severity = String(pick(finding, "severity", "priority", "classification") || "").toUpperCase();
      if (severity === "P0" || finding?.p0 === true) p0.set(findingKey(finding, `${number}:${index}`), finding);
    });
  }
  for (let number = 1; number <= HANDOFF_THRESHOLDS.red_team_lanes; number += 1) if (!seenLanes.has(number)) errors.push(`red-team lane ${number} is missing`);
  const receiptFindings = receipt?.findings || {};
  const receiptFindingList = Array.isArray(receiptFindings) ? receiptFindings : [];
  receiptFindingList.forEach((finding, index) => {
    const severity = String(pick(finding, "severity", "priority", "classification") || "").toUpperCase();
    if (severity === "P0" || finding?.p0 === true) p0.set(findingKey(finding, `receipt:${index}`), finding);
  });
  const receiptP0 = finiteInteger(pick(receiptFindings, "p0", "p0_count", "unique_p0"))
    ?? finiteInteger(pick(receipt, "p0_count", "unique_p0"));
  const uniqueP0 = p0.size || receiptP0 || 0;
  const receiptUnique = finiteInteger(pick(receiptFindings, "unique", "unique_findings"))
    ?? finiteInteger(pick(receipt, "unique_finding_count"));
  duplicates ||= Array.isArray(receipt?.finding_duplicates) ? receipt.finding_duplicates.length : 0;
  const status = String(receipt?.status || "").toUpperCase();
  if (status !== "PASS") errors.push(`red-team receipt status is ${status || "missing"}`);
  if (receipt?.semantic_change || receipt?.semantic_change_detected || receipt?.stale || (Array.isArray(receipt?.stale_reasons) && receipt.stale_reasons.length)) errors.push("red-team semantic change invalidated the session");
  if (Array.isArray(receipt?.failure_reasons) && receipt.failure_reasons.length) errors.push("red-team receipt contains failure reasons");
  const signature = receipt?.signature;
  if (!(typeof signature === "string" ? signature.length > 0 : signature && typeof signature === "object" && Object.keys(signature).length > 0)) errors.push("red-team receipt signature is missing");
  else if (typeof signature === "object") {
    try { verifySignedReceipt(receipt); } catch (error) { errors.push(`red-team receipt signature is invalid: ${error.message}`); }
  }
  const cleanup = receipt?.cleanup || {};
  if (cleanup.active_agents || cleanup.active_processes || cleanup.active_scratch || cleanup.alternate_target || cleanup.unbounded_logs) errors.push("red-team cleanup is incomplete");
  if (Object.hasOwn(cleanup, "complete") && cleanup.complete !== true) errors.push("red-team cleanup is incomplete");
  const normalizedErrors = sortedUnique(errors);
  const gate = normalizedErrors.length === 0
    && lanes.length === HANDOFF_THRESHOLDS.red_team_lanes
    && uniqueP0 === 0;
  return {
    status: gate ? "GREEN" : "RED",
    ok: gate,
    target_commit: targetCommit || null,
    quota: {
      lanes: HANDOFF_THRESHOLDS.red_team_lanes,
      waves: HANDOFF_THRESHOLDS.red_team_waves,
      concurrency: HANDOFF_THRESHOLDS.red_team_concurrency,
      completed_lanes: [...seenLanes].sort((left, right) => left - right).length,
      complete: lanes.length === HANDOFF_THRESHOLDS.red_team_lanes && [...Array(HANDOFF_THRESHOLDS.red_team_lanes)].every((_, index) => seenLanes.has(index + 1)),
    },
    valid_attempts: validAttempts,
    unique_p0: uniqueP0,
    unique_findings: receiptUnique || uniqueP0,
    duplicates,
    false_positives: falsePositives,
    semantic_change_stale: normalizedErrors.some((error) => error.includes("semantic change")),
    session: {
      manifest_sha256: manifestHash || null,
      binary_sha256: binarySha || null,
      commit: targetCommit || null,
    },
    signature_present: Boolean(signature),
    cleanup,
    errors: normalizedErrors,
    paths: { session: sessionPath, receipt: receiptPath },
  };
}

function queryTower({ root, towerCli, towerData }) {
  const cli = towerCli ? resolve(root, towerCli) : null;
  if (!cli || !existsSync(cli)) return { status: "RED", ok: false, open_p0: 0, refs: [], errors: [`Tower CLI is missing: ${cli || "unset"}`] };
  const args = [];
  if (towerData) args.push("--data", resolve(root, towerData));
  args.push("card", "list", "--json");
  const result = spawnSync(process.execPath, [cli, ...args], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.status !== 0) return { status: "RED", ok: false, open_p0: 0, refs: [], errors: [`Tower query failed: ${String(result.stderr || "").trim() || `exit ${result.status}`}`] };
  let cards;
  try { cards = JSON.parse(result.stdout); } catch (error) {
    return { status: "RED", ok: false, open_p0: 0, refs: [], errors: [`Tower response is not JSON: ${error.message}`] };
  }
  if (!Array.isArray(cards)) cards = Array.isArray(cards?.cards) ? cards.cards : null;
  if (!cards) return { status: "RED", ok: false, open_p0: 0, refs: [], errors: ["Tower response is not a card list"] };
  const open = cards.filter((card) => card?.priority === "P0" && !DONE_PHASES.has(card.phase));
  const invalid = open.filter((card) => !Number.isInteger(card.num));
  const refs = open.filter((card) => Number.isInteger(card.num)).map((card) => `#${card.num}`).sort((left, right) => Number(left.slice(1)) - Number(right.slice(1)));
  const errors = invalid.length ? ["Tower returned an open P0 without an exact card number"] : [];
  return {
    status: errors.length || open.length ? "RED" : "GREEN",
    ok: errors.length === 0 && open.length === 0,
    open_p0: open.length,
    count: open.length,
    refs,
    exact_refs: refs,
    errors,
    queried: true,
  };
}

function diskFreeBytes(path) {
  const result = spawnSync("df", ["-P", "-B1", "--", path], { encoding: "utf8" });
  if (result.status !== 0) return null;
  const lines = String(result.stdout || "").trim().split(/\r?\n/);
  if (lines.length < 2) return null;
  const fields = lines[lines.length - 1].trim().split(/\s+/);
  const bytes = Number(fields[3]);
  return Number.isFinite(bytes) ? bytes : null;
}

function resourcesReport({ root, resources = {}, capViolations = [] }) {
  const value = { ...resources };
  if (value.free_space_bytes === undefined) value.free_space_bytes = diskFreeBytes(root);
  if (value.free_space_gib === undefined && finiteNumber(value.free_space_bytes) !== null) value.free_space_gib = value.free_space_bytes / 1024 ** 3;
  if (value.memory_available_gib === undefined) {
    try {
      const meminfo = readFileSync("/proc/meminfo", "utf8");
      const match = meminfo.match(/^MemAvailable:\s+(\d+) kB$/m);
      value.memory_available_gib = match ? Number(match[1]) / 1024 ** 2 : null;
    } catch { value.memory_available_gib = null; }
  }
  const violations = [...capViolations];
  if (value.target_bytes == null || value.target_bytes > 80 * 1024 ** 3) violations.push("target over 80GiB");
  if (value.cache_bytes == null || value.cache_bytes > 4 * 1024 ** 3) violations.push("cache over 4GiB");
  if (value.interesting_bytes == null || value.interesting_bytes > 512 * 1024 ** 2) violations.push("interesting corpus over 512MiB");
  if (value.log_bytes == null || value.log_bytes > 1024 ** 2) violations.push("failure log over 1MiB");
  if (value.free_space_bytes == null || value.free_space_bytes < 16 * 1024 ** 3) violations.push("free space below 16GiB");
  if (value.memory_available_gib == null || value.memory_available_gib < 16) violations.push("available memory below 16GiB");
  const unique = sortedUnique(violations);
  return {
    ...value,
    status: unique.length ? "RED" : "GREEN",
    cap_state: unique.length ? "OVER_BUDGET" : "WITHIN_BUDGET",
    violations: unique,
    cap_violations: unique,
    caps: { target_gib: 80, cache_gib: 4, interesting_mib: 512, log_mib: 1, min_free_gib: 16, min_memory_gib: 16 },
  };
}

export function buildDashboard({
  root = process.cwd(),
  evidenceRoot = null,
  cacheRoot = null,
  manifestPath = null,
  targetRoot = "target",
  binaryPath = null,
  towerCli = null,
  towerData = null,
  target = null,
  resources = {},
  capViolations = [],
  state = null,
  result = null,
  now = new Date(),
} = {}) {
  const resolvedRoot = resolve(root);
  const resolvedEvidence = resolve(resolvedRoot, evidenceRoot || cacheRoot || process.env.JET_HARDENING_EVIDENCE_DIR || process.env.JET_HARDENING_EVIDENCE || ".cache/jet-hardening/v1");
  const resolvedBinary = binaryPath ? resolve(resolvedRoot, binaryPath) : join(resolvedRoot, targetRoot, "debug", "jet");
  const targetInfo = targetIdentity({ root: resolvedRoot, binaryPath: resolvedBinary, target });
  const manifestInfo = loadManifest({ root: resolvedRoot, manifestPath });
  const evidence = loadEvidence({ root: resolvedRoot, evidenceRoot: resolvedEvidence, manifestInfo, target: targetInfo });
  const conformance = conformanceReport(manifestInfo);
  const fuzz = fuzzReport(evidence, manifestInfo, targetInfo);
  const redTeam = loadRedTeam({ evidenceRoot: resolvedEvidence, target: targetInfo, manifestInfo });
  const tower = queryTower({ root: resolvedRoot, towerCli, towerData });
  const resourceInfo = resourcesReport({ root: resolvedRoot, resources, capViolations });
  const targetErrors = [...targetInfo.errors];
  const lastCycleStatus = String(state?.last_cycle?.status || result?.status || "").toUpperCase();
  if (state?.__error) targetErrors.push(`state is unreadable: ${state.__error}`);
  if (state?.blocked) targetErrors.push("rig state is blocked");
  if (lastCycleStatus === "RED") targetErrors.push("last rig cycle is RED");
  const targetGate = Boolean(
    targetInfo.commit
      && targetInfo.clean
      && targetInfo.binary_sha256
      && !state?.__error
      && !state?.blocked
      && lastCycleStatus !== "RED",
  );
  const gates = {
    target: { status: targetGate ? "GREEN" : "RED", ok: targetGate, errors: sortedUnique(targetErrors) },
    conformance: { status: conformance.status, ok: conformance.ok, errors: conformance.errors },
    fuzz: { status: fuzz.status, ok: fuzz.ok, errors: fuzz.errors },
    red_team: { status: redTeam.status, ok: redTeam.ok, errors: redTeam.errors },
    tower: { status: tower.status, ok: tower.ok, errors: tower.errors },
    resources: { status: resourceInfo.status, ok: resourceInfo.status === "GREEN", errors: resourceInfo.violations },
  };
  const ready = Object.values(gates).every((gate) => gate.ok);
  const reasons = sortedUnique(Object.values(gates).flatMap((gate) => gate.errors || []));
  const manifest = {
    path: manifestInfo.path,
    present: manifestInfo.present,
    readable: manifestInfo.readable,
    sha256: manifestInfo.hash,
    hash: manifestInfo.content_digest || manifestInfo.hash,
    content_digest: manifestInfo.content_digest,
    source_snapshot_hash: manifestInfo.source_snapshot_hash,
    stale: manifestInfo.stale,
    errors: manifestInfo.errors,
  };
  return {
    command: "status",
    status: ready ? "READY" : "NOT READY",
    decision: HANDOFF_THRESHOLDS.decision,
    thresholds: clone(HANDOFF_THRESHOLDS),
    root: resolvedRoot,
    target_root: resolve(resolvedRoot, targetRoot),
    binary: resolvedBinary,
    cache: cacheRoot ? resolve(resolvedRoot, cacheRoot) : resolvedEvidence,
    target: {
      commit: targetInfo.commit,
      clean: targetInfo.clean,
      binary_sha256: targetInfo.binary_sha256,
      path: resolvedBinary,
      errors: targetInfo.errors,
    },
    manifest,
    conformance,
    fuzz,
    red_team: redTeam,
    tower,
    resources: resourceInfo,
    state,
    last_cycle: state?.last_cycle || null,
    last_result: result,
    evidence: {
      root: resolvedEvidence,
      files: evidence.evidence_paths,
      errors: evidence.errors,
    },
    gates,
    reasons,
    generated_at: now instanceof Date ? now.toISOString() : String(now),
  };
}
