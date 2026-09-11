import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync } from "node:fs";
import { dirname, isAbsolute, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIGEST = /^sha256-[0-9a-f]{64}$/u;

export const OBSERVATION_FIELDS = Object.freeze([
  "result",
  "typed_failure",
  "mutation",
  "input_consumption",
  "ordered_effects",
  "cleanup",
  "resource_premises",
  "schedule",
  "allowed_schedules",
]);
export const PRESERVATION_FIELDS = Object.freeze([
  "origin_identity",
  "source_identity",
  "ir_identity",
  "target_identity",
  "side_conditions",
  "legality",
  "profitability",
  "target_representation",
  ...OBSERVATION_FIELDS,
]);
export const SCENARIOS = Object.freeze([
  "short_circuit",
  "early_exit",
  "typed_errors",
  "checked_arithmetic",
  "callbacks",
  "resource_cleanup",
  "input_consumption",
]);
const OBSERVATION_CERTIFICATE_SCHEMA = Object.freeze({
  lowering: "jet.lowering-observations.v1",
  optimization: "jet.optimization-observations.v1",
});
const OBSERVATION_IDENTITY_FIELDS = Object.freeze([
  "operation_id",
  "stage",
  "source_sha256",
  "compiler_source_sha256",
  "model_sha256",
  "checker_sha256",
  "configuration_sha256",
  "target_sha256",
  "obligations_sha256",
  "driver_sha256",
  "collector_sha256",
  "contract_execution_sha256",
]);
const OBSERVATION_COLLECTOR_PATH = "proof/compiler/observations/comparison-journal.mjs";
const REPLAY_ROUTES = Object.freeze(["aot", "jet_run", "interpreter", "check"]);
const RUNTIME_ROUTES = Object.freeze(["aot", "jet_run", "interpreter"]);
const OBSERVER_PATHS = Object.freeze({
  lowering: "proof/compiler/lowering/observe.mjs",
  optimization: "proof/compiler/optimization/observe.mjs",
});
const ROUTE_PROTOCOL = "canonical-tir-mir-route-v1";
const ROUTE_SCHEMA = "jet.canonical-tir-mir-route.v1";
const FORMAL_DISCHARGE_SCHEMA = Object.freeze({
  lowering: "jet.lowering-formal-discharge.v1",
  optimization: "jet.optimization-formal-discharge.v1",
});
const FORMAL_DISCHARGE_PATHS = Object.freeze({
  lowering: "proof/compiler/lowering/discharge.json",
  optimization: "proof/compiler/optimization/discharge.json",
});
const STAGE_OPERATION_COUNTS = Object.freeze({ lowering: 21, optimization: 15 });
export const TARGETS = Object.freeze(["scalar", "vector", "parallel", "layout"]);
export const NEGATIVE_CONTROL_KINDS = Object.freeze([
  "reordered_effects",
  "reordered_failure",
  "extra_input_consumption",
  "invalid_alias",
  "unsound_arithmetic_reassociation",
  "wrong_operation_certificate",
  "illegal_cost_selection",
]);
const STAGE_BOUNDARY = Object.freeze({
  lowering: "boundary:lowering",
  optimization: "boundary:optimization",
});
const CANONICAL_PREFIXES = Object.freeze(["tir-expr:", "tir-stmt:"]);

export class ContractError extends Error {
  constructor(code, message, details = undefined) {
    super(message);
    this.name = "ContractError";
    this.code = code;
    this.details = details;
  }
}

function fail(code, message, details = undefined) {
  throw new ContractError(code, message, details);
}

function text(value, label) {
  if (typeof value !== "string" || value.length === 0 || /[\u0000-\u001f\u007f]/u.test(value)) {
    fail("invalid_oracle", `${label} must be non-empty text`);
  }
  return value;
}

function object(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    fail("invalid_oracle", `${label} must be an object`);
  }
  return value;
}

function array(value, label) {
  if (!Array.isArray(value)) fail("invalid_oracle", `${label} must be an array`);
  return value;
}

function digest(value, label) {
  text(value, label);
  if (!DIGEST.test(value)) fail("invalid_oracle", `${label} must be a sha256-<hex> digest`);
  return value;
}

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    const result = {};
    for (const key of Object.keys(value).sort()) result[key] = canonical(value[key]);
    return result;
  }
  return value;
}

function canonicalJson(value) {
  return JSON.stringify(canonical(value));
}

function digestBytes(value) {
  return `sha256-${createHash("sha256").update(value).digest("hex")}`;
}

function digestText(value) {
  return digestBytes(Buffer.from(value, "utf8"));
}

function projectPath(value, label) {
  text(value, label);
  if (isAbsolute(value) || value.includes("\\") || value.includes("\0")) {
    fail("invalid_oracle", `${label} must be a project-relative path`);
  }
  const absolute = resolve(ROOT, value);
  const prefix = `${ROOT}${sep}`;
  if (absolute !== ROOT && !absolute.startsWith(prefix)) fail("invalid_oracle", `${label} escapes project root`);
  return absolute;
}

function regularFile(value, label) {
  const path = projectPath(value, label);
  let stat;
  try {
    stat = lstatSync(path);
  } catch (error) {
    fail("unavailable", `${label} is missing: ${error.message}`);
  }
  if (stat.isSymbolicLink() || !stat.isFile()) fail("invalid_oracle", `${label} is not a regular file`);
  return path;
}

function readJson(value, label) {
  const path = regularFile(value, label);
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail("invalid_oracle", `${label} is not valid JSON: ${error.message}`);
  }
}

function readSource(value, label) {
  const path = regularFile(value, label);
  try {
    return { path, text: readFileSync(path, "utf8"), sha256: digestBytes(readFileSync(path)) };
  } catch (error) {
    fail("unavailable", `${label} cannot be read: ${error.message}`);
  }
}

function hasSymbol(source, symbol) {
  const escaped = symbol.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
  return [
    new RegExp(`\\b(?:pub(?:\\([^)]*\\))?\\s+)?(?:async\\s+)?fn\\s+${escaped}\\b`, "u"),
    new RegExp(`\\b(?:struct|enum|trait|type|const|static)\\s+${escaped}\\b`, "u"),
    new RegExp(`\\b${escaped}\\b`, "u"),
  ].some((pattern) => pattern.test(source));
}

const LOWERING_OPERATIONS = Object.freeze([
  ["sema.desugar-os-switches", "desugaring", "crates/jet-sema/src/Sema/OSTarget.rs", "desugar_os_switches"],
  ["sema.desugar-migrations", "desugaring", "crates/jet-sema/src/Sema/SchemaMigration.rs", "desugar_migrations"],
  ["sema.desugar-member-spreads", "desugaring", "crates/jet-sema/src/Sema/MemberSpread.rs", "desugar_member_spreads"],
  ["sema.desugar-multi-head-functions", "desugaring", "crates/jet-sema/src/Sema/Bundle.rs", "desugar_multi_head_functions"],
  ["sema.normalize-operator-rhs", "normalization", "crates/jet-sema/src/Sema/Registration.rs", "normalize_operator_rhs"],
  ["tir.lower-checked-program", "representation", "crates/jet-codegen/src/Codegen/TIR/mod.rs", "lower_checked_tir_program_for"],
  ["tir.canonicalize-loop-forms", "normalization", "crates/jet-codegen/src/Codegen/TIR/opt.rs", "canonicalize_loop_forms"],
  ["tir.optimize-program", "normalization", "crates/jet-codegen/src/Codegen/TIR/opt.rs", "optimize_program"],
  ["tir.lower-value-block", "control_flow", "crates/jet-codegen/src/Codegen/TIR/lower/control_flow.rs", "lower_value_block"],
  ["tir.lower-statements", "control_flow", "crates/jet-codegen/src/Codegen/TIR/lower/statements.rs", "lower_stmts"],
  ["tir.lower-expression", "control_flow", "crates/jet-codegen/src/Codegen/TIR/lower/expressions.rs", "lower_expr"],
  ["tir.lower-function", "representation", "crates/jet-codegen/src/Codegen/TIR/lower/functions.rs", "lower_func"],
  ["tir.lower-lambda", "representation", "crates/jet-codegen/src/Codegen/TIR/lower/lambdas.rs", "lower_lambda"],
  ["mir.lower-expression", "control_flow", "crates/jet-codegen/src/Codegen/TIR/tir_to_mir_expr.rs", "lower_expr"],
  ["mir.lower-statements", "control_flow", "crates/jet-codegen/src/Codegen/TIR/tir_to_mir_stmt.rs", "lower_stmts"],
  ["mir.lower-function", "representation", "crates/jet-codegen/src/Codegen/TIR/mir.rs", "lower_function"],
  ["mir.lower-type", "representation", "crates/jet-codegen/src/Codegen/TIR/tir_to_mir_types.rs", "lower_type"],
  ["mir.lower-checked-program", "representation", "crates/jet-codegen/src/Codegen/TIR/mir.rs", "lower_checked_mir_program_for"],
  ["mir.lower-tir-to-mir", "representation", "crates/jet-codegen/src/Codegen/TIR/mir.rs", "lower_tir_to_mir"],
  ["mir.canonical-operation-model", "representation", "crates/jet-foundation/src/MIR.rs", "MirOperation"],
  ["mir.rust-emitter", "representation", "crates/jet-codegen/src/Codegen/MIRRust.rs", "emit_mir_program_into"],
]);

