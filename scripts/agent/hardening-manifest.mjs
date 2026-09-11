#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { canonicalJson, sha256 } from "./hardening-repro.mjs";
import {
  canonicalJson as proofCanonicalJson,
  digestBytes as proofDigestBytes,
  digestText as proofDigestText,
} from "./compiler-proof.mjs";

export const SURFACE_SCHEMA = "jet.hardening.surface.v1";
export const SURFACE_SCHEMA_VERSION = 1;
export const TIERS = Object.freeze(["aot", "jet_run", "interpreter"]);
export const DEFAULT_MANIFEST_PATH = ".jet/hardening-manifest.json";
export const CAPABILITY_RELATION_SCHEMA = "jet.capability.relation.v1";
export const CAPABILITY_RELATION_VERSION = 1;
export const CAPABILITY_FAMILIES = Object.freeze([
  "language",
  "core",
  "cli",
  "editor",
  "debugger",
  "build",
  "package",
  "ffi",
]);
export const CAPABILITY_DISPOSITIONS = Object.freeze([
  "planned",
  "implemented-unqualified",
  "passed",
  "failed",
  "stale",
  "unavailable",
  "unsupported",
  "owner-ratified-not-applicable",
]);
export const CAPABILITY_MODES_BY_FAMILY = Object.freeze({
  language: Object.freeze([...TIERS]),
  core: Object.freeze([...TIERS]),
  cli: Object.freeze(["cli"]),
  editor: Object.freeze(["editor"]),
  debugger: Object.freeze(["debugger"]),
  build: Object.freeze(["build"]),
  package: Object.freeze(["package"]),
  ffi: Object.freeze(["ffi"]),
});
const CAPABILITY_PATHS = Object.freeze({
  language: [
    "crates/jet-foundation/src/Syntax.rs",
    "crates/jet-codegen/src/Codegen/TIR/mod.rs",
  ],
  core: [
    "crates/jet-foundation/src/CoreModuleExports.rs",
    "crates/jet-codegen/src/Prelude/Core.jet",
    "crates/jet-foundation/src/Syntax/core_calls.rs",
    "crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs",
    "crates/jet-sema/src/Sema/CheckerCoreLib/core_types.rs",
  ],
  cli: ["crates/jet-cli/src/CLI.rs"],
  editor: ["crates/jet-devserver/src/EditorHost.rs"],
  debugger: [
    "crates/jet-foundation/src/Syntax/package_files.rs",
    "crates/jet-devserver/src/Canvas/debug_source_git.rs",
    "crates/jet-debug/src/lib.rs",
  ],
  build: [
    "crates/jet-foundation/src/BuildEffects.rs",
    "crates/jet-codegen/src/Prelude/Effects.jet",
    "crates/jet-pkg-model/src/Package/Blocks.rs",
  ],
  package: [
    "crates/jet-pkg-model/src/Package/mod.rs",
    "crates/jet-pkg-model/src/Package/Blocks.rs",
    "crates/jet-pkg-model/src/Bundler.rs",
  ],
  ffi: ["crates/jet-foundation/src/AST/ffi.rs"],
});
const CAPABILITY_MODE_ORDER = Object.freeze([
  "aot",
  "jet_run",
  "interpreter",
  "cli",
  "editor",
  "debugger",
  "build",
  "package",
  "ffi",
]);
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const DEFAULT_ROOT = resolve(SCRIPT_DIR, "../..");
const VALUE_NAMES = new Set(["pi", "e", "tau", "infinity", "nan"]);
const KIND_ORDER = Object.freeze(["module_call", "receiver_method", "field", "nominal_type"]);
const KIND_PREFIX = Object.freeze({
  module_call: "module:",
  receiver_method: "receiver:",
  field: "field:",
  nominal_type: "type:",
});
const PATHS = Object.freeze({
  moduleItems: "crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs",
  moduleTypes: "crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs",
  fields: "crates/jet-sema/src/Sema/CheckerCoreLib/core_types.rs",
  calls: "crates/jet-foundation/src/Syntax/core_calls.rs",
  fixedSigs: "crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs",
  surface: "crates/jet-foundation/src/Syntax/core_surface.rs",
  conformance: "scripts/agent/core-conformance.mjs",
  exclusions: "tests/conformance/exclusions.tsv",
});
const ROUTE_FILES = Object.freeze({
  aot: ["crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs", "crates/jet-codegen/src/Codegen/TIR/emit/helpers.rs"],
  jet_run: ["crates/jet-jit/src/jit/lower_ctx.rs", "crates/jet-jit/src/jit/runtime_host.rs", "crates/jet-jit/src/jit/types_meta.rs"],
  interpreter: ["crates/jet-jit/src/ambient_interp.rs", "crates/jet-jit/src/enc_stream/mod.rs", "crates/jet-codegen/src/Codegen/TIR/eval/exprs.rs", "crates/jet-comptime/src/Comptime/CorePureParity.rs"],
});
const VALID_STATUSES = new Set(["covered", "missing", "unrouted", "invalid", "excluded", "invalid-exclusion"]);
const SHA256_PATTERN = /^sha256:[0-9a-f]{64}$/;

function canonicalSourceIds(sourceIds = {}) {
  return Object.fromEntries(KIND_ORDER.map((kind) => [
    kind,
    [...(Array.isArray(sourceIds[kind]) ? sourceIds[kind] : [])].sort(compareStable),
  ]));
}

function sourceIdsDigest(sourceIds) {
  return sha256(canonicalJson(canonicalSourceIds(sourceIds)));
}

function publicStableId(value) {
  if (typeof value !== "string" || value.length === 0) return null;
  for (const prefix of Object.values(KIND_PREFIX)) if (value.startsWith(prefix)) return value;
  return `${KIND_PREFIX.module_call}${value}`;
}

function compareStable(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function uniqueSorted(values) {
  return [...new Set(values)].sort(compareStable);
}

function lineNumber(source, offset) {
  return source.slice(0, offset).split("\n").length;
}

function rowEvidence(path, source, offset, seam, stableId) {
  return `${path}:${lineNumber(source, offset)}:${seam}:${stableId}`;
}

function validDigest(value) {
  return typeof value === "string" && SHA256_PATTERN.test(value);
}

export function manifestContentDigest(manifest) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) {
    throw new Error("manifest is not an object");
  }
  const content = { ...manifest };
  delete content.content_digest;
  return sha256(canonicalJson(content));
}
export const CANDIDATE_SCHEMA = "jet.candidate.v1";
export const CANDIDATE_SCHEMA_VERSION = 1;
const CANDIDATE_SHA256_PATTERN = /^sha256-[0-9a-f]{64}$/;
export const CANDIDATE_DEPENDENCY_KEYS = Object.freeze([
  "source",
  "generator",
  "compiler",
  "configuration",
  "generated_artifact",
  "target_tool",
  "proof_model",
  "proof_checker",
  "proof_artifact",
  "consumed_test_oracle",
  "behavior",
  "performance",
  "platform",
  "binary",
  "documentation",
  "owner_acceptance",
  "release_claim",
]);
const CANDIDATE_FILE_DEPENDENCIES = new Set([
  "source",
  "generator",
  "compiler",
  "configuration",
  "generated_artifact",
  "target_tool",
  "proof_model",
  "proof_checker",
  "proof_artifact",
  "binary",
  "documentation",
]);

const CANDIDATE_WORKLOAD_REPORT_LABELS = Object.freeze([
  "linux-x86_64",
  "macos-x86_64",
  "windows-x86_64",
]);
const CANDIDATE_COMPILER_STAGES = Object.freeze([
  "semantics",
  "source",
  "checking",
  "comptime",
  "lowering",
  "optimization",
  "adapters",
  "prelude",
  "runtime",
]);
const CANDIDATE_DEPENDENCY_STATES = new Set([
  "available",
  "missing",
  "stale",
  "unknown",
  "unavailable",
  "failed",
  "excluded",
]);
const CANDIDATE_CLAIM_STATES = new Set([
  "green",
  "passed",
  "qualified",
  "ready",
  "blocked",
  "failed",
  "stale",
  "invalidated",
  "unknown",
  "unavailable",
  "excluded",
  "missing",
  "owner-ratified-not-applicable",
]);
const CANDIDATE_GREEN_STATES = new Set(["green", "passed", "qualified", "ready"]);
const CANDIDATE_PROOF_POSITIVE_STATES = new Set([
  "passed",
  "qualified",
  "proved",
  "valid",
  "complete",
  "observed",
  "verified",
  "matched",
  "accepted",
  "rejected",
]);

function candidateDigest(value) {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  if (CANDIDATE_SHA256_PATTERN.test(trimmed)) return trimmed;
  if (SHA256_PATTERN.test(trimmed)) return `sha256-${trimmed.slice("sha256:".length)}`;
  return /^[0-9a-f]{64}$/.test(trimmed) ? `sha256-${trimmed}` : null;
}

function validCandidateDigest(value) {
  return typeof value === "string" && CANDIDATE_SHA256_PATTERN.test(value);
}

function candidateIdentity(value, key) {
  if (!isRecord(value)) return null;
  const identity = cloneJson(value);
  const digest = candidateDigest(
    value.sha256
      ?? value.digest
      ?? value.content_sha256
      ?? value.identity_sha256
      ?? value.artifact_sha256,
  );
  identity.sha256 = digest;
  if (typeof identity.id !== "string" || identity.id.length === 0) {
    identity.id = typeof value.path === "string" && value.path.length > 0
      ? value.path
      : typeof value.commit === "string" && value.commit.length > 0
        ? value.commit
        : key;
  }
  if (typeof identity.status !== "string" || identity.status.length === 0) {
    identity.status = identity.sha256 && identity.id ? "available" : "missing";
  }
  return identity;
}

function candidateComparableIdentity(value, key) {
  const identity = candidateIdentity(value, key);
  if (!identity) return null;
  const comparable = cloneJson(identity);
  delete comparable.digest;
  delete comparable.content_sha256;
  delete comparable.identity_sha256;
  delete comparable.artifact_sha256;
  return comparable;
}

function candidateDependencyDigest(dependencies) {
  return proofDigestText(proofCanonicalJson(dependencies));
}

export function candidateContentDigest(candidate) {
  if (!isRecord(candidate)) throw new Error("candidate is not an object");
  const content = cloneJson(candidate);
  delete content.content_digest;
  if (isRecord(content.candidate_identity)) delete content.candidate_identity.record_sha256;
  return proofDigestText(proofCanonicalJson(content));
}

function candidateRecordDigest(candidate) {
  if (!isRecord(candidate)) throw new Error("candidate is not an object");
  const record = cloneJson(candidate);
  delete record.content_digest;
  record.compiler_proof = null;
  if (isRecord(record.candidate_identity)) delete record.candidate_identity.record_sha256;
  return proofDigestText(proofCanonicalJson(record));
}

function candidateDefaultId(commit, source) {
  const seed = typeof commit === "string" && commit.length > 0
    ? commit
    : source?.commit || source?.revision || source?.sha256 || "unknown";
  return `candidate-${String(seed).replace(/[^A-Za-z0-9._-]/g, "-")}`;
}

function candidateClaim(value, index) {
  const claim = isRecord(value) ? cloneJson(value) : {};
  claim.id = typeof claim.id === "string" && claim.id.length > 0 ? claim.id : `claim-${index + 1}`;
  claim.kind = typeof claim.kind === "string" && claim.kind.length > 0 ? claim.kind : "evidence";
  claim.status = typeof claim.status === "string" && claim.status.length > 0 ? claim.status : "unknown";
  claim.required = claim.required !== false;
  claim.dependencies = Array.isArray(claim.dependencies)
    ? uniqueSorted(claim.dependencies.filter((key) => typeof key === "string"))
    : [];
  claim.evidence = isRecord(claim.evidence) ? cloneJson(claim.evidence) : { status: "missing" };
  return claim;
}

function candidatePrerelease(value) {
  return isRecord(value)
    ? cloneJson(value)
    : {
      status: "prerelease",
      owner_decision: "#2927",
      breaking_changes_allowed: true,
      current_version_migration: "not-required",
    };
}

const CANDIDATE_WORKLOAD_IDENTITY_HEADER = Object.freeze(["version", "key", "value"]);
const CANDIDATE_WORKLOAD_REVIEW_HEADER = Object.freeze([
  "version",
  "reviewer",
  "reviewed_candidate",
  "compiler_sha256",
  "contract_sha256",
  "source_closure_sha256",
  "report_sha256",
  "workload_fairness",
  "peer_fairness",
  "authority_fairness",
  "measurement_fairness",
  "tier_fairness",
  "loss_ownership",
  "status",
  "evidence",
  "review_workflow",
  "review_run",
  "review_actor",
]);

function candidateWorkloadTable(path, expectedHeader) {
  if (!existsSync(path)) throw new Error(`candidate workload report is missing: ${path}`);
  const lines = readFileSync(path, "utf8").split(/\r?\n/).filter((line) => line.length > 0);
  if (lines.length < 2 || lines[0] !== expectedHeader.join("\t")) {
    throw new Error(`candidate workload report schema is invalid: ${path}`);
  }
  return lines.slice(1).map((line, index) => {
    const values = line.split("\t");
    if (values.length !== expectedHeader.length) {
      throw new Error(`candidate workload report row is invalid: ${path}:${index + 2}`);
    }
    return Object.fromEntries(expectedHeader.map((key, field) => [key, values[field]]));
  });
}

function candidateWorkloadIdentity(reportPath) {
  const rows = candidateWorkloadTable(join(reportPath, "identity.tsv"), CANDIDATE_WORKLOAD_IDENTITY_HEADER);
  const identity = {};
  for (const row of rows) {
    if (Object.hasOwn(identity, row.key) || !row.key || !row.value) {
      throw new Error(`candidate workload report identity is duplicated or empty: ${reportPath}`);
    }
    identity[row.key] = row.value;
  }
  const required = [
    "candidate_commit",
    "platform",
    "jet_binary_sha256",
    "peer_launcher_sha256",
    "contract_sha256",
    "source_closure_sha256",
  ];
  for (const key of required) {
    if (!identity[key]) throw new Error(`candidate workload report identity is missing: ${key}`);
  }
  const reviewRows = candidateWorkloadTable(join(reportPath, "review.tsv"), CANDIDATE_WORKLOAD_REVIEW_HEADER);
  if (reviewRows.length !== 1) throw new Error(`candidate workload report review cardinality is invalid: ${reportPath}`);
  const review = reviewRows[0];
  if (review.status !== "pass"
    || review.reviewed_candidate !== identity.candidate_commit
    || review.compiler_sha256 !== identity.jet_binary_sha256
    || review.contract_sha256 !== identity.contract_sha256
    || review.source_closure_sha256 !== identity.source_closure_sha256
    || !/^[0-9a-f]{64}$/.test(review.report_sha256)) {
    throw new Error(`candidate workload report review is not bound to its identity: ${reportPath}`);
  }
  for (const key of ["jet_binary_sha256", "peer_launcher_sha256", "contract_sha256", "source_closure_sha256"]) {
    if (!/^[0-9a-f]{64}$/.test(identity[key])) {
      throw new Error(`candidate workload report identity digest is invalid: ${reportPath}:${key}`);
    }
  }
  if (!/^[0-9a-f]{40}$/.test(identity.candidate_commit)) {
    throw new Error(`candidate workload report candidate commit is invalid: ${reportPath}`);
  }
  return { identity, review };
}