const OPTIMIZATION_OPERATIONS = Object.freeze([
  ["mir.optimize-pipeline", "pipeline", "crates/jet-foundation/src/MIROptimization.rs", "optimize_mir_program"],
  ["mir.legality-verification", "legality", "crates/jet-foundation/src/MIROptimization.rs", "verify_mir_legality"],
  ["mir.require-canonical-optimization", "legality", "crates/jet-foundation/src/MIROptimization.rs", "require_canonical_mir_optimization"],
  ["mir.pass-order-facts", "legality", "crates/jet-foundation/src/MIR.rs", "MirOptimizationPassId"],
  ["mir.inline-always", "normalization", "crates/jet-foundation/src/MIROptimization.rs", "expand_inline_always"],
  ["mir.unreachable-block-elimination", "control_flow", "crates/jet-foundation/src/MIROptimization.rs", "eliminate_unreachable_blocks"],
  ["mir.cfg-simplification", "control_flow", "crates/jet-foundation/src/MIROptimization.rs", "simplify_cfg"],
  ["mir.exact-constant-folding", "normalization", "crates/jet-foundation/src/MIROptimization.rs", "fold_exact_constants"],
  ["mir.bounds-check-elimination", "representation", "crates/jet-foundation/src/MIROptimization.rs", "derive_bounds_facts"],
  ["mir.dead-pure-value-elimination", "normalization", "crates/jet-foundation/src/MIROptimization.rs", "eliminate_dead_pure_values"],
  ["mir.canonical-loop-facts", "normalization", "crates/jet-foundation/src/MIROptimization.rs", "derive_loop_and_vector_facts"],
  ["mir.fixed-reduction-normalization", "normalization", "crates/jet-foundation/src/MIROptimization.rs", "normalize_fixed_reduction_loops"],
  ["mir.acceleration-facts", "profitability", "crates/jet-foundation/src/MIROptimization.rs", "refresh_acceleration_facts"],
  ["mir.canonical-program-order", "representation", "crates/jet-foundation/src/MIROptimization.rs", "canonicalize_program_order"],
  ["mir.acceleration-gate", "profitability", "crates/jet-foundation/src/MIROptimization/Acceleration.rs", "AccelerationGateInput"],
]);


const BASE_PREMISES = Object.freeze([
  "source_origin_is_retained",
  "typed_failure_is_retained",
  "mutation_is_retained",
  "input_consumption_is_exact",
  "ordered_effects_are_retained",
  "cleanup_edges_are_retained",
  "resource_premises_are_explicit",
  "target_applicability_is_checked",
]);

function registry(stage) {
  const rows = stage === "lowering" ? LOWERING_OPERATIONS : OPTIMIZATION_OPERATIONS;
  return rows.map(([id, category, source, symbol]) => ({
    id,
    stage,
    category,
    source,
    source_sha256: readSource(source, `${stage} operation ${id}`).sha256,
    symbol,
    required_observations: [...OBSERVATION_FIELDS],
    required_scenarios: [...SCENARIOS],
    premises: [...BASE_PREMISES],
    canonical_operations: [
      "tir-expr:TExprKind.Binary",
      "tir-expr:TExprKind.Call",
      "tir-stmt:TStmt.Return",
    ],
    origin: {
      kind: "source-span",
      identity: "source-origin-retained-through-shared-route",
    },
    side_conditions: [
      "source_order_preserved",
      "failure_order_preserved",
      "alias_and_ownership_premises_preserved",
      "target_and_layout_premises_preserved",
    ],
    legality: {
      route: stage === "optimization" ? "MirOptimization::verify_mir_legality" : "checked-sema-tir-mir-route",
      required: true,
      status: "unverified",
      cost_cannot_legalize: true,
    },
    profitability: {
      separate: true,
      route: stage === "optimization" ? "AccelerationGateInput-after-legality" : "not-a-legality-decision",
      status: "unverified",
      cannot_override_legality: true,
    },
    target_contract: {
      choices: [...TARGETS],
      numerical_contract: "same-source-numerical-contract",
      failure_contract: "same-source-failure-contract",
      layout_contract: "target-facts-bound",
    },
    rejection_route: "shared-semantic-route-preserves-invalidity",
  }));
}

export function operationRegistry(stage) {
  if (!Object.hasOwn(STAGE_BOUNDARY, stage)) fail("invalid_oracle", `unknown compiler proof stage: ${stage}`);
  return registry(stage);
}

function checkContract(contract, stage) {
  object(contract, `${stage} contract`);
  if (contract.schema !== `jet.${stage}-contract.v1`) fail("invalid_oracle", `${stage} contract schema mismatch`);
  if (contract.schema_version !== 1 || contract.contract_id !== `jet-${stage}-contract-v1`) {
    fail("invalid_oracle", `${stage} contract identity mismatch`);
  }
  object(contract.policy, `${stage} contract.policy`);
  if (contract.policy.decision_id !== "D-COMPILER-PROOF1" || contract.policy.status !== "ratified") {
    fail("invalid_oracle", `${stage} contract policy is not D-COMPILER-PROOF1=ratified`);
  }
  object(contract.claim, `${stage} contract.claim`);
  if (contract.claim.kind !== "implementation-bound-conditional-preservation") {
    fail("invalid_oracle", `${stage} contract claim kind mismatch`);
  }
  if (JSON.stringify(contract.claim.observation_alphabet) !== JSON.stringify(OBSERVATION_FIELDS)) {
    fail("invalid_oracle", `${stage} contract observation alphabet mismatch`);
  }
  if (JSON.stringify(contract.claim.preservation_dimensions) !== JSON.stringify(PRESERVATION_FIELDS)) {
    fail("invalid_oracle", `${stage} contract preservation dimensions mismatch`);
  }
  if (!SCENARIOS.every((scenario) => contract.claim.required_scenarios?.includes(scenario))) {
    fail("invalid_oracle", `${stage} contract omits required edge scenarios`);
  }
  const identity = object(contract.identity, `${stage} contract.identity`);
  if (identity.authority_key !== `compiler-proof.${stage}`) {
    fail("invalid_oracle", `${stage} contract replay authority key mismatch`);
  }
  object(contract.execution, `${stage} contract.execution`);
  if (
    contract.execution.schema !== "jet.compiler-inspection.v1"
    || contract.execution.runner !== "scripts/agent/jet-env"
    || contract.execution.compiler !== "target/debug/jet"
    || contract.execution.command !== `scripts/agent/jet-env full node proof/compiler/${stage}/observe.mjs --json`
  ) {
    fail("invalid_oracle", `${stage} contract execution identity mismatch`);
  }
  if (contract.execution.unknown_status !== "unverified" || contract.execution.no_execution_in_checker !== true) {
    fail("invalid_oracle", `${stage} contract must fail closed on unavailable observations`);
  }
  const observationCertificates = object(contract.execution.observation_certificates, `${stage} contract.execution.observation_certificates`);
  if (
    observationCertificates.path !== `proof/compiler/${stage}/observations.json`
    || observationCertificates.schema !== OBSERVATION_CERTIFICATE_SCHEMA[stage]
    || observationCertificates.producer !== "compiler-proof observation workflow"
    || observationCertificates.observer !== OBSERVER_PATHS[stage]
    || observationCertificates.protocol !== ROUTE_PROTOCOL
    || JSON.stringify(observationCertificates.required_identity_fields) !== JSON.stringify(OBSERVATION_IDENTITY_FIELDS)
    || JSON.stringify(observationCertificates.required_observations) !== JSON.stringify(OBSERVATION_FIELDS)
    || observationCertificates.positive_status !== "matched"
    || observationCertificates.universal_proof !== false
  ) {
    fail("invalid_oracle", `${stage} observation certificate carrier binding drifted`);
  }
  object(contract.inventory_join, `${stage} contract.inventory_join`);
  if (JSON.stringify(contract.inventory_join.authorities) !== JSON.stringify(["#2898", "#2919"])) {
    fail("invalid_oracle", `${stage} contract must join #2898 and #2919`);
  }
  object(contract.coverage, `${stage} contract.coverage`);
  if (contract.coverage.relation_path !== "proof/compiler/obligations.json" || contract.coverage.owner !== "#2939") {
    fail("invalid_oracle", `${stage} contract coverage authority mismatch`);
  }
  if (JSON.stringify(contract.coverage.boundary_ids) !== JSON.stringify([STAGE_BOUNDARY[stage]])) {
    fail("invalid_oracle", `${stage} contract boundary mismatch`);
  }
  if (!Array.isArray(contract.boundary?.blocked) || contract.boundary.qualification !== "blocked") {
    fail("invalid_oracle", `${stage} contract must keep qualification blocked`);
  }
  return contract;
}

function checkBinding(witness, stage) {
  object(witness.binding, `${stage} witness.binding`);
  for (const key of ["contract", "checker", "driver", "obligations", "model"]) {
    const ref = object(witness.binding[key], `${stage} witness.binding.${key}`);
    const source = readSource(ref.path, `${stage} witness.binding.${key}`);
    if (ref.sha256 !== source.sha256) fail("mismatch", `${stage} witness binding changed: ${ref.path}`);
  }
  array(witness.binding.authorities, `${stage} witness.binding.authorities`);
  for (const ref of witness.binding.authorities) {
    const source = readSource(ref.path, `${stage} witness authority`);
    if (ref.sha256 !== source.sha256) fail("mismatch", `${stage} witness authority changed: ${ref.path}`);
  }
}

function checkIdentity(witness, stage) {
  array(witness.identity_fields, `${stage} witness.identity_fields`);
  const required = [
    "model_sha256",
    "source_sha256",
    "compiler_source_sha256",
    "checker_sha256",
    "configuration_sha256",
    "target_sha256",
    "obligations_sha256",
  ];
  const observationCertificates = object(witness.observation_certificates, `${stage} witness.observation_certificates`);
  if (
    observationCertificates.path !== `proof/compiler/${stage}/observations.json`
    || observationCertificates.schema !== OBSERVATION_CERTIFICATE_SCHEMA[stage]
    || observationCertificates.producer !== "compiler-proof observation workflow"
    || observationCertificates.observer !== OBSERVER_PATHS[stage]
    || observationCertificates.protocol !== ROUTE_PROTOCOL
    || observationCertificates.status !== "unverified"
    || observationCertificates.universal_proof !== false
  ) {
    fail("invalid_oracle", `${stage} witness observation certificate carrier binding drifted`);
  }
  for (const field of required) if (!witness.identity_fields.includes(field)) fail("invalid_oracle", `${stage} identity omits ${field}`);
  object(witness.identity, `${stage} witness.identity`);
  for (const key of ["model", "source", "compiler", "checker", "configuration", "target", "obligations"]) {
    object(witness.identity[key], `${stage} witness.identity.${key}`);
    text(witness.identity[key].meaning, `${stage} witness.identity.${key}.meaning`);
  }
  object(witness.runner, `${stage} witness.runner`);
  if (witness.runner.launcher !== "scripts/agent/jet-env" || witness.runner.compiler !== "target/debug/jet") {
    fail("invalid_oracle", `${stage} witness runner mismatch`);
  }
  if (JSON.stringify(witness.runner.tiers) !== JSON.stringify(["aot", "jet_run", "interpreter", "check"])) {
    fail("invalid_oracle", `${stage} witness tiers mismatch`);
  }
  if (witness.runner.execution_requested !== false || witness.runner.unknown_policy !== "unverified") {
    fail("invalid_oracle", `${stage} witness cannot request execution or promote unknown observations`);
  }
}

function checkCoverage(witness, stage, obligations) {
  object(witness.generated_relation, `${stage} witness.generated_relation`);
  if (witness.generated_relation.path !== "proof/compiler/obligations.json") {
    fail("invalid_oracle", `${stage} generated relation path mismatch`);
  }
  if (JSON.stringify(witness.generated_relation.prefixes) !== JSON.stringify(CANONICAL_PREFIXES)) {
    fail("invalid_oracle", `${stage} generated relation selection mismatch`);
  }
  const rows = array(obligations.rows, "compiler obligations.rows");
  if (obligations.row_count !== rows.length) fail("mismatch", "compiler obligation row_count is stale");
  const boundary = rows.filter((row) => row.owner === "#2939" && row.id === STAGE_BOUNDARY[stage]);
  if (boundary.length !== 1) fail("mismatch", `${stage} requires exactly one #2939 boundary row`);
  object(witness.generated_relation.mapping, `${stage} generated relation mapping`);
  if (witness.generated_relation.mapping.default_status !== "mapped_unproved" || witness.generated_relation.mapping.qualification !== "blocked") {
    fail("invalid_oracle", `${stage} generated relation may not promote unmapped rows`);
  }
  const selected = rows.filter((row) => CANONICAL_PREFIXES.some((prefix) => row.id.startsWith(prefix)));
  if (selected.length === 0) fail("unavailable", `${stage} canonical operation denominator is empty`);
  const ids = selected.map((row) => row.id);
  const relationDigest = digestText(ids.join("\n"));
  if (witness.generated_relation.id_sha256 !== relationDigest) {
    fail("mismatch", `${stage} canonical operation denominator changed`, {
      recorded: witness.generated_relation.id_sha256,
      observed: relationDigest,
      row_count: selected.length,
    });
  }
  for (const id of witness.generated_relation.required_ids ?? []) {
    if (!ids.includes(id)) fail("invalid_oracle", `${stage} certificate names unknown canonical operation ${id}`);
  }
  return {
    boundary_id: boundary[0].id,
    row_count: selected.length,
    id_sha256: relationDigest,
    row_ids: ids,
    global_row_count: rows.length,
  };
}

function materializeCertificate(cert, witness, stage) {
  object(witness.certificate_defaults, `${stage} witness.certificate_defaults`);
  return {
    ...witness.certificate_defaults,
    ...cert,
    origin: { ...witness.certificate_defaults.origin, ...cert.origin },
    legality: { ...witness.certificate_defaults.legality, ...cert.legality },
    profitability: { ...witness.certificate_defaults.profitability, ...cert.profitability },
    target_contract: { ...witness.certificate_defaults.target_contract, ...cert.target_contract },
  };
}

function checkCertificate(cert, expected, canonicalIds, stage) {
  object(cert, `${stage} certificate`);
  if (cert.operation_id !== expected.id || cert.stage !== stage) fail("invalid_oracle", `${stage} certificate operation binding mismatch`);
  if (cert.source !== expected.source || cert.symbol !== expected.symbol) fail("invalid_oracle", `${stage} certificate implementation binding mismatch`);
  const implementation = readSource(cert.source, `${stage} certificate ${cert.operation_id}`);
  if (cert.source_sha256 !== implementation.sha256) {
    fail("mismatch", `${stage} source identity changed for ${cert.operation_id}`);
  }
  object(cert.origin, `${stage} certificate.origin`);
  if (cert.origin.identity !== expected.origin.identity) fail("invalid_oracle", `${stage} origin identity was not retained`);
  if (cert.source_identity !== "source-origin-retained" || cert.ir_identity !== "canonical-tir-mir-operation" || cert.target_identity !== "target-facts-bound") {
    fail("invalid_oracle", `${stage} source/IR/target identity was not retained`);
  }
  object(cert.legality, `${stage} certificate.legality`);
  if (cert.legality.required !== true || cert.legality.status !== "unverified" || cert.legality.cost_cannot_legalize !== true) {
    fail("invalid_oracle", `${stage} legality/profitability separation is missing`);
  }
  object(cert.profitability, `${stage} certificate.profitability`);
  if (cert.profitability.separate !== true || cert.profitability.status !== "unverified" || cert.profitability.cannot_override_legality !== true) {
    fail("invalid_oracle", `${stage} profitability is allowed to override legality`);
  }
  if (cert.rejection_route !== expected.rejection_route) fail("invalid_oracle", `${stage} rejection route does not preserve invalidity`);
  if (cert.effects_order !== "preserve" || cert.failure_order !== "preserve" || cert.input_consumption !== "exact" || cert.alias_policy !== "preserve-or-prove" || cert.numeric_rule !== "checked") {
    fail("invalid_oracle", `${stage} certificate drops an observable side condition`);
  }
  object(cert.target_contract, `${stage} certificate.target_contract`);
  if (JSON.stringify(cert.target_contract.choices) !== JSON.stringify(TARGETS)) fail("invalid_oracle", `${stage} target contract mismatch`);
  if (cert.target_contract.numerical_contract !== "same-source-numerical-contract" || cert.target_contract.failure_contract !== "same-source-failure-contract") {
    fail("invalid_oracle", `${stage} target numerical/failure contract mismatch`);
  }
}