export function readCandidateWorkloadReportRelation(reportRoot) {
  if (!existsSync(reportRoot)) throw new Error(`candidate workload report root is missing: ${reportRoot}`);
  const entries = readdirSync(reportRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .filter(({ name }) => /^release-compiled-workload-(linux-x86_64|macos-x86_64|windows-x86_64)-[0-9a-f]{40}$/.test(name))
    .sort((left, right) => compareStable(left.name, right.name));
  if (entries.length !== CANDIDATE_WORKLOAD_REPORT_LABELS.length) {
    throw new Error(`candidate workload report count is ${entries.length}, expected ${CANDIDATE_WORKLOAD_REPORT_LABELS.length}`);
  }
  const reports = entries.map((entry) => {
    const match = entry.name.match(/^release-compiled-workload-(linux-x86_64|macos-x86_64|windows-x86_64)-([0-9a-f]{40})$/);
    if (!match || !CANDIDATE_WORKLOAD_REPORT_LABELS.includes(match[1])) {
      throw new Error(`candidate workload report directory is invalid: ${entry.name}`);
    }
    const { identity, review } = candidateWorkloadIdentity(entry.path);
    const expectedPlatform = {
      "linux-x86_64": "linux",
      "macos-x86_64": "macos",
      "windows-x86_64": "windows",
    }[match[1]];
    if (identity.platform !== expectedPlatform) {
      throw new Error(`candidate workload report platform identity is stale: ${entry.name}`);
    }
    if (identity.candidate_commit !== match[2]) {
      throw new Error(`candidate workload report directory commit is stale: ${entry.name}`);
    }
    return {
      label: match[1],
      name: entry.name,
      path: entry.path,
      identity,
      review,
    };
  });
  const labels = new Set(reports.map((report) => report.label));
  if (labels.size !== CANDIDATE_WORKLOAD_REPORT_LABELS.length
    || CANDIDATE_WORKLOAD_REPORT_LABELS.some((label) => !labels.has(label))) {
    throw new Error("candidate workload reports do not cover every release platform");
  }
  const commits = new Set(reports.map((report) => report.identity.candidate_commit));
  const sources = new Set(reports.map((report) => report.identity.source_closure_sha256));
  const configurations = new Set(reports.map((report) => report.identity.contract_sha256));
  if (commits.size !== 1 || sources.size !== 1 || configurations.size !== 1) {
    throw new Error("candidate workload reports disagree on commit, source, or configuration identity");
  }
  const commit = reports[0].identity.candidate_commit;
  const sourceSha = candidateDigest(reports[0].identity.source_closure_sha256);
  const configurationSha = candidateDigest(reports[0].identity.contract_sha256);
  const compilerSha = proofDigestText(proofCanonicalJson(
    reports.map((report) => ({
      label: report.label,
      sha256: candidateDigest(report.identity.jet_binary_sha256),
    })),
  ));
  const targetToolSha = proofDigestText(proofCanonicalJson(
    reports.map((report) => ({
      label: report.label,
      platform: report.identity.platform,
      launcher_sha256: candidateDigest(report.identity.peer_launcher_sha256),
    })),
  ));
  const reportSha = proofDigestText(proofCanonicalJson(
    reports.map((report) => ({
      label: report.label,
      report_sha256: candidateDigest(report.review.report_sha256),
    })),
  ));
  const generatorPath = resolve(DEFAULT_ROOT, "tools/ci/compiled-workload-runner.mjs");
  if (!existsSync(generatorPath)) throw new Error("candidate workload generator is missing");
  const generatorSha = proofDigestBytes(readFileSync(generatorPath));
  const reportRootPath = relative(DEFAULT_ROOT, resolve(reportRoot)).split("\\").join("/");
  const reportEvidence = {
    status: "passed",
    valid_count: reports.length,
    report_sha256: reportSha,
    reports: reports.map((report) => ({
      label: report.label,
      path: relative(DEFAULT_ROOT, resolve(report.path)).split("\\").join("/"),
      candidate_commit: report.identity.candidate_commit,
      platform: report.identity.platform,
      compiler_sha256: candidateDigest(report.identity.jet_binary_sha256),
      report_sha256: candidateDigest(report.review.report_sha256),
    })),
  };
  const available = (id, sha256, extra = {}) => ({
    id,
    sha256,
    status: "available",
    ...extra,
  });
  return {
    commit,
    dependencies: {
      source: available("source-closure", sourceSha, { commit, path: "." }),
      generator: available("compiled-workload-runner", generatorSha, { path: "tools/ci/compiled-workload-runner.mjs" }),
      compiler: available("jet-release-binaries", compilerSha, { path: "dist/toolchain" }),
      configuration: available("compiled-workload-contract", configurationSha, { path: "tests/compiled_workloads" }),
      generated_artifact: available("compiled-workload-reports", reportSha, { path: reportRootPath }),
      target_tool: available("release-target-tools", targetToolSha, { path: reportRootPath }),
      consumed_test_oracle: available("compiled-workload-review", reportSha, { path: reportRootPath }),
      behavior: available("compiled-workload-behavior", reportSha),
      performance: available("compiled-workload-performance", reportSha),
      platform: available("compiled-workload-platform", targetToolSha),
      binary: available("jet-release-binaries", compilerSha, { path: "dist/toolchain" }),
    },
    evidence: reportEvidence,
  };
}

export function buildCandidateRecord({
  candidateId = null,
  commit = null,
  dependencies = {},
  claims = [],
  historicalReceipts = [],
  compilerProof = null,
  prerelease = null,
  status = "blocked",
  evidence = {},
} = {}) {
  const supplied = isRecord(dependencies) ? dependencies : {};
  const normalizedDependencies = Object.fromEntries(
    CANDIDATE_DEPENDENCY_KEYS.map((key) => [key, candidateIdentity(supplied[key], key)]),
  );
  const source = normalizedDependencies.source;
  const id = candidateId || candidateDefaultId(commit, source);
  const normalizedClaims = (Array.isArray(claims) ? claims : [])
    .map(candidateClaim)
    .sort((left, right) => compareStable(left.id, right.id));
  const normalizedReceipts = (Array.isArray(historicalReceipts) ? historicalReceipts : [])
    .map((receipt, index) => {
      const value = isRecord(receipt) ? cloneJson(receipt) : { id: `receipt-${index + 1}` };
      value.id = typeof value.id === "string" && value.id.length > 0 ? value.id : `receipt-${index + 1}`;
      value.historical = true;
      value.immutable = true;
      return value;
    })
    .sort((left, right) => compareStable(left.id, right.id));
  const candidate = {
    schema: CANDIDATE_SCHEMA,
    schema_version: CANDIDATE_SCHEMA_VERSION,
    candidate_identity: {
      id,
      commit: commit || source?.commit || source?.revision || null,
      source_content_sha256: candidateDigest(source?.sha256),
      compiler_sha256: normalizedDependencies.compiler?.sha256 || null,
      configuration_sha256: normalizedDependencies.configuration?.sha256 || null,
      target_tool_sha256: normalizedDependencies.target_tool?.sha256 || null,
      dependency_sha256: candidateDependencyDigest(normalizedDependencies),
      record_sha256: null,
    },
    status,
    prerelease: candidatePrerelease(prerelease),
    required_dependencies: [...CANDIDATE_DEPENDENCY_KEYS],
    dependencies: normalizedDependencies,
    claims: normalizedClaims,
    historical_receipts: normalizedReceipts,
    compiler_proof: compilerProof ? cloneJson(compilerProof) : null,
    evidence: isRecord(evidence) ? cloneJson(evidence) : {},
    content_digest: null,
  };
  candidate.content_digest = candidateContentDigest(candidate);
  candidate.candidate_identity.record_sha256 = candidateRecordDigest(candidate);
  return candidate;
}

function candidateDependencyRows(candidate) {
  const dependencies = isRecord(candidate?.dependencies) ? candidate.dependencies : {};
  return Object.fromEntries(CANDIDATE_DEPENDENCY_KEYS.map((key) => [
    key,
    isRecord(dependencies[key]) ? cloneJson(dependencies[key]) : null,
  ]));
}

function candidateReasonForState(key, state) {
  if (state === "missing") return `candidate dependency missing: ${key}`;
  if (state === "stale") return `candidate dependency stale: ${key}`;
  if (state === "unknown") return `candidate dependency unknown: ${key}`;
  if (state === "unavailable") return `candidate dependency unavailable: ${key}`;
  if (state === "failed") return `candidate dependency failed: ${key}`;
  if (state === "excluded") return `candidate dependency excluded: ${key}`;
  return `candidate dependency is not available: ${key}`;
}

function candidateObservedDependencies(current, currentDependencies) {
  if (isRecord(currentDependencies)) return currentDependencies;
  if (isRecord(current?.dependencies)) return current.dependencies;
  return null;
}

function candidateComposition(candidate, composition) {
  if (composition) return composition;
  if (isRecord(candidate?.compiler_proof)) return candidate.compiler_proof;
  if (isRecord(candidate?.proof?.composition)) return candidate.proof.composition;
  return null;
}

function candidateCompositionIdentityMatches(composition, candidateIdentity) {
  const composed = composition?.candidate_identity;
  if (!isRecord(composed) || !isRecord(candidateIdentity)) return false;
  const fields = [
    ["id", ["id"]],
    ["commit", ["commit"]],
    ["source_content_sha256", ["source_content_sha256"]],
    ["record_sha256", ["record_sha256"]],
    ["compiler_sha256", ["compiler_sha256"]],
    ["configuration_sha256", ["configuration_sha256"]],
    ["target_tool_sha256", ["target_tool_sha256"]],
    ["dependency_sha256", ["dependency_sha256"]],
  ];
  return fields.every(([field, names]) => {
    const expected = candidateIdentity[field];
    if (expected === null || expected === undefined) return false;
    const actual = names.map((name) => composed[name]).find((value) => value !== undefined && value !== null);
    return actual === expected;
  });
}

function candidateDependencyIdentityMatches(actual, expected) {
  return isRecord(actual)
    && isRecord(expected)
    && actual.id === expected.id
    && actual.status === expected.status
    && actual.sha256 === expected.sha256;
}

function candidateCompositionErrors(composition, candidate) {
  const errors = [];
  if (!isRecord(composition)) {
    errors.push("candidate compiler proof is missing");
    return errors;
  }
  if (composition.schema !== "jet.compiler-proof-composition.v1") {
    errors.push("candidate compiler proof schema is invalid");
  }
  if (composition.schema_version !== 1) errors.push("candidate compiler proof schema_version is invalid");
  if (composition.status !== "qualified") errors.push(`candidate compiler proof is ${composition.status || "unknown"}`);
  const compositionProofObject = composition.proof_object;
  if (!isRecord(compositionProofObject)
    || compositionProofObject.schema !== "jet.compiler-proof-object.v1"
    || !CANDIDATE_PROOF_POSITIVE_STATES.has(compositionProofObject.status)
    || !isRecord(compositionProofObject.artifact)
    || typeof compositionProofObject.artifact.path !== "string"
    || compositionProofObject.artifact.path.length === 0
    || !validCandidateDigest(compositionProofObject.artifact.sha256)
    || !isRecord(compositionProofObject.replay_ref)
    || typeof compositionProofObject.replay_ref.path !== "string"
    || compositionProofObject.replay_ref.path.length === 0
    || typeof compositionProofObject.replay_ref.claim_id !== "string"
    || compositionProofObject.replay_ref.claim_id.length === 0
    || !isRecord(compositionProofObject.authority)
    || compositionProofObject.authority.authority_key !== "compiler-proof.composition"
    || compositionProofObject.authority.obligation_id !== "compiler-proof.composition-candidate") {
    errors.push("candidate compiler proof composed replay proof object is missing or not approved");
  }
  const expectedProofArtifact = candidate.dependencies?.proof_artifact;
  if (!isRecord(expectedProofArtifact)
    || compositionProofObject?.artifact?.path !== expectedProofArtifact.path
    || candidateDigest(compositionProofObject?.artifact?.sha256) !== candidateDigest(expectedProofArtifact.sha256)) {
    errors.push("candidate compiler proof composed replay artifact does not match candidate proof artifact");
  }
  if (!candidateCompositionIdentityMatches(composition, candidate.candidate_identity)) {
    errors.push("candidate compiler proof identity does not match candidate");
  }
  if (!isRecord(composition.relation)) errors.push("candidate compiler proof relation is missing");
  else {
    if (composition.relation.schema !== "jet.compiler-refinement.v1") errors.push("candidate compiler proof relation schema is invalid");
    if (canonicalJson(composition.relation.stage_order) !== canonicalJson([...CANDIDATE_COMPILER_STAGES])) {
      errors.push("candidate compiler proof relation stage order is not canonical");
    }
    const relationStages = Array.isArray(composition.relation.stages)
      ? composition.relation.stages.map((row) => typeof row === "string" ? row : row?.stage)
      : null;
    if (!relationStages || canonicalJson(relationStages) !== canonicalJson([...CANDIDATE_COMPILER_STAGES])) {
      errors.push("candidate compiler proof relation stages are not canonical");
    }
  }
  const stages = composition.stage_evidence;
  if (!Array.isArray(stages)) {
    errors.push("candidate compiler proof stage evidence is missing");
  } else {
    if (stages.length !== CANDIDATE_COMPILER_STAGES.length
      || stages.some((row, index) => row?.stage !== CANDIDATE_COMPILER_STAGES[index])) {
      errors.push("candidate compiler proof stage evidence is not in canonical order");
    }
    const seen = new Set();
    for (const stage of CANDIDATE_COMPILER_STAGES) {
      const rows = stages.filter((row) => row?.stage === stage);
      if (rows.length !== 1) {
        errors.push(`candidate compiler proof stage evidence missing or duplicated: ${stage}`);
        continue;
      }
      const row = rows[0];
      seen.add(stage);
      if (!Object.hasOwn(row, "result") || !isRecord(row.result)) {
        errors.push(`candidate compiler proof stage result is missing or invalid: ${stage}`);
      }
      if (!Object.hasOwn(row, "result_sha256") || !validCandidateDigest(row.result_sha256)) {
        errors.push(`candidate compiler proof stage result digest is missing: ${stage}`);
      } else if (!isRecord(row.result)
        || row.result_sha256 !== proofDigestText(proofCanonicalJson(row.result))) {
        errors.push(`candidate compiler proof stage result digest is stale: ${stage}`);
      }
      if (row.status && !CANDIDATE_PROOF_POSITIVE_STATES.has(row.status)) {
        errors.push(`candidate compiler proof stage ${stage} is ${row.status}`);
      }
      if (row.result?.status && !CANDIDATE_PROOF_POSITIVE_STATES.has(row.result.status)) {
        errors.push(`candidate compiler proof stage result ${stage} is ${row.result.status}`);
      }
      for (const field of ["identity", "proof_object", "coverage", "relation"]) {
        if (!Object.hasOwn(row, field) || row[field] === null || row[field] === undefined) {
          errors.push(`candidate compiler proof stage ${stage} is missing ${field}`);
        }
      }
      const proofObject = row.proof_object;
      const proofPayload = proofObject?.artifact
        || proofObject?.evidence
        || proofObject?.replay_ref
        || proofObject?.replay;
      if (!isRecord(proofObject)
        || typeof proofObject.schema !== "string"
        || proofObject.schema.length === 0
        || !CANDIDATE_PROOF_POSITIVE_STATES.has(proofObject.status)
        || !isRecord(proofPayload)) {
        errors.push(`candidate compiler proof stage ${stage} proof object is not verified`);
      }
      const coverageStatus = row.coverage?.status ?? row.coverage?.qualification;
      if (!["qualified", "proved", "passed", "complete"].includes(coverageStatus)) {
        errors.push(`candidate compiler proof stage ${stage} coverage is unavailable`);
      }
      if (row.relation?.status !== undefined && !CANDIDATE_PROOF_POSITIVE_STATES.has(row.relation.status)) {
        errors.push(`candidate compiler proof stage ${stage} relation is unavailable`);
      }
      if (!isRecord(proofObject?.replay_ref)
        || typeof proofObject.replay_ref.path !== "string"
        || proofObject.replay_ref.path.length === 0
        || typeof proofObject.replay_ref.claim_id !== "string"
        || proofObject.replay_ref.claim_id.length === 0) {
        errors.push(`candidate compiler proof stage ${stage} replay reference is not verified`);
      }
      for (const field of ["source", "model", "proof", "checker", "compiler_artifact", "candidate_binary", "target", "configuration", "candidate"]) {
        if (!isRecord(row.identity?.[field])) errors.push(`candidate compiler proof stage ${stage} identity is missing ${field}`);
      }
      const stageDependencies = {
        source: "source",
        model: "proof_model",
        proof: "proof_artifact",
        checker: "proof_checker",
        compiler_artifact: "compiler",
        candidate_binary: "binary",
        target: "target_tool",
        configuration: "configuration",
      };
      for (const [field, dependencyKey] of Object.entries(stageDependencies)) {
        if (!candidateDependencyIdentityMatches(row.identity?.[field], candidate.dependencies?.[dependencyKey])) {
          errors.push(`candidate compiler proof stage ${stage} identity does not match dependency: ${dependencyKey}`);
        }
      }
      const candidateStage = row.identity?.candidate;
      if (!isRecord(candidateStage)
        || !candidateCompositionIdentityMatches({ candidate_identity: candidateStage }, candidate.candidate_identity)) {
        errors.push(`candidate compiler proof stage ${stage} identity does not match candidate`);
      }
    }
    for (const stage of stages) {
      if (!CANDIDATE_COMPILER_STAGES.includes(stage?.stage)) errors.push(`candidate compiler proof has unknown stage: ${stage?.stage || "missing"}`);
    }
    if (seen.size !== CANDIDATE_COMPILER_STAGES.length) errors.push("candidate compiler proof does not cover every required stage");
  }
  for (const field of ["supported_program_acceptance", "correct_rejection", "preservation", "foreign_assumptions"]) {
    if (!Object.hasOwn(composition, field)) errors.push(`candidate compiler proof is missing ${field}`);
  }
  const acceptance = composition.supported_program_acceptance;
  if (!CANDIDATE_PROOF_POSITIVE_STATES.has(acceptance?.status)
    || !Array.isArray(acceptance?.modes)
    || acceptance.modes.length === 0
    || !isRecord(acceptance?.evidence_ref)
    || typeof acceptance.evidence_ref.stage !== "string"
    || typeof acceptance.evidence_ref.case_id !== "string") {
    errors.push("candidate compiler proof supported-program acceptance is unavailable");
  } else {
    for (const mode of acceptance.modes) {
      if (!isRecord(mode)
        || typeof mode.mode !== "string"
        || !CANDIDATE_PROOF_POSITIVE_STATES.has(mode.status)
        || mode.accepted !== true
        || !isRecord(mode.observations)
        || mode.observations.result === undefined
        || !Number.isInteger(mode.exit_code)
        || mode.exit_code !== 0
        || mode.source_sha256 !== candidate.candidate_identity.source_content_sha256
        || mode.candidate_identity_sha256 !== candidate.candidate_identity.record_sha256) {
        errors.push(`candidate compiler proof supported-program acceptance mode is invalid: ${mode?.mode || "unknown"}`);
      }
    }
  }
  const rejection = composition.correct_rejection;
  if (!CANDIDATE_PROOF_POSITIVE_STATES.has(rejection?.status)
    || rejection.accepted !== false
    || !Array.isArray(rejection.modes)
    || rejection.modes.length === 0
    || rejection.source_sha256 !== candidate.candidate_identity.source_content_sha256
    || rejection.candidate_identity_sha256 !== candidate.candidate_identity.record_sha256
    || !isRecord(rejection.evidence_ref)
    || typeof rejection.evidence_ref.stage !== "string"
    || typeof rejection.evidence_ref.case_id !== "string") {
    errors.push("candidate compiler proof correct-rejection evidence is unavailable");
  } else {
    for (const mode of rejection.modes) {
      if (!isRecord(mode)
        || typeof mode.mode !== "string"
        || !CANDIDATE_PROOF_POSITIVE_STATES.has(mode.status)
        || mode.accepted !== false
        || !isRecord(mode.diagnostic)
        || typeof mode.diagnostic.code !== "string"
        || (!isRecord(mode.diagnostic.span) && typeof mode.diagnostic.span !== "string")
        || typeof mode.diagnostic.reason !== "string"
        || !isRecord(mode.observations)
        || (mode.observations.result === undefined && mode.result === undefined)
        || !Number.isInteger(mode.exit_code)
        || mode.exit_code === 0) {
        errors.push(`candidate compiler proof correct-rejection mode is invalid: ${mode?.mode || "unknown"}`);
      }
    }
  }
  if (!CANDIDATE_PROOF_POSITIVE_STATES.has(composition.preservation?.status)
    || composition.preservation.source_sha256 !== candidate.candidate_identity.source_content_sha256
    || composition.preservation.candidate_identity_sha256 !== candidate.candidate_identity.record_sha256) {
    errors.push("candidate compiler proof preservation evidence is unavailable");
  }
  if (!Array.isArray(composition.foreign_assumptions) || composition.foreign_assumptions.length === 0) {
    errors.push("candidate compiler proof foreign assumptions are invalid");
  }

  return errors;
}

function candidateClaimEvidence(claim, errors) {
  const evidence = isRecord(claim.evidence) ? claim.evidence : null;
  if (!evidence) {
    errors.push(`candidate claim ${claim.id} evidence is missing`);
    return;
  }
  const state = typeof evidence.status === "string" ? evidence.status : "missing";
  if (["unavailable", "unknown", "not_run", "failed", "stale"].includes(state)) {
    errors.push(`candidate claim ${claim.id} evidence is ${state}`);
  }
  const validCount = evidence.valid_count ?? evidence.valid_cases ?? evidence.valid;
  if (validCount !== undefined && (!Number.isFinite(Number(validCount)) || Number(validCount) <= 0)) {
    errors.push(`candidate claim ${claim.id} has zero-valid evidence`);
  }
  if (CANDIDATE_GREEN_STATES.has(claim.status)
    && !["passed", "qualified", "green", "valid"].includes(state)) {
    errors.push(`candidate claim ${claim.id} evidence is not passed`);
  }
}

export function validateCandidateRecord(candidate, {
  current = null,
  currentDependencies = null,
  sourceCommit = null,
  sourceSnapshotHash = null,
  binarySha256 = null,
  composition = null,
  requireQualified = false,
  requireReleaseClaim = false,
  releaseChannel = null,
} = {}) {
  const errors = [];
  const invalidations = [];
  if (!isRecord(candidate)) return { ok: false, errors: ["candidate is not an object"], invalidations: [] };
  if (candidate.schema !== CANDIDATE_SCHEMA) errors.push(`candidate schema must be ${CANDIDATE_SCHEMA}`);
  if (candidate.schema_version !== CANDIDATE_SCHEMA_VERSION) errors.push("candidate schema_version is invalid");
  if (!validCandidateDigest(candidate.content_digest)) errors.push("candidate content digest is missing or invalid");
  else if (candidate.content_digest !== candidateContentDigest(candidate)) errors.push("candidate content digest does not match content");
  if (!isRecord(candidate.candidate_identity)) errors.push("candidate identity is missing");
  else {
    const identity = candidate.candidate_identity;
    if (typeof identity.id !== "string" || identity.id.length === 0) errors.push("candidate identity id is missing");
    if (!validCandidateDigest(identity.record_sha256) || identity.record_sha256 !== candidateRecordDigest(candidate)) {
      errors.push("candidate identity record digest does not match candidate");
    }
    for (const field of ["source_content_sha256", "compiler_sha256", "configuration_sha256", "target_tool_sha256"]) {
      if (!validCandidateDigest(identity[field])) errors.push(`candidate identity ${field} is missing or invalid`);
    }
    const dependencyDigest = isRecord(candidate.dependencies) ? candidateDependencyDigest(candidate.dependencies) : null;
    if (identity.dependency_sha256 !== dependencyDigest) errors.push("candidate dependency identity digest is stale");
  }
  if (!isRecord(candidate.prerelease)) errors.push("candidate prerelease metadata is missing");
  else {
    if (candidate.prerelease.status !== "prerelease") errors.push("candidate prerelease metadata is not truthful");
    if (candidate.prerelease.owner_decision !== "#2927") errors.push("candidate prerelease metadata is not sourced from #2927");
    if (candidate.prerelease.current_version_migration !== "not-required") errors.push("candidate adds current-version migration machinery");
    if (releaseChannel === "stable" && candidate.prerelease.status === "prerelease") {
      errors.push("release channel stable conflicts with truthful prerelease metadata");
    }
  }
  if (!isRecord(candidate.dependencies)) errors.push("candidate dependencies are missing");
  const dependencies = candidateDependencyRows(candidate);
  const required = Array.isArray(candidate.required_dependencies)
    ? candidate.required_dependencies
    : [...CANDIDATE_DEPENDENCY_KEYS];
  if (canonicalJson(required) !== canonicalJson([...CANDIDATE_DEPENDENCY_KEYS])) {
    errors.push("candidate required dependencies are incomplete or not canonical");
  }
  for (const key of Object.keys(isRecord(candidate.dependencies) ? candidate.dependencies : {})) {
    if (!CANDIDATE_DEPENDENCY_KEYS.includes(key)) errors.push(`candidate dependency is unknown: ${key}`);
  }
  const dependencyIssues = new Map();
  const addDependencyIssue = (key, reason) => {
    if (!dependencyIssues.has(key)) dependencyIssues.set(key, []);
    dependencyIssues.get(key).push(reason);
    errors.push(reason);
  };
  for (const key of required) {
    const dependency = dependencies[key];
    if (!dependency) {
      addDependencyIssue(key, candidateReasonForState(key, "missing"));
      continue;
    }
    const state = dependency.status;
    if (!CANDIDATE_DEPENDENCY_STATES.has(state)) {
      addDependencyIssue(key, `candidate dependency has unknown status: ${key}`);
    } else if (state !== "available") {
      addDependencyIssue(key, candidateReasonForState(key, state));
    }
    if (typeof dependency.id !== "string" || dependency.id.length === 0) {
      addDependencyIssue(key, `candidate dependency identity id is missing: ${key}`);
    }
    if (!validCandidateDigest(dependency.sha256)) {
      addDependencyIssue(key, `candidate dependency identity digest is missing or invalid: ${key}`);
    }
    if (CANDIDATE_FILE_DEPENDENCIES.has(key)
      && (typeof dependency.path !== "string" || dependency.path.length === 0)) {
      addDependencyIssue(key, `candidate dependency path is missing: ${key}`);
    }
  }
  const candidateIdentityValues = candidate.candidate_identity;
  if (isRecord(candidateIdentityValues)) {
    const identityDependencies = [
      ["source_content_sha256", dependencies.source?.sha256],
      ["compiler_sha256", dependencies.compiler?.sha256],
      ["configuration_sha256", dependencies.configuration?.sha256],
      ["target_tool_sha256", dependencies.target_tool?.sha256],
    ];
    for (const [field, value] of identityDependencies) {
      if (!validCandidateDigest(value) || value !== candidateIdentityValues[field]) {
        errors.push(`candidate identity ${field} does not match dependency`);
      }
    }
  }
  const observed = candidateObservedDependencies(current, currentDependencies);
  if (observed) {
    for (const key of CANDIDATE_DEPENDENCY_KEYS) {
      if (!Object.hasOwn(observed, key)) continue;
      const actual = candidateComparableIdentity(observed[key], key);
      const recorded = candidateComparableIdentity(dependencies[key], key);
      if (!actual) {
        addDependencyIssue(key, candidateReasonForState(key, "missing"));
      } else if (canonicalJson(actual) !== canonicalJson(recorded)) {
        const reason = `candidate dependency changed: ${key} (recorded ${recorded?.sha256 || "missing"}, observed ${actual.sha256 || "missing"})`;
        addDependencyIssue(key, reason);
      }
    }
  }
  if (sourceCommit && dependencies.source && dependencies.source.commit !== sourceCommit) {
    addDependencyIssue("source", `candidate source commit changed (recorded ${dependencies.source.commit || "missing"}, observed ${sourceCommit})`);
  }
  if (sourceSnapshotHash && dependencies.source?.snapshot_hash
    && dependencies.source.snapshot_hash !== sourceSnapshotHash) {
    addDependencyIssue("source", `candidate source snapshot changed (recorded ${dependencies.source.snapshot_hash}, observed ${sourceSnapshotHash})`);
  }
  const observedBinary = candidateDigest(binarySha256);
  if (observedBinary && dependencies.binary && dependencies.binary.sha256 !== observedBinary) {
    addDependencyIssue("binary", `candidate binary changed (recorded ${dependencies.binary.sha256 || "missing"}, observed ${observedBinary})`);
  }
  if (!Array.isArray(candidate.claims) || candidate.claims.length === 0) {
    errors.push("candidate claims are missing");
  } else {
    const claimIds = new Set();
    for (const claim of candidate.claims) {
      if (!isRecord(claim) || typeof claim.id !== "string" || claim.id.length === 0) {
        errors.push("candidate claim identity is missing");
        continue;
      }
      if (claimIds.has(claim.id)) errors.push(`duplicate candidate claim: ${claim.id}`);
      claimIds.add(claim.id);
      if (!CANDIDATE_CLAIM_STATES.has(claim.status)) errors.push(`candidate claim ${claim.id} has unknown status: ${claim.status || "missing"}`);
      if (!Array.isArray(claim.dependencies) || claim.dependencies.length === 0) {
        errors.push(`candidate claim ${claim.id} has no dependency identities`);
      } else {
        for (const key of claim.dependencies) {
          if (!CANDIDATE_DEPENDENCY_KEYS.includes(key)) errors.push(`candidate claim ${claim.id} depends on unknown identity: ${key}`);
          if (dependencyIssues.has(key)) {
            const reason = `candidate claim ${claim.id} invalidated by ${key}: ${dependencyIssues.get(key).join("; ")}`;
            errors.push(reason);
            invalidations.push({ claim: claim.id, dependency: key, reason });
          }
        }
      }
      const ownerRatifiedNotApplicable = claim.status === "owner-ratified-not-applicable"
        && isRecord(claim.owner_exclusion)
        && Boolean(claim.owner_exclusion.reason && claim.owner_exclusion.owner && claim.owner_exclusion.decision);
      if (claim.status === "excluded" || claim.status === "owner-ratified-not-applicable") {
        const exclusion = claim.owner_exclusion;
        if (!isRecord(exclusion) || !exclusion.reason || !exclusion.owner || !exclusion.decision) {
          errors.push(`candidate claim ${claim.id} is incorrectly excluded`);
        }
      }
      if (claim.required !== false
        && !CANDIDATE_GREEN_STATES.has(claim.status)
        && !ownerRatifiedNotApplicable) {
        errors.push(`candidate claim ${claim.id} is ${claim.status || "unknown"}; qualification blocked`);
      }
      if (CANDIDATE_GREEN_STATES.has(claim.status)) candidateClaimEvidence(claim, errors);
    }
  }
  const proof = candidateComposition(candidate, composition);
  for (const error of candidateCompositionErrors(proof, candidate)) errors.push(error);
  const release = dependencies.release_claim;
  if (requireReleaseClaim || releaseChannel !== null) {
    if (!release) errors.push("candidate release claim is missing");
    else if (release.claim_status && !CANDIDATE_GREEN_STATES.has(release.claim_status)) {
      errors.push(`candidate release claim is ${release.claim_status}`);
    } else if (!release.claim_status) {
      errors.push("candidate release claim evidence status is missing");
    }
    if (releaseChannel && release?.channel && release.channel !== releaseChannel) {
      errors.push(`candidate release claim channel differs: recorded ${release.channel}, required ${releaseChannel}`);
    }
  }
  const historical = Array.isArray(candidate.historical_receipts) ? candidate.historical_receipts : null;
  if (!historical) errors.push("candidate historical receipts are missing");
  else {
    const receiptIds = new Set();
    for (const receipt of historical) {
      if (!isRecord(receipt) || typeof receipt.id !== "string" || receipt.id.length === 0) {
        errors.push("candidate historical receipt identity is missing");
        continue;
      }
      if (receiptIds.has(receipt.id)) errors.push(`duplicate candidate historical receipt: ${receipt.id}`);
      receiptIds.add(receipt.id);
      if (receipt.historical !== true) errors.push(`candidate receipt is not labeled historical: ${receipt.id}`);
      if (receipt.immutable !== true) errors.push(`candidate historical receipt is mutable: ${receipt.id}`);
    }
  }
  if (requireQualified && candidate.status !== "qualified") {
    errors.push(`candidate status is ${candidate.status || "unknown"}; qualification requires qualified`);
  }
  for (const [key, reasonsForKey] of dependencyIssues) {
    invalidations.push({ dependency: key, reasons: [...new Set(reasonsForKey)].sort(compareStable) });
  }
  return {
    ok: errors.length === 0,
    status: errors.length === 0 ? "QUALIFIED" : "BLOCKED",
    errors: [...new Set(errors)].sort(compareStable),
    invalidations: invalidations.sort((left, right) => compareStable(
      `${left.claim || ""}:${left.dependency || ""}`,
      `${right.claim || ""}:${right.dependency || ""}`,
    )),
    dependencies,
    claims: Array.isArray(candidate.claims) ? candidate.claims.map(cloneJson) : [],
    historical_receipts: historical ? historical.map(cloneJson) : [],
  };
}

export function verifyCandidateComposition(path, { root = DEFAULT_ROOT, candidate = null } = {}) {
  const resolvedRoot = resolve(root);
  const resolvedPath = resolve(resolvedRoot, path);
  const relativePath = relative(resolvedRoot, resolvedPath).split("\\").join("/");
  if (!relativePath || relativePath === ".." || relativePath.startsWith("../")) {
    return { ok: false, result: null, errors: ["candidate composition path escapes project root"] };
  }
  const driver = resolve(resolvedRoot, "scripts/agent/compiler-proof.mjs");
  if (!existsSync(driver)) return { ok: false, result: null, errors: [`candidate composition driver is missing: ${driver}`] };
  const child = spawnSync(process.execPath, [driver, "--compose", relativePath, "--json"], {
    cwd: resolvedRoot,
    encoding: "utf8",
    timeout: 15 * 60 * 1000,
    maxBuffer: 64 * 1024 * 1024,
    env: { ...process.env, LC_ALL: "C", LANG: "C", NO_COLOR: "1", CLICOLOR: "0" },
  });
  let result = null;
  try {
    result = JSON.parse(child.stdout || "");
  } catch (error) {
    return { ok: false, result: null, errors: [`candidate composition checker returned non-JSON output: ${error.message}`] };
  }
  const errors = [];
  if (result?.schema !== "jet.compiler-proof-composition-check.v1" || result?.stage !== "composition") {
    errors.push("candidate composition checker returned an unexpected schema");
  }
  if (child.error) errors.push(`candidate composition checker could not start: ${child.error.message}`);
  if (child.status !== 0) errors.push(`candidate composition checker exited ${child.status ?? "unknown"}`);
  if (result?.status !== "qualified") errors.push(`candidate composition result is ${result?.status || "unknown"}`);
  if (result?.proof_boundary?.universal_proof !== false) errors.push("candidate composition proof boundary is not conditional");
  const expected = candidate?.candidate_identity;
  const actual = result?.candidate_identity;
  for (const field of ["id", "commit", "source_content_sha256", "record_sha256", "compiler_sha256", "configuration_sha256", "target_tool_sha256", "dependency_sha256"]) {
    if (expected && actual?.[field] !== expected[field]) errors.push(`candidate composition identity differs: ${field}`);
  }
  return { ok: errors.length === 0, result, errors };
}

export function readCandidateRecord(path, options = {}) {
  if (typeof path !== "string" || path.length === 0) fail("candidate path is required");
  if (!existsSync(path)) fail(`unreadable candidate: ${path}`);
  let candidate;
  try {
    candidate = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(`unreadable candidate ${path}: ${error.message}`);
  }
  const { verifyComposition = true, compositionRoot = DEFAULT_ROOT, ...validationOptions } = options;
  const validation = validateCandidateRecord(candidate, validationOptions);
  if (!validation.ok) fail(validation.errors.join("\n"));
  if (verifyComposition) {
    const composition = verifyCandidateComposition(path, { root: compositionRoot, candidate });
    if (!composition.ok) fail(composition.errors.join("\n"));
  }
  return candidate;

}

export function invalidateCandidateClaims(candidate, substitutions = {}) {
  if (!isRecord(candidate)) throw new Error("candidate is not an object");
  const next = cloneJson(candidate);
  if (!isRecord(next.dependencies)) next.dependencies = {};
  const changed = [];
  for (const [key, value] of Object.entries(substitutions || {})) {
    if (!CANDIDATE_DEPENDENCY_KEYS.includes(key)) throw new Error(`unknown candidate dependency: ${key}`);
    next.dependencies[key] = value === null ? null : candidateIdentity(value, key);
    changed.push(key);
  }
  const reasons = new Map(changed.map((key) => [key, `candidate dependency changed: ${key}`]));
  for (const claim of Array.isArray(next.claims) ? next.claims : []) {
    if (!CANDIDATE_GREEN_STATES.has(claim.status)) continue;
    const dependencies = Array.isArray(claim.dependencies) ? claim.dependencies : [];
    const affected = dependencies.filter((key) => reasons.has(key));
    if (affected.length === 0) continue;
    claim.status = "invalidated";
    claim.evidence = {
      ...(isRecord(claim.evidence) ? claim.evidence : {}),
      status: "invalidated",
    };
    claim.invalidated_by = affected;
    claim.invalidated_reasons = affected.map((key) => reasons.get(key));
  }
  next.status = "blocked";
  next.invalidations = changed.map((key) => ({
    dependency: key,
    reason: reasons.get(key),
    claims: (Array.isArray(next.claims) ? next.claims : [])
      .filter((claim) => Array.isArray(claim.invalidated_by) && claim.invalidated_by.includes(key))
      .map((claim) => claim.id)
      .sort(compareStable),
  }));
  next.candidate_identity.source_content_sha256 = candidateDigest(next.dependencies.source?.sha256);
  next.candidate_identity.compiler_sha256 = next.dependencies.compiler?.sha256 || null;
  next.candidate_identity.configuration_sha256 = next.dependencies.configuration?.sha256 || null;
  next.candidate_identity.target_tool_sha256 = next.dependencies.target_tool?.sha256 || null;
  next.candidate_identity.dependency_sha256 = candidateDependencyDigest(next.dependencies);
  next.candidate_identity.record_sha256 = null;
  next.candidate_identity.record_sha256 = candidateRecordDigest(next);
  next.content_digest = candidateContentDigest(next);
  return next;
}

function candidateFixtureComposition(candidate, dependencies) {
  const identity = candidate.candidate_identity;
  const stage_evidence = CANDIDATE_COMPILER_STAGES.map((stage) => {
    const result = { status: "qualified", stage };
    return {
      stage,
      result,
      result_sha256: proofDigestText(proofCanonicalJson(result)),
      identity: {
        source: dependencies.source,
        model: dependencies.proof_model,
        proof: dependencies.proof_artifact,
        checker: dependencies.proof_checker,
        compiler_artifact: dependencies.compiler,
        candidate_binary: dependencies.binary,
        target: dependencies.target_tool,
        configuration: dependencies.configuration,
        candidate: identity,
      },
      proof_object: { schema: "jet.compiler-proof-object.v1", status: "qualified", artifact: { path: "fixture-proof", sha256: dependencies.proof_artifact.sha256 }, checker: dependencies.proof_checker, replay_ref: { path: "fixture-replay.json", claim_id: `${stage}-fixture` }, replay: { schema: "jet.compiler-proof-replay.v1", status: "passed" } },
      coverage: { status: "qualified", obligations: [`${stage}-obligation`] },
      relation: { status: "qualified", preserves: ["value", "error", "effect"] },
    };
  });
  return {
    schema: "jet.compiler-proof-composition.v1",
    schema_version: 1,
    status: "qualified",
    proof_object: {
      schema: "jet.compiler-proof-object.v1",
      status: "qualified",
      artifact: { path: "fixture-proof", sha256: dependencies.proof_artifact.sha256 },
      replay_ref: { path: "proof/compiler/toolchain/manifest.json", claim_id: "compiler-proof.composition-candidate" },
      authority: { authority_key: "compiler-proof.composition", obligation_id: "compiler-proof.composition-candidate" },
    },
    candidate_identity: {
      id: identity.id,
      commit: identity.commit,
      source_content_sha256: identity.source_content_sha256,
      record_sha256: identity.record_sha256,
      compiler_sha256: identity.compiler_sha256,
      configuration_sha256: identity.configuration_sha256,
      target_tool_sha256: identity.target_tool_sha256,
      dependency_sha256: identity.dependency_sha256,
    },
    relation: {
      schema: "jet.compiler-refinement.v1",
      stage_order: [...CANDIDATE_COMPILER_STAGES],
      representations: [],
      edges: [],
      observation_alphabet: ["value", "error", "effect"],
      preservation_dimensions: ["value", "error", "effect"],
      stages: [...CANDIDATE_COMPILER_STAGES],
    },
    stage_evidence,
    supported_program_acceptance: {
      status: "passed",
      modes: ["aot", "jet_run", "interpreter"].map((mode) => ({
        mode,
        status: "passed",
        accepted: true,
        observations: { status: "passed", result: { status: "passed" } },
        exit_code: 0,
        source_sha256: identity.source_content_sha256,
        candidate_identity_sha256: identity.record_sha256,
      })),
      evidence_ref: { stage: "semantics", case_id: "fixture-programs" },
    },
    correct_rejection: {
      status: "passed",
      accepted: false,
      modes: ["aot", "jet_run", "interpreter"].map((mode) => ({
        mode,
        status: "passed",
        accepted: false,
        diagnostic: { code: "E-fixture", span: "fixture:1:1", reason: "fixture rejection" },
        observations: { status: "passed" },
        result: { status: "rejected" },
        exit_code: 1,
      })),
      source_sha256: identity.source_content_sha256,
      candidate_identity_sha256: identity.record_sha256,
      evidence_ref: { stage: "checking", case_id: "fixture-rejection" },
    },
    preservation: {
      status: "passed",
      source_sha256: identity.source_content_sha256,
      candidate_identity_sha256: identity.record_sha256,
      evidence_ref: { stage: "runtime", case_id: "fixture-preservation" },
    },
    foreign_assumptions: [{ id: "fixture-foreign", status: "assumed", universal_proof: false, statement: "fixture foreign premise", boundary: "fixture boundary" }],
  };
}

function candidateFixture() {
  const dependencies = Object.fromEntries(CANDIDATE_DEPENDENCY_KEYS.map((key) => [key, {
    id: `fixture-${key}`,
    sha256: proofDigestText(`dependency:${key}`),
    status: "available",
    commit: "fixture-commit",
    ...(CANDIDATE_FILE_DEPENDENCIES.has(key) ? { path: `fixture/${key}` } : {}),
    ...(key === "source" ? { content_sha256: proofDigestText("fixture-source") } : {}),
    ...(key === "release_claim" ? { claim_status: "passed", channel: "prerelease" } : {}),
  }]));
  const claims = [
    { id: "proof", kind: "proof", status: "passed", required: true, dependencies: ["source", "generator", "compiler", "configuration", "generated_artifact", "target_tool", "proof_model", "proof_checker", "proof_artifact"], evidence: { status: "passed", valid_count: 1 } },
    { id: "behavior", kind: "behavior", status: "passed", required: true, dependencies: ["source", "compiler", "target_tool", "binary", "consumed_test_oracle"], evidence: { status: "passed", valid_count: 1 } },
    { id: "performance", kind: "performance", status: "passed", required: true, dependencies: ["source", "compiler", "target_tool", "binary", "performance"], evidence: { status: "passed", valid_count: 1 } },
    { id: "platform", kind: "platform", status: "passed", required: true, dependencies: ["source", "compiler", "target_tool", "binary", "platform"], evidence: { status: "passed", valid_count: 1 } },
    { id: "owner", kind: "owner", status: "passed", required: true, dependencies: ["documentation", "owner_acceptance", "release_claim"], evidence: { status: "passed", valid_count: 1 } },
  ];
  const candidate = buildCandidateRecord({
    candidateId: "fixture-candidate",
    commit: "fixture-commit",
    dependencies,
    claims,
    historicalReceipts: [{ id: "historical-fixture", identity: { id: "old", sha256: proofDigestText("old") } }],
    status: "qualified",
  });
  candidate.compiler_proof = candidateFixtureComposition(candidate, candidate.dependencies);
  candidate.content_digest = candidateContentDigest(candidate);
  return candidate;
}

export function candidateNegativeControls() {
  const base = candidateFixture();
  const controls = [];
  for (const key of ["source", "generator", "proof_model", "proof_checker", "target_tool", "binary", "documentation", "release_claim"]) {
    const replacement = {
      ...(base.dependencies[key] || {}),
      id: `substituted-${key}`,
      sha256: proofDigestText(`substituted:${key}`),
    };
    if (key === "release_claim") replacement.claim_status = "failed";
    const candidate = invalidateCandidateClaims(base, { [key]: replacement });
    const validation = validateCandidateRecord(candidate);
    const historical = candidate.historical_receipts?.[0];
    const dependentClaims = candidate.invalidations?.[0]?.claims || [];
    controls.push({
      control: key,
      accepted: validation.ok,
      invalidated_claims: dependentClaims,
      historical_immutable: historical?.historical === true && historical?.immutable === true,
      errors: validation.errors,
    });
  }
  return controls;
}

export function runCandidateNegativeControls() {
  const controls = candidateNegativeControls();
  if (controls.some((control) => control.accepted || !control.historical_immutable || control.invalidated_claims.length === 0)) {
    fail("candidate negative control was accepted or failed to invalidate a dependent claim");
  }
  return { schema: CANDIDATE_SCHEMA, status: "PASS", controls };
}

function sourceBytes(value) {
  return Buffer.isBuffer(value) || value instanceof Uint8Array
    ? Buffer.from(value)
    : Buffer.from(String(value), "utf8");
}

function fail(message) {
  throw new Error(message);
}

function decodeRustString(value) {
  try { return JSON.parse(`"${value}"`); } catch { return value; }
}

function quoted(text) {
  return Array.from(text.matchAll(/"([^"\\]*(?:\\.[^"\\]*)*)"/g), (match) => decodeRustString(match[1]));
}

function matching(text, start, opening, closing) {
  let depth = 0;
  let quote = null;
  let escaped = false;
  for (let index = start; index < text.length; index += 1) {
    const char = text[index];
    if (quote) {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === quote) quote = null;
      continue;
    }
    if (char === '"') { quote = '"'; continue; }
    if (char === opening) depth += 1;
    else if (char === closing && --depth === 0) return index;
  }
  fail(`unbalanced ${opening} at ${start}`);
}

function withoutComments(text) {
  let out = "";
  let quote = null;
  let escaped = false;
  let line = false;
  let block = 0;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    const next = text[index + 1];
    if (line) {
      if (char === "\n") { line = false; out += char; } else out += char === "\r" ? char : " ";
      continue;
    }
    if (block) {
      if (char === "/" && next === "*") { block += 1; out += "  "; index += 1; }
      else if (char === "*" && next === "/") { block -= 1; out += "  "; index += 1; }
      else out += char === "\n" || char === "\r" ? char : " ";
      continue;
    }
    if (quote) {
      out += char;
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === quote) quote = null;
      continue;
    }
    if (char === '"') { quote = '"'; out += char; continue; }
    if (char === "/" && next === "/") { line = true; out += "  "; index += 1; continue; }
    if (char === "/" && next === "*") { block = 1; out += "  "; index += 1; continue; }
    out += char;
  }
  return out;
}