function checkComposition(witness, stage, expected, canonicalIds) {
  object(witness.composition, `${stage} witness.composition`);
  const ordered = array(witness.composition.ordered_operations, `${stage} composition order`);
  const expectedIds = expected.map((row) => row.id);
  if (JSON.stringify(ordered) !== JSON.stringify(expectedIds)) fail("invalid_oracle", `${stage} composition does not cover the generated pass relation in order`);
  const preserves = array(witness.composition.preserves, `${stage} composition preserves`);
  for (const field of PRESERVATION_FIELDS) if (!preserves.includes(field)) fail("invalid_oracle", `${stage} composition drops ${field}`);
  if (witness.composition.status !== "unverified" || witness.composition.engine_specific_lowerer !== false) {
    fail("invalid_oracle", `${stage} composition status/route mismatch`);
  }
  for (const id of ordered) if (!canonicalIds.size || !id) fail("invalid_oracle", `${stage} composition has empty operation identity`);
}

function checkWitnessSources(witness, stage, expectedIds) {
  const sources = array(witness.witness_sources, `${stage} witness_sources`);
  const kinds = new Set();
  for (const row of sources) {
    object(row, `${stage} witness source`);
    text(row.id, `${stage} witness source.id`);
    if (row.stage !== stage || !["legal", "invalid"].includes(row.kind)) fail("invalid_oracle", `${stage} witness source kind mismatch`);
    kinds.add(row.kind);
    const source = readSource(row.path, `${stage} witness source ${row.id}`);
    if (row.source_sha256 !== source.sha256) fail("mismatch", `${stage} witness source changed: ${row.path}`);
    if (!array(row.operation_ids, `${stage} witness source operation_ids`).every((id) => expectedIds.has(id))) {
      fail("invalid_oracle", `${stage} witness source names unknown pass`);
    }
    if (row.expected_status !== (row.kind === "legal" ? "preserve" : "reject")) fail("invalid_oracle", `${stage} witness source expected status mismatch`);
  }
  if (!kinds.has("legal") || !kinds.has("invalid")) fail("invalid_oracle", `${stage} requires legal and invalid witness sources`);
}

function checkNegativeControls(witness, stage, expected, canonicalIds) {
  const controls = array(witness.negative_controls, `${stage} negative_controls`);
  const kinds = new Set();
  for (const control of controls) {
    object(control, `${stage} negative control`);
    if (!NEGATIVE_CONTROL_KINDS.includes(control.kind)) fail("invalid_oracle", `${stage} unknown negative control ${control.kind}`);
    kinds.add(control.kind);
    if (control.expected_status !== "reject" || control.checked_without_execution !== true) fail("invalid_oracle", `${stage} negative control is not fail-closed`);
    const target = expected.find((row) => row.id === control.operation_id);
    if (!target) fail("invalid_oracle", `${stage} negative control targets unknown pass`);
    const cert = witness.certificates.find((row) => row.operation_id === control.operation_id);
    if (!cert) fail("invalid_oracle", `${stage} negative control has no certificate target`);
    const mutated = JSON.parse(JSON.stringify(materializeCertificate(cert, witness, stage)));
    switch (control.kind) {
      case "reordered_effects":
        mutated.effects_order = "reversed";
        break;
      case "reordered_failure":
        mutated.failure_order = "reversed";
        break;
      case "extra_input_consumption":
        mutated.input_consumption = "additional";
        break;
      case "invalid_alias":
        mutated.alias_policy = "assumed-no-alias";
        break;
      case "unsound_arithmetic_reassociation":
        mutated.numeric_rule = "reassociated-without-proof";
        break;
      case "wrong_operation_certificate":
        mutated.operation_id = expected.find((row) => row.id !== target.id)?.id ?? "wrong-operation";
        break;
      case "illegal_cost_selection":
        mutated.profitability.cannot_override_legality = false;
        break;
    }
    let accepted = true;
    try {
      checkCertificate(mutated, target, canonicalIds, stage);
    } catch {
      accepted = false;
    }
    if (accepted) fail("invalid_oracle", `${stage} negative control did not reject ${control.kind}`);
  }
  for (const kind of NEGATIVE_CONTROL_KINDS) if (!kinds.has(kind)) fail("invalid_oracle", `${stage} omits negative control ${kind}`);
  return NEGATIVE_CONTROL_KINDS.map((kind) => ({ kind, status: "rejected" }));
}
const NULLABLE_OBSERVATION_FIELDS = new Set(["typed_failure", "mutation"]);

function hasObservation(value, key) {
  return value && typeof value === "object" && Object.hasOwn(value, key)
    && (value[key] !== null || NULLABLE_OBSERVATION_FIELDS.has(key))
    && value[key] !== undefined;
}

function rawObservation(value, label, nullable = false) {
  if (value === undefined || (value === null && !nullable)) fail("invalid_oracle", `${label} must retain a raw observation`);
  if (value === null) return null;
  if (value && typeof value === "object" && !Array.isArray(value)) {
    if (Object.hasOwn(value, "raw")) {
      if (value.raw === undefined || (value.raw === null && !nullable)) fail("invalid_oracle", `${label}.raw must be observed`);
      return value.raw;
    }
    if (Object.hasOwn(value, "value")
      && (Object.hasOwn(value, "status") || Object.hasOwn(value, "artifact") || Object.hasOwn(value, "observation"))) {
      if (value.value === undefined || (value.value === null && !nullable)) fail("invalid_oracle", `${label}.value must be observed`);
      return value.value;
    }
    const keys = Object.keys(value);
    if (keys.length > 0
      && keys.every((key) => ["status", "artifact", "reason", "source"].includes(key))
      && ["observed", "matched", "unverified", "failed", "unavailable"].includes(value.status)) {
      fail("invalid_oracle", `${label} contains only a marker/status and no raw observation`);
    }
  }
  return value;
}

function sameCanonical(left, right) {
  return canonicalJson(left) === canonicalJson(right);
}

function checkCanonicalPass(pass, expected, stage, mode) {
  object(pass, `${expected.id}@${mode}.pass`);
  if (pass.schema !== "jet.canonical-pass-record.v1"
    || pass.protocol !== "jet.canonical-pass.v1"
    || pass.stage !== stage
    || pass.operation_id !== expected.id
    || !Number.isInteger(pass.occurrence)
    || pass.occurrence < 1
    || !Number.isInteger(pass.order)
    || pass.order < 1) {
    fail("invalid_oracle", `${expected.id}@${mode} canonical pass identity is invalid`);
  }
  const source = object(pass.source, `${expected.id}@${mode}.pass.source`);
  if (source.path !== expected.source) fail("mismatch", `${expected.id}@${mode} canonical pass source identity is stale`);
  if (source.sha256 !== undefined && source.sha256 !== expected.source_sha256) {
    fail("mismatch", `${expected.id}@${mode} canonical pass source digest is stale`);
  }
  for (const [side, allowed] of [["input", ["ast", "tir", "mir"]], ["output", ["ast", "tir", "mir"]]]) {
    const snapshot = object(pass[side], `${expected.id}@${mode}.pass.${side}`);
    if (!allowed.includes(snapshot.representation)) {
      fail("invalid_oracle", `${expected.id}@${mode}.pass.${side} representation is invalid`);
    }
    object(snapshot.canonical_payload, `${expected.id}@${mode}.pass.${side}.canonical_payload`);
    object(snapshot.identity, `${expected.id}@${mode}.pass.${side}.identity`);
  }
  const premises = array(pass.premises, `${expected.id}@${mode}.pass.premises`);
  if (JSON.stringify(premises) !== JSON.stringify(BASE_PREMISES)) {
    fail("invalid_oracle", `${expected.id}@${mode}.pass premise order/content drifted`);
  }
  if (!["preserve", "checked", "reject"].includes(pass.disposition)) {
    fail("invalid_oracle", `${expected.id}@${mode}.pass disposition is invalid`);
  }
  return pass;
}

function checkCanonicalRouteRecord(record, expected, _expectedIds, stage) {
  const routes = object(record.routes, `${expected.id}.routes`);
  const observations = object(record.observations, `${expected.id}.observations`);
  const observedModes = [];
  const missingModes = [];
  const observedDimensions = {};
  const missingDimensions = {};
  const checkedRoutes = {};
  const checkedObservations = {};
  const suppliedModes = Object.keys(routes);
  for (const mode of suppliedModes) if (!REPLAY_ROUTES.includes(mode)) {
    fail("invalid_oracle", `${expected.id} names unsupported canonical route mode ${mode}`);
  }
  for (const mode of REPLAY_ROUTES) {
    const route = routes[mode];
    const modeObservations = observations[mode];
    if (route === undefined || modeObservations === undefined) {
      missingModes.push(mode);
      missingDimensions[mode] = [...OBSERVATION_FIELDS];
      continue;
    }
    object(route, `${expected.id}@${mode}.route`);
    if (route.schema !== ROUTE_SCHEMA
      || route.protocol !== ROUTE_PROTOCOL
      || route.stage !== stage
      || route.mode !== mode
      || route.operation_id !== expected.id) {
      fail("invalid_oracle", `${expected.id}@${mode} canonical route identity is incomplete`);
    }
    const passes = array(route.passes, `${expected.id}@${mode}.route.passes`);
    let previousOrder = 0;
    const occurrences = new Set();
    for (const pass of passes) {
      const checked = checkCanonicalPass(pass, expected, stage, mode);
      if (checked.order <= previousOrder || occurrences.has(checked.occurrence)) {
        fail("invalid_oracle", `${expected.id}@${mode} canonical pass order/occurrence is not monotonic`);
      }
      previousOrder = checked.order;
      occurrences.add(checked.occurrence);
    }
    if (route.observations !== undefined || route.tir !== undefined || route.mir !== undefined) {
      fail("invalid_oracle", `${expected.id}@${mode} static route contains runtime observations`);
    }
    if (route.coverage !== undefined) {
      const coverage = object(route.coverage, `${expected.id}@${mode}.route.coverage`);
      if (coverage.occurrence_count !== passes.length
        || coverage.observed !== (passes.length > 0)) {
        fail("invalid_oracle", `${expected.id}@${mode} route coverage does not match pass records`);
      }
    }
    object(modeObservations, `${expected.id}@${mode}.raw_observations`);
    const dimensions = [];
    const missing = [];
    for (const field of OBSERVATION_FIELDS) {
      const present = hasObservation(modeObservations, field);
      if (!present) {
        missing.push(field);
        continue;
      }
      const nullable = NULLABLE_OBSERVATION_FIELDS.has(field);
      const raw = rawObservation(modeObservations[field], `${expected.id}@${mode}.observations.${field}`, nullable);
      if (modeObservations[field] && typeof modeObservations[field] === "object"
        && !Array.isArray(modeObservations[field]) && modeObservations[field].artifact !== undefined) {
        observationArtifact(modeObservations[field].artifact, `${expected.id}@${mode}.observations.${field}.artifact`);
      }
      void raw;
      dimensions.push(field);
    }
    checkedRoutes[mode] = route;
    checkedObservations[mode] = modeObservations;
    if (dimensions.length === 0) {
      missingModes.push(mode);
      missingDimensions[mode] = [...OBSERVATION_FIELDS];
      continue;
    }
    observedModes.push(mode);
    observedDimensions[mode] = dimensions;
    missingDimensions[mode] = missing;
  }
  for (const mode of Object.keys(observations)) if (!REPLAY_ROUTES.includes(mode)) {
    fail("invalid_oracle", `${expected.id} observations name unsupported route mode ${mode}`);
  }
  return {
    routes: checkedRoutes,
    observations: checkedObservations,
    observed_modes: observedModes,
    missing_modes: missingModes,
    observed_dimensions: observedDimensions,
    missing_dimensions: missingDimensions,
    coverage: {
      observed_modes: observedModes,
      missing_modes: missingModes,
      pass_modes: REPLAY_ROUTES.filter((mode) => checkedRoutes[mode]?.passes?.length > 0),
      missing_pass_modes: REPLAY_ROUTES.filter((mode) => !(checkedRoutes[mode]?.passes?.length > 0)),
    },
    matched: Boolean(
      routes.check
      && observations.check
      && routes.check.passes?.length > 0,
    ) && RUNTIME_ROUTES.every((mode) => observedDimensions[mode]?.length === OBSERVATION_FIELDS.length
      && checkedRoutes[mode]?.passes?.length > 0),
  };
}

function observationArtifact(ref, label) {
  object(ref, label);
  text(ref.path, `${label}.path`);
  digest(ref.sha256, `${label}.sha256`);
  const source = readSource(ref.path, label);
  if (source.sha256 !== ref.sha256) fail("mismatch", `${label} digest changed`);
  return source.sha256;
}

function expectedObservationIdentity(witness, contract, expected, stage) {
  const source = readSource(expected.source, `${stage} observation source ${expected.id}`);
  const sourceCertificate = witness.certificates.find((candidate) => candidate.operation_id === expected.id);
  const materialized = materializeCertificate(sourceCertificate, witness, stage);
  return {
    operation_id: expected.id,
    stage,
    source_sha256: source.sha256,
    compiler_source_sha256: digestText(canonicalJson(witness.binding.authorities)),
    model_sha256: witness.binding.model.sha256,
    checker_sha256: witness.binding.checker.sha256,
    configuration_sha256: witness.binding.contract.sha256,
    target_sha256: digestText(canonicalJson(materialized.target_contract)),
    obligations_sha256: witness.binding.obligations.sha256,
    driver_sha256: witness.binding.driver.sha256,
    collector_sha256: readSource(OBSERVATION_COLLECTOR_PATH, `${stage} observation collector`).sha256,
    contract_execution_sha256: digestText(canonicalJson(contract.execution)),
  };
}