function codeOnly(text) {
  let out = "";
  let quote = null;
  let escaped = false;
  let line = false;
  let block = 0;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    const next = text[index + 1];
    if (line) {
      if (char === "\n") { line = false; out += char; } else out += char === "\r" ? char : " ";
      continue;
    }
    if (block) {
      if (char === "/" && next === "*") { block += 1; out += "  "; index += 1; }
      else if (char === "*" && next === "/") { block -= 1; out += "  "; index += 1; }
      else out += char === "\n" || char === "\r" ? char : " ";
      continue;
    }
    if (quote) {
      out += char === "\n" || char === "\r" ? char : " ";
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === quote) quote = null;
      continue;
    }
    if (char === '"') { quote = '"'; out += " "; continue; }
    if (char === "/" && next === "/") { line = true; out += "  "; index += 1; continue; }
    if (char === "/" && next === "*") { block = 1; out += "  "; index += 1; continue; }
    out += char;
  }
  return out;
}

function rustStringConstants(source) {
  const out = new Map();
  const clean = withoutComments(source);
  for (const match of clean.matchAll(/pub\s+const\s+([A-Z][A-Z0-9_]*)\s*:\s*&str\s*=\s*"([^"\\]*(?:\\.[^"\\]*)*)"/g)) {
    out.set(match[1], decodeRustString(match[2]));
  }
  return out;
}

function rustStrings(text, constants) {
  const out = new Set(quoted(text));
  for (const match of text.matchAll(/\b(?:Syntax::)?([A-Z][A-Z0-9_]*)\b/g)) {
    if (constants.has(match[1])) out.add(constants.get(match[1]));
  }
  return [...out];
}

function splitTopLevel(text) {
  const parts = [];
  let start = 0;
  let round = 0;
  let square = 0;
  let curly = 0;
  let quote = false;
  let escaped = false;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    if (quote) {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === '"') quote = false;
      continue;
    }
    if (char === '"') { quote = true; continue; }
    if (char === "(") round += 1;
    else if (char === ")") round -= 1;
    else if (char === "[") square += 1;
    else if (char === "]") square -= 1;
    else if (char === "{") curly += 1;
    else if (char === "}") curly -= 1;
    else if (char === "," && round === 0 && square === 0 && curly === 0) {
      parts.push(text.slice(start, index).trim());
      start = index + 1;
    }
  }
  parts.push(text.slice(start).trim());
  return parts;
}

function calls(source, needle) {
  const clean = withoutComments(source);
  const out = [];
  let cursor = 0;
  while (true) {
    const start = clean.indexOf(needle, cursor);
    if (start < 0) break;
    const open = start + needle.length - 1;
    const close = matching(clean, open, "(", ")");
    out.push({ start, args: splitTopLevel(clean.slice(open + 1, close)) });
    cursor = close + 1;
  }
  return out;
}