function checkObservationCertificates(witness, contract, expected, expectedIds, stage) {
  const path = `proof/compiler/${stage}/observations.json`;
  const descriptor = {
    path,
    schema: OBSERVATION_CERTIFICATE_SCHEMA[stage],
    universal_proof: false,
  };
  if (!existsSync(projectPath(path, `${stage} observation certificates`))) {
    return {
      ...descriptor,
      status: "unverified",
      records: [],
      matched_keys: [],
      reason: "no compiler-owned canonical pass records were supplied by the collector",
    };
  }
  const payload = readJson(path, `${stage} observation certificates`);
  if (payload.schema !== descriptor.schema || payload.schema_version !== 1 || payload.stage !== stage) {
    fail("invalid_oracle", `${stage} observation certificate schema/version/stage is invalid`);
  }
  if (payload.universal_proof !== false
    || payload.producer !== "compiler-proof observation workflow"
    || payload.observer !== OBSERVER_PATHS[stage]
    || payload.runner !== "scripts/agent/jet-env"
    || payload.compiler !== "target/debug/jet"
    || payload.checker !== `proof/compiler/${stage}/check.mjs`
    || payload.protocol !== ROUTE_PROTOCOL
    || !["unverified", "observed", "matched", "failed", "unavailable"].includes(payload.status)) {
    fail("invalid_oracle", `${stage} observation certificates are not bound to the canonical future collector`);
  }
  const observerSource = readSource(OBSERVER_PATHS[stage], `${stage} observation collector`);
  if (payload.observer_sha256 !== observerSource.sha256) fail("mismatch", `${stage} observation collector identity changed`);
  if (JSON.stringify(payload.identity_fields) !== JSON.stringify(OBSERVATION_IDENTITY_FIELDS)
    || JSON.stringify(payload.observation_fields) !== JSON.stringify(OBSERVATION_FIELDS)
    || JSON.stringify(payload.operation_ids) !== JSON.stringify(expected.map((row) => row.id))
    || JSON.stringify(payload.modes) !== JSON.stringify(REPLAY_ROUTES)) {
    fail("invalid_oracle", `${stage} observation certificate dimensions or operation order drifted`);
  }
  const records = array(payload.records, `${stage} observation certificate records`);
  const expectedById = new Map(expected.map((row) => [row.id, row]));
  const seen = new Set();
  const matched = [];
  const supplied = [];
  for (const record of records) {
    object(record, `${stage} observation certificate record`);
    const operationId = text(record.operation_id, `${stage} observation operation_id`);
    if (seen.has(operationId)) fail("invalid_oracle", `${stage} duplicate observation certificate ${operationId}`);
    const operation = expectedById.get(operationId);
    if (!operation) fail("mismatch", `${stage} observation certificate names unknown operation ${operationId}`);
    seen.add(operationId);
    if (record.stage !== stage) fail("invalid_oracle", `${operationId} observation stage mismatch`);
    const certificate = object(record.certificate, `${operationId}.certificate`);
    if (
      certificate.schema !== "jet.compiler-transformation-observation.v1"
      || certificate.operation_id !== operationId
      || !["observed", "matched"].includes(certificate.status)
      || certificate.universal_proof !== false
      || certificate.producer !== "compiler-proof observation workflow"
      || certificate.observer !== OBSERVER_PATHS[stage]
      || certificate.checker !== `proof/compiler/${stage}/check.mjs`
      || certificate.runner !== "scripts/agent/jet-env"
      || certificate.method !== "canonical_route_observation"
      || certificate.protocol !== ROUTE_PROTOCOL
    ) {
      fail("invalid_oracle", `${operationId} observation certificate is not a supported canonical route observation`);
    }
    text(certificate.execution_id, `${operationId}.certificate.execution_id`);
    const observerSource = readSource(OBSERVER_PATHS[stage], `${operationId}.certificate.observer`);
    if (certificate.observer_sha256 !== observerSource.sha256) {
      fail("mismatch", `${operationId} observation collector identity changed`);
    }
    const identityFields = array(record.identity_fields, `${operationId}.identity_fields`);
    if (JSON.stringify(identityFields) !== JSON.stringify(OBSERVATION_IDENTITY_FIELDS)) {
      fail("invalid_oracle", `${operationId} observation identity fields drifted`);
    }
    const expectedIdentity = expectedObservationIdentity(witness, contract, operation, stage);
    const identity = object(record.identity, `${operationId}.identity`);
    for (const field of OBSERVATION_IDENTITY_FIELDS) {
      if (identity[field] !== expectedIdentity[field]) fail("mismatch", `${operationId} observation identity does not match current source-bound identity`);
    }
    const observationOrder = array(record.observation_order, `${operationId}.observation_order`);
    if (JSON.stringify(observationOrder) !== JSON.stringify(OBSERVATION_FIELDS)) {
      fail("invalid_oracle", `${operationId} observation order drifted`);
    }
    const scenarios = array(record.scenarios, `${operationId}.scenarios`);
    if (JSON.stringify(scenarios) !== JSON.stringify(SCENARIOS)) {
      fail("invalid_oracle", `${operationId} observation scenarios are incomplete`);
    }
    const premises = array(record.premises, `${operationId}.premises`);
    if (JSON.stringify(premises) !== JSON.stringify(BASE_PREMISES)) {
      fail("invalid_oracle", `${operationId} observation premises are incomplete`);
    }
    const routeResult = checkCanonicalRouteRecord(record, operation, [...expectedIds], stage);
    const entry = {
      operation_id: operationId,
      identity,
      routes: routeResult.routes,
      observations: routeResult.observations,
      observed_modes: routeResult.observed_modes,
      missing_modes: routeResult.missing_modes,
      observed_dimensions: routeResult.observed_dimensions,
      missing_dimensions: routeResult.missing_dimensions,
      coverage: routeResult.coverage,
      ...(record.runtime_captures === undefined ? {} : { runtime_captures: record.runtime_captures }),
      certificate,
      ...(record.replay === undefined ? {} : { replay: record.replay }),
    };
    supplied.push(entry);
    if (routeResult.matched) matched.push(entry);
  }
  const missing = expected.map((row) => row.id).filter((id) => !seen.has(id));
  const incomplete = supplied
    .filter((record) => !matched.some((candidate) => candidate.operation_id === record.operation_id))
    .map((record) => record.operation_id);
  const matchedKeys = matched.map((record) => record.operation_id).sort();
  const complete = missing.length === 0 && incomplete.length === 0 && matched.length === expected.length;
  return {
    ...descriptor,
    schema_version: payload.schema_version,
    producer: payload.producer,
    observer: payload.observer,
    protocol: payload.protocol,
    status: complete ? "matched" : supplied.length > 0 ? "observed" : "unverified",
    records: supplied,
    matched_records: matched,
    matched_keys: matchedKeys,
    missing_operations: [...missing, ...incomplete.filter((id) => !missing.includes(id))],
    reason: complete
      ? "complete canonical pass records matched raw runtime dimensions; this is bounded correspondence, not semantic legality or proof replay"
      : "canonical pass records are incomplete; missing operations, modes, or dimensions remain unverified",
  };
}

function registeredAuthority(manifest, stage, operationId, expectedOperation) {
  const authorityKey = `compiler-proof.${stage}`;
  const registry = object(manifest.approved_authorities, "manifest.approved_authorities");
  const approved = object(registry[authorityKey], `manifest.approved_authorities.${authorityKey}`);
  if (approved.schema !== "jet.compiler-proof-authority.v1" || approved.stage !== stage) {
    fail("invalid_oracle", `${authorityKey} approved authority schema/stage is invalid`);
  }
  const goals = object(approved.goals, `${authorityKey}.goals`);
  if (!goals[operationId] || typeof goals[operationId] !== "object" || Array.isArray(goals[operationId])) {
    fail("unavailable", `${authorityKey} has no registered semantic goal for ${operationId}`);
  }
  const goal = object(goals[operationId], `${authorityKey}.goals.${operationId}`);
  const contractPath = text(approved.contract_path, `${authorityKey}.contract_path`);
  const contractSha256 = digest(approved.contract_sha256, `${authorityKey}.contract_sha256`);
  const obligationsPath = text(approved.obligations_path, `${authorityKey}.obligations_path`);
  const obligationsSha256 = digest(approved.obligations_sha256, `${authorityKey}.obligations_sha256`);
  const model = object(goal.model, `${authorityKey}.${operationId}.model`);
  const modelRef = {
    path: text(model.path, `${authorityKey}.${operationId}.model.path`),
    sha256: digest(model.sha256, `${authorityKey}.${operationId}.model.sha256`),
  };
  const implementation = object(goal.implementation, `${authorityKey}.${operationId}.implementation`);
  const implementationRef = {
    ...implementation,
    path: text(implementation.path, `${authorityKey}.${operationId}.implementation.path`),
    sha256: digest(implementation.sha256, `${authorityKey}.${operationId}.implementation.sha256`),
  };
  const correspondence = object(goal.correspondence, `${authorityKey}.${operationId}.correspondence`);
  text(correspondence.method, `${authorityKey}.${operationId}.correspondence.method`);
  text(correspondence.route, `${authorityKey}.${operationId}.correspondence.route`);
  if (goal.claim_id !== undefined && text(goal.claim_id, `${authorityKey}.${operationId}.claim_id`) === "") {
    fail("invalid_oracle", `${authorityKey}.${operationId}.claim_id is empty`);
  }
  if (implementationRef.path !== expectedOperation.source || implementationRef.sha256 !== expectedOperation.source_sha256) {
    fail("mismatch", `${operationId} registered implementation is not the current operation source`);
  }
  const contractSource = readSource(contractPath, `${authorityKey}.${operationId}.contract`);
  if (contractSource.sha256 !== contractSha256) fail("mismatch", `${authorityKey}.${operationId} contract identity changed`);
  const obligationsSource = readSource(obligationsPath, `${authorityKey}.${operationId}.obligations`);
  if (obligationsSource.sha256 !== obligationsSha256) fail("mismatch", `${authorityKey}.${operationId} obligations identity changed`);
  const modelSource = readSource(modelRef.path, `${authorityKey}.${operationId}.model`);
  if (modelSource.sha256 !== modelRef.sha256) fail("mismatch", `${authorityKey}.${operationId} model identity changed`);
  const implementationSource = readSource(implementationRef.path, `${authorityKey}.${operationId}.implementation`);
  if (implementationSource.sha256 !== implementationRef.sha256) fail("mismatch", `${authorityKey}.${operationId} implementation identity changed`);
  return {
    authority_key: authorityKey,
    obligation_id: operationId,
    contract: { path: contractPath, sha256: contractSha256 },
    obligations: { path: obligationsPath, sha256: obligationsSha256 },
    model: modelRef,
    proposition: text(goal.proposition, `${authorityKey}.${operationId}.proposition`),
    formal_goal: text(goal.formal_goal, `${authorityKey}.${operationId}.formal_goal`),
    implementation: implementationRef,
    correspondence,
    ...(goal.candidate === undefined ? {} : { candidate: goal.candidate }),
  };
}

function checkFormalDischarge(witness, contract, expected, observations, expectedIds, stage) {
  const path = FORMAL_DISCHARGE_PATHS[stage];
  const descriptor = {
    path,
    schema: FORMAL_DISCHARGE_SCHEMA[stage],
    universal_proof: false,
  };
  if (!existsSync(projectPath(path, `${stage} formal discharge`))) {
    return {
      ...descriptor,
      status: "unavailable",
      reason: "no registered semantic goal and pinned replay report were supplied",
      records: [],
      proved_keys: [],
    };
  }
  const payload = readJson(path, `${stage} formal discharge`);
  if (payload.schema !== descriptor.schema || payload.schema_version !== 1 || payload.stage !== stage) {
    fail("invalid_oracle", `${stage} formal discharge schema/version/stage is invalid`);
  }
  if (payload.universal_proof !== false || payload.producer !== "compiler-proof observation workflow") {
    fail("invalid_oracle", `${stage} formal discharge is not bounded to the compiler-proof workflow`);
  }
  const expectedById = new Map(expected.map((row) => [row.id, row]));
  const completeObservation = (record) => RUNTIME_ROUTES.every((mode) => record.observed_dimensions?.[mode]?.length === OBSERVATION_FIELDS.length);
  const observedById = new Map((observations.matched_records ?? [])
    .filter((record) => completeObservation(record))
    .map((record) => [record.operation_id, record]));
  const records = array(payload.records, `${stage} formal discharge records`);
  const seen = new Set();
  const proved = [];
  for (const record of records) {
    object(record, `${stage} formal discharge record`);
    const operationId = text(record.operation_id, `${stage} formal discharge operation_id`);
    if (seen.has(operationId)) fail("invalid_oracle", `${stage} duplicate formal discharge ${operationId}`);
    seen.add(operationId);
    const expectedOperation = expectedById.get(operationId);
    if (!expectedOperation) fail("mismatch", `${stage} formal discharge names unknown operation ${operationId}`);
    const observation = observedById.get(operationId);
    if (!observation) fail("non-proof", `${operationId} formal discharge has no complete canonical route observation`);
    const replayRef = object(record.replay_ref, `${operationId}.replay_ref`);
    if (replayRef.path !== "proof/compiler/toolchain/manifest.json") {
      fail("invalid_oracle", `${operationId} formal discharge must use the approved pinned toolchain manifest`);
    }
    const claimId = text(replayRef.claim_id, `${operationId}.replay_ref.claim_id`);
    const manifest = readJson(replayRef.path, `${operationId}.replay_ref.path`);
    if (manifest.schema !== "jet.compiler-proof.v1" || manifest.schema_version !== 1) {
      fail("invalid_oracle", `${operationId} formal discharge manifest schema/version is invalid`);
    }
    const registered = registeredAuthority(manifest, stage, operationId, expectedOperation);
    const authorityRef = { authority_key: `compiler-proof.${stage}`, obligation_id: operationId };
    if (!sameCanonical(record.authority, authorityRef)) {
      fail("mismatch", `${operationId} formal discharge authority must name the registered obligation`);
    }
    const canonicalGoal = object(record.canonical_goal, `${operationId}.canonical_goal`);
    if (!sameCanonical(canonicalGoal, registered)) {
      fail("mismatch", `${operationId} formal discharge goal is not the registered semantic goal`);
    }
    if (record.goal_sha256 !== undefined && record.goal_sha256 !== digestText(canonicalJson(canonicalGoal))) {
      fail("mismatch", `${operationId} formal discharge goal digest changed`);
    }
    const replay = object(record.replay, `${operationId}.replay`);
    if (replay.schema !== "jet.compiler-proof.v1" || replay.status !== "proved" || replay.claim_id !== claimId) {
      fail("non-proof", `${operationId} formal discharge replay is not a canonical proved report`);
    }
    if (!sameCanonical(replay.authority, registered)) {
      fail("mismatch", `${operationId} replay authority differs from the registered semantic goal`);
    }
    if (replay.identity_sha256 !== undefined) {
      digest(replay.identity_sha256, `${operationId}.replay.identity_sha256`);
    }
    const rawReplay = object(replay.replay, `${operationId}.replay.replay`);
    if (rawReplay.exit_code !== 0) fail("non-proof", `${operationId} formal discharge replay exited unsuccessfully`);
    const kernelReplay = object(rawReplay.kernel_replay, `${operationId}.replay.replay.kernel_replay`);
    if (kernelReplay.exit_code !== 0) fail("non-proof", `${operationId} pinned kernel replay exited unsuccessfully`);
    const claim = array(manifest.claims, `${operationId} compiler-proof claims`).find((candidate) => candidate.id === claimId);
    if (!claim) fail("mismatch", `${operationId} formal discharge claim is not in the approved manifest`);
    if (replay.identity_sha256 !== claim.identity_sha256) {
      fail("mismatch", `${operationId} formal discharge replay identity does not match the manifest claim`);
    }
    if (claim.result !== "proved"
      || claim.proof?.formal_goal !== canonicalGoal.formal_goal
      || claim.proposition !== canonicalGoal.proposition
      || !sameCanonical(claim.model, registered.model)
      || !sameCanonical(claim.implementation, registered.implementation)
      || !sameCanonical(claim.correspondence, registered.correspondence)
      || !manifest.binding_methods?.includes(claim.correspondence?.method)
      || claim.implementation?.path !== expectedOperation.source
      || claim.implementation?.sha256 !== expectedOperation.source_sha256) {
      fail("non-proof", `${operationId} formal discharge claim does not bind the registered operation goal/correspondence`);
    }
    const components = object(replay.components, `${operationId}.replay.components`);
    if (components.claim?.proposition !== canonicalGoal.proposition
      || components.proof?.formal_goal !== canonicalGoal.formal_goal
      || !sameCanonical(components.proof?.artifact, claim.proof?.artifact)
      || !sameCanonical(components.model, registered.model)
      || !sameCanonical(components.implementation, registered.implementation)
      || !sameCanonical(components.correspondence, registered.correspondence)) {
      fail("mismatch", `${operationId} replay claim/proof is not bound to the registered semantic goal`);
    }
    for (const field of ["model", "source", "implementation", "correspondence", "candidate"]) {
      if (!sameCanonical(components[field], claim[field])) {
        fail("mismatch", `${operationId} replay ${field} component differs from its manifest claim`);
      }
    }
    if (components.correspondence.route !== registered.correspondence.route
      || components.implementation.path !== expectedOperation.source
      || components.implementation.sha256 !== expectedOperation.source_sha256) {
      fail("non-proof", `${operationId} replay components are not bound to the actual operation`);
    }
    proved.push({
      operation_id: operationId,
      canonical_goal: canonicalGoal,
      authority: authorityRef,
      goal_sha256: digestText(canonicalJson(canonicalGoal)),
      replay: {
        schema: replay.schema,
        status: replay.status,
        claim_id: replay.claim_id,
        identity_sha256: replay.identity_sha256,
        components,
        checker: replay.checker,
        replay: rawReplay,
      },
    });
  }
  const provedKeys = proved.map((record) => record.operation_id).sort();
  const missing = expected.map((row) => row.id).filter((id) => !seen.has(id));
  return {
    ...descriptor,
    schema_version: payload.schema_version,
    producer: payload.producer,
    status: missing.length === 0 && proved.length === expected.length ? "proved" : proved.length > 0 ? "partial" : "unverified",
    records: proved,
    proved_keys: provedKeys,
    missing_operations: missing,
    reason: missing.length === 0 && proved.length === expected.length
      ? "each observed operation has a registered semantic goal and a report from the pinned compiler-proof replay"
      : "formal discharge is incomplete; observations do not qualify semantic legality or proof replay by themselves",
  };
}