function moduleItemsFromSource(moduleItemsSource, surfaceSource) {
  const start = moduleItemsSource.indexOf("pub fn core_module_items");
  const end = moduleItemsSource.indexOf("/// Ratified nominal types", start);
  if (start < 0 || end < 0) fail("core module item source anchors disappeared");
  const body = withoutComments(moduleItemsSource.slice(start, end));
  const constants = rustStringConstants(surfaceSource);
  const modules = new Map();
  const arms = /^\s*((?:"[^"]+"\s*(?:\|\s*)?)+)=>\s*&\[/gm;
  for (const arm of body.matchAll(arms)) {
    const opening = body.indexOf("[", arm.index + arm[0].length - 1);
    const close = matching(body, opening, "[", "]");
    const names = rustStrings(body.slice(opening + 1, close), constants);
    for (const module of rustStrings(arm[1], constants)) {
      if (!modules.has(module)) modules.set(module, new Set());
      for (const name of names) modules.get(module).add(name);
    }
  }
  if (body.includes('module == "core.compiler.lang"')) modules.set("core.compiler.lang", new Set());
  const memStart = surfaceSource.indexOf("pub const CORE_MEM_GATE_TIERS");
  const memTable = surfaceSource.indexOf("= &[", memStart);
  if (memStart < 0 || memTable < 0) fail("CORE_MEM_GATE_TIERS source anchor disappeared");
  const memClean = withoutComments(surfaceSource);
  const memOpening = memClean.indexOf("[", memClean.indexOf("= &[", memClean.indexOf("pub const CORE_MEM_GATE_TIERS")));
  const memClose = matching(memClean, memOpening, "[", "]");
  const memNames = rustStrings(memClean.slice(memOpening + 1, memClose), rustStringConstants(surfaceSource));
  if (memNames.length === 0) fail("CORE_MEM_GATE_TIERS yielded no names");
  modules.set("core.mem", new Set(memNames));
  return modules;
}

function explicitTypes(moduleItemsSource) {
  const start = moduleItemsSource.indexOf("pub(crate) fn core_module_type_item");
  if (start < 0) return new Map();
  const body = withoutComments(moduleItemsSource.slice(start));
  const out = new Map();
  const pair = /\(\s*((?:"[^"]+"\s*(?:\|\s*)?)+)\s*,\s*((?:"[^"]+"\s*(?:\|\s*)?)+)\s*\)/g;
  for (const match of body.matchAll(pair)) {
    const modules = quoted(match[1]);
    const names = quoted(match[2]);
    for (const module of modules) {
      if (!out.has(module)) out.set(module, new Set());
      for (const name of names) out.get(module).add(name);
    }
  }
  return out;
}

function moduleItemEvidence(source, module, member, stableId) {
  const clean = withoutComments(source);
  const moduleIndex = clean.indexOf(`"${module}"`);
  const memberIndex = clean.indexOf(`"${member}"`, Math.max(0, moduleIndex));
  const offset = memberIndex >= 0 ? memberIndex : moduleIndex >= 0 ? moduleIndex : 0;
  return [rowEvidence(PATHS.moduleItems, source, offset, "core-module-registry", stableId)];
}

function typeMembershipEvidence(source, stableId) {
  const untagged = untaggedId(stableId);
  const dot = untagged.lastIndexOf(".");
  const module = dot < 0 ? "" : untagged.slice(0, dot);
  const type = dot < 0 ? untagged : untagged.slice(dot + 1);
  return moduleItemEvidence(source, module, type, stableId);
}

function parseCoreCallRegistry(callsSource, path = PATHS.calls) {
  const events = [
    ...calls(callsSource, "CoreCallRecord::new(").map((call) => ({ ...call, kind: "module_call" })),
    ...calls(callsSource, "CoreCallRecord::receiver(").map((call) => ({ ...call, kind: "receiver_method" })),
  ].sort((left, right) => left.start - right.start);
  const modules = new Map();
  const receivers = new Map();
  for (const [index, event] of events.entries()) {
    const block = codeOnly(callsSource.slice(event.start, events[index + 1]?.start ?? callsSource.length));
    if (event.kind === "module_call") {
      const module = quoted(event.args[0] || "")[0];
      const member = quoted(event.args[1] || "")[0];
      if (!module || !member || !module.startsWith("core.")) continue;
      const stable_id = `module:${module}.${member}`;
      modules.set(stable_id, {
        stable_id,
        module,
        member,
        aot_direct: !/\.without_direct_aot\s*\(\s*\)/.test(block),
        jit_direct: !/\.without_direct_jit\s*\(\s*\)/.test(block),
        interpreter_explicit: /\.with_(?:pure_route|interpreter_route)\s*\(/.test(block),
        evidence: [rowEvidence(path, callsSource, event.start, "CoreCallRecord::new", stable_id)],
      });
      continue;
    }
    const types = quoted(event.args[0] || "");
    const member = quoted(event.args[1] || "")[0];
    if (!member || types.length === 0) continue;
    for (const type of types) {
      const stable_id = `receiver:${type}.${member}`;
      receivers.set(stable_id, {
        stable_id,
        type,
        member,
        evidence: [rowEvidence(path, callsSource, event.start, "CoreCallRecord::receiver", stable_id)],
      });
    }
  }
  return { modules, receivers };
}

function parseReceiverRows(callsSource) {
  return [...parseCoreCallRegistry(callsSource).receivers.values()]
    .sort((left, right) => compareStable(left.stable_id, right.stable_id));
}

function parsePlainCallRows(callsSource) {
  return [...parseCoreCallRegistry(callsSource).modules.values()]
    .map((row) => row.stable_id)
    .sort(compareStable);
}

function parsePairLiterals(source) {
  const clean = withoutComments(source);
  const out = [];
  const pattern = /\(\s*"([^"\\]*(?:\\.[^"\\]*)*)"\s*,\s*"([^"\\]*(?:\\.[^"\\]*)*)"\s*\)/g;
  for (const match of clean.matchAll(pattern)) {
    out.push({
      first: decodeRustString(match[1]),
      second: decodeRustString(match[2]),
      start: match.index,
    });
  }
  return out;
}

function parseFieldRows(typesSource, surfaceSource) {
  const clean = withoutComments(typesSource);
  const evidence = new Map();
  const add = (type, field, offset = 0, path = PATHS.fields, seam = "core-field-registry") => {
    if (!/^[A-Z][A-Za-z0-9_]*$/.test(type) || !/^[a-z_][A-Za-z0-9_]*$/.test(field)) return;
    const stable = `${type}.${field}`;
    if (!evidence.has(stable)) evidence.set(stable, []);
    evidence.get(stable).push(rowEvidence(
      path,
      path === PATHS.fields ? typesSource : surfaceSource,
      offset,
      seam,
      `field:${stable}`,
    ));
  };
  const pair = /\(\s*((?:"[^"]+"\s*(?:\|\s*)?)+)\s*,\s*((?:"[^"]+"\s*(?:\|\s*)?)+)\s*\)\s*=>/g;
  for (const match of clean.matchAll(pair)) {
    for (const type of quoted(match[1])) for (const field of quoted(match[2])) add(type, field, match.index);
  }
  for (const fieldMatch of clean.matchAll(/match\s+field\s*\{/g)) {
    const opening = clean.indexOf("{", fieldMatch.index + fieldMatch[0].length - 1);
    let close;
    try { close = matching(clean, opening, "{", "}"); } catch { continue; }
    const context = clean.slice(Math.max(0, fieldMatch.index - 2200), fieldMatch.index);
    const types = new Set();
    for (const match of context.matchAll(/(?:type_name|name)\s*==\s*"([A-Z][A-Za-z0-9_]*)"/g)) types.add(match[1]);
    for (const match of context.matchAll(/matches!\(\s*(?:type_name|name)\s*,\s*((?:"[^"]+"\s*(?:\|\s*)?)+)\)/g)) {
      for (const type of quoted(match[1])) types.add(type);
    }
    const body = clean.slice(opening + 1, close);
    const fields = [];
    for (const match of body.matchAll(/"([a-z_][A-Za-z0-9_]*)"\s*(?:\|\s*)*(?==>)/g)) {
      fields.push({ name: match[1], offset: opening + 1 + match.index });
    }
    for (const type of types) for (const field of fields) add(type, field.name, field.offset);
  }
  const constructable = clean.indexOf("pub(crate) fn core_constructable_fields");
  if (constructable >= 0) {
    const body = clean.slice(constructable);
    for (const match of body.matchAll(/"([A-Z][A-Za-z0-9_]*)"\s*=>\s*Some\s*\(\s*vec!\s*\[/g)) {
      const opening = body.indexOf("[", match.index + match[0].length - 1);
      let close;
      try { close = matching(body, opening, "[", "]"); } catch { continue; }
      for (const field of body.slice(opening + 1, close).matchAll(/\(\s*"([a-z_][A-Za-z0-9_]*)"/g)) {
        add(match[1], field[1], constructable + opening + 1 + field.index, PATHS.fields, "core-constructable-field");
      }
    }
  }
  // A field table can use Syntax string constants for a reserved type name.
  const constants = rustStringConstants(surfaceSource);
  for (const [name, value] of constants) {
    if (!/^[A-Z][A-Za-z0-9_]*$/.test(value)) continue;
    const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    for (const match of clean.matchAll(new RegExp(`Syntax::${escaped}\\s*[,)]`, "g"))) {
      const context = clean.slice(Math.max(0, match.index - 400), match.index + 400);
      const fields = quoted(context).filter((field) => /^[a-z_][A-Za-z0-9_]*$/.test(field));
      for (const field of fields) add(value, field);
    }
  }
  return [...evidence.keys()].sort(compareStable).map((stable) => {
    const dot = stable.lastIndexOf(".");
    return {
      stable_id: `field:${stable}`,
      type: stable.slice(0, dot),
      field: stable.slice(dot + 1),
      evidence: uniqueSorted(evidence.get(stable)),
    };
  });
}

function coreConformanceInventory(root, fallback) {
  const script = join(root, "scripts/agent/core-conformance.mjs");
  if (!existsSync(script)) return fallback;
  const result = spawnSync(process.execPath, [script, "--inventory"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.status !== 0) fail(`core conformance inventory failed: ${result.stderr.trim()}`);
  try {
    const parsed = JSON.parse(result.stdout);
    if (!Array.isArray(parsed)) fail("core conformance inventory is not an array");
    return parsed;
  } catch (error) {
    fail(`core conformance inventory is unreadable: ${error.message}`);
  }
}
function sourceMembership(root) {
  const moduleItemsSource = readFileSync(join(root, PATHS.moduleItems), "utf8");
  const surfaceSource = readFileSync(join(root, PATHS.surface), "utf8");
  const modules = moduleItemsFromSource(moduleItemsSource, surfaceSource);
  const fallbackCalls = [];
  const moduleNames = new Set(modules.keys());
  for (const [module, names] of modules) for (const name of names) {
    if (!/^[A-Z]/.test(name) && !VALUE_NAMES.has(name) && !moduleNames.has(`${module}.${name}`)) {
      fallbackCalls.push(`${module}.${name}`);
    }
  }
  const moduleCalls = coreConformanceInventory(root, [...new Set(fallbackCalls)].sort(compareStable))
    .map((value) => normalizeId("module_call", value));
  const explicit = explicitTypes(moduleItemsSource);
  const typeSet = new Set();
  for (const [module, names] of modules) {
    for (const name of names) if (/^[A-Z]/.test(name)) typeSet.add(`type:${module}.${name}`);
  }
  for (const [module, names] of explicit) {
    for (const name of names) typeSet.add(`type:${module}.${name}`);
  }
  const callsSource = readFileSync(join(root, PATHS.calls), "utf8");
  const registry = { source: callsSource, rows: parseCoreCallRegistry(callsSource) };
  const receivers = parseReceiverRows(callsSource);
  const fields = parseFieldRows(readFileSync(join(root, PATHS.fields), "utf8"), surfaceSource);
  const sourceIds = {
    module_call: uniqueSorted(moduleCalls),
    receiver_method: uniqueSorted(receivers.map((row) => row.stable_id)),
    field: uniqueSorted(fields.map((row) => row.stable_id)),
    nominal_type: uniqueSorted([...typeSet]),
  };
  const typeRows = sourceIds.nominal_type.map((stable_id) => ({
    stable_id,
    evidence: typeMembershipEvidence(moduleItemsSource, stable_id),
  }));
  return {
    moduleItemsSource,
    surfaceSource,
    moduleCalls: sourceIds.module_call,
    receiverRows: receivers,
    fieldRows: fields,
    typeRows,
    registry,
    sourceIds,
  };
}

function normalizedSeedEntries(seeds) {
  if (seeds instanceof Map) return [...seeds.entries()];
  if (isRecord(seeds)) return Object.entries(seeds);
  return [];
}

function seedMap(surface, members) {
  const out = new Map();
  const memberIds = new Set(Object.values(members).flat());
  const paths = new Map();
  for (const [rawId, seed] of normalizedSeedEntries(surface.seeds)) {
    const stableId = publicStableId(rawId);
    if (!stableId) fail("recipe has no stable public identity");
    if (!memberIds.has(stableId)) fail(`recipe has no manifest row: ${stableId}`);
    if (out.has(stableId)) fail(`duplicate recipe identity: ${stableId}`);
    if (!isRecord(seed)) fail(`recipe is not an object: ${stableId}`);
    if (seed.stable_id !== undefined && seed.stable_id !== stableId) {
      fail(`recipe identity mismatch: ${stableId}`);
    }
    if (typeof seed.path !== "string" || seed.path.length === 0) {
      fail(`recipe path is required: ${stableId}`);
    }
    if (!Array.isArray(seed.errors) || seed.errors.some((error) => typeof error !== "string" || error.length === 0)) {
      fail(`recipe diagnostics are invalid: ${stableId}`);
    }
    if (paths.has(seed.path)) fail(`duplicate recipe path: ${seed.path}`);
    paths.set(seed.path, stableId);
    out.set(stableId, seed);
  }
  return out;
}


function walk(root) {
  if (!existsSync(root)) return [];
  const out = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    if (entry.isDirectory() && entry.name.startsWith(".")) continue;
    const path = join(root, entry.name);
    if (entry.isDirectory()) out.push(...walk(path));
    else if (entry.isFile() && entry.name !== "package.jet" && entry.name.endsWith(".jet")) out.push(path);
  }
  return out.sort();
}

function seedKey(corpusRoot, path) {
  const rel = relative(corpusRoot, path).replaceAll("\\", "/");
  const parts = rel.split("/");
  const name = parts.pop().replace(/\.jet$/, "");
  return parts.length ? `${parts.join(".")}.${name}` : name;
}

function observerCalls(code) {
  const out = [];
  for (const match of code.matchAll(/(?<![A-Za-z0-9_.])(?:print|eprint|assert)\s*\(/g)) {
    const open = match.index + match[0].lastIndexOf("(");
    try { out.push({ operation: match[0].trim().slice(0, -1), open, close: matching(code, open, "(", ")") }); } catch { /* invalid sink */ }
  }
  return out;
}

function expressionCalls(code) {
  const out = [];
  const pattern = /(?<![A-Za-z0-9_.])(?:[A-Za-z_][A-Za-z0-9_]*\s*\.\s*)*[A-Za-z_][A-Za-z0-9_]*\s*(?:<[^{}]*>\s*)?\(/g;
  for (const match of code.matchAll(pattern)) {
    const open = match.index + match[0].lastIndexOf("(");
    try {
      out.push({
        start: match.index,
        open,
        close: matching(code, open, "(", ")"),
        callee: match[0].slice(0, match[0].lastIndexOf("(")).trim().replace(/\s+/g, ""),
      });
    } catch { /* invalid call */ }
  }
  return out;
}

function bindingExpressionContinues(code, newline) {
  let next = newline + 1;
  while (next < code.length && /[ \t\r]/.test(code[next])) next += 1;
  if (/^(?:\?\?|\.|,|&&|\|\||[+\-*\/%<>=])/.test(code.slice(next))) return true;
  let previous = newline - 1;
  while (previous >= 0 && /[ \t\r]/.test(code[previous])) previous -= 1;
  return /(?:\?\?|[,(+\-*\/%<>=])$/.test(code.slice(Math.max(0, previous - 2), previous + 1));
}

function bindingExpressionEnd(code, start) {
  let round = 0;
  let square = 0;
  let curly = 0;
  for (let index = start; index < code.length; index += 1) {
    const char = code[index];
    if (char === "(") round += 1;
    else if (char === ")") round -= 1;
    else if (char === "[") square += 1;
    else if (char === "]") square -= 1;
    else if (char === "{") curly += 1;
    else if (char === "}") {
      if (round === 0 && square === 0 && curly === 0) return index;
      curly -= 1;
    } else if (char === "\n" && round === 0 && square === 0 && curly === 0 && !bindingExpressionContinues(code, index)) return index;
    else if (char === ";" && round === 0 && square === 0 && curly === 0) return index;
  }
  return code.length;
}

function bindingDeclarations(code, observers) {
  const out = [];
  const pattern = /(?:^|[{};])\s*(@?[A-Za-z_][A-Za-z0-9_]*)\s*(?:::|:=)\s*/gm;
  for (const match of code.matchAll(pattern)) {
    const start = match.index + match[0].indexOf(match[1]);
    out.push({ name: match[1], start, rhsStart: match.index + match[0].length });
  }
  for (let index = 0; index < out.length; index += 1) {
    const declaration = out[index];
    const next = out[index + 1]?.start ?? code.length;
    const observer = observers.find(({ open }) => open >= declaration.rhsStart)?.open ?? code.length;
    declaration.end = Math.min(next, observer, bindingExpressionEnd(code, declaration.rhsStart));
  }
  return out;
}

function nearbyOperator(code, start, end, containerStart, containerEnd) {
  const before = code.slice(containerStart, start).replace(/[\s()]+$/g, "");
  const after = code.slice(end, containerEnd).replace(/^[\s()]+/g, "");
  if (/^(?:\?\?|===|!==|==|!=)/.test(after) || /(?:\?\?|===|!==|==|!=)$/.test(before)) {
    return after.startsWith("??") || before.endsWith("??") ? "error-propagation" : "equality";
  }
  return null;
}

function valueContext(code, start, end, containerStart, containerEnd, calls, observers, allowPropagation = true) {
  const enclosing = calls
    .filter(({ open, close }) => open < start && end <= close && !observers.some((observer) => observer.open === open))
    .sort((left, right) => (left.close - left.open) - (right.close - right.open))[0] || null;
  const member = code.slice(end, containerEnd).match(/^\s*\.\s*([A-Za-z_][A-Za-z0-9_]*)/)?.[1] || null;
  const operator = nearbyOperator(code, start, end, containerStart, containerEnd);
  const usableOperator = operator === "error-propagation" && !allowPropagation ? null : operator;
  return {
    aware: Boolean(enclosing || member || usableOperator),
    follow_up: member || enclosing?.callee || usableOperator || null,
  };
}

function traceBinding(code, binding, after, opaque, declarations, observers, calls, rootBinding, inheritedAware = false, inheritedFollowUp = null, seen = new Set()) {
  const traceKey = `${binding}:${after}`;
  if (seen.has(traceKey)) return null;
  seen.add(traceKey);
  const escaped = binding.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const use = new RegExp(`(?<![A-Za-z0-9_])${escaped}(?![A-Za-z0-9_])`, "g");
  for (const match of code.matchAll(use)) {
    const start = match.index;
    if (start <= after) continue;
    const observer = observers.find(({ open, close }) => open < start && start < close);
    if (observer) {
      const context = valueContext(code, start, start + binding.length, observer.open + 1, observer.close, calls, observers);
      const aware = inheritedAware || context.aware;
      if (!opaque || aware) {
        const followUp = inheritedFollowUp || context.follow_up;
        return {
          kind: aware ? "follow-up" : "primitive",
          operation: observer.operation,
          binding: rootBinding,
          follow_up: followUp,
          type_aware: true,
          observed_type: aware ? "derived-primitive" : "declared-return-value",
        };
      }
      continue;
    }
    const next = declarations.find((declaration) => declaration.start >= after && declaration.rhsStart <= start && start < declaration.end);
    if (!next) continue;
    const context = valueContext(code, start, start + binding.length, next.rhsStart, next.end, calls, observers);
    const traced = traceBinding(
      code,
      next.name,
      next.end,
      opaque,
      declarations,
      observers,
      calls,
      rootBinding,
      inheritedAware || context.aware,
      inheritedFollowUp || context.follow_up,
      seen,
    );
    if (traced) return traced;
  }
  return null;
}

function likelyOpaque(module, member) {
  return /(?:^|\.)(open|connect|listen|accept|bind|stdin|stdout|stderr|reader|writer|session|socket|request|response|handle|lock|watch|spawn|transaction|cursor|stream|server|client)$/.test(`${module}.${member}`);
}

function explicitPropagationObserver(code, callClose, observers) {
  const lineEnd = code.indexOf("\n", callClose);
  const tail = code.slice(callClose + 1, lineEnd < 0 ? code.length : lineEnd);
  if (!/\?\?\s*panic\s*\(/.test(tail)) return null;
  return observers.find(({ open }) => open > callClose) || null;
}

function seedInspection(key, source) {
  const errors = [];
  const dot = key.lastIndexOf(".");
  const module = key.slice(0, dot);
  const member = key.slice(dot + 1);
  const expectedMarker = `// core-conformance: ${key}`;
  if (source.split(/\r?\n/, 1)[0] !== expectedMarker) errors.push(`missing exact marker ${expectedMarker}`);
  const code = codeOnly(source);
  const unitRun = code.match(/\bfn\s+run\s*\(\s*\)\s*\{/);
  if (unitRun) {
    const opening = unitRun.index + unitRun[0].lastIndexOf("{");
    const closing = matching(code, opening, "{", "}");
    if (/\?\?\s*return\s+Err\s*\(/.test(code.slice(opening + 1, closing))) {
      errors.push("Unit run cannot use ?? return Err(...) propagation");
      return { errors, sink: null };
    }
  }
  const aliasMatches = [...code.matchAll(new RegExp(`^\\s*use\\s+${module.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s+as\\s+([A-Za-z_][A-Za-z0-9_]*)`, "gm"))];
  if (aliasMatches.length !== 1) return { errors: [...errors, `expected one use for ${module}`], sink: null };
  const alias = aliasMatches[0][1];
  const callMatches = [...code.matchAll(new RegExp(`(?<![A-Za-z0-9_.])${alias}\\s*\\.\\s*${member}\\s*(?:<[^{}]*>\\s*)?\\(`, "g"))];
  if (callMatches.length !== 1) return { errors: [...errors, `expected one ${key} call`], sink: null };
  const callStart = callMatches[0].index;
  const callOpen = callStart + callMatches[0][0].lastIndexOf("(");
  let callClose;
  try { callClose = matching(code, callOpen, "(", ")"); } catch { return { errors: [...errors, `unbalanced ${key} call`], sink: null }; }
  const observers = observerCalls(code);
  const calls = expressionCalls(code);
  const directObserver = observers.find(({ open, close }) => open < callStart && callStart < close);
  if (directObserver) {
    const context = valueContext(code, callStart, callClose + 1, directObserver.open + 1, directObserver.close, calls, observers);
    if (likelyOpaque(module, member) && !context.aware) errors.push("opaque result needs a follow-up operation before observation");
    return {
      errors,
      sink: {
        kind: "call-result",
        operation: directObserver.operation,
        follow_up: context.follow_up,
        type_aware: true,
        observed_type: context.aware ? "derived-primitive" : "return-value",
      },
    };
  }
  const declarations = bindingDeclarations(code, observers);
  const binding = declarations.find(({ rhsStart, end }) => rhsStart <= callStart && callStart < end) || null;
  if (!binding) {
    const propagationObserver = explicitPropagationObserver(code, callClose, observers);
    if (propagationObserver && !likelyOpaque(module, member)) {
      return {
        errors,
        sink: {
          kind: "propagated-result",
          operation: propagationObserver.operation,
          follow_up: "error-propagation",
          type_aware: true,
          observed_type: "successful-continuation",
        },
      };
    }
    errors.push("direct result has no observable sink");
    if (likelyOpaque(module, member)) errors.push("opaque result needs a follow-up operation before observation");
    return { errors, sink: null };
  }
  if (binding.name.startsWith("_")) errors.push(`result is bound to discard name ${binding.name}`);
  const sourceContext = valueContext(code, callStart, callClose + 1, binding.rhsStart, binding.end, calls, observers, false);
  const sink = traceBinding(
    code,
    binding.name,
    binding.end,
    likelyOpaque(module, member),
    declarations,
    observers,
    calls,
    binding.name,
    sourceContext.aware,
    sourceContext.follow_up,
  );
  if (!sink) {
    errors.push(`bound result ${binding.name} is never consumed by print/eprint/assert`);
    return { errors, sink: null };
  }
  return { errors, sink };
}

function parseExclusions(root) {
  const path = join(root, PATHS.exclusions);
  if (!existsSync(path)) return new Map();
  const out = new Map();
  for (const [index, raw] of readFileSync(path, "utf8").split(/\r?\n/).entries()) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const fields = raw.split("\t");
    if (fields.length < 2 || !fields[0].trim() || !fields[1].trim()) fail(`malformed exclusion at line ${index + 1}`);
    const key = fields[0].trim();
    if (out.has(key)) fail(`duplicate exclusion: ${key}`);
    out.set(key, { reason: fields[1].trim(), owner: fields[2]?.trim() || null, decision: fields[3]?.trim() || null });
  }
  return out;
}

function normalizeId(kind, value) {
  if (typeof value === "string") {
    if (value.startsWith(`${KIND_PREFIX[kind]}`)) return value;
    return `${KIND_PREFIX[kind]}${value}`;
  }
  if (value && typeof value.stable_id === "string") return normalizeId(kind, value.stable_id);
  if (kind === "receiver_method" && value?.type && value?.member) return `receiver:${value.type}.${value.member}`;
  if (kind === "field" && value?.type && value?.field) return `field:${value.type}.${value.field}`;
  fail(`cannot derive ${kind} stable id`);
}

function untaggedId(id) {
  return id.replace(/^(module|receiver|field|type):/, "");
}

function asSet(values, kind) {
  return new Set((values || []).map((value) => normalizeId(kind, value)));
}

function routeIdentity(id) {
  return id.startsWith("module:") ? id.slice("module:".length) : id;
}

function routeSourceEvidence(root, paths, pattern, seam, stableId) {
  for (const path of paths) {
    const absolute = join(root, path);
    if (!existsSync(absolute)) continue;
    const source = readFileSync(absolute, "utf8");
    const match = codeOnly(source).match(pattern);
    if (match) return [rowEvidence(path, source, match.index, seam, stableId)];
  }
  return [];
}

function routeFactsFromSources(
  root,
  moduleIds,
  receiverRows,
  fieldRows,
  typeRows,
  plainRows,
  registry = null,
) {
  const expectedModules = new Set(moduleIds.map(routeIdentity));
  const actual = Object.fromEntries(TIERS.map((tier) => [tier, []]));
  const add = (tier, stableId, route, evidence, seam) => {
    const proof = uniqueSorted(evidence.filter(Boolean));
    if (proof.length === 0) return;
    const rows = actual[tier];
    const existing = rows.find((row) => row.stable_id === stableId && row.route === route);
    if (existing) {
      existing.evidence = uniqueSorted([...existing.evidence, ...proof]);
      return;
    }
    rows.push({ stable_id: stableId, route, seam, evidence: proof });
  };

  const callsSource = registry?.source || (existsSync(join(root, PATHS.calls)) ? readFileSync(join(root, PATHS.calls), "utf8") : "");
  const callRegistry = registry?.rows || parseCoreCallRegistry(callsSource);
  const moduleRegistry = callRegistry.modules || new Map();
  const receiverRegistry = callRegistry.receivers || new Map();

  const genericLookups = {
    aot: routeSourceEvidence(root, ROUTE_FILES.aot, /(?:crate::)?Syntax::core_call\s*\(\s*module\s*,\s*method\s*\)/, "aot-core-call-dispatch", "__route__"),
    jet_run: routeSourceEvidence(root, ROUTE_FILES.jet_run, /jet_foundation::Syntax::core_call\s*\(\s*module\s*,\s*method\s*\)/, "jet-run-core-call-dispatch", "__route__"),
    interpreter: routeSourceEvidence(root, ROUTE_FILES.interpreter, /jet_foundation::Syntax::core_call\s*\(\s*module\s*,\s*method\s*\)/, "interpreter-core-call-dispatch", "__route__"),
  };
  const routeSupports = {
    aot: (entry) => entry?.aot_direct !== false,
    jet_run: (entry) => entry?.jit_direct !== false,
    interpreter: (entry) => entry?.interpreter_explicit === true,
  };

  for (const row of moduleRegistry.values()) {
    if (!expectedModules.has(row.stable_id.slice("module:".length))) continue;
    for (const tier of TIERS) {
      if (!routeSupports[tier](row)) continue;
      const lookup = genericLookups[tier];
      if (lookup.length === 0) continue;
      add(
        tier,
        row.stable_id,
        `${tier}:canonical-core-call-lookup`,
        [...row.evidence, ...lookup.map((item) => item.replace("__route__", row.stable_id))],
        "core-call-registry",
      );
    }
  }

  // Hand-written literal arms remain valid, but only when the exact pair is
  // present in the registry. The row-specific registry proof prevents an
  // unrelated pair in a route file from becoming blanket coverage.
  for (const tier of TIERS) {
    for (const path of ROUTE_FILES[tier]) {
      const absolute = join(root, path);
      if (!existsSync(absolute)) continue;
      const source = readFileSync(absolute, "utf8");
      for (const pair of parsePairLiterals(source)) {
        const stableId = `module:${pair.first}.${pair.second}`;
        const entry = moduleRegistry.get(stableId);
        if (!expectedModules.has(`${pair.first}.${pair.second}`) || !entry) continue;
        add(
          tier,
          stableId,
          `${tier}:literal-dispatch`,
          [
            ...entry.evidence,
            rowEvidence(path, source, pair.start, "literal-dispatch", stableId),
          ],
          "core-call-registry",
        );
      }
    }
  }

  const receiverLookup = routeSourceEvidence(
    root,
    ROUTE_FILES.interpreter,
    /core_receiver_method\s*\(/,
    "interpreter-receiver-dispatch",
    "__route__",
  );
  if (receiverLookup.length > 0) {
    for (const receiver of receiverRows) {
      const entry = receiverRegistry.get(receiver.stable_id);
      if (!entry) continue;
      add(
        "interpreter",
        receiver.stable_id,
        "interpreter:canonical-receiver-lookup",
        [...(receiver.evidence || entry.evidence || []), ...receiverLookup.map((item) => item.replace("__route__", receiver.stable_id))],
        "core-receiver-registry",
      );
    }
  }

  const shapePatterns = {
    aot: /core_struct_field_rust_name\s*\(/,
    jet_run: /core_struct_field_(?:type|names|index|layout)\s*\(/,
    interpreter: /TExprKind::Field|struct_field_types/,
  };
  for (const tier of TIERS) {
    const shapeEvidence = routeSourceEvidence(root, ROUTE_FILES[tier], shapePatterns[tier], `${tier}-core-shape-dispatch`, "__route__");
    if (shapeEvidence.length === 0) continue;
    for (const row of fieldRows) {
      add(
        tier,
        row.stable_id,
        `${tier}:canonical-field-shape`,
        [...(row.evidence || []), ...shapeEvidence.map((item) => item.replace("__route__", row.stable_id))],
        "core-field-registry",
      );
    }
    for (const row of typeRows) {
      add(
        tier,
        row.stable_id,
        `${tier}:canonical-type-shape`,
        [...(row.evidence || []), ...shapeEvidence.map((item) => item.replace("__route__", row.stable_id))],
        "core-type-registry",
      );
    }
  }
  for (const rows of Object.values(actual)) {
    rows.sort((left, right) => compareStable(left.stable_id, right.stable_id) || compareStable(left.route, right.route));
  }
  return actual;
}

function sourceFiles(root) {
  const files = new Set(Object.values(PATHS).map((path) => path));
  for (const paths of Object.values(ROUTE_FILES)) for (const path of paths) files.add(path);
  for (const paths of Object.values(CAPABILITY_PATHS)) for (const path of paths) files.add(path);
  const corpus = join(root, "tests/conformance/corpus");
  for (const path of walk(corpus)) files.add(relative(root, path).replaceAll("\\", "/"));
  return [...files].sort(compareStable);
}

export function sourceSnapshotFromContents(entries) {
  const files = Object.entries(entries).map(([path, contents]) => ({
    path,
    bytes: sourceBytes(contents).length,
    sha256: sha256(contents),
  })).sort((left, right) => compareStable(left.path, right.path));
  return { algorithm: "sha256", files, hash: sha256(canonicalJson(files)) };
}

export function sourceSnapshot(root = DEFAULT_ROOT, files = sourceFiles(root)) {
  const entries = {};
  for (const path of files) {
    const absolute = join(root, path);
    if (!existsSync(absolute)) fail(`manifest source is unreadable: ${path}`);
    entries[path] = readFileSync(absolute);
  }
  return sourceSnapshotFromContents(entries);
}

function defaultSurface(root) {
  const membership = sourceMembership(root);
  const {
    moduleItemsSource,
    surfaceSource,
    moduleCalls,
    receiverRows,
    fieldRows,
    typeRows,
    registry,
    sourceIds,
  } = membership;
  const plainRows = parsePlainCallRows(registry.source);
  const routes = routeFactsFromSources(root, moduleCalls, receiverRows, fieldRows, typeRows, plainRows, registry);
  const seeds = new Map();
  const corpusRoot = join(root, "tests/conformance/corpus");
  for (const path of walk(corpusRoot)) {
    const key = seedKey(corpusRoot, path);
    const stableId = `module:${key}`;
    if (!moduleCalls.includes(stableId)) continue;
    if (seeds.has(stableId)) fail(`duplicate conformance seed: ${stableId}`);
    const source = readFileSync(path, "utf8");
    seeds.set(stableId, { path: relative(root, path).replaceAll("\\", "/"), source, ...seedInspection(key, source) });
  }
  const exclusions = parseExclusions(root);
  return {
    moduleCalls,
    receivers: receiverRows,
    fields: fieldRows,
    types: sourceIds.nominal_type.map((stable_id) => ({ stable_id })),
    routes,
    seeds,
    exclusions,
    snapshot: sourceSnapshot(root),
    membershipSources: {
      module_call: [PATHS.moduleItems, PATHS.conformance],
      receiver_method: [PATHS.calls],
      field: [PATHS.fields],
      nominal_type: [PATHS.moduleTypes],
    },
    membershipEvidence: {
      module_call: Object.fromEntries(moduleCalls.map((stable_id) => [
        stable_id,
        moduleItemEvidence(moduleItemsSource, untaggedId(stable_id).slice(0, untaggedId(stable_id).lastIndexOf(".")), untaggedId(stable_id).slice(untaggedId(stable_id).lastIndexOf(".") + 1), stable_id),
      ])),
      receiver_method: Object.fromEntries(receiverRows.map((row) => [row.stable_id, row.evidence || []])),
      field: Object.fromEntries(fieldRows.map((row) => [row.stable_id, row.evidence || []])),
      nominal_type: Object.fromEntries(typeRows.map((row) => [row.stable_id, row.evidence || []])),
    },
  };
}

function normalizeRoutes(routes = {}) {
  const out = Object.fromEntries(TIERS.map((tier) => [tier, []]));
  for (const tier of TIERS) {
    for (const value of routes[tier] || []) {
      if (typeof value === "string") {
        out[tier].push({ stable_id: value.includes(":") ? value : `module:${value}`, route: `${tier}:fixture`, seam: null, evidence: [] });
      } else {
        out[tier].push({
          stable_id: value.stable_id,
          route: value.route || `${tier}:fixture`,
          seam: value.seam || null,
          evidence: uniqueSorted([...(value.evidence || [])]),
        });
      }
    }
    out[tier].sort((left, right) => compareStable(left.stable_id, right.stable_id) || compareStable(left.route, right.route));
  }
  return out;
}

function normalizedExclusions(exclusions = new Map()) {
  const out = new Map();
  const entries = exclusions instanceof Map ? exclusions.entries() : Object.entries(exclusions);
  for (const [key, value] of entries) {
    const stable = typeof key === "string" && key.includes(":") ? key : `module:${key}`;
    if (out.has(stable)) fail(`duplicate exclusion: ${stable}`);
    out.set(stable, typeof value === "string" ? { reason: value, owner: null, decision: null } : { ...value });
  }
  return out;
}

function rowIdentity(kind, value) {
  const stable_id = normalizeId(kind, value);
  const untagged = untaggedId(stable_id);
  const dot = untagged.lastIndexOf(".");
  const owner = dot < 0 ? untagged : untagged.slice(0, dot);
  const member = dot < 0 ? untagged : untagged.slice(dot + 1);
  return { stable_id, owner, member };
}

function rowDomain(row) {
  const value = row.owner || row.member;
  const bits = value.split(".");
  return bits.length > 1 ? bits[1] : "core";
}

function buildRow(kind, value, surface, routes, exclusions) {
  const identity = rowIdentity(kind, value);
  const routeRows = [];
  for (const tier of TIERS) {
    for (const route of routes[tier]) {
      if (route.stable_id !== identity.stable_id) continue;
      routeRows.push({ tier, route: route.route, seam: route.seam, evidence: route.evidence });
    }
  }
  const applicable = [...new Set(routeRows.map((route) => route.tier))].sort((left, right) => TIERS.indexOf(left) - TIERS.indexOf(right));
  const exclusion = exclusions.get(identity.stable_id) || exclusions.get(untaggedId(identity.stable_id)) || null;
  const seed = surface.seeds?.get(identity.stable_id) || surface.seeds?.[identity.stable_id] || null;
  const executable = Boolean(seed);
  const invalid = seed && seed.errors?.length > 0;
  let status = exclusion ? "excluded" : invalid ? "invalid" : executable ? "covered" : applicable.length ? "missing" : "unrouted";
  if (exclusion && (!exclusion.reason || !exclusion.owner || !exclusion.decision)) status = "invalid-exclusion";
  const row = {
    stable_id: identity.stable_id,
    kind,
    owner: identity.owner,
    member: identity.member,
    domain: rowDomain(identity),
    applicable_tiers: applicable,
    projections: routeRows.sort((left, right) => TIERS.indexOf(left.tier) - TIERS.indexOf(right.tier) || compareStable(left.route, right.route)),
    dispatcher_arms: routeRows.map((route) => route.route),
    membership_sources: [...(surface.membershipSources?.[kind] || [])].sort(compareStable),
    membership_evidence: uniqueSorted(surface.membershipEvidence?.[kind]?.[identity.stable_id] || value.evidence || []),
    seed: seed?.path || null,
    value_consuming: exclusion || !executable ? null : !invalid,
    sink: exclusion || !executable ? null : seed.sink,
    status,
    exclusion: exclusion ? { ...exclusion } : null,
  };
  if (invalid) row.errors = [...seed.errors];
  return row;
}

function capabilitySlug(value) {
  const text = String(value);
  const readable = text.normalize("NFKD").replace(/[^A-Za-z0-9]+/g, "-").replace(/^-+|-+$/g, "").toLowerCase();
  return readable || `x-${Buffer.from(text).toString("hex")}`;
}

function capabilitySource(root, path, role, sourceOfTruth = null) {
  const absolute = join(root, path);
  if (!existsSync(absolute)) return null;
  const text = readFileSync(absolute, "utf8");
  return {
    path,
    text,
    digest: sha256(text),
    role,
    source_of_truth: sourceOfTruth,
  };
}

function capabilitySourceIdentity(source, offset = 0, authority = null, revisionSource = null) {
  const revision = revisionSource || source;
  return {
    path: source.path,
    line: lineNumber(source.text, offset),
    digest: revision.digest,
    authority: authority || source.source_of_truth || source.path,
    projection: revision === source ? null : { path: source.path, digest: source.digest },
  };
}

function capabilityDelimited(text, start, opening = "{", closing = "}") {
  const open = text.indexOf(opening, start);
  if (open < 0) return null;
  try {
    const close = matching(text, open, opening, closing);
    return { body: text.slice(open + 1, close), bodyStart: open + 1, end: close + 1 };
  } catch {
    return null;
  }
}

function capabilityArray(text, start) {
  const equals = text.indexOf("=", start);
  return equals < 0 ? null : capabilityDelimited(text, equals, "[", "]");
}

function capabilityEnumVariants(source, enumName) {
  const start = source.text.indexOf(`enum ${enumName}`);
  const block = start < 0 ? null : capabilityDelimited(source.text, start);
  if (!block) return [];
  return [...block.body.matchAll(/^\s*([A-Za-z_][A-Za-z0-9_]*)\s*(?=,|\(|\{)/gm)]
    .map((match) => ({ name: match[1], offset: block.bodyStart + match.index }));
}

function capabilityStructFields(source, structName) {
  const start = source.text.indexOf(`struct ${structName}`);
  const block = start < 0 ? null : capabilityDelimited(source.text, start);
  if (!block) return [];
  return [...block.body.matchAll(/^\s*pub\s+([A-Za-z_][A-Za-z0-9_]*)\s*:/gm)]
    .map((match) => ({ name: match[1], offset: block.bodyStart + match.index }));
}

function capabilityQuoted(body) {
  return [...body.matchAll(/"([^"\\]*(?:\\.[^"\\]*)*)"/g)]
    .map((match) => decodeRustString(match[1]));
}

function capabilityAdd(identities, {
  id,
  family,
  kind,
  label,
  detail,
  sourceIdentity,
  module = null,
  legacyStableId = null,
}) {
  if (!id || !CAPABILITY_FAMILIES.includes(family) || !sourceIdentity) return;
  const existing = identities.get(id);
  if (existing) {
    if (legacyStableId && !existing.legacy_stable_ids.includes(legacyStableId)) {
      existing.legacy_stable_ids.push(legacyStableId);
      existing.legacy_stable_ids.sort(compareStable);
    }
    return;
  }
  identities.set(id, {
    capability_id: id,
    id,
    family,
    kind,
    label: String(label || id),
    detail: detail ?? `${family} ${kind} capability`,
    module,
    applicable_modes: [...CAPABILITY_MODES_BY_FAMILY[family]],
    source_identity: sourceIdentity,
    legacy_stable_ids: legacyStableId ? [legacyStableId] : [],
  });
}

function capabilityLegacyId(stableId) {
  if (typeof stableId !== "string") return null;
  const match = stableId.match(/^(module|receiver|field|type):(.*)$/);
  if (!match) return null;
  const bits = match[2].split(".");
  const member = bits.pop() || match[2];
  const owner = bits.join(".");
  const kind = match[1] === "module"
    ? "call"
    : match[1] === "receiver"
      ? "receiver"
      : match[1] === "field"
        ? "field"
        : "type";
  return `core.${kind}.${capabilitySlug(owner)}${owner ? "." : ""}${capabilitySlug(member)}`;
}

function capabilityInventory(root, legacyRows = []) {
  const identities = new Map();
  const sources = new Map();
  const read = (family, path, role, sourceOfTruth = null) => {
    const source = capabilitySource(root, path, role, sourceOfTruth);
    if (source) sources.set(path, source);
    return source;
  };
  const add = (family, path, role, value) => {
    const source = sources.get(path) || read(family, path, role);
    if (!source) return;
    capabilityAdd(identities, { ...value, sourceIdentity: capabilitySourceIdentity(source, value.offset || 0, value.authority, value.revisionSource) });
  };

  const syntax = read("language", CAPABILITY_PATHS.language[0], "language lexical registry");
  if (syntax) {
    const ledger = syntax.text.indexOf("pub const LEXICAL_LEDGER");
    const block = ledger < 0 ? null : capabilityArray(syntax.text, ledger);
    for (const match of (block?.body || "").matchAll(/LexicalEntry\s*\{\s*spelling:\s*"([^"\\]*(?:\\.[^"\\]*)*)"\s*,\s*meaning:\s*"([^"\\]*(?:\\.[^"\\]*)*)"\s*,\s*decision:\s*"([^"\\]*(?:\\.[^"\\]*)*)"/g)) {
      const spelling = decodeRustString(match[1]);
      add("language", syntax.path, "language lexical registry", {
        id: `language.lexical.${capabilitySlug(spelling)}`,
        kind: "lexical",
        label: spelling,
        detail: decodeRustString(match[2]),
        offset: (block?.bodyStart || 0) + match.index,
      });
    }
  }
  const tir = read("language", CAPABILITY_PATHS.language[1], "language TIR registry");
  if (tir) for (const [enumName, kind] of [["TExprKind", "tir-expr"], ["TStmt", "tir-stmt"]]) {
    for (const variant of capabilityEnumVariants(tir, enumName)) {
      add("language", tir.path, "language TIR registry", {
        id: `language.${kind}.${capabilitySlug(variant.name)}`,
        kind,
        label: variant.name,
        detail: `${enumName} capability variant`,
        offset: variant.offset,
      });
    }
  }

  const exportsSource = read("core", CAPABILITY_PATHS.core[0], "generated Core module registry", CAPABILITY_PATHS.core[1]);
  const coreSource = read("core", CAPABILITY_PATHS.core[1], "Core source");
  const callsSource = read("core", CAPABILITY_PATHS.core[2], "Core call registry");
  if (exportsSource) {
    const names = capabilityArray(exportsSource.text, exportsSource.text.indexOf("pub const CORE_MODULE_NAMES"));
    for (const match of (names?.body || "").matchAll(/"([^"\\]*(?:\\.[^"\\]*)*)"/g)) {
      const name = decodeRustString(match[1]);
      add("core", exportsSource.path, "generated Core module registry", {
        id: `core.module.${capabilitySlug(name)}`,
        kind: "module",
        label: name,
        detail: "public Core module",
        module: name,
        offset: (names?.bodyStart || 0) + match.index,
        authority: coreSource?.path || null,
        revisionSource: coreSource || null,
      });
    }
    const declarations = new Map();
    for (const match of exportsSource.text.matchAll(/CoreModuleDeclaration\s*\{\s*module:\s*"([^"\\]*(?:\\.[^"\\]*)*)"\s*,\s*members:\s*CORE_MODULE_(\d+)_MEMBERS/g)) {
      declarations.set(match[2], { module: decodeRustString(match[1]), offset: match.index });
    }
    for (const match of exportsSource.text.matchAll(/const CORE_MODULE_(\d+)_MEMBERS\s*:\s*&\[&str\]\s*=\s*&\[/g)) {
      const declaration = declarations.get(match[1]);
      const block = capabilityArray(exportsSource.text, match.index);
      if (!declaration || !block) continue;
      for (const valueMatch of block.body.matchAll(/"([^"\\]*(?:\\.[^"\\]*)*)"/g)) {
        const value = decodeRustString(valueMatch[1]);
        add("core", exportsSource.path, "generated Core member registry", {
          id: `core.member.${capabilitySlug(declaration.module)}.${capabilitySlug(value)}`,
          kind: "member",
          label: value,
          detail: `public member of ${declaration.module}`,
          module: declaration.module,
          offset: block.bodyStart + valueMatch.index,
          authority: coreSource?.path || null,
          revisionSource: coreSource || null,
        });
      }
    }
    for (const match of exportsSource.text.matchAll(/const CORE_MODULE_(\d+)_TYPES\s*:\s*&\[\(&str,\s*CoreLeafKind\)\]\s*=\s*&\[/g)) {
      const declaration = declarations.get(match[1]);
      const block = capabilityArray(exportsSource.text, match.index);
      if (!declaration || !block) continue;
      for (const valueMatch of block.body.matchAll(/\(\s*"([^"\\]*(?:\\.[^"\\]*)*)"\s*,\s*CoreLeafKind::/g)) {
        const value = decodeRustString(valueMatch[1]);
        add("core", exportsSource.path, "generated Core type registry", {
          id: `core.type.${capabilitySlug(declaration.module)}.${capabilitySlug(value)}`,
          kind: "type",
          label: value,
          detail: `public type export of ${declaration.module}`,
          module: declaration.module,
          offset: block.bodyStart + valueMatch.index,
          authority: coreSource?.path || null,
          revisionSource: coreSource || null,
        });
      }
    }
  }
  if (callsSource) {
    for (const match of callsSource.text.matchAll(/CoreCallRecord::new\(\s*"([^"\\]*(?:\\.[^"\\]*)*)"\s*,\s*"([^"\\]*(?:\\.[^"\\]*)*)"/g)) {
      const module = decodeRustString(match[1]);
      const member = decodeRustString(match[2]);
      add("core", callsSource.path, "Core call registry", {
        id: `core.call.${capabilitySlug(module)}.${capabilitySlug(member)}`,
        kind: "call",
        label: member,
        detail: `call projection for ${module}.${member}`,
        module,
        offset: match.index,
      });
    }
    for (const match of callsSource.text.matchAll(/CoreCallRecord::receiver\(\s*&\[[\s\S]*?\]\s*,\s*"([^"\\]*(?:\\.[^"\\]*)*)"/g)) {
      const member = decodeRustString(match[1]);
      add("core", callsSource.path, "Core receiver registry", {
        id: `core.receiver.${capabilitySlug(member)}`,
        kind: "receiver",
        label: member,
        detail: "receiver-method projection",
        offset: match.index,
      });
    }
  }

  const cli = read("cli", CAPABILITY_PATHS.cli[0], "CLI command and flag registry");
  if (cli) {
    const commands = capabilityArray(cli.text, cli.text.indexOf("pub const COMMANDS"));
    for (const match of (commands?.body || "").matchAll(/CommandSpec\s*\{\s*name:\s*"([^"\\]*(?:\\.[^"\\]*)*)"/g)) {
      const name = decodeRustString(match[1]);
      add("cli", cli.path, "CLI command registry", {
        id: `cli.command.${capabilitySlug(name)}`, kind: "command", label: name, detail: "top-level Jet command", offset: (commands?.bodyStart || 0) + match.index,
      });
    }
    for (const match of cli.text.matchAll(/const\s+([A-Z][A-Z0-9_]*)_ACTIONS\s*:\s*&\[NestedCommandSpec\]\s*=\s*&\[/g)) {
      const group = match[1].toLowerCase().replace(/_/g, "-");
      const block = capabilityArray(cli.text, match.index);
      for (const action of (block?.body || "").matchAll(/NestedCommandSpec\s*\{\s*name:\s*"([^"\\]*(?:\\.[^"\\]*)*)"/g)) {
        const name = decodeRustString(action[1]);
        add("cli", cli.path, "CLI nested command registry", {
          id: `cli.action.${capabilitySlug(group)}.${capabilitySlug(name)}`, kind: "action", label: name, detail: `nested ${group} action`, offset: (block?.bodyStart || 0) + action.index,
        });
      }
    }
    const inspect = cli.text.indexOf("pub enum InspectPlane");
    for (const variant of (inspect < 0 ? [] : capabilityEnumVariants(cli, "InspectPlane"))) {
      add("cli", cli.path, "CLI inspect registry", {
        id: `cli.inspect.${capabilitySlug(variant.name)}`, kind: "inspect-plane", label: variant.name, detail: `inspect plane ${variant.name}`, offset: variant.offset,
      });
    }
    const flags = capabilityArray(cli.text, cli.text.indexOf("const BASE_FLAGS"));
    const flagConstants = Object.fromEntries([...cli.text.matchAll(/(?:pub\s+)?const\s+([A-Z][A-Z0-9_]*)\s*:\s*&str\s*=\s*"([^"\\]*(?:\\.[^"\\]*)*)"/g)]
      .map((match) => [match[1], decodeRustString(match[2])]));
    for (const match of (flags?.body || "").matchAll(/FlagSpec\s*\{\s*long:\s*([^,}\n]+)/g)) {
      const raw = match[1].trim();
      const value = flagConstants[raw] || raw.replace(/^"|"$/g, "");
      add("cli", cli.path, "CLI flag registry", {
        id: `cli.flag.${capabilitySlug(value)}`, kind: "flag", label: value, detail: "canonical CLI flag", offset: (flags?.bodyStart || 0) + match.index,
      });
    }
  }

  const editor = read("editor", CAPABILITY_PATHS.editor[0], "editor host protocol registry");
  if (editor) {
    for (const match of editor.text.matchAll(/pub const (EDITOR_[A-Z0-9_]+_METHOD):\s*&str\s*=\s*"([^"\\]*(?:\\.[^"\\]*)*)"/g)) {
      const value = decodeRustString(match[2]);
      add("editor", editor.path, "editor method registry", {
        id: `editor.method.${capabilitySlug(value)}`, kind: "method", label: value, detail: match[1], offset: match.index,
      });
    }
    for (const enumName of ["EditorHost", "EditorCommand"]) for (const variant of capabilityEnumVariants(editor, enumName)) {
      add("editor", editor.path, "editor command registry", {
        id: `editor.${enumName === "EditorHost" ? "host" : "command"}.${capabilitySlug(variant.name)}`,
        kind: enumName === "EditorHost" ? "host" : "command",
        label: variant.name,
        detail: `${enumName} variant`,
        offset: variant.offset,
      });
    }
  }

  const debuggerSyntax = read("debugger", CAPABILITY_PATHS.debugger[0], "debugger command registry");
  const debuggerSource = read("debugger", CAPABILITY_PATHS.debugger[1], "debugger backend registry");
  if (debuggerSyntax) for (const match of debuggerSyntax.text.matchAll(/pub const (DBG_[A-Z0-9_]+):\s*&str\s*=\s*"([^"\\]*(?:\\.[^"\\]*)*)"/g)) {
    const value = decodeRustString(match[2]);
    add("debugger", debuggerSyntax.path, "debugger command registry", {
      id: value === "(jet)" ? "debugger.prompt.jet" : `debugger.command.${capabilitySlug(value)}`,
      kind: value === "(jet)" ? "prompt" : "command", label: value, detail: match[1], offset: match.index,
    });
  }
  if (debuggerSource) for (const variant of capabilityEnumVariants(debuggerSource, "DebugTier")) {
    const wire = debuggerSource.text.slice(variant.offset).match(new RegExp(`"([^"\\n]+)"\\s*=>\\s*Ok\\(Self::${variant.name}\\)`))?.[1] || variant.name;
    add("debugger", debuggerSource.path, "debugger backend registry", {
      id: `debugger.tier.${capabilitySlug(wire)}`, kind: "tier", label: wire, detail: `${variant.name} debugger backend`, offset: variant.offset,
    });
  }

  const effects = read("build", CAPABILITY_PATHS.build[0], "build-effect registry", CAPABILITY_PATHS.build[1]);
  const effectsSource = read("build", CAPABILITY_PATHS.build[1], "build-effect source");
  if (effects) for (const variant of capabilityEnumVariants(effects, "BuildEffect")) add("build", effects.path, "build-effect registry", {
    id: `build.effect.${capabilitySlug(variant.name)}`,
    kind: "effect",
    label: variant.name,
    detail: "declared build capability",
    offset: variant.offset,
    authority: effectsSource?.path || CAPABILITY_PATHS.build[1],
    revisionSource: effectsSource || null,
  });
  const blocks = read("build", CAPABILITY_PATHS.build[2], "package build-profile registry");
  if (blocks) for (const enumName of ["BuildOptimize", "BuildPanic"]) for (const variant of capabilityEnumVariants(blocks, enumName)) add("build", blocks.path, "package build-profile registry", {
    id: `build.${capabilitySlug(enumName)}.${capabilitySlug(variant.name)}`, kind: "profile-option", label: variant.name, detail: `${enumName} option`, offset: variant.offset,
  });

  const packageSource = read("package", CAPABILITY_PATHS.package[0], "package facts registry");
  const packageBlocks = read("package", CAPABILITY_PATHS.package[1], "package target registry");
  const bundler = read("package", CAPABILITY_PATHS.package[2], "package bundle registry");
  if (packageSource) {
    for (const variant of capabilityEnumVariants(packageSource, "PackageOutputKind")) add("package", packageSource.path, "package facts registry", {
      id: `package.output-kind.${capabilitySlug(variant.name)}`, kind: "output-kind", label: variant.name, detail: "declared package output kind", offset: variant.offset,
    });
    for (const field of capabilityStructFields(packageSource, "PackageFacts")) add("package", packageSource.path, "package facts registry", {
      id: `package.fact.${capabilitySlug(field.name)}`, kind: "fact", label: field.name, detail: "PackageFacts field", offset: field.offset,
    });
  }
  if (packageBlocks) for (const enumName of ["Target", "TargetProfileIdentity"]) for (const variant of capabilityEnumVariants(packageBlocks, enumName)) add("package", packageBlocks.path, "package target registry", {
    id: `package.${capabilitySlug(enumName)}.${capabilitySlug(variant.name)}`, kind: "target-kind", label: variant.name, detail: `declared ${enumName} kind`, offset: variant.offset,
  });
  if (bundler) for (const variant of capabilityEnumVariants(bundler, "BundleTarget")) add("package", bundler.path, "package bundle registry", {
    id: `package.bundle-target.${capabilitySlug(variant.name)}`, kind: "bundle-target", label: variant.name, detail: "declared package bundle target", offset: variant.offset,
  });

  const ffi = read("ffi", CAPABILITY_PATHS.ffi[0], "foreign binder descriptor registry");
  if (ffi) for (const match of ffi.text.matchAll(/binder\(\s*ForeignLanguage::([A-Za-z0-9_]+)\s*,\s*BinderRuntime::([A-Za-z0-9_]+)\s*,\s*BindingStubKind::([A-Za-z0-9_]+)\s*,\s*BinderStatus::([A-Za-z0-9_]+)\s*,\s*ForeignAbiContract::([A-Za-z0-9_]+)[\s\S]*?ForeignProvider::([A-Za-z0-9_]+)/g)) {
    const language = match[1];
    add("ffi", ffi.path, "foreign binder descriptor registry", {
      id: `ffi.binder.${capabilitySlug(language)}`, kind: "binder", label: language, detail: {
        runtime: match[2], stub_kind: match[3], status: match[4], contract: match[5], provider: match[6],
      }, offset: match.index,
    });
  }
  // The legacy Core membership extractor is the authoritative backstop for
  // public fields, receiver methods, nominal types, and calls not repeated in
  // generated exports. It contributes identities to this same relation.
  const membership = sourceMembership(root);
  const membershipSources = [
    ["module_call", membership.moduleCalls, CAPABILITY_PATHS.core[3], "Core semantic item projection"],
    ["receiver_method", membership.receiverRows, CAPABILITY_PATHS.core[2], "Core receiver registry"],
    ["field", membership.fieldRows, CAPABILITY_PATHS.core[4], "Core field registry"],
    ["nominal_type", membership.typeRows, CAPABILITY_PATHS.core[3], "Core type registry"],
  ];
  for (const [kind, values, path, role] of membershipSources) {
    const source = sources.get(path) || read("core", path, role);
    if (!source) continue;
    for (const value of values || []) {
      const stableId = normalizeId(kind, value);
      const capabilityId = capabilityLegacyId(stableId);
      if (!capabilityId) continue;
      capabilityAdd(identities, {
        id: capabilityId,
        family: "core",
        kind,
        label: value.member || value.field || value.name || stableId,
        detail: `authoritative Core membership ${stableId}`,
        sourceIdentity: capabilitySourceIdentity(source),
        legacyStableId: stableId,
      });
    }
  }

  for (const legacy of legacyRows) {
    const capabilityId = capabilityLegacyId(legacy?.stable_id);
    if (!capabilityId) continue;
    const sourcePath = legacy.membership_sources?.[0] || CAPABILITY_PATHS.core[0];
    const source = sources.get(sourcePath) || read("core", sourcePath, "legacy hardening membership");
    if (!source) continue;
    const dot = capabilityId.lastIndexOf(".");
    capabilityAdd(identities, {
      id: capabilityId,
      family: "core",
      kind: legacy.kind,
      label: legacy.member || capabilityId.slice(dot + 1),
      detail: `legacy hardening identity ${legacy.stable_id}`,
      sourceIdentity: capabilitySourceIdentity(source, 0, source.path),
      legacyStableId: legacy.stable_id,
    });
  }
  return [...identities.values()]
    .map((identity) => ({ ...identity, legacy_stable_ids: [...identity.legacy_stable_ids].sort(compareStable) }))
    .sort((left, right) => compareStable(left.capability_id, right.capability_id));
}
function capabilityFixtureIdentities(surface) {
  const sourcePath = surface.membershipSources?.module_call?.[0] || "fixture.rs";
  const snapshotFile = surface.snapshot?.files?.find((file) => file.path === sourcePath);
  const source = {
    path: sourcePath,
    text: "fixture",
    digest: snapshotFile?.sha256 || sha256("fixture"),
    source_of_truth: sourcePath,
  };
  const identities = [];
  for (const [kind, values] of [
    ["module_call", surface.moduleCalls || []],
    ["receiver_method", surface.receivers || []],
    ["field", surface.fields || []],
    ["nominal_type", surface.types || []],
  ]) for (const value of values) {
    const stableId = normalizeId(kind, value);
    const capabilityId = capabilityLegacyId(stableId);
    if (!capabilityId) continue;
    identities.push({
      capability_id: capabilityId,
      id: capabilityId,
      family: "core",
      kind,
      label: value.member || value.field || value.name || stableId,
      detail: `fixture capability for ${stableId}`,
      module: null,
      applicable_modes: [...TIERS],
      source_identity: capabilitySourceIdentity(source),
      legacy_stable_ids: [stableId],
    });
  }
  const seen = new Map();
  for (const identity of identities) seen.set(identity.capability_id, identity);
  return [...seen.values()].sort((left, right) => compareStable(left.capability_id, right.capability_id));
}

function capabilityLegacyRowsById(legacyRows) {
  const out = new Map();
  for (const row of legacyRows || []) if (row?.stable_id) out.set(capabilityLegacyId(row.stable_id), row);
  return out;
}

function capabilityContract(identity, mode) {
  const meaning = typeof identity.detail === "string" ? identity.detail : `${identity.family} ${identity.kind} ${identity.label}`;
  return {
    meaning,
    intended_behavior: `the ${identity.family} capability ${identity.label} is available through its ${mode} route`,
    expected_observation: "an independent consumer-visible result matching the ratified contract",
    failure_behavior: "report the exact candidate, source, mode, oracle, and owner rather than shrinking the denominator",
    ratification: "owner-controlled source registry; runtime qualification remains outstanding",
  };
}

function capabilityRoute(identity, mode) {
  return {
    kind: "registry",
    mode,
    route: `${mode}:source-registry:${identity.capability_id}`,
    seam: "owner-controlled source registry",
    evidence: [identity.source_identity.path],
    shared_implementation_key: `${identity.family}:${mode}:source-registry`,
    source_identity: identity.source_identity,
  };
}

function capabilityObservable(legacy) {
  if (legacy?.value_consuming === true && legacy.sink) {
    return {
      value_consuming: true,
      sink: cloneJson(legacy.sink),
      status: "authored",
      reason: "legacy source witness records a type-aware value-consuming sink",
    };
  }
  return {
    value_consuming: false,
    sink: null,
    status: "unavailable",
    reason: "declaration, command presence, or bind-and-discard is not an observable result",
  };
}

function cloneJson(value) {
  return value === undefined ? undefined : JSON.parse(JSON.stringify(value));
}

function capabilityDisposition(identity, mode, legacy, route) {
  if (legacy?.status === "excluded") {
    return legacy.exclusion?.reason && legacy.exclusion?.owner && legacy.exclusion?.decision
      ? "owner-ratified-not-applicable"
      : "failed";
  }
  if (legacy?.status === "invalid" || legacy?.status === "invalid-exclusion") return "failed";
  if (identity.family === "ffi" && identity.detail?.status === "Planned") return "planned";
  if (!route) return "planned";
  return "implemented-unqualified";
}

function capabilityCandidateIdentity(identity, manifest, mode, disposition) {
  return {
    status: disposition === "passed" ? "available" : "unavailable",
    id: null,
    source_snapshot_hash: manifest,
    compiler: null,
    tool: null,
    target: null,
    mode,
  };
}

function capabilityRelationDigest(relation) {
  const content = { ...relation };
  delete content.content_digest;
  return sha256(canonicalJson(content));
}

export function buildCapabilityRelation({
  root = DEFAULT_ROOT,
  sourceSnapshotHash = null,
  legacyRows = [],
  surface = null,
} = {}) {
  const relationSnapshotHash = sourceSnapshotHash || surface?.snapshot?.hash || sourceSnapshot(root).hash;
  const identities = surface
    ? capabilityFixtureIdentities(surface)
    : capabilityInventory(root, legacyRows);
  const legacyById = capabilityLegacyRowsById(legacyRows);
  const rows = [];
  for (const identity of identities) for (const mode of identity.applicable_modes) {
    const legacy = identity.legacy_stable_ids.map((id) => legacyById.get(capabilityLegacyId(id)) || legacyById.get(id)).find(Boolean)
      || legacyById.get(identity.legacy_stable_ids[0])
      || null;
    const contract = capabilityContract(identity, mode);
    const route = capabilityRoute(identity, mode, legacy);
    const observable = capabilityObservable(legacy);
    const disposition = capabilityDisposition(identity, mode, legacy, route);
    const exclusion = disposition === "owner-ratified-not-applicable" ? {
      reason: legacy.exclusion.reason,
      owner: legacy.exclusion.owner,
      decision: legacy.exclusion.decision,
    } : null;
    const rowId = `${identity.capability_id}@${mode}`;
    const candidate = capabilityCandidateIdentity(identity, relationSnapshotHash, mode, disposition);
    rows.push({
      row_id: rowId,
      stable_id: rowId,
      capability_id: identity.capability_id,
      family: identity.family,
      kind: identity.kind,
      label: identity.label,
      mode,
      applicable_modes: [...identity.applicable_modes],
      contract,
      route,
      observable,
      oracle: {
        method: "independent-expected-behavior",
        independent_expected_behavior: true,
        shared: Boolean(route?.shared_implementation_key),
        status: "unavailable",
        expected_behavior: contract.expected_observation,
        source: "ratified-contract",
      },
      evidence: {
        status: "unavailable",
        class: "source-only",
        source_identity: identity.source_identity,
        execution: null,
        compiler: null,
        tool: null,
        target: null,
        candidate,
        candidate_identity: candidate,
      },
      identity: {
        source: identity.source_identity,
        compiler: null,
        tool: null,
        target: null,
        candidate,
      },
      source_identity: identity.source_identity,
      compiler_identity: null,
      tool_identity: null,
      target_identity: null,
      candidate_identity: candidate,
      owner: {
        card: "#2942",
        link: "#2942",
        capability_id: identity.capability_id,
        mode,
      },
      owner_link: "#2942",
      disposition,
      status: disposition,
      counted: true,
      exclusion,
      legacy_stable_ids: [...identity.legacy_stable_ids],
      shared_implementation_key: route?.shared_implementation_key || null,
    });
  }
  rows.sort((left, right) => compareStable(left.row_id, right.row_id));
  const byFamily = Object.fromEntries(CAPABILITY_FAMILIES.map((family) => [
    family,
    identities.filter((identity) => identity.family === family).length,
  ]));
  const byMode = Object.fromEntries(CAPABILITY_MODE_ORDER.map((mode) => [
    mode,
    rows.filter((row) => row.mode === mode).length,
  ]));
  const dispositions = Object.fromEntries(CAPABILITY_DISPOSITIONS.map((disposition) => [
    disposition,
    rows.filter((row) => row.disposition === disposition).length,
  ]));
  const relation = {
    schema: CAPABILITY_RELATION_SCHEMA,
    schema_version: CAPABILITY_RELATION_VERSION,
    source_snapshot_hash: relationSnapshotHash,
    identities,
    denominator: {
      identity_ids: identities.map((identity) => identity.capability_id),
      identity_count: identities.length,
      row_count: rows.length,
      counted_rows: rows.filter((row) => row.counted !== false).length,
      by_family: byFamily,
      by_mode: byMode,
      dispositions,
    },
    rows,
    exclusions: rows.filter((row) => row.exclusion).map((row) => ({
      row_id: row.row_id,
      capability_id: row.capability_id,
      mode: row.mode,
      ...row.exclusion,
    })),
    consumers: {
      inspect: { relation: "manifest.capability_relation.rows", identity: "row_id", evidence: "row.evidence" },
      hardening: { relation: "manifest.capability_relation.rows", identity: "row_id", evidence: "row.evidence" },
      learner: { relation: "manifest.capability_relation.rows", projection: "jet-learning-census-v1.records" },
      release: { relation: "manifest.capability_relation.rows", candidate: "row.candidate_identity" },
    },
    revision_rule: "source_snapshot_hash and every source_identity digest must match before evidence is current",
    content_digest: null,
  };
  relation.content_digest = capabilityRelationDigest(relation);
  return relation;
}

export function readCapabilityRelation(manifest) {
  const relation = manifest?.capability_relation || manifest;
  const validation = validateCapabilityRelation(relation, {
    sourceSnapshotHash: manifest?.source_snapshot?.hash || null,
    sourceSnapshotFiles: manifest?.source_snapshot?.files || null,
  });
  if (!validation.ok) fail(validation.errors.join("; "));
  return relation;
}

export function validateCapabilityRelation(relation, { sourceSnapshotHash = null, sourceSnapshotFiles = null } = {}) {
  const errors = [];
  if (!isRecord(relation)) return { ok: false, errors: ["capability relation is not an object"] };
  if (relation.schema !== CAPABILITY_RELATION_SCHEMA) errors.push(`capability relation schema must be ${CAPABILITY_RELATION_SCHEMA}`);
  if (relation.schema_version !== CAPABILITY_RELATION_VERSION) errors.push("capability relation schema_version is invalid");
  if (!validDigest(relation.content_digest)) errors.push("capability relation content digest is missing or invalid");
  else if (relation.content_digest !== capabilityRelationDigest(relation)) errors.push("capability relation content digest does not match content");
  if (!Array.isArray(relation.identities)) errors.push("capability relation identities are required");
  const snapshotByPath = new Map(
    Array.isArray(sourceSnapshotFiles)
      ? sourceSnapshotFiles.filter(isRecord).map((file) => [file.path, file.sha256])
      : [],
  );
  if (!Array.isArray(relation.rows)) errors.push("capability relation rows are required");
  const identities = new Map();
  if (!validDigest(relation.source_snapshot_hash)) errors.push("capability relation source snapshot hash is missing or invalid");
  else if (sourceSnapshotHash && relation.source_snapshot_hash !== sourceSnapshotHash) errors.push("capability relation source snapshot is stale");
  for (const identity of Array.isArray(relation.identities) ? relation.identities : []) {
    if (!isRecord(identity) || typeof identity.capability_id !== "string" || !CAPABILITY_FAMILIES.includes(identity.family)) {
      errors.push("capability relation identity is malformed");
      continue;
    }
    if (identities.has(identity.capability_id)) errors.push(`duplicate capability identity: ${identity.capability_id}`);
    identities.set(identity.capability_id, identity);
    if (!Array.isArray(identity.applicable_modes) || identity.applicable_modes.length === 0) errors.push(`capability identity has no applicable modes: ${identity.capability_id}`);
    if (!identity.source_identity?.path || !validDigest(identity.source_identity.digest)) {
      errors.push(`capability identity source is invalid: ${identity.capability_id}`);
    } else if (snapshotByPath.size > 0) {
      const sourceIdentity = identity.source_identity;
      const digests = [
        snapshotByPath.get(sourceIdentity.path),
        snapshotByPath.get(sourceIdentity.authority),
        snapshotByPath.get(sourceIdentity.projection?.path),
      ].filter(Boolean);
      if (!digests.includes(sourceIdentity.digest)) errors.push(`capability identity source is stale: ${identity.capability_id}`);
    }
  }
  const rows = Array.isArray(relation.rows) ? relation.rows : [];
  const rowIds = new Set();
  for (const row of rows) {
    const id = row?.row_id;
    if (!isRecord(row) || typeof id !== "string" || !id) {
      errors.push("capability relation row is malformed");
      continue;
    }
    if (rowIds.has(id)) errors.push(`duplicate capability row: ${id}`);
    rowIds.add(id);
    const identity = identities.get(row.capability_id);
    if (!identity) errors.push(`capability row has no identity: ${id}`);
    if (!identity?.applicable_modes?.includes(row.mode)) errors.push(`capability row has an inapplicable mode: ${id}`);
    if (typeof row.capability_id === "string" && typeof row.mode === "string" && row.row_id !== `${row.capability_id}@${row.mode}`) {
      errors.push(`capability row identity is not canonical: ${id}`);
    }
    if (!CAPABILITY_DISPOSITIONS.includes(row.disposition)) errors.push(`capability row has an invalid disposition: ${id}`);
    for (const field of ["contract", "route", "observable", "oracle", "evidence", "identity", "owner", "source_identity", "candidate_identity"]) {
      if (!isRecord(row[field])) errors.push(`capability row ${field} is missing: ${id}`);
    }
    if (row.counted !== true) errors.push(`capability row is not counted: ${id}`);
    if (row.disposition !== "passed" && typeof row.owner_link !== "string") errors.push(`non-pass capability row has no owner link: ${id}`);
    if (row.disposition === "passed") {
      if (row.evidence?.status !== "passed" || row.oracle?.independent_expected_behavior !== true || row.observable?.value_consuming !== true) {
        errors.push(`passed capability row lacks evidence, independent oracle, or observable value: ${id}`);
      }
      const exactIdentity = row.identity?.compiler && row.identity?.tool && row.identity?.target && row.identity?.candidate;
      if (!isRecord(exactIdentity) || row.identity?.candidate?.status !== "available"
        || !isRecord(row.compiler_identity) || !isRecord(row.tool_identity)
        || !isRecord(row.target_identity) || !isRecord(row.candidate_identity)
        || row.candidate_identity.status !== "available") {
        errors.push(`passed capability row lacks exact candidate identity: ${id}`);
      }
      if (row.oracle?.shared === true && row.oracle?.independent_expected_behavior !== true) errors.push(`shared implementation lacks independent oracle: ${id}`);
    }
    if (row.disposition === "owner-ratified-not-applicable"
      && (!row.exclusion?.reason || !row.exclusion?.owner || !row.exclusion?.decision)) {
      errors.push(`capability exclusion is not owner-ratified: ${id}`);
    }
    if (row.disposition === "passed" && row.observable?.value_consuming === false) errors.push(`discarded capability output marked passed: ${id}`);
    if (row.disposition === "passed" && row.candidate_identity?.status === "stale") errors.push(`stale candidate marked passed: ${id}`);
  }
  for (const identity of identities.values()) {
    for (const mode of identity.applicable_modes || []) {
      const rowId = `${identity.capability_id}@${mode}`;
      if (!rowIds.has(rowId)) errors.push(`capability relation is missing applicable mode row: ${rowId}`);
    }
  }
  const denominator = relation.denominator;
  if (!isRecord(denominator)) errors.push("capability relation denominator is required");
  else {
    if (denominator.row_count !== rows.length || denominator.counted_rows !== rows.filter((row) => row.counted === true).length) {
      errors.push("capability relation denominator row count is stale");
    }
    const denominatorIds = Array.isArray(denominator.identity_ids) ? denominator.identity_ids : [];
    if (denominator.identity_count !== identities.size || denominatorIds.length !== identities.size || new Set(denominatorIds).size !== identities.size
      || denominatorIds.some((id) => !identities.has(id))) errors.push("capability relation denominator identities are stale");
    const dispositionCounts = Object.fromEntries(CAPABILITY_DISPOSITIONS.map((disposition) => [
      disposition,
      rows.filter((row) => row.disposition === disposition).length,
    ]));
    if (canonicalJson(denominator.dispositions || {}) !== canonicalJson(dispositionCounts)) {
      errors.push("capability relation denominator dispositions are stale");
    }
  }
  return { ok: errors.length === 0, errors: [...new Set(errors)].sort(compareStable) };
}

const CAPABILITY_NEGATIVE_CONTROL_NAMES = Object.freeze([
  "new-member",
  "missing-mode",
  "discarded-output",
  "shared-wrong-oracle",
  "stale-identity",
  "invalid-exclusion",
]);
export const CAPABILITY_NEGATIVE_CONTROLS = CAPABILITY_NEGATIVE_CONTROL_NAMES;

function capabilityFixtureRelation() {
  const sourceIdentity = { path: "fixture.rs", line: 1, digest: sha256("fixture"), authority: "fixture.rs", projection: null };
  const identity = {
    capability_id: "core.fixture.member",
    id: "core.fixture.member",
    family: "core",
    kind: "member",
    label: "member",
    detail: "fixture",
    applicable_modes: ["aot"],
    source_identity: sourceIdentity,
    legacy_stable_ids: [],
  };
  const contract = capabilityContract(identity, "aot");
  const candidate = { status: "unavailable", id: null, source_snapshot_hash: sha256("fixture"), compiler: null, tool: null, target: null, mode: "aot" };
  const row = {
    row_id: "core.fixture.member@aot",
    stable_id: "core.fixture.member@aot",
    capability_id: identity.capability_id,
    family: "core",
    kind: "member",
    label: "member",
    mode: "aot",
    applicable_modes: ["aot"],
    contract,
    route: { kind: "registry", mode: "aot", route: "aot:fixture", seam: "fixture", evidence: ["fixture.rs"], shared_implementation_key: "fixture", source_identity: sourceIdentity },
    observable: { value_consuming: false, sink: null, status: "unavailable", reason: "fixture" },
    oracle: { method: "independent-expected-behavior", independent_expected_behavior: true, shared: false, status: "unavailable", expected_behavior: contract.expected_observation, source: "fixture" },
    evidence: { status: "unavailable", class: "source-only", source_identity: sourceIdentity, execution: null, compiler: null, tool: null, target: null, candidate, candidate_identity: candidate },
    identity: { source: sourceIdentity, compiler: null, tool: null, target: null, candidate },
    source_identity: sourceIdentity,
    compiler_identity: null,
    tool_identity: null,
    target_identity: null,
    candidate_identity: candidate,
    owner: { card: "#2942", link: "#2942", capability_id: identity.capability_id, mode: "aot" },
    owner_link: "#2942",
    disposition: "implemented-unqualified",
    status: "implemented-unqualified",
    counted: true,
    exclusion: null,
    legacy_stable_ids: [],
    shared_implementation_key: "fixture",
  };
  const relation = {
    schema: CAPABILITY_RELATION_SCHEMA,
    schema_version: CAPABILITY_RELATION_VERSION,
    generated_by: "fixture",
    source_snapshot_hash: sha256("fixture"),
    identities: [identity],
    denominator: {
      identity_ids: [identity.capability_id],
      identity_count: 1,
      row_count: 1,
      counted_rows: 1,
      by_family: { core: 1 },
      by_mode: { aot: 1 },
      dispositions: Object.fromEntries(CAPABILITY_DISPOSITIONS.map((disposition) => [disposition, disposition === "implemented-unqualified" ? 1 : 0])),
    },
    rows: [row],
    exclusions: [],
    consumers: {},
    revision_rule: "fixture",
    content_digest: null,
  };
  relation.content_digest = capabilityRelationDigest(relation);
  return relation;
}

export function capabilityNegativeControls() {
  const controls = [];
  const base = capabilityFixtureRelation();
  const cases = [
    ["new-member", (value) => { value.identities.push({ ...value.identities[0], capability_id: "core.fixture.new", id: "core.fixture.new" }); }],
    ["missing-mode", (value) => { value.denominator.row_count = 0; value.rows = []; }],
    ["discarded-output", (value) => { value.rows[0].disposition = "passed"; value.rows[0].status = "passed"; }],
    ["shared-wrong-oracle", (value) => { value.rows[0].disposition = "passed"; value.rows[0].status = "passed"; value.rows[0].oracle.shared = true; value.rows[0].oracle.independent_expected_behavior = false; }],
    ["stale-identity", (value) => { value.rows[0].disposition = "passed"; value.rows[0].status = "passed"; value.rows[0].candidate_identity.status = "stale"; value.rows[0].identity.candidate.status = "stale"; }],
    ["invalid-exclusion", (value) => { value.rows[0].disposition = "owner-ratified-not-applicable"; value.rows[0].status = "owner-ratified-not-applicable"; value.rows[0].exclusion = { reason: "fixture", owner: null, decision: null }; }],
  ];
  for (const [name, mutate] of cases) {
    const fixture = cloneJson(base);
    mutate(fixture);
    fixture.content_digest = capabilityRelationDigest(fixture);
    const validation = validateCapabilityRelation(fixture);
    controls.push({ control: name, accepted: validation.ok, expected_disposition: "non-pass", errors: validation.errors });
  }
  return controls;
}

export function runCapabilityNegativeControls() {
  const controls = capabilityNegativeControls();
  if (controls.some((control) => control.accepted)) fail("capability negative control was accepted");
  return { schema: CAPABILITY_RELATION_SCHEMA, status: "PASS", controls };
}

export function buildManifest({ root = DEFAULT_ROOT, surface = null } = {}) {
  const actual = surface || defaultSurface(root);
  const routes = normalizeRoutes(actual.routes);
  const exclusions = normalizedExclusions(actual.exclusions);
  const members = {};
  for (const kind of KIND_ORDER) {
    const values = [...(actual[kind === "module_call"
      ? "moduleCalls"
      : kind === "receiver_method"
        ? "receivers"
        : kind === "field"
          ? "fields"
          : "types"] || [])];
    const ids = values.map((value) => normalizeId(kind, value));
    const seen = new Set();
    for (const stableId of ids) {
      if (seen.has(stableId)) fail(`duplicate public row: ${stableId}`);
      seen.add(stableId);
    }
    members[kind] = ids.sort(compareStable);
  }
  const seeds = seedMap(actual, members);
  const rowSurface = { ...actual, seeds };
  const rows = KIND_ORDER.flatMap((kind) => members[kind].map((value) => buildRow(kind, value, rowSurface, routes, exclusions)))
    .sort((left, right) => compareStable(left.stable_id, right.stable_id));
  const sourceIdsSha256 = sourceIdsDigest(members);
  const snapshot = actual.snapshot || sourceSnapshot(root);
  const manifest = {
    schema: SURFACE_SCHEMA,
    schema_version: SURFACE_SCHEMA_VERSION,
    source_snapshot: snapshot,
    denominator: {
      source_ids: members,
      source_ids_sha256: sourceIdsSha256,
      total: Object.values(members).reduce((sum, ids) => sum + ids.length, 0),
      counts: {
        ...Object.fromEntries(KIND_ORDER.map((kind) => [kind, members[kind].length])),
        exclusions: exclusions.size,
      },
    },
    actual_routes: routes,
    exclusions: [...exclusions.entries()].sort(([left], [right]) => compareStable(left, right)).map(([stable_id, value]) => ({ stable_id, ...value })),
    rows,
    capability_relation: buildCapabilityRelation({
      root,
      sourceSnapshotHash: snapshot.hash,
      legacyRows: rows,
      surface,
    }),
  };
  manifest.content_digest = manifestContentDigest(manifest);
  const validation = validateManifest(manifest, {
    expectedIds: {
      source_ids: members,
      source_ids_sha256: sourceIdsSha256,
      total: manifest.denominator.total,
      counts: manifest.denominator.counts,
    },
    expectedExclusions: exclusions,
    root,
  });
  if (!validation.ok) fail(validation.errors.join("\n"));
  return manifest;
}

function expectedRouteMap(manifest) {
  const out = Object.fromEntries(TIERS.map((tier) => [tier, new Map()]));
  for (const tier of TIERS) for (const route of Array.isArray(manifest.actual_routes?.[tier]) ? manifest.actual_routes[tier] : []) {
    if (!isRecord(route) || typeof route.stable_id !== "string" || typeof route.route !== "string") continue;
    const key = `${route.stable_id}\u0000${route.route}`;
    if (out[tier].has(key)) return { error: `duplicate actual route fact: ${tier}:${route.stable_id}:${route.route}` };
    out[tier].set(key, route);
  }
  return out;
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function validateSourceSnapshot(snapshot, errors) {
  if (!isRecord(snapshot)) {
    errors.push("manifest source snapshot is required");
    return;
  }
  if (snapshot.algorithm !== "sha256") errors.push("manifest source snapshot algorithm must be sha256");
  if (!Array.isArray(snapshot.files)) {
    errors.push("manifest source snapshot files are required");
    return;
  }
  let previousPath = null;
  const paths = new Set();
  for (const file of snapshot.files) {
    if (!isRecord(file)) {
      errors.push("manifest source snapshot has an invalid file");
      continue;
    }
    if (typeof file.path !== "string" || file.path.length === 0) errors.push("manifest source snapshot file path is required");
    else {
      if (paths.has(file.path)) errors.push(`duplicate manifest source snapshot file: ${file.path}`);
      paths.add(file.path);
      if (previousPath !== null && compareStable(previousPath, file.path) > 0) errors.push("manifest source snapshot files are not sorted");
      previousPath = file.path;
    }
    if (!Number.isInteger(file.bytes) || file.bytes < 0) errors.push(`invalid manifest source snapshot byte count: ${file.path || "?"}`);
    if (!validDigest(file.sha256)) errors.push(`invalid manifest source snapshot digest: ${file.path || "?"}`);
  }
  if (!validDigest(snapshot.hash)) errors.push("manifest source snapshot hash is missing or invalid");
  else if (snapshot.hash !== sha256(canonicalJson(snapshot.files))) errors.push("manifest source snapshot hash does not match files");
}

function expectedSourceMembership(expectedIds, root, errors) {
  if (expectedIds === null || expectedIds === undefined) {
    try {
      const sourceIds = sourceMembership(root).sourceIds;
      const normalized = canonicalSourceIds(sourceIds);
      return {
        sourceIds: normalized,
        source_ids_sha256: sourceIdsDigest(normalized),
        total: Object.values(normalized).reduce((sum, ids) => sum + ids.length, 0),
        counts: Object.fromEntries(KIND_ORDER.map((kind) => [kind, normalized[kind].length])),
      };
    } catch (error) {
      errors.push(`authoritative public membership unavailable: ${error.message}`);
      return null;
    }
  }

  if (!isRecord(expectedIds) || !isRecord(expectedIds.source_ids)) {
    errors.push("expected membership must be a complete denominator object");
    return null;
  }
  const sourceIds = expectedIds.source_ids;
  const suppliedDigest = expectedIds.source_ids_sha256;
  const expectedTotal = expectedIds.total;
  const expectedCounts = expectedIds.counts;
  const normalized = canonicalSourceIds(sourceIds);
  if (isRecord(sourceIds)) {
    for (const key of Object.keys(sourceIds)) {
      if (!KIND_ORDER.includes(key)) errors.push(`expected membership has unknown source IDs: ${key}`);
    }
  } else {
    errors.push("expected membership source IDs are required");
  }
  for (const kind of KIND_ORDER) {
    const ids = sourceIds?.[kind];
    if (!Array.isArray(ids)) {
      errors.push(`expected membership is missing ${kind}`);
      continue;
    }
    if (new Set(ids).size !== ids.length) errors.push(`expected membership contains duplicate ${kind}`);
    if (ids.some((id) => typeof id !== "string" || !id.startsWith(KIND_PREFIX[kind]) || id.length <= KIND_PREFIX[kind].length)) {
      errors.push(`expected membership has invalid ${kind}`);
    }
    if (canonicalJson(ids) !== canonicalJson(normalized[kind])) errors.push(`expected membership ${kind} is not sorted`);
  }
  const digest = sourceIdsDigest(normalized);
  if (!validDigest(suppliedDigest)) errors.push("expected membership source_ids_sha256 is missing or invalid");
  else if (suppliedDigest !== digest) errors.push("expected membership source_ids_sha256 does not match source IDs");
  const total = Object.values(normalized).reduce((sum, ids) => sum + ids.length, 0);
  if (!Number.isInteger(expectedTotal) || expectedTotal < 0) errors.push("expected membership total is missing or invalid");
  else if (expectedTotal !== total) errors.push("expected membership total is stale");
  if (!isRecord(expectedCounts)) {
    errors.push("expected membership counts are required");
  } else {
    for (const key of Object.keys(expectedCounts)) {
      if (!KIND_ORDER.includes(key) && key !== "exclusions") errors.push(`expected membership has unknown count: ${key}`);
    }
    for (const kind of KIND_ORDER) {
      if (!Number.isInteger(expectedCounts[kind]) || expectedCounts[kind] < 0) errors.push(`expected membership count is invalid: ${kind}`);
      else if (expectedCounts[kind] !== normalized[kind].length) errors.push(`expected membership count mismatch: ${kind}`);
    }
    if (!Number.isInteger(expectedCounts.exclusions) || expectedCounts.exclusions < 0) errors.push("expected membership exclusion count is invalid");
  }
  return {
    sourceIds: normalized,
    source_ids_sha256: digest,
    total,
    counts: expectedCounts,
  };
}
function expectedExclusionRecords(expectedExclusions, root, errors) {
  let records = expectedExclusions;
  if (records === null || records === undefined) {
    try {
      records = parseExclusions(root);
    } catch (error) {
      errors.push(`authoritative exclusions unavailable: ${error.message}`);
      return null;
    }
  }
  try {
    const normalized = normalizedExclusions(records);
    return new Map([...normalized.entries()].map(([stableId, details]) => {
      const value = { ...details };
      delete value.stable_id;
      return [stableId, value];
    }));
  } catch (error) {
    errors.push(`authoritative exclusions are invalid: ${error.message}`);
    return null;
  }
}

function exclusionDetails(value) {
  const details = { ...value };
  delete details.stable_id;
  return details;
}
const CORPUS_PATH_PREFIX = "tests/conformance/corpus/";

function sourceSeedId(path) {
  if (typeof path !== "string" || !path.startsWith(CORPUS_PATH_PREFIX) || !path.endsWith(".jet")) return null;
  const relativePath = path.slice(CORPUS_PATH_PREFIX.length).replaceAll("\\", "/");
  if (relativePath.includes("..")) return null;
  return `module:${relativePath.slice(0, -".jet".length).replaceAll("/", ".")}`;
}

function validateAuthoritativeRecipes(manifest, root, errors) {
  const snapshotPaths = new Set(Array.isArray(manifest.source_snapshot?.files)
    ? manifest.source_snapshot.files.filter(isRecord).map((file) => file.path)
    : []);
  const seenPaths = new Map();
  for (const row of (Array.isArray(manifest.rows) ? manifest.rows : []).filter(isRecord)) {
    if (row.seed === null || row.seed === undefined) continue;
    if (typeof row.seed !== "string" || row.seed.length === 0 || row.seed.startsWith("/") || row.seed.includes("..")) {
      errors.push(`manifest row seed path is invalid: ${row.stable_id}`);
      continue;
    }
    if (seenPaths.has(row.seed)) errors.push(`duplicate manifest recipe path: ${row.seed}`);
    seenPaths.set(row.seed, row.stable_id);
    if (snapshotPaths.size > 0 && !snapshotPaths.has(row.seed)) errors.push(`manifest recipe is outside source snapshot: ${row.stable_id}`);
    const seedId = sourceSeedId(row.seed);
    if (seedId !== row.stable_id) {
      errors.push(`manifest recipe identity mismatch: ${row.stable_id}`);
      continue;
    }
    const absolute = join(root, row.seed);
    if (!existsSync(absolute)) {
      errors.push(`manifest recipe is unreadable: ${row.stable_id}`);
      continue;
    }
    let inspection;
    try {
      inspection = seedInspection(untaggedId(seedId), readFileSync(absolute, "utf8"));
    } catch (error) {
      errors.push(`manifest recipe inspection failed: ${row.stable_id}: ${error.message}`);
      continue;
    }
    if (row.status === "covered") {
      if (inspection.errors.length > 0) errors.push(`manifest recipe is not executable: ${row.stable_id}`);
      if (canonicalJson(row.sink) !== canonicalJson(inspection.sink)) errors.push(`manifest recipe sink differs from source: ${row.stable_id}`);
      if (Object.hasOwn(row, "errors")) errors.push(`covered row has recipe diagnostics: ${row.stable_id}`);
    } else if (row.status === "invalid") {
      if (!Array.isArray(row.errors) || canonicalJson(row.errors) !== canonicalJson(inspection.errors)) errors.push(`manifest recipe diagnostics differ from source: ${row.stable_id}`);
      if (canonicalJson(row.sink) !== canonicalJson(inspection.sink)) errors.push(`manifest recipe sink differs from source: ${row.stable_id}`);
    }
  }
}


export function validateManifest(manifest, { expectedIds = null, expectedExclusions = null, currentSnapshotHash = null, root = DEFAULT_ROOT } = {}) {
  const errors = [];
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) return { ok: false, errors: ["manifest is not an object"] };
  const expectedContentDigest = manifestContentDigest(manifest);
  if (manifest.schema !== SURFACE_SCHEMA) errors.push(`manifest schema must be ${SURFACE_SCHEMA}`);
  if (manifest.schema_version !== SURFACE_SCHEMA_VERSION) errors.push(`manifest schema_version must be ${SURFACE_SCHEMA_VERSION}`);
  if (!validDigest(manifest.content_digest)) errors.push("manifest content digest is missing or invalid");
  else if (manifest.content_digest !== expectedContentDigest) errors.push("manifest content digest does not match manifest");
  validateSourceSnapshot(manifest.source_snapshot, errors);
  const expectedMembership = expectedSourceMembership(expectedIds, root, errors);
  const ratifiedExclusions = expectedIds !== null && expectedExclusions === null
    ? (errors.push("expected owner-ratified exclusions are required"), null)
    : expectedExclusionRecords(expectedExclusions, root, errors);

  const rows = Array.isArray(manifest.rows) ? manifest.rows : [];
  if (!Array.isArray(manifest.rows)) errors.push("manifest rows are required");
  if (expectedIds === null) {
    try {
      const current = sourceSnapshot(root);
      if (manifest.source_snapshot?.hash !== current.hash) errors.push("manifest source snapshot is stale");
    } catch (error) {
      errors.push(`authoritative source snapshot unavailable: ${error.message}`);
    }
  }
  if (expectedIds === null) validateAuthoritativeRecipes(manifest, root, errors);
  const seen = new Set();
  const rowsById = new Map();
  let previousRowId = null;
  for (const row of rows) {
    if (!isRecord(row) || typeof row.stable_id !== "string" || row.stable_id.length === 0) {
      errors.push("manifest row has no stable_id");
      continue;
    }
    const stableId = row.stable_id;
    if (seen.has(stableId)) errors.push(`duplicate stable_id: ${stableId}`);
    seen.add(stableId);
    rowsById.set(stableId, row);
    if (previousRowId !== null && compareStable(previousRowId, stableId) > 0) errors.push("manifest rows are not sorted");
    previousRowId = stableId;

    const prefix = KIND_PREFIX[row.kind];
    if (!KIND_ORDER.includes(row.kind)) errors.push(`unowned public row kind: ${row.kind}: ${stableId}`);
    if (!prefix || !stableId.startsWith(prefix) || stableId.length === prefix.length) errors.push(`stable_id kind mismatch: ${stableId}`);
    for (const field of ["kind", "owner", "member", "domain", "status"]) {
      if (typeof row[field] !== "string" || row[field].length === 0) errors.push(`manifest row ${field} is required: ${stableId}`);
    }
    if (prefix) {
      const untagged = stableId.slice(prefix.length);
      const dot = untagged.lastIndexOf(".");
      const owner = dot < 0 ? untagged : untagged.slice(0, dot);
      const member = dot < 0 ? untagged : untagged.slice(dot + 1);
      if (row.owner !== owner || row.member !== member) errors.push(`manifest row identity mismatch: ${stableId}`);
      if (typeof row.owner === "string" && typeof row.member === "string" && row.domain !== rowDomain(row)) errors.push(`manifest row domain mismatch: ${stableId}`);
    }

    const tiers = row.applicable_tiers;
    if (!Array.isArray(tiers)) errors.push(`manifest row applicable_tiers is required: ${stableId}`);
    else {
      if ([...new Set(tiers)].length !== tiers.length) errors.push(`duplicate tier projection: ${stableId}`);
      for (const tier of tiers) if (!TIERS.includes(tier)) errors.push(`unknown tier projection ${tier}: ${stableId}`);
      if (canonicalJson(tiers) !== canonicalJson([...tiers].sort((left, right) => TIERS.indexOf(left) - TIERS.indexOf(right)))) errors.push(`manifest tiers are not sorted: ${stableId}`);
    }

    const projections = row.projections;
    const projectionKeys = new Set();
    if (!Array.isArray(projections)) errors.push(`manifest row projections are required: ${stableId}`);
    else {
      for (const projection of projections) {
        if (!isRecord(projection)) {
          errors.push(`manifest row has an invalid projection: ${stableId}`);
          continue;
        }
        if (!TIERS.includes(projection.tier)) errors.push(`unknown projection tier ${projection.tier}: ${stableId}`);
        if (typeof projection.route !== "string" || projection.route.length === 0) errors.push(`projection route is required: ${stableId}`);
        const key = `${projection.tier}\u0000${projection.route}`;
        if (projectionKeys.has(key)) errors.push(`duplicate row projection: ${stableId}:${projection.tier}:${projection.route}`);
        projectionKeys.add(key);
        if (!Object.prototype.hasOwnProperty.call(projection, "seam") || (projection.seam !== null && (typeof projection.seam !== "string" || projection.seam.length === 0))) {
          errors.push(`projection seam is invalid: ${stableId}`);
        }
        if (!Array.isArray(projection.evidence) || projection.evidence.some((item) => typeof item !== "string" || item.length === 0)) errors.push(`projection evidence is invalid: ${stableId}`);
      }
      const ordered = [...projections].sort((left, right) => TIERS.indexOf(left?.tier) - TIERS.indexOf(right?.tier) || compareStable(left?.route || "", right?.route || ""));
      if (canonicalJson(projections) !== canonicalJson(ordered)) errors.push(`manifest row projections are not sorted: ${stableId}`);
    }
    const projectionTiers = Array.isArray(projections) ? projections.filter(isRecord).map((projection) => projection.tier) : [];
    if (Array.isArray(tiers) && canonicalJson([...tiers].sort()) !== canonicalJson([...new Set(projectionTiers)].sort())) errors.push(`applicable tier projection mismatch: ${stableId}`);
    if (!Array.isArray(row.dispatcher_arms)) errors.push(`manifest dispatcher arms are required: ${stableId}`);
    else if (Array.isArray(projections) && canonicalJson(row.dispatcher_arms) !== canonicalJson(projections.filter(isRecord).map((projection) => projection.route))) errors.push(`dispatcher arms mismatch: ${stableId}`);

    for (const field of ["membership_sources", "membership_evidence"]) {
      if (!Array.isArray(row[field]) || row[field].some((item) => typeof item !== "string" || item.length === 0)) errors.push(`manifest row ${field} is invalid: ${stableId}`);
      else if (canonicalJson(row[field]) !== canonicalJson(uniqueSorted(row[field]))) errors.push(`manifest row ${field} is not sorted: ${stableId}`);
    }
    if (row.seed !== null && (typeof row.seed !== "string" || row.seed.length === 0)) errors.push(`manifest row seed is invalid: ${stableId}`);
    if (row.value_consuming !== null && typeof row.value_consuming !== "boolean") errors.push(`manifest row value_consuming is invalid: ${stableId}`);
    if (row.sink !== null && !isRecord(row.sink)) errors.push(`manifest row sink is invalid: ${stableId}`);
    if (row.exclusion !== null && !isRecord(row.exclusion)) errors.push(`manifest row exclusion is invalid: ${stableId}`);
    if (!VALID_STATUSES.has(row.status)) errors.push(`unknown manifest row status ${row.status}: ${stableId}`);
    if (row.status === "covered") {
      if (row.exclusion !== null) errors.push(`covered row has an exclusion record: ${stableId}`);
      if (row.value_consuming !== true || !row.seed || !row.sink || row.sink.type_aware !== true || typeof row.sink.operation !== "string" || row.sink.operation.length === 0) {
        errors.push(`executable row has no type-aware observable sink: ${stableId}`);
      }
    } else if (row.status === "excluded") {
      if (!row.exclusion?.reason || !row.exclusion?.owner || !row.exclusion?.decision) errors.push(`exclusion is not owner-ratified: ${stableId}`);
      if (row.seed !== null || row.value_consuming !== null || row.sink !== null) errors.push(`excluded row has executable proof: ${stableId}`);
    } else if (row.status === "missing" || row.status === "unrouted") {
      errors.push(`unresolved public row: ${stableId} (requires a real seed or owner-ratified exclusion)`);
      if (row.seed !== null || row.value_consuming !== null || row.sink !== null) errors.push(`unresolved row has executable proof: ${stableId}`);
      if (row.status === "unrouted" && tiers?.length !== 0) errors.push(`unrouted row has coverage: ${stableId}`);
    } else if (row.status === "invalid") {
      const diagnostics = Array.isArray(row.errors) ? row.errors : [];
      if (!row.seed) errors.push(`recipe-invalid row has no recipe: ${stableId}`);
      if (row.value_consuming === true) errors.push(`invalid row has executable proof: ${stableId}`);
      if (!Array.isArray(row.errors) || diagnostics.some((error) => typeof error !== "string" || error.length === 0)) {
        errors.push(`recipe diagnostics are invalid: ${stableId}`);
      }
      errors.push(`recipe-invalid public row: ${stableId}${diagnostics.length ? ` (${diagnostics.join("; ")})` : ""}`);
    } else if (row.status === "invalid-exclusion") {
      errors.push(`invalid owner-ratified exclusion: ${stableId}`);
      if (row.seed !== null || row.value_consuming !== null || row.sink !== null) errors.push(`invalid exclusion has executable proof: ${stableId}`);
    }
  }

  const denominator = isRecord(manifest.denominator) ? manifest.denominator : null;
  if (!denominator) errors.push("manifest denominator is required");
  const sourceIds = denominator && isRecord(denominator.source_ids) ? denominator.source_ids : null;
  const counts = denominator && isRecord(denominator.counts) ? denominator.counts : null;
  if (!sourceIds) errors.push("manifest denominator source_ids are required");
  if (!counts) errors.push("manifest denominator counts are required");
  if (sourceIds && !validDigest(denominator.source_ids_sha256)) errors.push("manifest denominator source_ids_sha256 is missing or invalid");
  else if (sourceIds && denominator.source_ids_sha256 !== sourceIdsDigest(sourceIds)) errors.push("manifest denominator source_ids_sha256 does not match source IDs");
  if (!Number.isInteger(denominator?.total) || denominator.total < 0) errors.push("manifest denominator total is missing or invalid");
  if (counts) {
    for (const key of Object.keys(counts)) {
      if (!KIND_ORDER.includes(key) && key !== "exclusions") errors.push(`manifest denominator has unknown count: ${key}`);
    }
    for (const kind of KIND_ORDER) {
      if (!Number.isInteger(counts[kind]) || counts[kind] < 0) errors.push(`manifest denominator count is invalid: ${kind}`);
    }
    if (!Number.isInteger(counts.exclusions) || counts.exclusions < 0) errors.push("manifest exclusion count is invalid");
  }
  const memberIds = new Set();
  const denominatorIds = new Set();
  if (sourceIds) {
    for (const key of Object.keys(sourceIds)) {
      if (!KIND_ORDER.includes(key)) errors.push(`manifest denominator has unknown source IDs: ${key}`);
    }
  }
  for (const kind of KIND_ORDER) {
    const source = sourceIds && Array.isArray(sourceIds[kind]) ? sourceIds[kind] : null;
    if (!source) {
      errors.push(`manifest denominator is missing ${kind}`);
      continue;
    }
    const sourceSet = new Set(source);
    if (sourceSet.size !== source.length) errors.push(`denominator contains duplicate ${kind}`);
    let previousId = null;
    for (const id of source) {
      if (typeof id !== "string" || id.length === 0 || !id.startsWith(KIND_PREFIX[kind])) errors.push(`denominator has invalid ${kind}: ${id}`);
      else {
        memberIds.add(id);
        if (denominatorIds.has(id)) errors.push(`duplicate denominator identity: ${id}`);
        denominatorIds.add(id);
        if (previousId !== null && compareStable(previousId, id) > 0) errors.push(`denominator ${kind} is not sorted`);
        previousId = id;
      }
    }
    if (counts && counts[kind] !== source.length) errors.push(`denominator count mismatch: ${kind}`);
    const rowSet = new Set(rows.filter((row) => row?.kind === kind).map((row) => row?.stable_id));
    for (const id of sourceSet) if (!rowSet.has(id)) errors.push(`source membership missing from manifest: ${id}`);
    for (const id of rowSet) if (!sourceSet.has(id)) errors.push(`manifest row is not source membership: ${id}`);
  }
  const sourceTotal = [...denominatorIds].length;
  if (Number.isInteger(denominator?.total) && denominator.total !== sourceTotal) errors.push("manifest denominator total is stale");
  if (expectedMembership && sourceIds) {
    for (const kind of KIND_ORDER) {
      const actual = Array.isArray(sourceIds[kind]) ? sourceIds[kind] : [];
      const expected = expectedMembership.sourceIds[kind];
      const actualSet = new Set(actual);
      const expectedSet = new Set(expected);
      for (const id of expectedSet) if (!actualSet.has(id)) errors.push(`public source membership missing from manifest: ${id}`);
      for (const id of actualSet) if (!expectedSet.has(id)) errors.push(`manifest public row is not source membership: ${id}`);
      if (canonicalJson(actual) !== canonicalJson(expected)) errors.push(`manifest source membership is stale: ${kind}`);
    }
    if (sourceIds && denominator.source_ids_sha256 !== expectedMembership.source_ids_sha256) {
      errors.push("manifest denominator source_ids_sha256 is stale");
    }
  }
  if (expectedIds !== null && expectedMembership) {
    if (denominator?.total !== expectedMembership.total) errors.push("manifest denominator total differs from injected identity");
    if (counts && expectedMembership.counts) {
      for (const kind of KIND_ORDER) {
        if (counts[kind] !== expectedMembership.counts[kind]) errors.push(`manifest denominator count differs from injected identity: ${kind}`);
      }
      if (counts.exclusions !== expectedMembership.counts.exclusions) errors.push("manifest exclusion count differs from injected identity");
    }
  }
  if (expectedIds !== null && ratifiedExclusions && expectedMembership?.counts?.exclusions !== ratifiedExclusions.size) {
    errors.push("expected membership exclusion count does not match owner-ratified exclusions");
  }
  if (counts && counts.exclusions === undefined) errors.push("manifest exclusion count is required");

  const exclusions = manifest.exclusions;
  const exclusionsById = new Map();
  if (!Array.isArray(exclusions)) errors.push("manifest exclusions are required");
  else {
    let previousExclusion = null;
    for (const exclusion of exclusions) {
      if (!isRecord(exclusion) || typeof exclusion.stable_id !== "string" || exclusion.stable_id.length === 0) {
        errors.push("manifest exclusion has no stable_id");
        continue;
      }
      const stableId = exclusion.stable_id;
      if (exclusionsById.has(stableId)) errors.push(`duplicate exclusion identity: ${stableId}`);
      exclusionsById.set(stableId, exclusion);
      if (previousExclusion !== null && compareStable(previousExclusion, stableId) > 0) errors.push("manifest exclusions are not sorted");
      previousExclusion = stableId;
      const kind = KIND_ORDER.find((candidate) => stableId.startsWith(KIND_PREFIX[candidate]));
      if (!kind) errors.push(`exclusion has invalid stable_id: ${stableId}`);
      if (!memberIds.has(stableId)) errors.push(`exclusion is not source membership: ${stableId}`);
      for (const field of ["reason", "owner", "decision"]) {
        if (typeof exclusion[field] !== "string" || exclusion[field].length === 0) errors.push(`exclusion ${field} is required: ${stableId}`);
      }
    }
    if (counts && counts.exclusions !== exclusions.length) errors.push("denominator exclusion count mismatch");
  }
  if (ratifiedExclusions) {
    for (const [stableId, expected] of ratifiedExclusions) {
      const recorded = exclusionsById.get(stableId);
      if (!recorded) errors.push(`manifest is missing owner-ratified exclusion: ${stableId}`);
      else if (canonicalJson(exclusionDetails(recorded)) !== canonicalJson(expected)) errors.push(`manifest exclusion differs from owner-ratified record: ${stableId}`);
    }
    for (const stableId of exclusionsById.keys()) {
      if (!ratifiedExclusions.has(stableId)) errors.push(`manifest exclusion is not owner-ratified: ${stableId}`);
    }
  }
  if (ratifiedExclusions && counts && counts.exclusions !== ratifiedExclusions.size) {
    errors.push("manifest denominator exclusion count does not match owner-ratified exclusions");
  }
  for (const row of rows.filter(isRecord)) {
    const recorded = exclusionsById.get(row.stable_id);
    if (row.exclusion === null) {
      if (recorded) errors.push(`manifest exclusion is missing from row: ${row.stable_id}`);
      if (row.status === "excluded") errors.push(`excluded row has no exclusion record: ${row.stable_id}`);
    } else if (!recorded) {
      errors.push(`row exclusion is not persisted: ${row.stable_id}`);
    } else {
      const recordedDetails = { ...recorded };
      delete recordedDetails.stable_id;
      if (canonicalJson(row.exclusion) !== canonicalJson(recordedDetails) || row.status !== "excluded") errors.push(`row exclusion mismatch: ${row.stable_id}`);
    }
  }
  for (const stableId of exclusionsById.keys()) if (!rowsById.has(stableId)) errors.push(`exclusion has no manifest row: ${stableId}`);

  const actualRoutes = manifest.actual_routes;
  if (!isRecord(actualRoutes)) errors.push("manifest actual_routes are required");
  else {
    for (const key of Object.keys(actualRoutes)) if (!TIERS.includes(key)) errors.push(`unknown route tier: ${key}`);
    for (const tier of TIERS) {
      const routes = actualRoutes[tier];
      if (!Array.isArray(routes)) {
        errors.push(`manifest routes are missing ${tier}`);
        continue;
      }
      let previousRoute = null;
      const routeKeys = new Set();
      for (const route of routes) {
        if (!isRecord(route) || typeof route.stable_id !== "string" || typeof route.route !== "string" || route.route.length === 0) {
          errors.push(`manifest route is invalid: ${tier}`);
          continue;
        }
        const key = `${route.stable_id}\u0000${route.route}`;
        if (routeKeys.has(key)) errors.push(`duplicate actual route fact: ${tier}:${route.stable_id}:${route.route}`);
        routeKeys.add(key);
        if (!KIND_ORDER.some((kind) => route.stable_id.startsWith(KIND_PREFIX[kind]))) errors.push(`route has invalid stable_id: ${tier}:${route.stable_id}`);
        if (!Object.prototype.hasOwnProperty.call(route, "seam") || (route.seam !== null && (typeof route.seam !== "string" || route.seam.length === 0))) errors.push(`route seam is invalid: ${tier}:${route.stable_id}`);
        if (!Array.isArray(route.evidence) || route.evidence.some((item) => typeof item !== "string" || item.length === 0)) errors.push(`route evidence is invalid: ${tier}:${route.stable_id}`);
        const identity = `${route.stable_id}\u0000${route.route}`;
        if (previousRoute !== null && compareStable(previousRoute, identity) > 0) errors.push(`manifest routes are not sorted: ${tier}`);
        previousRoute = identity;
      }
    }
  }
  const routeMap = expectedRouteMap(manifest);
  if (routeMap.error) errors.push(routeMap.error);
  for (const tier of TIERS) for (const route of (Array.isArray(manifest.actual_routes?.[tier]) ? manifest.actual_routes[tier] : []).filter(isRecord)) {
    if (!seen.has(route.stable_id)) errors.push(`dispatcher arm has no public row: ${tier}:${route.stable_id}`);
    else {
      const row = rowsById.get(route.stable_id);
      const projection = Array.isArray(row?.projections)
        ? row.projections.find((candidate) => isRecord(candidate) && candidate.tier === tier && candidate.route === route.route)
        : null;
      if (!projection) errors.push(`dispatcher arm missing projection: ${tier}:${route.stable_id}:${route.route}`);
      else if (canonicalJson({ seam: projection.seam, evidence: projection.evidence }) !== canonicalJson({ seam: route.seam, evidence: route.evidence })) errors.push(`dispatcher arm proof mismatch: ${tier}:${route.stable_id}:${route.route}`);
    }
  }
  for (const row of rows.filter(isRecord)) for (const projection of (Array.isArray(row.projections) ? row.projections : []).filter(isRecord)) {
    const route = routeMap[projection.tier]?.get(`${row.stable_id}\u0000${projection.route}`) || null;
    if (!route) errors.push(`fake coverage or missing dispatcher arm: ${projection.tier}:${row.stable_id}:${projection.route}`);
  }
  const constructorRows = rows.filter((row) => row?.member === "new");
  const constructorIds = new Set();
  for (const row of constructorRows) {
    const key = `${row.kind}:${row.owner}`;
    if (constructorIds.has(key)) errors.push(`duplicate constructor: ${key}`);
    constructorIds.add(key);
    if (row.kind === "receiver_method" && !rows.some((candidate) => candidate?.kind === "nominal_type" && candidate.member === row.owner)) {
      // Receiver type names are often unqualified in the registry; this check
      // only rejects an actually unowned constructor, not a missing module path.
      if (!row.owner) errors.push(`constructor has no owner: ${row.stable_id}`);
    }
  }
  if (manifest.capability_relation !== undefined) {
    const capabilityValidation = validateCapabilityRelation(manifest.capability_relation, {
      sourceSnapshotHash: manifest.source_snapshot?.hash || null,
      sourceSnapshotFiles: manifest.source_snapshot?.files || null,
    });
    if (!capabilityValidation.ok) {
      for (const error of capabilityValidation.errors) errors.push(`capability relation: ${error}`);
    }
  }
  if (currentSnapshotHash !== null && currentSnapshotHash !== undefined && manifest.source_snapshot?.hash !== currentSnapshotHash) errors.push("manifest source snapshot is stale");
  return { ok: errors.length === 0, errors: [...new Set(errors)].sort() };
}

export function manifestIsStale(manifest, root = DEFAULT_ROOT) {
  return manifest?.source_snapshot?.hash !== sourceSnapshot(root).hash;
}

export function readManifest(path, { root = DEFAULT_ROOT } = {}) {
  if (typeof path !== "string" || path.length === 0) fail("manifest path is required");
  if (!existsSync(path)) fail(`unreadable manifest: ${path}`);
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(`unreadable manifest ${path}: ${error.message}`);
  }
  const validation = validateManifest(manifest, { currentSnapshotHash: sourceSnapshot(root).hash });
  if (!validation.ok) fail(validation.errors.join("\n"));
  return manifest;
}

function hostileFixtures() {
  const surface = {
    moduleCalls: ["core.test.call", "core.test.missing"],
    receivers: [{ type: "Widget", member: "read" }],
    fields: [{ type: "Widget", field: "value" }],
    types: ["core.test.Widget"],
    routes: {
      aot: [{ stable_id: "module:core.test.call", route: "aot:fixture" }],
      jet_run: [{ stable_id: "receiver:Widget.read", route: "jet_run:fixture" }],
      interpreter: [{ stable_id: "field:Widget.value", route: "interpreter:fixture" }],
    },
    seeds: new Map([[
      "module:core.test.call",
      { path: "tests/conformance/corpus/core/test/call.jet", errors: [], sink: { type_aware: true, operation: "print", kind: "primitive" } },
    ]]),
    exclusions: new Map([
      ["module:core.test.missing", { reason: "fixture has no executable seed", owner: "owner", decision: "D-2335" }],
      ["receiver:Widget.read", { reason: "fixture receiver is not a callable seed", owner: "owner", decision: "D-2335" }],
      ["field:Widget.value", { reason: "fixture field is not a callable seed", owner: "owner", decision: "D-2335" }],
      ["type:core.test.Widget", { reason: "fixture nominal type is not a callable seed", owner: "owner", decision: "D-2335" }],
    ]),
    snapshot: sourceSnapshotFromContents({ "fixture.rs": "one" }),
    membershipSources: Object.fromEntries(KIND_ORDER.map((kind) => [kind, ["fixture.rs"]])),
  };
  const manifest = buildManifest({ surface });
  const fixtureExpectedIds = JSON.parse(JSON.stringify(manifest.denominator));
  const fixtureExpectedExclusions = surface.exclusions;
  const validateFixture = (value, options = {}) => validateManifest(value, {
    ...options,
    expectedIds: fixtureExpectedIds,
    expectedExclusions: fixtureExpectedExclusions,
  });
  if (!validateFixture(manifest).ok) fail("valid manifest fixture rejected");
  const missingIdentity = JSON.parse(JSON.stringify(fixtureExpectedIds));
  delete missingIdentity.source_ids_sha256;
  if (validateManifest(manifest, { expectedIds: missingIdentity, expectedExclusions: fixtureExpectedExclusions }).ok) fail("missing injected denominator digest accepted");
  if (validateManifest(manifest, { expectedIds: fixtureExpectedIds.source_ids, expectedExclusions: fixtureExpectedExclusions }).ok) fail("legacy injected membership shape accepted");
  const rehash = (value) => { value.content_digest = manifestContentDigest(value); return value; };
  const tamperedContent = JSON.parse(JSON.stringify(manifest));
  tamperedContent.rows[0].domain = "tampered";
  const tamperedValidation = validateFixture(tamperedContent);
  if (tamperedValidation.ok || !tamperedValidation.errors.includes("manifest content digest does not match manifest")) fail("manifest content tampering accepted");
  const missingReceiver = JSON.parse(JSON.stringify(manifest));
  missingReceiver.rows = missingReceiver.rows.filter((row) => row.stable_id !== "receiver:Widget.read");
  missingReceiver.denominator.source_ids.receiver_method = ["receiver:Widget.read"];
  rehash(missingReceiver);
  if (validateFixture(missingReceiver).ok) fail("missing receiver accepted");
  const missingField = JSON.parse(JSON.stringify(manifest));
  missingField.rows = missingField.rows.filter((row) => row.stable_id !== "field:Widget.value");
  rehash(missingField);
  if (validateFixture(missingField).ok) fail("missing field accepted");
  const fakeCoverage = JSON.parse(JSON.stringify(manifest));
  fakeCoverage.rows.find((row) => row.stable_id === "module:core.test.call").projections.push({ tier: "interpreter", route: "fake:coverage", evidence: [] });
  fakeCoverage.rows.find((row) => row.stable_id === "module:core.test.call").applicable_tiers.push("interpreter");
  rehash(fakeCoverage);
  if (validateFixture(fakeCoverage).ok) fail("fake coverage accepted");
  const fakeRow = JSON.parse(JSON.stringify(manifest));
  const fake = JSON.parse(JSON.stringify(fakeRow.rows[0]));
  fake.stable_id = "field:Widget.fake";
  fake.member = "fake";
  fakeRow.rows.push(fake);
  rehash(fakeRow);
  if (validateFixture(fakeRow).ok) fail("fake row accepted");
  const fakeRoute = JSON.parse(JSON.stringify(manifest));
  fakeRoute.actual_routes.aot.push({ stable_id: "module:core.test.call", route: "aot:fake", seam: null, evidence: [] });
  rehash(fakeRoute);
  if (validateFixture(fakeRoute).ok) fail("fake route accepted");
  const duplicate = JSON.parse(JSON.stringify(manifest));
  duplicate.rows.push(duplicate.rows[0]);
  rehash(duplicate);
  if (validateFixture(duplicate).ok) fail("duplicate constructor/row accepted");
  const duplicateIdentity = JSON.parse(JSON.stringify(manifest));
  duplicateIdentity.denominator.source_ids.module_call.push("module:core.test.call");
  rehash(duplicateIdentity);
  if (validateFixture(duplicateIdentity).ok) fail("duplicate denominator identity accepted");
  const staleTotal = JSON.parse(JSON.stringify(manifest));
  staleTotal.denominator.total -= 1;
  rehash(staleTotal);
  if (validateFixture(staleTotal).ok) fail("stale denominator total accepted");
  const staleSourceDigest = JSON.parse(JSON.stringify(manifest));
  staleSourceDigest.denominator.source_ids_sha256 = sha256("forged");
  rehash(staleSourceDigest);
  if (validateFixture(staleSourceDigest).ok) fail("stale source membership digest accepted");
  const observerless = JSON.parse(JSON.stringify(manifest));
  const observed = observerless.rows.find((row) => row.stable_id === "module:core.test.call");
  observed.status = "covered";
  observed.value_consuming = false;
  observed.sink = null;
  rehash(observerless);
  if (validateFixture(observerless).ok) fail("observerless value accepted");
  const forgedExclusion = JSON.parse(JSON.stringify(manifest));
  const forgedRecord = forgedExclusion.exclusions.find((entry) => entry.stable_id === "module:core.test.missing");
  const forgedRow = forgedExclusion.rows.find((row) => row.stable_id === "module:core.test.missing");
  forgedRecord.reason = "forged reason";
  forgedRow.exclusion.reason = "forged reason";
  rehash(forgedExclusion);
  if (validateFixture(forgedExclusion).ok) fail("forged exclusion accepted");
  const malformedExclusion = JSON.parse(JSON.stringify(manifest));
  malformedExclusion.exclusions.push({ stable_id: "module:core.test.call", reason: "", owner: "owner", decision: "D-2335" });
  malformedExclusion.denominator.counts.exclusions = 1;
  rehash(malformedExclusion);
  if (validateFixture(malformedExclusion).ok) fail("malformed exclusion accepted");
  const invalidExclusion = JSON.parse(JSON.stringify(manifest));
  const excluded = invalidExclusion.rows.find((row) => row.stable_id === "module:core.test.call");
  excluded.status = "excluded";
  excluded.exclusion = { reason: "reason", owner: null, decision: "D-2335" };
  excluded.value_consuming = null;
  excluded.sink = null;
  rehash(invalidExclusion);
  if (validateFixture(invalidExclusion).ok) fail("invalid exclusion accepted");
  const stale = JSON.parse(JSON.stringify(manifest));
  stale.source_snapshot.hash = sha256("changed");
  rehash(stale);
  if (validateFixture(stale, { currentSnapshotHash: manifest.source_snapshot.hash }).ok) fail("stale snapshot accepted");
  const inspectFixture = (body, fixtureKey = "core.test.open") => {
    const dot = fixtureKey.lastIndexOf(".");
    const fixtureModule = fixtureKey.slice(0, dot);
    return seedInspection(
      fixtureKey,
      [`// core-conformance: ${fixtureKey}`, `use ${fixtureModule} as api`, "", "fn run() {", ...body.split("\n").map((line) => `    ${line}`), "}"].join("\n"),
    );
  };
  for (const [name, body, fixtureKey] of [
    ["nested", `print(api.inspect(api.open("value")))`],
    ["propagation", `print(api.open("value") ?? panic("open"))`],
    ["equality", `value :: api.open("value") ?? panic("open")\nprint(value == "value")`],
    ["argument", `value :: api.open("value") ?? panic("open")\nprint(api.inspect(value))`],
    ["method", `value :: api.open("value") ?? panic("open")\nprint(value.len())`],
    ["transitive", `value :: api.open("value") ?? panic("open")\nalias :: value\nprint(alias.len())`],
    ["direct-propagation", `api.call("value") ?? panic("call")\nprint("done")`, "core.test.call"],
  ]) {
    const result = inspectFixture(body, fixtureKey);
    if (result.errors.length) fail(`accepted observer fixture rejected: ${name}`);
  }
  for (const [name, body, fixtureKey] of [
    ["discard", `_ :: api.open("value") ?? panic("open")\nprint("done")`],
    ["compile-only", `api.open("value")`],
    ["direct-discard", `api.call("value")`, "core.test.call"],
    ["unit-return-propagation", `api.call("value") ?? return Err("call")\nprint("done")`, "core.test.call"],
    ["unit-bound-return-propagation", `value :: api.open("value") ?? return Err("open")\nprint(value.len())`],
    ["unused", `value :: api.open("value") ?? panic("open")\nprint("done")`],
    ["opaque-direct", `print(api.open("value"))`],
  ]) {
    if (inspectFixture(body, fixtureKey).errors.length === 0) fail(`rejected observer fixture accepted: ${name}`);
  }
  console.log("hardening manifest hostile fixtures: PASS");
  return 0;
}

function candidateBuild(args) {
  const inputAt = args.indexOf("--candidate-build");
  const inputPath = args[inputAt + 1];
  if (!inputPath || inputPath.startsWith("--")) fail("--candidate-build requires an evidence JSON path");
  const outputAt = args.indexOf("--output");
  const outputPath = outputAt >= 0 ? args[outputAt + 1] : ".jet/candidate.json";
  if (!outputPath || outputPath.startsWith("--")) fail("--output requires a candidate JSON path");
  if (!existsSync(inputPath)) fail(`unreadable candidate evidence: ${inputPath}`);
  let input;
  try {
    input = JSON.parse(readFileSync(inputPath, "utf8"));
  } catch (error) {
    fail(`unreadable candidate evidence ${inputPath}: ${error.message}`);
  }
  const candidate = buildCandidateRecord({
    candidateId: input.candidate_id ?? input.candidate_identity?.id,
    commit: input.commit ?? input.candidate_identity?.commit,
    dependencies: input.dependencies,
    claims: input.claims,
    historicalReceipts: input.historical_receipts ?? input.historicalReceipts,
    compilerProof: input.compiler_proof ?? input.composition,
    prerelease: input.prerelease,
    status: input.status,
    evidence: input.evidence,
  });
  mkdirSync(dirname(resolve(outputPath)), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(candidate, null, 2)}\n`);
  let validation = validateCandidateRecord(candidate, {
    requireQualified: args.includes("--require-qualified"),
    requireReleaseClaim: args.includes("--require-release-claim"),
    releaseChannel: args.includes("--release-channel")
      ? args[args.indexOf("--release-channel") + 1]
      : null,
  });
  if (validation.ok) {
    const composition = verifyCandidateComposition(outputPath, { candidate });
    if (!composition.ok) {
      validation = {
        ...validation,
        ok: false,
        status: "BLOCKED",
        errors: [...validation.errors, ...composition.errors],
      };
    }
  }
  console.log(`candidate: wrote ${outputPath}; ${candidate.status.toUpperCase()}`);
  if (!validation.ok) {
    for (const error of validation.errors) console.error(`candidate: ${error}`);
    return 1;
  }
  console.log(`candidate: ${candidate.candidate_identity.id}; VALID`);
  return 0;
}

function main(args) {
  if (args.includes("--candidate-build")) return candidateBuild(args);
  if (args.includes("--candidate-negative-controls")) {
    console.log(canonicalJson(runCandidateNegativeControls()));
    return 0;
  }
  const candidateCheckAt = args.indexOf("--candidate-check");
  if (candidateCheckAt >= 0) {
    const path = args[candidateCheckAt + 1];
    if (!path || path.startsWith("--")) fail("--candidate-check requires a path");
    const releaseAt = args.indexOf("--release-channel");
    const releaseChannel = releaseAt >= 0 ? args[releaseAt + 1] : null;
    if (releaseAt >= 0 && (!releaseChannel || releaseChannel.startsWith("--"))) fail("--release-channel requires a value");
    const candidate = readCandidateRecord(path, {
      requireQualified: args.includes("--require-qualified"),
      requireReleaseClaim: args.includes("--require-release-claim"),
      releaseChannel,
    });
    console.log(`candidate: ${candidate.candidate_identity.id}; ${candidate.status.toUpperCase()}`);
    return 0;
  }
  if (checkAt >= 0) {
    const path = args[checkAt + 1];
    if (!path || path.startsWith("--")) fail("--check requires a path");
    const manifest = readManifest(path);
    const counts = Object.fromEntries(["covered", "missing", "unrouted", "excluded", "invalid", "invalid-exclusion"].map((status) => [status, manifest.rows.filter((row) => row.status === status).length]));
    console.log(`hardening manifest: ${manifest.rows.length} tagged rows; ${counts.covered} covered; ${counts.missing} missing; ${counts.unrouted} unrouted; ${counts.excluded} excluded; ${counts.invalid + counts["invalid-exclusion"]} invalid`);
    console.log(`hardening manifest: source ${manifest.source_snapshot.hash}; VALID`);
    return 0;
  }
  const manifest = buildManifest();
  const validation = validateManifest(manifest);
  if (!validation.ok) { for (const error of validation.errors) console.error(`error: ${error}`); return 1; }
  if (args.includes("--generate")) { process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`); return 0; }
  const writeAt = args.indexOf("--write");
  if (writeAt >= 0) {
    const path = args[writeAt + 1];
    if (!path) fail("--write requires a path");
    mkdirSync(dirname(resolve(path)), { recursive: true });
    writeFileSync(path, `${JSON.stringify(manifest, null, 2)}\n`);
    console.log(`hardening manifest: wrote ${path}`);
  }
  const counts = Object.fromEntries(["covered", "missing", "unrouted", "excluded", "invalid", "invalid-exclusion"].map((status) => [status, manifest.rows.filter((row) => row.status === status).length]));
  console.log(`hardening manifest: ${manifest.rows.length} tagged rows; ${counts.covered} covered; ${counts.missing} missing; ${counts.unrouted} unrouted; ${counts.excluded} excluded; ${counts.invalid + counts["invalid-exclusion"]} invalid`);
  console.log(`hardening manifest: source ${manifest.source_snapshot.hash}; VALID`);
  return 0;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try { process.exitCode = main(process.argv.slice(2)); }
  catch (error) { console.error(`error: ${error.message}`); process.exitCode = 1; }
}