export function runContractCheck(stage) {
  const contract = checkContract(readJson(`proof/compiler/${stage}/contract.json`, `${stage} contract`), stage);
  const witness = readJson(`proof/compiler/${stage}/witnesses.json`, `${stage} witnesses`);
  if (witness.schema !== `jet.${stage}-witnesses.v1` || witness.schema_version !== 1 || witness.stage !== stage) {
    fail("invalid_oracle", `${stage} witness schema/stage mismatch`);
  }
  if (JSON.stringify(witness.observation_fields) !== JSON.stringify(OBSERVATION_FIELDS)) fail("invalid_oracle", `${stage} witness observation fields mismatch`);
  if (JSON.stringify(witness.preservation_fields) !== JSON.stringify(PRESERVATION_FIELDS)) fail("invalid_oracle", `${stage} witness preservation fields mismatch`);
  checkBinding(witness, stage);
  checkIdentity(witness, stage);
  const obligations = readJson("proof/compiler/obligations.json", "compiler obligations");
  const coverage = checkCoverage(witness, stage, obligations);
  const expected = operationRegistry(stage);
  if (expected.length !== STAGE_OPERATION_COUNTS[stage]) fail("invalid_oracle", `${stage} operation denominator changed`);
  const expectedIds = new Set(expected.map((row) => row.id));
  if (JSON.stringify(contract.implementation_route?.registered_operations) !== JSON.stringify(expected.map((row) => row.id))) {
    fail("invalid_oracle", `${stage} contract implementation route does not cover the registered production operations`);
  }
  const certificates = array(witness.certificates, `${stage} certificates`);
  if (certificates.length !== expected.length) fail("invalid_oracle", `${stage} certificate count does not cover production pass relation`);
  const seen = new Set();
  for (const row of expected) {
    const cert = certificates.find((candidate) => candidate.operation_id === row.id);
    if (!cert) fail("invalid_oracle", `${stage} missing implementation-bound certificate for ${row.id}`);
    checkCertificate(materializeCertificate(cert, witness, stage), row, new Set(coverage.row_ids), stage);
    seen.add(cert.operation_id);
  }
  if (seen.size !== expected.length) fail("invalid_oracle", `${stage} has duplicate or missing pass certificates`);
  checkComposition(witness, stage, expected, new Set(coverage.row_ids));
  checkWitnessSources(witness, stage, expectedIds);
  const negatives = checkNegativeControls(witness, stage, expected, new Set(coverage.row_ids));
  const observations = checkObservationCertificates(witness, contract, expected, expectedIds, stage);
  const formal = checkFormalDischarge(witness, contract, expected, observations, expected.map((row) => row.id), stage);
  const matchedOperationIds = new Set(observations.matched_keys);
  const formalOperationIds = new Set(formal.proved_keys);
  const formalComplete = formal.status === "proved" && formalOperationIds.size === expected.length;
  const observedComplete = observations.status === "matched" && matchedOperationIds.size === expected.length;
  return {
    schema: `jet.${stage}-contract-check.v1`,
    schema_version: 1,
    stage,
    operation_ids: expected.map((row) => row.id),
    operation_count: expected.length,
    observation_fields: [...OBSERVATION_FIELDS],
    premises: [...BASE_PREMISES],
    status: formalComplete ? "qualified" : "blocked",
    contract: {
      path: `proof/compiler/${stage}/contract.json`,
      schema: contract.schema,
      qualification: contract.boundary.qualification,
    },
    generated_operation_relation: {
      source: "proof/compiler/obligations.json",
      authorities: ["#2898", "#2919"],
      boundary_id: coverage.boundary_id,
      canonical_operation_row_count: coverage.row_count,
      canonical_operation_id_sha256: coverage.id_sha256,
      global_row_count: coverage.global_row_count,
      production_pass_count: expected.length,
      mapping: {
        default_status: "mapped_unproved",
        proved_rows: 0,
        observed_passes: matchedOperationIds.size,
        qualification: formalComplete ? "qualified" : "blocked",
      },
      implementation_bound: true,
      semantic_observations: observations.status,
    },
    operations: expected.map((row) => {
      const record = observations.records.find((candidate) => candidate.operation_id === row.id);
      return {
        id: row.id,
        category: row.category,
        source: row.source,
        symbol: row.symbol,
        status: matchedOperationIds.has(row.id) ? "matched" : "unverified",
        observation_status: record?.observed_modes?.length
          ? RUNTIME_ROUTES.every((mode) => record.observed_dimensions?.[mode]?.length === OBSERVATION_FIELDS.length) ? "matched" : "observed"
          : "unverified",
        observation_fields: [...(record ? [...new Set(Object.values(record.observed_dimensions ?? {}).flat())] : [])],
        legality: "unverified",
        profitability: "separate-and-unverified",
        targets: [...TARGETS],
      };
    }),
    negative_controls: negatives,
    observation_certificates: observations,
    formal_discharge: formal,
    compiler_execution: {
      requested: false,
      status: observedComplete ? "matched" : observations.records.length > 0 ? "observed" : "unverified",
      reason: observedComplete
        ? "canonical TIR/MIR route observations matched raw dimensions; this checker did not execute Jet"
        : "stage checker consumes only future collector route observations; it does not execute Jet or infer missing dimensions",
    },
    proof_replay: {
      status: formalComplete ? "proved" : "unverified",
      requested: false,
      qualification: formalComplete ? "qualified" : "blocked",
      observed_operations: observations.matched_keys,
      formal_operations: formal.proved_keys,
      missing_operations: [...new Set([...observations.missing_operations, ...formal.missing_operations])],
      reason: formal.reason,
    },
    proof_boundary: {
      qualification: formalComplete ? "qualified" : "blocked",
      universal_proof: false,
      proven: [
        "production pass inventory is joined to current implementation paths",
        "certificates retain source identity, origin, side conditions, legality/profitability separation, and observation order",
        "negative controls structurally reject forged, reordered, over-consuming, aliased, unsound, and wrong-operation certificates",
      ],
      assumed: [
        "#2898 canonical operation denominator and #2919 shared implementation route remain authoritative",
        "ratified D-COMPILER-PROOF1, D-TIER-ONEIR1, D-ACCEL1, I3, and I9 contracts",
        "foreign compiler, target ABI/layout, and resource environments at named boundaries",
      ],
      tested: [
        "legal and invalid witness sources are identity-bound but not executed by this checker",
        "source/IR/target identity and premise-retention checks are present",
        "canonical TIR/MIR route observations are matched as bounded correspondence, not semantic legality or proof replay",
      ],
      blocked: [
        "universal preservation for every source, MIR state, target, and resource schedule",
        ...(formalComplete ? [] : ["registered semantic goals and pinned compiler-proof replay for every operation"]),
        "qualification of optimization profitability or foreign target behavior",
      ],
    },
  };
}

export function errorPayload(stage, error) {
  const code = error instanceof ContractError ? error.code : "invalid_oracle";
  const status = ["unavailable", "mismatch", "invalid_oracle"].includes(code) ? code : "invalid_oracle";
  return {
    schema: `jet.${stage}-contract-check.v1`,
    schema_version: 1,
    stage,
    status,
    error: {
      code,
      message: error?.message ?? String(error),
      details: error instanceof ContractError ? error.details ?? null : null,
    },
  };
}

export function canonicalJsonOutput(value) {
  return `${canonicalJson(value)}\n`;
}

export {
  ROOT,
  digestBytes,
  digestText,
  readSource,
  readJson,
  projectPath,
  canonical,
  canonicalJson,
  OBSERVATION_CERTIFICATE_SCHEMA,
  OBSERVATION_IDENTITY_FIELDS,
  REPLAY_ROUTES,
  RUNTIME_ROUTES,
  OBSERVER_PATHS,
  ROUTE_PROTOCOL,
  ROUTE_SCHEMA,
  FORMAL_DISCHARGE_SCHEMA,
  FORMAL_DISCHARGE_PATHS,
  STAGE_OPERATION_COUNTS,
  BASE_PREMISES,
};
