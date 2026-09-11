#!/usr/bin/env bash
# Card #1414: fail closed until complete-work measurements and review exist.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

CORPUS="$ROOT/tests/compiled_workloads"
MANIFEST="$CORPUS/manifest.tsv"
DOMAIN_CONTRACT="$CORPUS/domain_contract.tsv"
PEERS="$CORPUS/peer_ledger.tsv"
PEER_ADAPTERS="$CORPUS/peer_adapter_ledger.tsv"
ADAPTERS="$CORPUS/adapter_ledger.tsv"
METRIC_CONTRACT="$CORPUS/metric_contract.tsv"
MEASUREMENT_POLICY="$CORPUS/measurement_policy.tsv"
TIER_MATRIX="$CORPUS/tier_matrix.tsv"
CANARIES="$CORPUS/canaries.tsv"
TOOLCHAINS="$CORPUS/toolchain_contract.tsv"
CORE_CHECKER="$ROOT/scripts/agent/check-core-surface-ledger.mjs"

MANIFEST_HEADER=$'version\ttask_id\tdomain\tcase\tdeclared_outcome\tinput\texpected\tauthority\tadapters\tplatforms\tevidence\ttower_card\tloss_cards'
DOMAIN_HEADER=$'version\ttask_id\tallowed_dependencies\tmachine_spec\tvariant\tscoring'
PEER_HEADER=$'version\ttask_id\tselection\tlanguage\tprogram\tsource_url\tsource_revision\tbuild_command\trun_command\tdependency_rule\tsource_boundary\tapplicable_targets\towner'
PEER_ADAPTER_HEADER=$'version\tpeer_key\ttask_id\tselection\tlanguage\tpeer_source\tpeer_hostile\tpeer_commit'
ADAPTER_HEADER=$'version\ttask_id\tjet_source\tjet_hostile\tpeer_source\tpeer_hostile\tpeer_commit'
METRIC_HEADER=$'version\tmetric\tunit\tcomparison\tmissing_policy'
POLICY_HEADER=$'version\tmetric\tsamples\tmin_samples\tmin_value\toutlier_mad_multiplier\tmax_relative_stdev\tmax_outliers\ttolerance_ratio\trust_tolerance_ratio'
TIER_HEADER=$'version\ttask_id\tplatform\ttarget\ttier\trequirement\tavailability\tavailability_reason\trationale'
CANARY_HEADER=$'version\tcanary\tmutation\trequired_failure'
TOOLCHAIN_HEADER=$'version\tlanguage\tcommand\tversion_prefix'
IDENTITY_HEADER=$'version\tkey\tvalue'
SAMPLE_HEADER=$'version\ttask_id\tlanguage\tmetric\tsample\tvalue\tunit\tmethod\tplatform\ttarget\ttier'
STATISTICS_HEADER=$'version\ttask_id\tlanguage\tmetric\tsamples\tmedian\tmin\tmax\trelative_stdev\toutliers\tthreshold\ttolerance_ratio\tstatus\tloss_owner\tevidence\tplatform\ttarget\ttier'
RECEIPT_HEADER=$'version\ttask_id\tlanguage\tsource_sha256\tinput_sha256\texpected_sha256\toutput_sha256\thostile_input_sha256\thostile_output_sha256\tenvironment\tmachine\ttool_version\tcommand\tpeer_commit\texit_code\thostile_exit_code\tpeer_launcher_path\tpeer_launcher_version\tpeer_launcher_sha256\tauthority'
TIER_RECEIPT_HEADER=$'version\ttask_id\tlanguage\tplatform\ttarget\ttier\tartifact_sha256\toutput_sha256\tcommand\tstatus'
OUTCOME_HEADER=$'version\ttask_id\tpeer_language\tpeer_program\tinput\texpected\toutcome\ttoolchain_id\tjet_tool_version\tpeer_tool_version\tdependency_rule\tsource_boundary\tjet_status\tpeer_status\tloss_owner\treview_status\treview_evidence'
MEASUREMENT_HEADER=$'version\ttask_id\tlanguage\tmetric\tvalue\tunit\ttoolchain_id\tevidence\tstatus\tloss_owner\tplatform\ttarget\ttier'
TIER_REPORT_HEADER=$'version\ttask_id\tlanguage\tplatform\ttarget\ttier\tstatus\tevidence\tloss_owner'
PEER_COVERAGE_HEADER=$'version\tpeer_key\ttask_id\tselection\tlanguage\tprogram\tsource\tsource_revision\tsource_sha256\tinput_sha256\texpected_sha256\toutput_sha256\thostile_input_sha256\thostile_output_sha256\tstatus\tplatform\ttarget\ttier\tcommand\tevidence'
PEER_MEASUREMENT_HEADER=$'version\tpeer_key\ttask_id\tselection\tlanguage\tmetric\tsamples\tmedian\tmin\tmax\trelative_stdev\tunit\tstatus\tevidence\tplatform\ttarget\ttier'
REVIEW_HEADER=$'version\treviewer\treviewed_candidate\tcompiler_sha256\tcontract_sha256\tsource_closure_sha256\treport_sha256\tworkload_fairness\tpeer_fairness\tauthority_fairness\tmeasurement_fairness\ttier_fairness\tloss_ownership\tstatus\tevidence\treview_workflow\treview_run\treview_actor'
REPORT_FILES=(identity.tsv samples.tsv statistics.tsv outcomes.tsv measurements.tsv tiers.tsv receipts.tsv tier_receipts.tsv peer_coverage.tsv peer_measurements.tsv)

METRICS=(source_effort build_time edit_time runtime memory artifact_size diagnostics debugging deployment unsafe_burden)
LANGUAGES=(rust cxx go swift zig domain)
TOWER_CLI="$ROOT/plugins/tower/tower.mjs"
declare -A live_tower_owner_cache=()
declare -gA policy_tolerance=() policy_rust_tolerance=()
declare -gA policy_samples=()
declare -gA policy_min_value=() policy_outlier_mad_multiplier=() policy_max_relative_stdev=() policy_max_outliers=()

policy_tolerance_for_peer() {
  local peer_language="$1" metric="$2"
  if [[ "$peer_language" == rust ]]; then
    printf '%s' "${policy_rust_tolerance[$metric]}"
  else
    printf '%s' "${policy_tolerance[$metric]}"
  fi
}

metric_is_loss() {
  local peer_language="$1" metric="$2" jet_value="$3" peer_value="$4" tolerance
  tolerance="$(policy_tolerance_for_peer "$peer_language" "$metric")"
  if awk -v jet="$jet_value" -v peer="$peer_value" 'BEGIN { exit !(jet == 0 && peer == 0) }'; then
    fail "zero/zero comparison is not a valid measurement: $metric"
  fi
  if [[ "$peer_language" == rust ]]; then
    awk -v jet="$jet_value" -v peer="$peer_value" -v tolerance="$tolerance" \
      'BEGIN { if (peer == 0) exit !(jet > 0); exit !(jet > peer * tolerance) }'
  else
    awk -v jet="$jet_value" -v peer="$peer_value" -v tolerance="$tolerance" \
      'BEGIN { if (peer == 0) exit !(jet > 0); exit !(jet >= peer * tolerance) }'
  fi
}

fail() { echo "compiled workload gate: $*" >&2; exit 1; }
usage() { echo "usage: bash tools/ci/compiled-workload-gate.sh --contract|--check REPORT_DIR" >&2; exit 64; }

file_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -- "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -- "$1" | awk '{print $1}'
  else
    fail "a SHA-256 utility is required to verify report receipts"
  fi
}
hash_input_path() {
  local input="$1"
  node - "$input" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const input = process.argv[2];
const root = fs.lstatSync(input);
if (!root.isDirectory()) {
  if (!root.isFile()) process.exit(1);
  process.stdout.write(crypto.createHash("sha256").update(fs.readFileSync(input)).digest("hex"));
  process.exit(0);
}
const sidecar = path.join(input, ".fixture-modes.tsv");
let sidecarBytes = null;
try {
  const sidecarStat = fs.lstatSync(sidecar);
  if (!sidecarStat.isFile()) process.exit(1);
  sidecarBytes = fs.readFileSync(sidecar);
} catch (error) {
  if (error.code !== "ENOENT") process.exit(1);
}
const overrides = new Map();
if (sidecarBytes !== null) {
  let text;
  try {
    text = new TextDecoder("utf-8", { fatal: true }).decode(sidecarBytes);
  } catch {
    process.exit(1);
  }
  const lines = text.split(/\r?\n/);
  if (lines.at(-1) === "") lines.pop();
  for (const line of lines) {
    const fields = line.split("\t");
    if (fields.length !== 2) process.exit(1);
    const [relative, mode] = fields;
    const parts = relative.split("/");
    if (!relative || relative === ".fixture-modes.tsv" || relative.includes("\\") || relative.includes(":") ||
        parts.some(part => !part || part === "." || part === "..") ||
        !/^(?:[0-7]{3}|0[0-7]{3})$/.test(mode) || Number.parseInt(mode, 8) > 0o777 ||
        overrides.has(relative)) process.exit(1);
    overrides.set(relative, Number.parseInt(mode, 8).toString(8));
  }
}
const entries = [];
function walk(relative, file) {
  const stat = fs.lstatSync(file);
  let type;
  if (stat.isDirectory()) type = "dir";
  else if (stat.isSymbolicLink()) type = "symlink";
  else if (stat.isFile()) type = "file";
  else process.exit(1);
  entries.push({ relative, file, type, mode: (stat.mode & 0o7777).toString(8) });
  if (type === "dir") {
    for (const name of fs.readdirSync(file)) {
      if (relative === "." && name === ".fixture-modes.tsv") continue;
      walk(path.posix.join(relative, name), path.join(file, name));
    }
  }
}
walk(".", input);
const entriesByPath = new Map(entries.map(entry => [entry.relative, entry]));
for (const [relative] of overrides) {
  const entry = entriesByPath.get(relative);
  if (!entry || (entry.type !== "file" && entry.type !== "dir")) process.exit(1);
}
for (const entry of entries) {
  if (overrides.has(entry.relative)) entry.mode = overrides.get(entry.relative);
}
entries.sort((left, right) => left.relative < right.relative ? -1 : left.relative > right.relative ? 1 : 0);
const digest = crypto.createHash("sha256");
for (const entry of entries) {
  const link = entry.type === "symlink" ? fs.readlinkSync(entry.file) : "";
  digest.update(entry.relative);
  digest.update("\0");
  digest.update(entry.type);
  digest.update("\0");
  digest.update(entry.mode);
  digest.update("\0");
  digest.update(link);
  digest.update("\0");
  if (entry.type === "file") digest.update(fs.readFileSync(entry.file));
  else if (entry.type === "symlink") digest.update(link);
  digest.update("\0");
}
if (sidecarBytes !== null) {
  digest.update(".fixture-modes.tsv");
  digest.update("\0");
  digest.update("control");
  digest.update("\0");
  digest.update(sidecarBytes);
  digest.update("\0");
  for (const relative of [...overrides.keys()].sort()) {
    digest.update("mode");
    digest.update("\0");
    digest.update(relative);
    digest.update("\0");
    digest.update(overrides.get(relative));
    digest.update("\0");
  }
}
process.stdout.write(digest.digest("hex"));
NODE
}
hash_input_method() {
  [[ -d "$1" ]] && printf '%s' tree-mode-v1 || printf '%s' file-bytes-v1
}

hash_relative_files() {
  node - "$ROOT" "$@" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const args = process.argv.slice(2);
const root = args.shift();
const files = [];
function walk(file) {
  const stat = fs.lstatSync(file);
  if (stat.isDirectory()) {
    if (path.basename(file) === ".jet") return;
    for (const name of fs.readdirSync(file)) walk(path.join(file, name));
  } else if (stat.isFile()) {
    files.push(file);
  }
}
for (const relative of args) walk(path.join(root, relative));
files.sort((a, b) => {
  const left = path.relative(root, a).split(path.sep).join("/");
  const right = path.relative(root, b).split(path.sep).join("/");
  return left < right ? -1 : left > right ? 1 : 0;
});
const digest = crypto.createHash("sha256");
for (const file of files) {
  digest.update(path.relative(root, file).split(path.sep).join("/"));
  digest.update("\0");
  digest.update(fs.readFileSync(file));
  digest.update("\0");
}
process.stdout.write(digest.digest("hex"));
NODE
}

hash_report_files() {
  local report_dir="$1"
  node - "$report_dir" "${REPORT_FILES[@]}" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const args = process.argv.slice(2);
const root = args.shift();
const files = args.sort();
const digest = crypto.createHash("sha256");
for (const relative of files) {
  const file = path.join(root, relative);
  if (!fs.statSync(file).isFile()) process.exit(1);
  digest.update(relative);
  digest.update("\0");
  digest.update(fs.readFileSync(file));
  digest.update("\0");
}
process.stdout.write(digest.digest("hex"));
NODE
}

safe_path() {
  local value="$1"
  [[ -n "$value" && "$value" != /* && "$value" != *:* && "$value" != *\\* ]] || return 1
  local part
  IFS=/ read -ra parts <<< "$value"
  for part in "${parts[@]}"; do
    [[ -n "$part" && "$part" != . && "$part" != .. ]] || return 1
  done
}
path_exists() { [[ -e "$1" || -L "$1" ]]; }

owner_ok() {
  local value="$1"
  [[ "$value" =~ ^#[0-9]+$ || "$value" =~ ^non-goal:D-[A-Za-z0-9][A-Za-z0-9-]*$ ]]
}

has_word() { [[ ";$1;" == *";$2;"* || ",$1," == *",$2,"* ]]; }
is_nonzero_sha256() {
  [[ "$1" =~ ^[0-9a-f]{64}$ && ! "$1" =~ ^0{64}$ ]]
}


peer_applies() {
  local targets="$1" platform="$2" target="$3"
  local target_alias="${target%%-*}"
  has_word "$targets" "$platform" || has_word "$targets" "$target" || has_word "$targets" "$target_alias"
}

live_tower_card() {
  local owner="$1" card
  owner_ok "$owner" && [[ "$owner" =~ ^#[0-9]+$ ]] || return 1
  if [[ -n "${live_tower_owner_cache[$owner]+x}" ]]; then
    [[ "${live_tower_owner_cache[$owner]}" == 1 ]]
    return
  fi
  # `card show` is read-only; reject its history fallback as stale ownership.
  if ! card="$(node "$TOWER_CLI" card show "$owner" --json 2>/dev/null)"; then
    live_tower_owner_cache[$owner]=0
    return 1
  fi
  if node -e '
    const [raw, ref] = process.argv.slice(1);
    let card;
    try { card = JSON.parse(raw); } catch { process.exit(1); }
    const number = Number(ref.slice(1));
    if (card.archived === true || card.phase === "done" || card.phase === "frozen" || !Number.isInteger(card.num) || card.num !== number) process.exit(1);
  ' "$card" "$owner" >/dev/null 2>&1; then
    live_tower_owner_cache[$owner]=1
    return 0
  fi
  live_tower_owner_cache[$owner]=0
  return 1
}

ratified_non_goal() {
  local owner="$1" decision
  owner_ok "$owner" && [[ "$owner" == non-goal:* ]] || return 1
  decision="${owner#non-goal:}"
  local ruling
  if ! ruling="$(node "$TOWER_CLI" decision show "$decision" --json 2>/dev/null)"; then
    return 1
  fi
  node -e '
    const [raw] = process.argv.slice(1);
    let decision;
    try { decision = JSON.parse(raw); } catch { process.exit(1); }
    if (decision.status !== "ratified" || typeof decision.outcome !== "string" || !decision.outcome.trim()) process.exit(1);
  ' "$ruling" >/dev/null 2>&1
}

require_live_loss_owner() {
  local owner="$1" context="$2"
  if [[ "$owner" =~ ^#[0-9]+$ ]]; then
    live_tower_card "$owner" || fail "loss owner is not a live Tower card: $context ($owner)"
  elif [[ "$owner" == non-goal:* ]]; then
    ratified_non_goal "$owner" || fail "loss owner is not a ratified non-goal: $context ($owner)"
  else
    fail "loss owner is not a card or ratified non-goal: $context"
  fi
}
require_manifest_loss_owner() {
  local task_id="$1" owner="$2" context="$3" expected
  expected="${task_loss[$task_id]-}"
  [[ -n "$expected" && "$owner" == "$expected" ]] \
    || fail "loss owner does not match manifest loss_cards: $context ($owner != ${expected:-missing})"
  require_live_loss_owner "$owner" "$context"
}

reviewer_is_independent() {
  local reviewer="${1,,}"
  [[ "$reviewer" =~ ^[^[:space:]]+$ ]] || return 1
  [[ "$reviewer" != *compiled-workload-gate* && "$reviewer" != *compiled-workload-runner* && "$reviewer" != *synthetic* ]] || return 1
  [[ ! "$reviewer" =~ (^|[-_./:])ci($|[-_./:]) ]]
}


static_contract() {
  for file in "$MANIFEST" "$DOMAIN_CONTRACT" "$PEERS" "$PEER_ADAPTERS" "$ADAPTERS" "$METRIC_CONTRACT" "$MEASUREMENT_POLICY" "$TIER_MATRIX" "$CANARIES" "$TOOLCHAINS"; do
    [[ -f "$file" ]] || fail "missing frozen contract: ${file#$ROOT/}"
  done
  node "$CORE_CHECKER" --check >/dev/null || fail "Core competitor ledger drifted"
  [[ "$(head -n 1 "$MANIFEST")" == "$MANIFEST_HEADER" ]] || fail "manifest schema drifted"
  [[ "$(head -n 1 "$DOMAIN_CONTRACT")" == "$DOMAIN_HEADER" ]] || fail "domain contract schema drifted"
  [[ "$(head -n 1 "$PEERS")" == "$PEER_HEADER" ]] || fail "peer ledger schema drifted"
  [[ "$(head -n 1 "$PEER_ADAPTERS")" == "$PEER_ADAPTER_HEADER" ]] || fail "peer adapter ledger schema drifted"
  [[ "$(head -n 1 "$ADAPTERS")" == "$ADAPTER_HEADER" ]] || fail "adapter ledger schema drifted"
  [[ "$(head -n 1 "$METRIC_CONTRACT")" == "$METRIC_HEADER" ]] || fail "metric contract schema drifted"
  [[ "$(head -n 1 "$MEASUREMENT_POLICY")" == "$POLICY_HEADER" ]] || fail "measurement policy schema drifted"
  [[ "$(head -n 1 "$TIER_MATRIX")" == "$TIER_HEADER" ]] || fail "tier matrix schema drifted"
  [[ "$(head -n 1 "$CANARIES")" == "$CANARY_HEADER" ]] || fail "removal canary schema drifted"
  [[ "$(head -n 1 "$TOOLCHAINS")" == "$TOOLCHAIN_HEADER" ]] || fail "toolchain contract schema drifted"
  local expected_fields
  for expected_fields in "$MANIFEST:13" "$DOMAIN_CONTRACT:6" "$PEERS:13" "$PEER_ADAPTERS:8" "$ADAPTERS:7" "$METRIC_CONTRACT:5" "$MEASUREMENT_POLICY:10" "$TIER_MATRIX:9" "$CANARIES:4" "$TOOLCHAINS:4"; do
    file="${expected_fields%:*}"
    header="${expected_fields##*:}"
    awk -F '\t' -v expected="$header" 'NR > 1 && NF != expected { bad = 1; print NR ": expected " expected " fields, got " NF > "/dev/stderr" } END { exit bad }' "$file" || fail "row width drifted: ${file#$ROOT/}"
  done

  declare -gA task_domain task_input task_expected task_outcome task_adapter task_platform task_tower task_loss task_network task_authority
  local line version id domain case_name outcome input expected authority adapters platforms evidence tower loss expected_authority
  while IFS=$'\t' read -r version id domain case_name outcome input expected authority adapters platforms evidence tower loss; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "$id" && -z "${task_domain[$id]+x}" ]] || fail "bad or duplicate manifest row: $id"
    safe_path "$input" && safe_path "$expected" || fail "unsafe manifest fixture path: $id"
    path_exists "$CORPUS/$input" && [[ -f "$CORPUS/$expected" ]] || fail "manifest fixture is missing: $id"
    expected_authority="argv=input-root;cwd=scratch;host=ambient;network=disabled;external-write=disabled"
    [[ "$id" == service-json-http ]] && expected_authority="argv=input-root;cwd=scratch;host=ambient;network=loopback-only;external-write=disabled"
    [[ "$id" == cross-platform-notes ]] && expected_authority="build-input=input-root;cwd=scratch;host=ambient;network=disabled;external-write=disabled"
    [[ "$authority" == "$expected_authority" ]] || fail "authority drifted: $id"
    has_word "$adapters" jet || fail "Jet adapter missing: $id"
    [[ "$platforms" == *linux=* && "$platforms" == *macos=* && "$platforms" == *windows=* && "$platforms" == *cross-target=* ]] || fail "platform matrix incomplete: $id"
    [[ "$evidence" == docs/research/compiled-workload-source-boundary-2026-09.md#* ]] || fail "public task evidence missing: $id"
    [[ "$tower" == "#1414" ]] || fail "manifest owner drifted: $id"
    owner_ok "$loss" || fail "manifest loss owner is not auditable: $id"
    task_domain[$id]="$domain"; task_input[$id]="$input"; task_expected[$id]="$expected"
    task_outcome[$id]="$outcome"; task_adapter[$id]="$adapters"; task_platform[$id]="$platforms"
    task_tower[$id]="$tower"; task_loss[$id]="$loss"; task_authority[$id]="$authority"
    task_network[$id]="disabled"
    [[ "$authority" == *network=loopback-only* ]] && task_network[$id]="loopback-only"
  done < "$MANIFEST"
  ((${#task_domain[@]} == 7)) || fail "manifest task count drifted: ${#task_domain[@]}"
  local required_domain
  for required_domain in systems service cli library compute embedded cross-platform-application; do
    local found=0
    for id in "${!task_domain[@]}"; do [[ "${task_domain[$id]}" == "$required_domain" ]] && found=1; done
    ((found)) || fail "manifest lacks domain: $required_domain"
  done

  declare -A domain_seen=()
  local domain_id domain_deps domain_machine domain_variant domain_scoring
  while IFS=$'\t' read -r version domain_id domain_deps domain_machine domain_variant domain_scoring; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "${task_domain[$domain_id]+x}" ]] || fail "domain contract names unknown task: $domain_id"
    [[ -z "${domain_seen[$domain_id]+x}" ]] || fail "duplicate domain contract: $domain_id"
    [[ "$domain_scoring" == "#1414:v1;exit=0;stdout=exact;cold=recorded;warm=equal;input=unchanged;scratch=closed" ]] || fail "domain scoring drifted: $domain_id"
    [[ -n "$domain_deps" && -n "$domain_machine" && -n "$domain_variant" ]] || fail "incomplete domain contract: $domain_id"
    domain_seen[$domain_id]=1
  done < "$DOMAIN_CONTRACT"

  declare -gA toolchain_command toolchain_prefix
  declare -A toolchain_seen=()
  local tool_version tool_language tool_command tool_prefix
  while IFS=$'\t' read -r tool_version tool_language tool_command tool_prefix; do
    [[ -z "$tool_version" || "$tool_version" == version ]] && continue
    [[ "$tool_version" == 1 && "$tool_language" =~ ^(jet|rust|cxx|go|zig|domain)$ ]] || fail "bad toolchain contract row: $tool_language"
    [[ -z "${toolchain_seen[$tool_language]+x}" && -n "$tool_command" && -n "$tool_prefix" ]] || fail "duplicate or incomplete toolchain contract: $tool_language"
    toolchain_seen[$tool_language]=1
    toolchain_command[$tool_language]="$tool_command"
    toolchain_prefix[$tool_language]="$tool_prefix"
  done < "$TOOLCHAINS"
  ((${#toolchain_seen[@]} == 6)) || fail "toolchain contract count drifted"
  for id in "${!task_domain[@]}"; do [[ -n "${domain_seen[$id]+x}" ]] || fail "domain contract missing task: $id"; done
  declare -gA selected_language selected_program selected_dependency selected_boundary selected_targets selected_revision
  declare -gA peer_task peer_selection_by_key peer_language_by_key peer_program_by_key peer_targets_by_key peer_revision_by_key peer_seen_key
  declare -gA peer_adapter_source peer_adapter_hostile peer_adapter_commit
  declare -gA peer_languages_seen
  local selection language program source_url source_revision build_command run_command dependency_rule source_boundary targets owner peer_key
  declare -A selected_seen=()
  while IFS=$'\t' read -r version id selection language program source_url source_revision build_command run_command dependency_rule source_boundary targets owner; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "${task_domain[$id]+x}" ]] || fail "peer ledger names unknown task: $id"
    [[ "$selection" == best-applicable || "$selection" == candidate ]] || fail "bad peer selection: $id"
    [[ "$language" =~ ^(rust|cxx|go|swift|zig|domain)$ ]] || fail "bad peer language: $id/$language"
    [[ "$source_url" == https://* && "$source_revision" =~ ^(tag:[^[:space:]]+|[0-9a-f]{40})$ ]] || fail "peer source is not pinned: $id/$language"
    [[ -n "$program" && -n "$build_command" && -n "$run_command" && -n "$dependency_rule" && -n "$source_boundary" && -n "$targets" ]] || fail "incomplete peer row: $id/$language"
    owner_ok "$owner" || fail "peer owner is not auditable: $id/$language"
    peer_key="$id|$selection|$language|$program"
    [[ -z "${peer_seen_key[$peer_key]+x}" ]] || fail "duplicate peer ledger key: $peer_key"
    peer_seen_key[$peer_key]=1
    peer_task[$peer_key]="$id"; peer_selection_by_key[$peer_key]="$selection"; peer_language_by_key[$peer_key]="$language"
    peer_program_by_key[$peer_key]="$program"; peer_targets_by_key[$peer_key]="$targets"; peer_revision_by_key[$peer_key]="$source_revision"
    peer_languages_seen[$language]=1
    if [[ "$selection" == best-applicable ]]; then
      [[ -z "${selected_seen[$id]+x}" ]] || fail "more than one best applicable peer: $id"
      has_word "${task_adapter[$id]}" "$language" || fail "selected peer is not a declared task rail: $id/$language"
      selected_seen[$id]=1; selected_language[$id]="$language"; selected_program[$id]="$program"
      selected_dependency[$id]="$dependency_rule"; selected_boundary[$id]="$source_boundary"; selected_targets[$id]="$targets"; selected_revision[$id]="$source_revision"
    fi
  done < "$PEERS"
  for id in "${!task_domain[@]}"; do [[ -n "${selected_seen[$id]+x}" ]] || fail "no best applicable peer: $id"; done
  for language in "${LANGUAGES[@]}"; do [[ -n "${peer_languages_seen[$language]+x}" ]] || fail "peer language missing: $language"; done

  local peer_adapter_version peer_adapter_key peer_adapter_id peer_adapter_selection peer_adapter_language peer_adapter_source_path peer_adapter_hostile_path peer_adapter_peer_commit
  while IFS=$'\t' read -r peer_adapter_version peer_adapter_key peer_adapter_id peer_adapter_selection peer_adapter_language peer_adapter_source_path peer_adapter_hostile_path peer_adapter_peer_commit; do
    [[ -z "$peer_adapter_version" || "$peer_adapter_version" == version ]] && continue
    [[ "$peer_adapter_version" == 1 && -n "${peer_seen_key[$peer_adapter_key]+x}" ]] || fail "peer adapter names unknown ledger row: $peer_adapter_key"
    [[ -z "${peer_adapter_source[$peer_adapter_key]+x}" ]] || fail "duplicate peer adapter row: $peer_adapter_key"
    [[ "$peer_adapter_id" == "${peer_task[$peer_adapter_key]}" && "$peer_adapter_selection" == "${peer_selection_by_key[$peer_adapter_key]}" &&
      "$peer_adapter_language" == "${peer_language_by_key[$peer_adapter_key]}" ]] || fail "peer adapter identity drifted: $peer_adapter_key"
    safe_path "$peer_adapter_source_path" && safe_path "$peer_adapter_hostile_path" || fail "unsafe peer adapter path: $peer_adapter_key"
    path_exists "$CORPUS/$peer_adapter_source_path" && path_exists "$CORPUS/$peer_adapter_hostile_path" || fail "peer adapter fixture is missing: $peer_adapter_key"
    [[ "$peer_adapter_peer_commit" == "${peer_revision_by_key[$peer_adapter_key]}" && "$peer_adapter_peer_commit" =~ ^[0-9a-f]{40}$ ]] || fail "peer adapter revision is not immutable: $peer_adapter_key"
    peer_adapter_source[$peer_adapter_key]="$peer_adapter_source_path"
    peer_adapter_hostile[$peer_adapter_key]="$peer_adapter_hostile_path"
    peer_adapter_commit[$peer_adapter_key]="$peer_adapter_peer_commit"
  done < "$PEER_ADAPTERS"
  ((${#peer_adapter_source[@]} == ${#peer_seen_key[@]})) || fail "peer adapter ledger does not cover every peer"
  for peer_key in "${!peer_seen_key[@]}"; do [[ -n "${peer_adapter_source[$peer_key]+x}" ]] || fail "peer adapter missing: $peer_key"; done

  declare -gA adapter_jet_source adapter_jet_hostile adapter_peer_source adapter_peer_hostile adapter_peer_commit
  declare -A adapter_seen=()
  local adapter_version adapter_id jet_source jet_hostile peer_source peer_hostile peer_commit
  while IFS=$'\t' read -r adapter_version adapter_id jet_source jet_hostile peer_source peer_hostile peer_commit; do
    [[ -z "$adapter_version" || "$adapter_version" == version ]] && continue
    [[ "$adapter_version" == 1 && -n "${task_domain[$adapter_id]+x}" ]] || fail "bad adapter ledger row: $adapter_id"
    [[ -z "${adapter_seen[$adapter_id]+x}" ]] || fail "duplicate adapter ledger row: $adapter_id"
    safe_path "$jet_source" && safe_path "$jet_hostile" && safe_path "$peer_source" && safe_path "$peer_hostile" || fail "unsafe adapter ledger path: $adapter_id"
    path_exists "$CORPUS/$jet_source" && path_exists "$CORPUS/$jet_hostile" && path_exists "$CORPUS/$peer_source" && path_exists "$CORPUS/$peer_hostile" && [[ -f "$CORPUS/expected/$adapter_id.hostile.out" ]] || fail "adapter ledger fixture is missing: $adapter_id"
    [[ "$jet_hostile" == "$peer_hostile" ]] || fail "hostile input drifted between adapters: $adapter_id"
    [[ "$peer_commit" == "${selected_revision[$adapter_id]}" ]] || fail "adapter peer revision drifted: $adapter_id"
    [[ "$peer_commit" =~ ^[0-9a-f]{40}$ ]] || fail "adapter peer revision is not immutable: $adapter_id"
    adapter_jet_source[$adapter_id]="$jet_source"
    adapter_jet_hostile[$adapter_id]="$jet_hostile"
    adapter_peer_source[$adapter_id]="$peer_source"
    adapter_peer_hostile[$adapter_id]="$peer_hostile"
    adapter_peer_commit[$adapter_id]="$peer_commit"
    adapter_seen[$adapter_id]=1
  done < "$ADAPTERS"
  ((${#adapter_seen[@]} == ${#task_domain[@]})) || fail "adapter ledger count drifted"
  for id in "${!task_domain[@]}"; do [[ -n "${adapter_seen[$id]+x}" ]] || fail "adapter ledger missing task: $id"; done

  declare -gA metric_unit metric_seen
  local metric unit comparison missing_policy
  while IFS=$'\t' read -r version metric unit comparison missing_policy; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -z "${metric_seen[$metric]+x}" ]] || fail "bad or duplicate metric contract row: $metric"
    [[ "$comparison" == lower && "$missing_policy" == fail ]] || fail "metric policy is not fail-closed: $metric"
    metric_seen[$metric]=1; metric_unit[$metric]="$unit"
  done < "$METRIC_CONTRACT"
  ((${#metric_seen[@]} == ${#METRICS[@]})) || fail "metric contract count drifted"
  for metric in "${METRICS[@]}"; do [[ -n "${metric_seen[$metric]+x}" ]] || fail "required metric missing: $metric"; done

  declare -A policy_seen=()
  local policy_version policy_metric samples min_samples min_value outlier_mad_multiplier max_relative_stdev max_outliers tolerance_ratio rust_tolerance_ratio
  while IFS=$'\t' read -r policy_version policy_metric samples min_samples min_value outlier_mad_multiplier max_relative_stdev max_outliers tolerance_ratio rust_tolerance_ratio; do
    [[ -z "$policy_version" || "$policy_version" == version ]] && continue
    [[ "$policy_version" == 1 && -n "${metric_seen[$policy_metric]+x}" ]] || fail "bad measurement policy row: $policy_metric"
    [[ -z "${policy_seen[$policy_metric]+x}" ]] || fail "duplicate measurement policy row: $policy_metric"
    [[ "$samples" =~ ^[1-9][0-9]*$ && "$min_samples" =~ ^[1-9][0-9]*$ && "$min_value" =~ ^[0-9]+([.][0-9]+)?$ && "$outlier_mad_multiplier" =~ ^[0-9]+([.][0-9]+)?$ && "$max_relative_stdev" =~ ^[0-9]+([.][0-9]+)?$ && "$max_outliers" =~ ^[0-9]+$ && "$tolerance_ratio" =~ ^[0-9]+([.][0-9]+)?$ && "$rust_tolerance_ratio" =~ ^[0-9]+([.][0-9]+)?$ ]] || fail "measurement policy value is invalid: $policy_metric"
    [[ "$tolerance_ratio" == 1.00 && "$rust_tolerance_ratio" == 1.05 ]] || fail "peer ratio law drifted: $policy_metric"
    ((min_samples <= samples)) || fail "measurement policy minimum exceeds samples: $policy_metric"
    policy_seen[$policy_metric]=1
    policy_tolerance[$policy_metric]="$tolerance_ratio"
    policy_rust_tolerance[$policy_metric]="$rust_tolerance_ratio"
    policy_samples[$policy_metric]="$samples"
    policy_min_value[$policy_metric]="$min_value"
    policy_outlier_mad_multiplier[$policy_metric]="$outlier_mad_multiplier"
    policy_max_relative_stdev[$policy_metric]="$max_relative_stdev"
    policy_max_outliers[$policy_metric]="$max_outliers"
  done < "$MEASUREMENT_POLICY"
  ((${#policy_seen[@]} == ${#METRICS[@]})) || fail "measurement policy count drifted"
  for metric in "${METRICS[@]}"; do [[ -n "${policy_seen[$metric]+x}" ]] || fail "measurement policy missing: $metric"; done

  declare -gA tier_requirement tier_target tier_tier tier_availability tier_availability_reason tier_rationale tier_seen
  local platform target tier requirement availability availability_reason rationale key
  declare -A global_platform=() global_tier=()
  while IFS=$'\t' read -r version id platform target tier requirement availability availability_reason rationale; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "${task_domain[$id]+x}" ]] || fail "bad tier matrix row: $id"
    [[ "$tier" == aot || "$tier" == jit ]] || fail "bad execution tier: $id/$tier"
    [[ "$requirement" == required || "$requirement" == excluded ]] || fail "bad tier requirement: $id/$tier"
    case "$availability:$availability_reason" in
      local:host-toolchain|local:host-runtime|local:browser-runtime|external:host-platform|external:cross-toolchain|excluded:freestanding-profile) ;;
      *) fail "bad tier availability metadata: $id/$platform/$target/$tier" ;;
    esac
    case "$platform:$target:$tier:$requirement" in
      linux:x86_64-unknown-linux-gnu:aot:required|linux:x86_64-unknown-linux-gnu:jit:required) ;;
      macos:x86_64-apple-darwin:aot:required|windows:x86_64-pc-windows-msvc:aot:required) ;;
      cross-target:aarch64-unknown-linux-gnu:aot:required|cross-target:web:aot:required) ;;
      linux:thumbv7em-none-eabihf:aot:excluded|macos:thumbv7em-none-eabihf:aot:excluded|windows:thumbv7em-none-eabihf:aot:excluded|cross-target:thumbv7em-none-eabihf:aot:excluded|linux:thumbv7em-none-eabihf:jit:excluded) ;;
      *) fail "bad tier target metadata: $id/$platform/$target/$tier" ;;
    esac
    case "$platform:$target:$tier:$requirement" in
      linux:x86_64-unknown-linux-gnu:aot:required) [[ "$availability:$availability_reason" == local:host-toolchain ]] || fail "native Linux AOT availability drifted: $id" ;;
      linux:x86_64-unknown-linux-gnu:jit:required) [[ "$availability:$availability_reason" == local:host-runtime ]] || fail "Linux JIT availability drifted: $id" ;;
      macos:x86_64-apple-darwin:aot:required|windows:x86_64-pc-windows-msvc:aot:required) [[ "$availability:$availability_reason" == external:host-platform ]] || fail "host target availability drifted: $id/$platform" ;;
      cross-target:aarch64-unknown-linux-gnu:aot:required) [[ "$availability:$availability_reason" == external:cross-toolchain ]] || fail "cross-target availability drifted: $id" ;;
      cross-target:web:aot:required) [[ "$availability:$availability_reason" == local:browser-runtime ]] || fail "web target availability drifted: $id" ;;
      *) [[ "$availability:$availability_reason" == excluded:freestanding-profile ]] || fail "excluded target availability drifted: $id/$platform/$target/$tier" ;;
    esac
    key="$id|$platform|$target|$tier"; [[ -z "${tier_seen[$key]+x}" ]] || fail "duplicate tier matrix row: $key"
    tier_seen[$key]=1; tier_requirement[$key]="$requirement"; tier_target[$key]="$target"; tier_tier[$key]="$tier"; tier_availability[$key]="$availability"; tier_availability_reason[$key]="$availability_reason"; tier_rationale[$key]="$rationale"
    global_platform[$platform]=1; global_tier[$tier]=1
    [[ -n "$rationale" ]] || fail "tier rationale missing: $key"
  done < "$TIER_MATRIX"
  for platform in linux macos windows cross-target; do [[ -n "${global_platform[$platform]+x}" ]] || fail "tier platform missing: $platform"; done
  for tier in aot jit; do [[ -n "${global_tier[$tier]+x}" ]] || fail "execution tier missing: $tier"; done

  ((${#tier_seen[@]} == 39)) || fail "tier matrix row count drifted: ${#tier_seen[@]}"
  local embedded_key
  for embedded_key in \
    "embedded-sensor-ring|linux|thumbv7em-none-eabihf|aot" \
    "embedded-sensor-ring|macos|thumbv7em-none-eabihf|aot" \
    "embedded-sensor-ring|windows|thumbv7em-none-eabihf|aot" \
    "embedded-sensor-ring|cross-target|thumbv7em-none-eabihf|aot" \
    "embedded-sensor-ring|linux|thumbv7em-none-eabihf|jit"; do
    [[ "${tier_requirement[$embedded_key]-}" == excluded \
      && "${tier_rationale[$embedded_key]-}" == *"#2046"* \
      && "${tier_rationale[$embedded_key]-}" == *"#2300"* ]] \
      || fail "embedded tier must remain explicitly blocked: $embedded_key"
  done
  embedded_key="cross-platform-notes|cross-target|web|aot"
  [[ "${tier_requirement[$embedded_key]-}" == required ]] || fail "cross-platform web tier must remain required"

  local canary_count=0 canary
  declare -A canary_seen=()
  while IFS=$'\t' read -r version canary mutation required_failure; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "$canary" && -n "$mutation" && -n "$required_failure" ]] || fail "bad removal canary row"
    [[ -z "${canary_seen[$canary]+x}" ]] || fail "duplicate removal canary: $canary"
    canary_seen[$canary]=1
    canary_count=$((canary_count + 1))
  done < "$CANARIES"
  ((canary_count == 20)) || fail "removal canary set is incomplete: expected 20, got $canary_count"
  local required_canary
  for required_canary in \
    missing-outcome unrelated-live-owner missing-metric missing-tier changed-input embedded-review-claim stale-candidate \
    comparison-dependency-drift comparison-toolchain-drift statistics-drift receipt-input-drift \
    unowned-metric-loss required-tier-loss toolchain-drift missing-samples missing-statistics \
    missing-receipt missing-tier-receipt missing-peer-coverage candidate-peer-loss; do
    [[ -n "${canary_seen[$required_canary]+x}" ]] || fail "required removal canary missing: $required_canary"
  done
  echo "compiled workload contract: pass tasks=${#task_domain[@]} peer-rows=${#peer_seen_key[@]} languages=${#peer_languages_seen[@]} metrics=${#metric_seen[@]} tiers=${#tier_seen[@]}"
}

check_report() {
  local report_dir="$1"
  [[ -d "$report_dir" ]] || fail "missing report directory: $report_dir"
  local identity="$report_dir/identity.tsv" samples="$report_dir/samples.tsv" statistics="$report_dir/statistics.tsv"
  local outcomes="$report_dir/outcomes.tsv" measurements="$report_dir/measurements.tsv" tiers="$report_dir/tiers.tsv"
  local receipts="$report_dir/receipts.tsv" tier_receipts="$report_dir/tier_receipts.tsv"
  local peer_coverage="$report_dir/peer_coverage.tsv" peer_measurements="$report_dir/peer_measurements.tsv"
  local review="$report_dir/review.tsv"
  [[ -f "$identity" && -f "$samples" && -f "$statistics" && -f "$outcomes" && -f "$measurements" && -f "$tiers" && -f "$receipts" && -f "$tier_receipts" && -f "$peer_coverage" && -f "$peer_measurements" ]] \
    || fail "report must contain the complete release and all-peer coverage tables"
  [[ -f "$review" ]] || fail "report must contain external review.tsv"
  local current_candidate
  if [[ -n "${JET_COMPILED_WORKLOAD_CANDIDATE_COMMIT:-}" ]]; then
    current_candidate="$JET_COMPILED_WORKLOAD_CANDIDATE_COMMIT"
  else
    current_candidate="$(git rev-parse HEAD 2>/dev/null)" || fail "current candidate commit is unavailable"
  fi
  [[ "$current_candidate" =~ ^[0-9a-f]{40}$ ]] || fail "current candidate commit is not immutable"
  [[ "$(head -n 1 "$identity")" == "$IDENTITY_HEADER" ]] || fail "identity report schema drifted"
  [[ "$(head -n 1 "$samples")" == "$SAMPLE_HEADER" ]] || fail "sample report schema drifted"
  [[ "$(head -n 1 "$statistics")" == "$STATISTICS_HEADER" ]] || fail "statistics report schema drifted"
  [[ "$(head -n 1 "$outcomes")" == "$OUTCOME_HEADER" ]] || fail "outcome report schema drifted"
  [[ "$(head -n 1 "$measurements")" == "$MEASUREMENT_HEADER" ]] || fail "measurement report schema drifted"
  [[ "$(head -n 1 "$tiers")" == "$TIER_REPORT_HEADER" ]] || fail "tier report schema drifted"
  [[ "$(head -n 1 "$receipts")" == "$RECEIPT_HEADER" ]] || fail "receipt report schema drifted"
  [[ "$(head -n 1 "$tier_receipts")" == "$TIER_RECEIPT_HEADER" ]] || fail "tier receipt report schema drifted"
  [[ "$(head -n 1 "$peer_coverage")" == "$PEER_COVERAGE_HEADER" ]] || fail "peer coverage report schema drifted"
  [[ "$(head -n 1 "$peer_measurements")" == "$PEER_MEASUREMENT_HEADER" ]] || fail "peer measurement report schema drifted"
  local expected_fields file header
  for expected_fields in "$identity:3" "$samples:11" "$statistics:18" "$outcomes:17" "$measurements:13" "$tiers:9" "$receipts:20" "$tier_receipts:10" "$peer_coverage:20" "$peer_measurements:17"; do
    file="${expected_fields%:*}"
    header="${expected_fields##*:}"
    awk -F '\t' -v expected="$header" 'NF != expected { bad = 1; print NR ": expected " expected " fields, got " NF > "/dev/stderr" } END { exit bad }' "$file" \
      || fail "report row width drifted: ${file#$report_dir/}"
  done

  declare -A identity_seen=() toolchain_identity_seen=()
  local identity_version identity_key identity_value identity_platform identity_candidate identity_tool_language identity_tool_field
  while IFS=$'\t' read -r identity_version identity_key identity_value; do
    [[ -z "$identity_version" || "$identity_version" == version ]] && continue
    [[ "$identity_version" == 1 && -n "$identity_key" && -n "$identity_value" ]] || fail "incomplete report identity"
    [[ -z "${identity_seen[$identity_key]+x}" ]] || fail "duplicate report identity: $identity_key"
    identity_seen[$identity_key]="$identity_value"
    case "$identity_key" in
      toolchain_*)
        identity_tool_language="${identity_key#toolchain_}"
        identity_tool_field="${identity_tool_language##*_}"
        identity_tool_language="${identity_tool_language%_*}"
        [[ -n "${toolchain_prefix[$identity_tool_language]+x}" ]] || fail "unknown toolchain identity: $identity_key"
        [[ "$identity_tool_field" == path || "$identity_tool_field" == sha256 || "$identity_tool_field" == version ]] \
          || fail "invalid toolchain identity: $identity_key"
        toolchain_identity_seen[$identity_key]="$identity_value"
        ;;
    esac
  done < "$identity"
  for identity_key in candidate_commit platform environment machine jet_tool_version jet_binary_sha256 peer_launcher_path peer_launcher_version peer_launcher_sha256 contract_sha256 source_closure_sha256 samples peer_commits; do
    [[ -n "${identity_seen[$identity_key]+x}" ]] || fail "report identity is missing: $identity_key"
  done
  declare -A required_toolchain_languages=([jet]=1)
  local required_toolchain_language
  for required_toolchain_language in "${!task_domain[@]}"; do
    required_toolchain_languages["${selected_language[$required_toolchain_language]}"]=1
  done
  for required_toolchain_language in "${!required_toolchain_languages[@]}"; do
    for identity_tool_field in path sha256 version; do
      identity_key="toolchain_${required_toolchain_language}_${identity_tool_field}"
      [[ -n "${toolchain_identity_seen[$identity_key]+x}" ]] || fail "report toolchain identity is missing: $identity_key"
    done
    identity_key="toolchain_${required_toolchain_language}_path"
    [[ "${toolchain_identity_seen[$identity_key]}" == /* ]] || fail "report toolchain path is not absolute: $required_toolchain_language"
    identity_key="toolchain_${required_toolchain_language}_sha256"
    is_nonzero_sha256 "${toolchain_identity_seen[$identity_key]}" || fail "report toolchain digest is invalid: $required_toolchain_language"
    identity_key="toolchain_${required_toolchain_language}_version"
    [[ "${toolchain_identity_seen[$identity_key]}" == "${toolchain_prefix[$required_toolchain_language]}"* ]] \
      || fail "report toolchain version is not pinned: $required_toolchain_language"
  done
  for identity_key in "${!identity_seen[@]}"; do
    case "$identity_key" in
      candidate_commit|platform|environment|machine|jet_tool_version|jet_binary_sha256|peer_launcher_path|peer_launcher_version|peer_launcher_sha256|contract_sha256|source_closure_sha256|samples|peer_commits|toolchain_*) ;;
      *) fail "unknown report identity: $identity_key" ;;
    esac
  done
  identity_candidate="${identity_seen[candidate_commit]}"
  [[ "$identity_candidate" == "$current_candidate" ]] || fail "report identity candidate is stale"
  [[ "$identity_candidate" =~ ^[0-9a-f]{40}$ ]] || fail "report identity candidate is not immutable"
  identity_platform="${identity_seen[platform]}"
  case "$identity_platform" in
    linux|macos|windows) ;;
    *) fail "report identity platform is invalid: $identity_platform" ;;
  esac
  [[ "${identity_seen[contract_sha256]}" =~ ^[0-9a-f]{64}$ ]] || fail "report contract identity is invalid"
  [[ "${identity_seen[source_closure_sha256]}" =~ ^[0-9a-f]{64}$ ]] || fail "report source identity is invalid"
  is_nonzero_sha256 "${identity_seen[jet_binary_sha256]}" || fail "report compiler identity is invalid"
  [[ -n "${identity_seen[peer_launcher_path]}" && "${identity_seen[peer_launcher_path]}" == /* ]] \
    || fail "report peer launcher identity is invalid"
  if [[ "$identity_platform" == windows ]]; then
    [[ "${identity_seen[peer_launcher_path],,}" == *.exe ]] || fail "Windows peer launcher identity is not executable"
  else
    [[ "${identity_seen[peer_launcher_path],,}" != *.exe ]] || fail "non-Windows peer launcher identity is a Windows executable"
  fi
  [[ "${identity_seen[peer_launcher_version]}" == compiled-workload-peer-isolation-v1 ]] \
    || fail "report peer launcher contract is invalid"
  is_nonzero_sha256 "${identity_seen[peer_launcher_sha256]}" || fail "report peer launcher digest is invalid"
  [[ "${identity_seen[samples]}" =~ ^[1-9][0-9]*$ ]] || fail "report sample identity is invalid"
  [[ "${identity_seen[samples]}" == "${policy_samples[source_effort]}" ]] || fail "report sample identity disagrees with policy"
  [[ "${identity_seen[environment]}" == "os=$identity_platform;ci=compiled-workload;locale=C;network="* && -n "${identity_seen[machine]}" && -n "${identity_seen[jet_tool_version]}" ]] \
    || fail "report environment identity is incomplete"
  local expected_contract_hash expected_source_closure_hash expected_peer_commit
  expected_contract_hash="$(hash_relative_files \
    tests/compiled_workloads/manifest.tsv \
    tests/compiled_workloads/domain_contract.tsv \
    tests/compiled_workloads/peer_ledger.tsv \
    tests/compiled_workloads/peer_adapter_ledger.tsv \
    tests/compiled_workloads/metric_contract.tsv \
    tests/compiled_workloads/tier_matrix.tsv \
    tests/compiled_workloads/canaries.tsv \
    tests/compiled_workloads/adapter_ledger.tsv \
    tests/compiled_workloads/measurement_policy.tsv \
    tests/compiled_workloads/toolchain_contract.tsv)" \
    || fail "report contract identity could not be recomputed"
  expected_source_closure_hash="$(hash_relative_files \
    tests/compiled_workloads/adapters \
    tests/compiled_workloads/fixtures \
    tests/compiled_workloads/expected \
    tests/compiled_workloads/task-definitions)" \
    || fail "report source identity could not be recomputed"
  [[ "${identity_seen[source_closure_sha256]}" == "$expected_source_closure_hash" ]] || fail "report source identity is stale"
  declare -A report_peer_seen=()
  local peer_entry peer_id peer_revision
  IFS=',' read -ra peer_entries <<< "${identity_seen[peer_commits]}"
  ((${#peer_entries[@]} == ${#task_domain[@]})) || fail "report peer identity denominator drifted"
  for peer_entry in "${peer_entries[@]}"; do
    [[ "$peer_entry" == *:* ]] || fail "report peer identity is malformed"
    peer_id="${peer_entry%%:*}"
    peer_revision="${peer_entry#*:}"
    [[ -n "${task_domain[$peer_id]+x}" && "$peer_revision" == "${selected_revision[$peer_id]}" ]] \
      || fail "report peer identity is invalid: $peer_entry"
    [[ -z "${report_peer_seen[$peer_id]+x}" ]] || fail "duplicate report peer identity: $peer_id"
    report_peer_seen[$peer_id]=1
  done
  for id in "${!task_domain[@]}"; do
    [[ -n "${report_peer_seen[$id]+x}" ]] || fail "report peer identity is incomplete: $id"
  done

  declare -A outcome_seen=() metric_report_seen=() tier_report_seen=() tier_report_status=() tier_report_artifact=() tier_report_output=()
  declare -A outcome_jet_version=() outcome_peer_version=() outcome_jet_status=() outcome_loss_owner=()
  declare -A task_tier_loss=() task_tier_loss_rows=()
  local version id peer_language peer_program input expected outcome toolchain_id jet_tool_version peer_tool_version dependency_rule source_boundary jet_status peer_status loss_owner review_status review_evidence expected_tool_identity
  local report_platform="" report_candidate="" declared_platform declared_candidate
  while IFS=$'\t' read -r version id peer_language peer_program input expected outcome toolchain_id jet_tool_version peer_tool_version dependency_rule source_boundary jet_status peer_status loss_owner review_status review_evidence; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "${task_domain[$id]+x}" ]] || fail "outcome names unknown task: $id"
    [[ -z "${outcome_seen[$id]+x}" ]] || fail "duplicate outcome: $id"
    [[ "$peer_language" == "${selected_language[$id]}" && "$peer_program" == "${selected_program[$id]}" ]] || fail "outcome peer is not selected peer: $id"
    [[ "$input" == "${task_input[$id]}" && "$expected" == "${task_expected[$id]}" && "$outcome" == "${task_outcome[$id]}" ]] || fail "input or outcome drifted: $id"
    [[ "$toolchain_id" == platform=* ]] || fail "comparison platform is missing: $id"
    declared_platform="${toolchain_id#platform=}"
    declared_platform="${declared_platform%%;*}"
    case "$declared_platform" in
      linux|macos|windows) ;;
      *) fail "comparison platform is invalid: $id" ;;
    esac
    [[ "$declared_platform" == "$identity_platform" ]] || fail "report identity platform disagrees: $id"
    [[ "$toolchain_id" =~ (^|;)candidate=([0-9a-f]{40})(;|$) ]] || fail "comparison candidate is missing: $id"
    declared_candidate="${BASH_REMATCH[2]}"
    [[ "$declared_candidate" == "$current_candidate" ]] || fail "comparison candidate is stale: $id"
    if [[ -z "$report_candidate" ]]; then
      report_candidate="$declared_candidate"
    else
      [[ "$declared_candidate" == "$report_candidate" ]] || fail "report mixes candidate commits: $id"
    fi
    if [[ -z "$report_platform" ]]; then
      report_platform="$declared_platform"
    else
      [[ "$declared_platform" == "$report_platform" ]] || fail "report mixes platforms: $id"
    fi
    peer_applies "${selected_targets[$id]}" "$declared_platform" "$declared_platform" || fail "selected peer does not support report platform: $id/$declared_platform"
    [[ -n "$jet_tool_version" && -n "$peer_tool_version" && -n "$dependency_rule" && -n "$source_boundary" ]] || fail "comparison identity incomplete: $id"
    [[ "$jet_tool_version" == "${identity_seen[jet_tool_version]}" && "$jet_tool_version" == "${toolchain_identity_seen[toolchain_jet_version]}" ]] \
      || fail "Jet toolchain identity disagreed with report identity: $id"
    expected_tool_identity="toolchain_${peer_language}_version"
    [[ "$peer_tool_version" == "${toolchain_identity_seen[$expected_tool_identity]}" ]] \
      || fail "peer toolchain identity disagreed with report identity: $id"
    [[ "$dependency_rule" == "${selected_dependency[$id]}" && "$source_boundary" == "${selected_boundary[$id]}" ]] \
      || fail "peer comparison identity drifted: $id"
    [[ "$jet_tool_version" == "${toolchain_prefix[jet]}"* ]] || fail "Jet tool version is not frozen: $id"
    [[ "$peer_tool_version" == "${toolchain_prefix[$peer_language]}"* ]] || fail "peer tool version is not frozen: $id"
    [[ "$jet_status" == pass || "$jet_status" == loss ]] || fail "Jet result is not measured: $id"
    [[ "$peer_status" == pass ]] || fail "peer result is not measured: $id"
    if [[ "$jet_status" == loss ]]; then
      require_manifest_loss_owner "$id" "$loss_owner" "$id"
    else
      [[ "$loss_owner" == - ]] || fail "unexpected Jet loss owner: $id"
    fi
    [[ "$review_status" == pending && "$review_evidence" == "-" ]] || fail "producer report already carries review claims: $id"
    outcome_jet_version[$id]="$jet_tool_version"
    outcome_peer_version[$id]="$peer_tool_version"
    outcome_jet_status[$id]="$jet_status"
    outcome_loss_owner[$id]="$loss_owner"
    outcome_seen[$id]=1
  done < "$outcomes"
  ((${#outcome_seen[@]} == ${#task_domain[@]})) || fail "outcome report does not cover frozen manifest"

  declare -A sample_report_seen=() sample_group_count=() sample_group_values=()
  local sample_version sample_id sample_language sample_metric sample_number sample_value sample_unit sample_method sample_platform sample_target sample_tier sample_key sample_group_key
  while IFS=$'\t' read -r sample_version sample_id sample_language sample_metric sample_number sample_value sample_unit sample_method sample_platform sample_target sample_tier; do
    [[ -z "$sample_version" || "$sample_version" == version ]] && continue
    [[ "$sample_version" == 1 && -n "${task_domain[$sample_id]+x}" ]] || fail "sample names unknown task: $sample_id"
    [[ "$sample_language" == jet || "$sample_language" == "${selected_language[$sample_id]}" ]] || fail "sample language is not Jet or selected peer: $sample_id/$sample_language"
    [[ "$sample_platform" == "$report_platform" && "$sample_target" == native && "$sample_tier" == aot ]] || fail "sample tier or target is not native AOT: $sample_id/$sample_language/$sample_metric"
    [[ -n "${metric_unit[$sample_metric]+x}" && "$sample_unit" == "${metric_unit[$sample_metric]}" ]] || fail "sample unit is invalid: $sample_id/$sample_metric"
    [[ "$sample_number" =~ ^[1-9][0-9]*$ && "$sample_number" -le "${policy_samples[$sample_metric]}" ]] || fail "sample number is invalid: $sample_id/$sample_language/$sample_metric"
    [[ "$sample_value" =~ ^[0-9]+([.][0-9]+)?$ && -n "$sample_method" ]] || fail "sample value is invalid: $sample_id/$sample_language/$sample_metric"
    awk -v value="$sample_value" -v minimum="${policy_min_value[$sample_metric]}" 'BEGIN { exit !(value >= minimum) }' \
      || fail "sample is below policy minimum: $sample_id/$sample_language/$sample_metric"
    sample_key="$sample_id|$sample_language|$sample_metric|$sample_number"
    [[ -z "${sample_report_seen[$sample_key]+x}" ]] || fail "duplicate sample: $sample_key"
    sample_report_seen[$sample_key]=1
    sample_group_key="$sample_id|$sample_language|$sample_metric"
    sample_group_count["$sample_group_key"]=$(( ${sample_group_count[$sample_group_key]:-0} + 1 ))
    sample_group_values["$sample_group_key"]="${sample_group_values[$sample_group_key]-}${sample_value},"
  done < "$samples"
  local sample_id2 sample_language2 sample_metric2
  for sample_id2 in "${!task_domain[@]}"; do
    for sample_language2 in jet "${selected_language[$sample_id2]}"; do
      for sample_metric2 in "${METRICS[@]}"; do
        sample_group_key="$sample_id2|$sample_language2|$sample_metric2"
        [[ "${sample_group_count[$sample_group_key]:-0}" == "${policy_samples[$sample_metric2]}" ]] \
          || fail "missing sample: $sample_group_key"
      done
    done
  done

  local language metric value unit toolchain_id evidence status metric_loss measurement_platform measurement_target measurement_tier expected_unit expected_measurement_tool_identity
  declare -A metric_value_report=() metric_status_report=() metric_owner_report=()
  while IFS=$'\t' read -r version id language metric value unit toolchain_id evidence status metric_loss measurement_platform measurement_target measurement_tier; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 ]] || fail "measurement version drifted: $id"
    [[ -n "${task_domain[$id]+x}" ]] || fail "measurement names unknown task: $id"
    [[ "$language" == jet || "$language" == "${selected_language[$id]}" ]] || fail "measurement language is not Jet or selected peer: $id/$language"
    [[ "$measurement_platform" == "$report_platform" && "$measurement_target" == native && "$measurement_tier" == aot ]] || fail "measurement tier or target is not native AOT: $id/$language/$metric"
    expected_unit="${metric_unit[$metric]-}"
    expected_measurement_tool_identity="toolchain_${language}_version"
    [[ "$toolchain_id" == "${toolchain_identity_seen[$expected_measurement_tool_identity]}" ]] \
      || fail "measurement toolchain identity disagreed with report identity: $id/$language/$metric"
    [[ -n "$expected_unit" && "$unit" == "$expected_unit" && -n "$value" && -n "$toolchain_id" && -n "$evidence" ]] || fail "incomplete measurement: $id/$language/$metric"
    [[ "$value" =~ ^[0-9]+([.][0-9]+)?$ ]] || fail "measurement value is invalid: $id/$language/$metric"
    [[ "$status" == measured || "$status" == loss ]] || fail "measurement is not complete: $id/$language/$metric"
    if [[ "$language" == "${selected_language[$id]}" ]]; then
      [[ "$status" == measured && "$metric_loss" == - ]] || fail "peer measurement cannot be a loss: $id/$metric"
    fi
    if [[ "$status" == loss ]]; then
      require_manifest_loss_owner "$id" "$metric_loss" "$id/$language/$metric"
    else
      [[ "$metric_loss" == - ]] || fail "unexpected metric loss owner: $id/$language/$metric"
    fi
    key="$id|$language|$metric"; [[ -z "${metric_report_seen[$key]+x}" ]] || fail "duplicate measurement: $key"; metric_report_seen[$key]=1
    metric_value_report[$key]="$value"
    metric_status_report[$key]="$status"
    metric_owner_report[$key]="$metric_loss"
  done < "$measurements"
  local id2 language2 metric2
  declare -A task_metric_loss=() task_metric_loss_metrics=()
  for id2 in "${!task_domain[@]}"; do
    for language2 in jet "${selected_language[$id2]}"; do
      for metric2 in "${METRICS[@]}"; do
        [[ -n "${metric_report_seen[$id2|$language2|$metric2]+x}" ]] || fail "missing measurement: $id2/$language2/$metric2"
      done
    done
    for metric2 in "${METRICS[@]}"; do
      local jet_key peer_key jet_value peer_value jet_loses
      jet_key="$id2|jet|$metric2"
      peer_key="$id2|${selected_language[$id2]}|$metric2"
      jet_value="${metric_value_report[$jet_key]}"
      peer_value="${metric_value_report[$peer_key]}"
      jet_loses=0
      if metric_is_loss "${selected_language[$id2]}" "$metric2" "$jet_value" "$peer_value"; then
        jet_loses=1
      fi
      if ((jet_loses)); then
        task_metric_loss[$id2]=1
        task_metric_loss_metrics[$id2]="${task_metric_loss_metrics[$id2]-}${metric2},"
        [[ "${metric_status_report[$jet_key]}" == loss ]] || fail "Jet metric loss is not recorded: $id2/$metric2"
        require_manifest_loss_owner "$id2" "${metric_owner_report[$jet_key]}" "$id2/$metric2"
        [[ "${metric_owner_report[$jet_key]}" == "${outcome_loss_owner[$id2]}" ]] || fail "Jet loss owners disagree: $id2/$metric2"
      else
        [[ "${metric_status_report[$jet_key]}" == measured ]] || fail "Jet metric loss is unsupported by values: $id2/$metric2"
        [[ "${metric_owner_report[$jet_key]}" == - ]] || fail "unexpected Jet metric loss owner: $id2/$metric2"
      fi
    done
    if [[ -n "${task_metric_loss[$id2]+x}" ]]; then
      [[ "${outcome_jet_status[$id2]}" == loss ]] || fail "Jet outcome hides metric loss: $id2"
      require_manifest_loss_owner "$id2" "${outcome_loss_owner[$id2]}" "$id2"
    fi
  done

  declare -A statistics_report_seen=() statistics_median=() statistics_status=() statistics_owner=()
  local stat_version stat_id stat_language stat_metric stat_sample_count stat_median stat_min stat_max stat_relative stat_outliers stat_threshold stat_tolerance stat_status stat_owner stat_evidence stat_platform stat_target stat_tier stat_key expected_stat_tolerance
  while IFS=$'\t' read -r stat_version stat_id stat_language stat_metric stat_sample_count stat_median stat_min stat_max stat_relative stat_outliers stat_threshold stat_tolerance stat_status stat_owner stat_evidence stat_platform stat_target stat_tier; do
    [[ -z "$stat_version" || "$stat_version" == version ]] && continue
    [[ "$stat_version" == 1 && -n "${task_domain[$stat_id]+x}" ]] || fail "statistics names unknown task: $stat_id"
    [[ "$stat_language" == jet || "$stat_language" == "${selected_language[$stat_id]}" ]] || fail "statistics language is not Jet or selected peer: $stat_id/$stat_language"
    [[ "$stat_platform" == "$report_platform" && "$stat_target" == native && "$stat_tier" == aot ]] || fail "statistics tier or target is not native AOT: $stat_id/$stat_language/$stat_metric"
    [[ -n "${metric_unit[$stat_metric]+x}" && "$stat_sample_count" == "${policy_samples[$stat_metric]}" ]] || fail "statistics sample count is invalid: $stat_id/$stat_language/$stat_metric"
    [[ "$stat_median" =~ ^[0-9]+([.][0-9]+)?$ && "$stat_min" =~ ^[0-9]+([.][0-9]+)?$ && "$stat_max" =~ ^[0-9]+([.][0-9]+)?$ && "$stat_relative" =~ ^[0-9]+([.][0-9]+)?$ && "$stat_outliers" =~ ^[0-9]+$ && "$stat_threshold" =~ ^[0-9]+([.][0-9]+)?$ && "$stat_tolerance" =~ ^[0-9]+([.][0-9]+)?$ ]] \
      || fail "statistics values are invalid: $stat_id/$stat_language/$stat_metric"
    [[ "$stat_status" == measured || "$stat_status" == loss ]] || fail "statistics status is invalid: $stat_id/$stat_language/$stat_metric"
    if [[ "$stat_language" == "${selected_language[$stat_id]}" ]]; then
      [[ "$stat_status" == measured && "$stat_owner" == - ]] || fail "peer statistics cannot be a loss: $stat_id/$stat_metric"
    elif [[ "$stat_status" == loss ]]; then
      require_manifest_loss_owner "$stat_id" "$stat_owner" "$stat_id/$stat_metric"
    else
      [[ "$stat_owner" == - ]] || fail "unexpected statistics loss owner: $stat_id/$stat_metric"
    fi
    expected_stat_tolerance="$(policy_tolerance_for_peer "${selected_language[$stat_id]}" "$stat_metric")"
    [[ "$stat_tolerance" == "$expected_stat_tolerance" && -n "$stat_evidence" ]] || fail "statistics policy evidence drifted: $stat_id/$stat_language/$stat_metric"
    sample_group_key="$stat_id|$stat_language|$stat_metric"
    [[ -n "${sample_group_values[$sample_group_key]+x}" ]] || fail "statistics samples are missing: $sample_group_key"
    awk \
      -v csv="${sample_group_values[$sample_group_key]}" \
      -v expected_median="$stat_median" \
      -v expected_min="$stat_min" \
      -v expected_max="$stat_max" \
      -v expected_relative="$stat_relative" \
      -v expected_outliers="$stat_outliers" \
      -v minimum="${policy_min_value[$stat_metric]}" \
      -v multiplier="${policy_outlier_mad_multiplier[$stat_metric]}" \
      -v max_relative="${policy_max_relative_stdev[$stat_metric]}" \
      -v max_outliers="${policy_max_outliers[$stat_metric]}" '
      function abs(value) { return value < 0 ? -value : value }
      function sort_values(array, count, i, j, temp) {
        for (i = 1; i <= count; i++) for (j = i + 1; j <= count; j++) {
          if (array[j] < array[i]) { temp = array[i]; array[i] = array[j]; array[j] = temp }
        }
      }
      function median_values(array, count, middle) {
        middle = int((count + 1) / 2)
        if (count % 2) return array[middle]
        return (array[middle] + array[middle + 1]) / 2
      }
      BEGIN {
        count = split(csv, raw, ",")
        if (raw[count] == "") count--
        if (count < 1) exit 1
        for (i = 1; i <= count; i++) ordered[i] = raw[i] + 0
        sort_values(ordered, count)
        med = median_values(ordered, count)
        min = ordered[1]
        max = ordered[count]
        sum = 0
        for (i = 1; i <= count; i++) sum += ordered[i]
        mean = sum / count
        variance = 0
        for (i = 1; i <= count; i++) variance += (ordered[i] - mean) * (ordered[i] - mean)
        relative = mean == 0 ? 0 : sqrt(variance / count) / abs(mean)
        for (i = 1; i <= count; i++) deviations[i] = abs(ordered[i] - med)
        sort_values(deviations, count)
        mad = median_values(deviations, count)
        limit = mad == 0 ? 0 : mad * multiplier
        outliers = 0
        for (i = 1; i <= count; i++) if (mad == 0 ? ordered[i] != med : abs(ordered[i] - med) > limit) outliers++
        epsilon = 1e-9
        if (min + epsilon < minimum || relative > max_relative + epsilon || outliers > max_outliers) exit 1
        if (abs(med - expected_median) > epsilon * (1 + abs(expected_median))) exit 1
        if (abs(min - expected_min) > epsilon * (1 + abs(expected_min))) exit 1
        if (abs(max - expected_max) > epsilon * (1 + abs(expected_max))) exit 1
        if (abs(relative - expected_relative) > epsilon * (1 + abs(expected_relative))) exit 1
        if (outliers != expected_outliers) exit 1
      }' \
      || fail "statistics disagree with samples: $sample_group_key"
    stat_key="$stat_id|$stat_language|$stat_metric"
    [[ -z "${statistics_report_seen[$stat_key]+x}" ]] || fail "duplicate statistics: $stat_key"
    statistics_report_seen[$stat_key]=1
    statistics_median[$stat_key]="$stat_median"
    statistics_status[$stat_key]="$stat_status"
    statistics_owner[$stat_key]="$stat_owner"
  done < "$statistics"
  for id2 in "${!task_domain[@]}"; do
    for language2 in jet "${selected_language[$id2]}"; do
      for metric2 in "${METRICS[@]}"; do
        stat_key="$id2|$language2|$metric2"
        [[ -n "${statistics_report_seen[$stat_key]+x}" ]] || fail "missing statistics: $stat_key"
        [[ "${statistics_median[$stat_key]}" == "${metric_value_report[$stat_key]}" ]] || fail "statistics median disagrees with measurement: $stat_key"
        [[ "${statistics_status[$stat_key]}" == "${metric_status_report[$stat_key]}" && "${statistics_owner[$stat_key]}" == "${metric_owner_report[$stat_key]}" ]] \
          || fail "statistics status disagrees with measurement: $stat_key"
      done
    done
  done

  declare -A peer_coverage_seen=() peer_measurement_seen=() peer_measurement_median=()
  local peer_key coverage_version coverage_task coverage_selection coverage_language coverage_program coverage_source coverage_revision
  local coverage_source_hash coverage_input_hash coverage_expected_hash coverage_output_hash coverage_hostile_input_hash coverage_hostile_output_hash
  local coverage_status coverage_platform coverage_target coverage_tier coverage_command coverage_evidence expected_hash expected_hostile_hash actual_hash
  while IFS=$'\t' read -r coverage_version peer_key coverage_task coverage_selection coverage_language coverage_program coverage_source coverage_revision \
    coverage_source_hash coverage_input_hash coverage_expected_hash coverage_output_hash coverage_hostile_input_hash coverage_hostile_output_hash \
    coverage_status coverage_platform coverage_target coverage_tier coverage_command coverage_evidence; do
    [[ -z "$coverage_version" || "$coverage_version" == version ]] && continue
    [[ "$coverage_version" == 1 && -n "${peer_seen_key[$peer_key]+x}" ]] || fail "peer coverage names unknown ledger row: $peer_key"
    [[ -z "${peer_coverage_seen[$peer_key]+x}" ]] || fail "duplicate peer coverage: $peer_key"
    [[ "$coverage_task" == "${peer_task[$peer_key]}" && "$coverage_selection" == "${peer_selection_by_key[$peer_key]}" &&
      "$coverage_language" == "${peer_language_by_key[$peer_key]}" && "$coverage_program" == "${peer_program_by_key[$peer_key]}" ]] \
      || fail "peer coverage identity drifted: $peer_key"
    [[ "$coverage_source" == "${peer_adapter_source[$peer_key]}" && "$coverage_revision" == "${peer_revision_by_key[$peer_key]}" ]] \
      || fail "peer coverage source identity drifted: $peer_key"
    actual_hash="$(hash_input_path "$CORPUS/$coverage_source")" || fail "peer coverage source could not be hashed: $peer_key"
    [[ "$coverage_source_hash" == "$actual_hash" ]] || fail "peer coverage source hash drifted: $peer_key"
    actual_hash="$(hash_input_path "$CORPUS/${task_input[$coverage_task]}")" || fail "peer coverage input could not be hashed: $peer_key"
    [[ "$coverage_input_hash" == "$actual_hash" ]] || fail "peer coverage input hash drifted: $peer_key"
    expected_hash="$(file_sha256 "$CORPUS/${task_expected[$coverage_task]}")"
    expected_hostile_hash="$(file_sha256 "$CORPUS/expected/${coverage_task}.hostile.out")"
    [[ "$coverage_expected_hash" == "$expected_hash" ]] || fail "peer coverage expected hash drifted: $peer_key"
    actual_hash="$(hash_input_path "$CORPUS/${peer_adapter_hostile[$peer_key]}")" || fail "peer coverage hostile input could not be hashed: $peer_key"
    [[ "$coverage_hostile_input_hash" == "$actual_hash" ]] || fail "peer coverage hostile input hash drifted: $peer_key"
    [[ "$coverage_platform" == "$report_platform" && "$coverage_target" == - && "$coverage_tier" == native && -n "$coverage_command" && -n "$coverage_evidence" ]] \
      || fail "peer coverage platform or evidence is incomplete: $peer_key"
    if peer_applies "${peer_targets_by_key[$peer_key]}" "$report_platform" "$report_platform"; then
      [[ "$coverage_status" == measured ]] || fail "applicable peer coverage is not measured: $peer_key"
      is_nonzero_sha256 "$coverage_output_hash" || fail "peer coverage output hash is invalid: $peer_key"
      is_nonzero_sha256 "$coverage_hostile_output_hash" || fail "peer coverage hostile output hash is invalid: $peer_key"
      [[ "$coverage_output_hash" == "$coverage_expected_hash" && "$coverage_hostile_output_hash" == "$expected_hostile_hash" ]] \
        || fail "peer coverage output drifted: $peer_key"
      [[ "$coverage_command" == build=* && "$coverage_command" == *';run='* && "$coverage_command" == *';hostile='* ]] \
        || fail "peer coverage execution receipt is incomplete: $peer_key"
      [[ "$coverage_evidence" == toolchain=* && "$coverage_evidence" == *';source-sha256='* &&
        "$coverage_evidence" == *';input-sha256='* && "$coverage_evidence" == *';expected-sha256='* &&
        "$coverage_evidence" == *';output-sha256='* && "$coverage_evidence" == *';hostile-input-sha256='* &&
        "$coverage_evidence" == *';hostile-output-sha256='* && "$coverage_evidence" == *';diagnostic='* ]] \
        || fail "peer coverage evidence is incomplete: $peer_key"
    else
      [[ "$coverage_status" == not-applicable && "$coverage_output_hash" == - && "$coverage_hostile_output_hash" == - &&
        "$coverage_command" == not-run:host-platform && "$coverage_evidence" == peer-targets=* && "$coverage_evidence" == *';reason=host-platform' ]] \
        || fail "non-applicable peer coverage is not explicit: $peer_key"
    fi
    peer_coverage_seen[$peer_key]=1
  done < "$peer_coverage"

  local measurement_version measurement_peer_key measurement_task measurement_selection measurement_language measurement_metric
  local measurement_samples measurement_median measurement_min measurement_max measurement_relative measurement_unit measurement_status
  local measurement_evidence measurement_platform measurement_target measurement_tier measurement_toolchain
  while IFS=$'\t' read -r measurement_version measurement_peer_key measurement_task measurement_selection measurement_language measurement_metric \
    measurement_samples measurement_median measurement_min measurement_max measurement_relative measurement_unit measurement_status \
    measurement_evidence measurement_platform measurement_target measurement_tier; do
    [[ -z "$measurement_version" || "$measurement_version" == version ]] && continue
    [[ "$measurement_version" == 1 && -n "${peer_seen_key[$measurement_peer_key]+x}" ]] || fail "peer measurement names unknown ledger row: $measurement_peer_key"
    [[ "$measurement_task" == "${peer_task[$measurement_peer_key]}" && "$measurement_selection" == "${peer_selection_by_key[$measurement_peer_key]}" &&
      "$measurement_language" == "${peer_language_by_key[$measurement_peer_key]}" ]] || fail "peer measurement identity drifted: $measurement_peer_key"
    [[ -n "${metric_unit[$measurement_metric]+x}" && "$measurement_unit" == "${metric_unit[$measurement_metric]}" ]] || fail "peer measurement unit is invalid: $measurement_peer_key/$measurement_metric"
    [[ "$measurement_platform" == "$report_platform" && "$measurement_target" == - && "$measurement_tier" == native ]] || fail "peer measurement platform or tier drifted: $measurement_peer_key/$measurement_metric"
    measurement_toolchain="toolchain_${measurement_language}_version"
    if peer_applies "${peer_targets_by_key[$measurement_peer_key]}" "$report_platform" "$report_platform"; then
      [[ "$measurement_status" == measured && "$measurement_samples" == "${policy_samples[$measurement_metric]}" &&
        "$measurement_median" =~ ^[0-9]+([.][0-9]+)?$ && "$measurement_min" =~ ^[0-9]+([.][0-9]+)?$ &&
        "$measurement_max" =~ ^[0-9]+([.][0-9]+)?$ && "$measurement_relative" =~ ^[0-9]+([.][0-9]+)?$ &&
        "$measurement_evidence" == *"toolchain=${toolchain_identity_seen[$measurement_toolchain]}"* ]] \
        || fail "peer measurement is incomplete: $measurement_peer_key/$measurement_metric"
      [[ "${peer_coverage_seen[$measurement_peer_key]+x}" ]] || fail "peer measurement lacks coverage receipt: $measurement_peer_key"
      peer_measurement_median["$measurement_peer_key|$measurement_metric"]="$measurement_median"
    else
      [[ "$measurement_status" == not-applicable && "$measurement_samples" == 0 &&
        "$measurement_median" == - && "$measurement_min" == - && "$measurement_max" == - &&
        "$measurement_relative" == - && "$measurement_evidence" == peer-targets=* &&
        "$measurement_evidence" == *";reason=host-platform" ]] \
        || fail "non-applicable peer measurement is incomplete: $measurement_peer_key/$measurement_metric"
    fi
    key="$measurement_peer_key|$measurement_metric"
    [[ -z "${peer_measurement_seen[$key]+x}" ]] || fail "duplicate peer measurement: $key"
    peer_measurement_seen[$key]=1
  done < "$peer_measurements"
  for peer_key in "${!peer_seen_key[@]}"; do
    [[ -n "${peer_coverage_seen[$peer_key]+x}" ]] || fail "missing peer coverage: $peer_key"
    for metric in "${METRICS[@]}"; do
      [[ -n "${peer_measurement_seen[$peer_key|$metric]+x}" ]] || fail "missing peer measurement: $peer_key/$metric"
    done
  done
  local comparison_peer_task comparison_peer_language comparison_jet_key comparison_peer_metric_key
  local comparison_jet_value comparison_peer_value
  for peer_key in "${!peer_seen_key[@]}"; do
    comparison_peer_task="${peer_task[$peer_key]}"
    if ! peer_applies "${peer_targets_by_key[$peer_key]}" "$report_platform" "$report_platform"; then
      continue
    fi
    comparison_peer_language="${peer_language_by_key[$peer_key]}"
    for metric in "${METRICS[@]}"; do
      comparison_jet_key="$comparison_peer_task|jet|$metric"
      comparison_peer_metric_key="$peer_key|$metric"
      comparison_jet_value="${metric_value_report[$comparison_jet_key]-}"
      comparison_peer_value="${peer_measurement_median[$comparison_peer_metric_key]-}"
      [[ -n "$comparison_jet_value" && -n "$comparison_peer_value" ]] \
        || fail "peer comparison is missing a required cell: $peer_key/$metric"
      if metric_is_loss "$comparison_peer_language" "$metric" "$comparison_jet_value" "$comparison_peer_value"; then
        fail "peer comparison loss: $peer_key/$metric (Jet=$comparison_jet_value peer=$comparison_peer_value)"
      fi
    done
  done

  local report_id platform target tier tier_status tier_evidence tier_loss
  while IFS=$'\t' read -r version id language platform target tier tier_status tier_evidence tier_loss; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 ]] || fail "tier report version drifted: $id"
    key="$id|$platform|$target|$tier"
    [[ -n "${tier_seen[$key]+x}" ]] || fail "tier report names undeclared row: $key"
    [[ "$language" == jet || "$language" == "${selected_language[$id]}" ]] || fail "tier report language is not Jet or selected peer: $key"
    [[ "$tier_status" == pass || "$tier_status" == loss || "$tier_status" == not-applicable ]] || fail "bad tier status: $key"
    local expected_tier_status in_scope tier_availability_value tier_reason_value
    tier_availability_value="${tier_availability[$key]-}"
    tier_reason_value="${tier_availability_reason[$key]-}"
    [[ -n "$tier_availability_value" && -n "$tier_reason_value" ]] || fail "tier availability metadata is missing: $key"
    in_scope=0
    [[ "$platform" == "$report_platform" || ( "$report_platform" == linux && "$platform" == cross-target ) ]] && in_scope=1
    if [[ "$in_scope" -eq 0 || "${tier_requirement[$key]}" == excluded ]]; then
      expected_tier_status=not-applicable
    elif [[ "$language" == jet ]]; then
      expected_tier_status=measured
    elif [[ "$tier" == jit ]]; then
      expected_tier_status=not-applicable
    elif ! peer_applies "${selected_targets[$id]}" "$platform" "$target"; then
      expected_tier_status=not-applicable
    else
      expected_tier_status=measured
    fi
    if [[ "$expected_tier_status" == not-applicable ]]; then
      [[ "$tier_status" == not-applicable ]] || fail "tier applicability drifted: $key/$language"
      [[ "$tier_evidence" == not-run:* ]] || fail "tier exclusion evidence is missing: $key/$language"
      if [[ "${tier_requirement[$key]}" == excluded ]]; then
        [[ "$tier_evidence" == *"availability=excluded;reason=freestanding-profile"* ]] \
          || fail "excluded tier reason is missing: $key/$language"
      elif [[ "$in_scope" -eq 0 ]]; then
        [[ "$tier_evidence" == *"availability=${tier_availability_value};reason=${tier_reason_value}"* ]] \
          || fail "external tier reason is missing: $key/$language"
      elif [[ "$tier" == jit ]]; then
        [[ "$tier_evidence" == *"reason=peer-native-only" ]] \
          || fail "peer tier reason is missing: $key/$language"
      else
        [[ "$tier_evidence" == *"reason=peer-target-not-declared" ]] \
          || fail "peer target reason is missing: $key/$language"
      fi
    else
      [[ "$tier_status" == pass || "$tier_status" == loss ]] || fail "tier proof missing: $key/$language"
    fi
    [[ -n "$tier_evidence" ]] || fail "tier evidence missing: $key"
    if [[ "$tier_status" == loss ]]; then
      [[ "$language" == jet ]] || fail "peer tier cannot be a loss: $key/$language"
      require_manifest_loss_owner "$id" "$tier_loss" "$key"
      task_tier_loss[$id]=1
      task_tier_loss_rows[$id]="${task_tier_loss_rows[$id]-}${platform}/${target}/${tier},"
      [[ "${outcome_jet_status[$id]}" == loss ]] || fail "Jet outcome hides tier loss: $key"
      [[ "${outcome_loss_owner[$id]}" == "$tier_loss" ]] || fail "Jet tier and outcome loss owners disagree: $key"
    else
      [[ "$tier_loss" == - ]] || fail "unexpected tier loss owner: $key/$language"
    fi
    report_id="$key|$language"
    local tier_artifact_value tier_output_value
    if [[ "$tier_status" == pass || "$tier_status" == loss ]]; then
      [[ "$tier_evidence" =~ (^|;)artifact=(-|[0-9a-f]{64})(;|$) ]] || fail "tier artifact evidence is missing: $report_id"
      tier_artifact_value="${BASH_REMATCH[2]}"
      if [[ "$tier" == jit && "$language" == jet ]]; then
        [[ "$tier_artifact_value" == - ]] || is_nonzero_sha256 "$tier_artifact_value" \
          || fail "Jet JIT tier artifact must be absent or nonzero: $report_id"
      else
        is_nonzero_sha256 "$tier_artifact_value" || fail "tier artifact evidence is empty: $report_id"
      fi
      tier_report_artifact[$report_id]="$tier_artifact_value"
      [[ "$tier_evidence" =~ (^|;)output=(-|[0-9a-f]{64})(;|$) ]] || fail "tier output evidence is missing: $report_id"
      tier_output_value="${BASH_REMATCH[2]}"
      tier_report_output[$report_id]="$tier_output_value"
      if [[ "$tier" == jit && "$language" == jet ]]; then
        is_nonzero_sha256 "$tier_output_value" || fail "Jet JIT tier output is required: $report_id"
      elif [[ "$platform" != cross-target || "$target" == web ]]; then
        is_nonzero_sha256 "${tier_report_output[$report_id]}" || fail "tier output evidence is required: $report_id"
      fi
    fi
    [[ -z "${tier_report_seen[$report_id]+x}" ]] || fail "duplicate tier report: $report_id"
    tier_report_seen[$report_id]=1
    tier_report_status[$report_id]="$tier_status"
  done < "$tiers"
  local req_key
  for id2 in "${!task_domain[@]}"; do
    if [[ -n "${task_metric_loss[$id2]+x}" || -n "${task_tier_loss[$id2]+x}" ]]; then
      [[ "${outcome_jet_status[$id2]}" == loss ]] || fail "Jet outcome hides measured loss: $id2"
      require_manifest_loss_owner "$id2" "${outcome_loss_owner[$id2]}" "$id2"
    else
      [[ "${outcome_jet_status[$id2]}" == pass && "${outcome_loss_owner[$id2]}" == - ]] \
        || fail "Jet outcome contradicts measured results: $id2"
    fi
  done
  for req_key in "${!tier_seen[@]}"; do
    for language2 in jet "${selected_language[${req_key%%|*}]}"; do
      [[ -n "${tier_report_seen[$req_key|$language2]+x}" ]] || fail "missing tier proof: $req_key/$language2"
    done
  done

  declare -A receipt_seen=()
  local receipt_version receipt_id receipt_language source_sha input_sha expected_sha output_sha hostile_input_sha hostile_output_sha receipt_environment receipt_machine receipt_tool receipt_command receipt_peer_commit exit_code hostile_exit_code receipt_launcher_path receipt_launcher_version receipt_launcher_sha receipt_authority receipt_key expected_source expected_hostile
  while IFS=$'\t' read -r receipt_version receipt_id receipt_language source_sha input_sha expected_sha output_sha hostile_input_sha hostile_output_sha receipt_environment receipt_machine receipt_tool receipt_command receipt_peer_commit exit_code hostile_exit_code receipt_launcher_path receipt_launcher_version receipt_launcher_sha receipt_authority; do
    [[ -z "$receipt_version" || "$receipt_version" == version ]] && continue
    [[ "$receipt_version" == 1 && -n "${task_domain[$receipt_id]+x}" ]] || fail "receipt names unknown task: $receipt_id"
    [[ "$receipt_language" == jet || "$receipt_language" == "${selected_language[$receipt_id]}" ]] || fail "receipt language is not Jet or selected peer: $receipt_id/$receipt_language"
    [[ "$receipt_launcher_path" == "${identity_seen[peer_launcher_path]}" && "$receipt_launcher_version" == "${identity_seen[peer_launcher_version]}" && "$receipt_launcher_sha" == "${identity_seen[peer_launcher_sha256]}" ]] \
      || fail "receipt peer launcher identity drifted: $receipt_id/$receipt_language"
    [[ "$receipt_authority" == "${task_authority[$receipt_id]}" ]] \
      || fail "receipt authority was not enforced: $receipt_id/$receipt_language"
    [[ "$receipt_command" == *";source-hash-method=$(hash_input_method "$CORPUS/$([[ "$receipt_language" == jet ]] && printf '%s' "${adapter_jet_source[$receipt_id]}" || printf '%s' "${adapter_peer_source[$receipt_id]}")")"* ]] \
      || fail "receipt source mode normalization is missing: $receipt_id/$receipt_language"
    [[ "$receipt_command" == *";input-hash-method=$(hash_input_method "$CORPUS/${task_input[$receipt_id]}")"* ]] \
      || fail "receipt input mode normalization is missing: $receipt_id/$receipt_language"
    [[ "$receipt_command" == *";hostile-input-hash-method=$(hash_input_method "$CORPUS/$([[ "$receipt_language" == jet ]] && printf '%s' "${adapter_jet_hostile[$receipt_id]}" || printf '%s' "${adapter_peer_hostile[$receipt_id]}")")"* ]] \
      || fail "receipt hostile mode normalization is missing: $receipt_id/$receipt_language"
    [[ "$receipt_environment" == "os=$report_platform;ci=compiled-workload;locale=C;network=${task_network[$receipt_id]}" && -n "$receipt_machine" && -n "$receipt_tool" && -n "$receipt_command" ]] \
      || fail "receipt identity is incomplete: $receipt_id/$receipt_language"
    [[ "$receipt_machine" != *fixture* && "$receipt_tool" != *fixture* && "$receipt_command" != fixture && "$receipt_command" != fixture:* ]] \
      || fail "receipt provenance is synthetic: $receipt_id/$receipt_language"
    if [[ "$receipt_language" == jet ]]; then
      [[ "$receipt_tool" == "${outcome_jet_version[$receipt_id]-}" ]] || fail "Jet receipt tool identity drifted: $receipt_id"
    else
      [[ "$receipt_tool" == "${outcome_peer_version[$receipt_id]-}" ]] || fail "peer receipt tool identity drifted: $receipt_id"
    fi
    [[ "$receipt_peer_commit" == "${selected_revision[$receipt_id]}" && "$exit_code" == 0 && "$hostile_exit_code" == 0 ]] || fail "receipt execution identity is invalid: $receipt_id/$receipt_language"
    if [[ "$receipt_language" == jet ]]; then
      expected_source="${adapter_jet_source[$receipt_id]}"
      expected_hostile="${adapter_jet_hostile[$receipt_id]}"
    else
      expected_source="${adapter_peer_source[$receipt_id]}"
      expected_hostile="${adapter_peer_hostile[$receipt_id]}"
    fi
    [[ "$source_sha" == "$(hash_input_path "$CORPUS/$expected_source")" ]] || fail "receipt source drifted: $receipt_id/$receipt_language"
    [[ "$input_sha" == "$(hash_input_path "$CORPUS/${task_input[$receipt_id]}")" ]] || fail "receipt input drifted: $receipt_id/$receipt_language"
    [[ "$expected_sha" == "$(file_sha256 "$CORPUS/${task_expected[$receipt_id]}")" && "$output_sha" == "$expected_sha" ]] || fail "receipt normal output drifted: $receipt_id/$receipt_language"
    [[ "$hostile_input_sha" == "$(hash_input_path "$CORPUS/$expected_hostile")" ]] || fail "receipt hostile input drifted: $receipt_id/$receipt_language"
    [[ "$hostile_output_sha" == "$(file_sha256 "$CORPUS/expected/$receipt_id.hostile.out")" ]] || fail "receipt hostile output drifted: $receipt_id/$receipt_language"
    receipt_key="$receipt_id|$receipt_language"
    [[ -z "${receipt_seen[$receipt_key]+x}" ]] || fail "duplicate receipt: $receipt_key"
    receipt_seen[$receipt_key]=1
  done < "$receipts"
  for id2 in "${!task_domain[@]}"; do
    for language2 in jet "${selected_language[$id2]}"; do
      [[ -n "${receipt_seen[$id2|$language2]+x}" ]] || fail "missing receipt: $id2/$language2"
    done
  done

  declare -A tier_receipt_seen=() tier_receipt_status=()
  local tier_receipt_version tier_receipt_id tier_receipt_language tier_receipt_platform tier_receipt_target tier_receipt_tier tier_artifact_sha tier_output_sha tier_receipt_command tier_receipt_result tier_receipt_key report_tier_key
  while IFS=$'\t' read -r tier_receipt_version tier_receipt_id tier_receipt_language tier_receipt_platform tier_receipt_target tier_receipt_tier tier_artifact_sha tier_output_sha tier_receipt_command tier_receipt_result; do
    [[ -z "$tier_receipt_version" || "$tier_receipt_version" == version ]] && continue
    [[ "$tier_receipt_version" == 1 ]] || fail "tier receipt version drifted: $tier_receipt_id"
    report_tier_key="$tier_receipt_id|$tier_receipt_platform|$tier_receipt_target|$tier_receipt_tier"
    [[ -n "${tier_seen[$report_tier_key]+x}" ]] || fail "tier receipt names undeclared row: $report_tier_key"
    [[ "$tier_receipt_language" == jet || "$tier_receipt_language" == "${selected_language[$tier_receipt_id]}" ]] || fail "tier receipt language is invalid: $report_tier_key/$tier_receipt_language"
    [[ "$tier_receipt_result" == pass || "$tier_receipt_result" == loss || "$tier_receipt_result" == not-applicable ]] || fail "tier receipt status is invalid: $report_tier_key"
    [[ "$tier_artifact_sha" == - || "$tier_artifact_sha" =~ ^[0-9a-f]{64}$ ]] || fail "tier receipt artifact hash is invalid: $report_tier_key"
    [[ "$tier_output_sha" == - || "$tier_output_sha" =~ ^[0-9a-f]{64}$ ]] || fail "tier receipt output hash is invalid: $report_tier_key"
    if [[ "$tier_receipt_result" == pass || "$tier_receipt_result" == loss ]]; then
      if [[ "$tier_receipt_tier" == jit && "$tier_receipt_language" == jet ]]; then
        [[ "$tier_artifact_sha" == - ]] || is_nonzero_sha256 "$tier_artifact_sha" \
          || fail "Jet JIT tier receipt artifact must be absent or nonzero: $report_tier_key"
      else
        is_nonzero_sha256 "$tier_artifact_sha" || fail "tier receipt artifact hash is missing: $report_tier_key/$tier_receipt_language"
      fi
      [[ "$tier_artifact_sha" == "${tier_report_artifact[$report_tier_key|$tier_receipt_language]}" ]] || fail "tier artifact receipt disagrees: $report_tier_key/$tier_receipt_language"
      if [[ "$tier_receipt_tier" == jit && "$tier_receipt_language" == jet ]]; then
        is_nonzero_sha256 "$tier_output_sha" || fail "Jet JIT tier receipt output hash is missing: $report_tier_key"
        [[ "$tier_output_sha" == "${tier_report_output[$report_tier_key|$tier_receipt_language]}" ]] || fail "tier output receipt disagrees: $report_tier_key/$tier_receipt_language"
      elif [[ "$tier_receipt_platform" != cross-target || "$tier_receipt_target" == web ]]; then
        is_nonzero_sha256 "$tier_output_sha" || fail "tier receipt output hash is missing: $report_tier_key/$tier_receipt_language"
        [[ "$tier_output_sha" == "${tier_report_output[$report_tier_key|$tier_receipt_language]}" ]] || fail "tier output receipt disagrees: $report_tier_key/$tier_receipt_language"
      else
        [[ "$tier_output_sha" == - && "${tier_report_output[$report_tier_key|$tier_receipt_language]}" == - ]] || fail "cross-target output receipt is invalid: $report_tier_key/$tier_receipt_language"
      fi
    else
      [[ "$tier_artifact_sha" == - && "$tier_output_sha" == - ]] || fail "not-applicable tier receipt has artifacts: $report_tier_key/$tier_receipt_language"
    fi
    if [[ "$tier_receipt_result" == pass || "$tier_receipt_result" == loss ]]; then
      [[ "$tier_receipt_command" != not-run:* ]] || fail "measured tier was not executed: $report_tier_key/$tier_receipt_language"
    else
      [[ "$tier_receipt_command" == not-run:* ]] || fail "not-applicable tier command is not declared: $report_tier_key/$tier_receipt_language"
    fi
    [[ -n "$tier_receipt_command" ]] || fail "tier receipt command is missing: $report_tier_key"
    tier_receipt_key="$report_tier_key|$tier_receipt_language"
    [[ -z "${tier_receipt_seen[$tier_receipt_key]+x}" ]] || fail "duplicate tier receipt: $tier_receipt_key"
    tier_receipt_seen[$tier_receipt_key]=1
    tier_receipt_status[$tier_receipt_key]="$tier_receipt_result"
  done < "$tier_receipts"
  for report_tier_key in "${!tier_seen[@]}"; do
    for language2 in jet "${selected_language[${report_tier_key%%|*}]}"; do
      [[ -n "${tier_receipt_seen[$report_tier_key|$language2]+x}" ]] || fail "missing tier receipt: $report_tier_key/$language2"
      [[ "${tier_receipt_status[$report_tier_key|$language2]}" == "${tier_report_status[$report_tier_key|$language2]}" ]] || fail "tier receipt status disagrees: $report_tier_key/$language2"
    done
  done
  local review_version review_reviewer reviewed_candidate review_compiler review_contract review_source review_digest
  local workload_fairness peer_fairness authority_fairness measurement_fairness tier_fairness loss_ownership review_verdict review_evidence
  local review_workflow review_run review_actor
  local review_rows=0 expected_report_digest
  [[ "$(head -n 1 "$review")" == "$REVIEW_HEADER" ]] || fail "external review schema drifted"
  awk -F '\t' -v expected=18 'NF != expected { bad = 1; print NR ": expected " expected " fields, got " NF > "/dev/stderr" } END { exit bad }' "$review" \
    || fail "external review row width drifted"
  while IFS=$'\t' read -r review_version review_reviewer reviewed_candidate review_compiler review_contract review_source review_digest workload_fairness peer_fairness authority_fairness measurement_fairness tier_fairness loss_ownership review_verdict review_evidence review_workflow review_run review_actor; do
    [[ -z "$review_version" || "$review_version" == version ]] && continue
    review_rows=$((review_rows + 1))
    [[ "$review_version" == 1 && -n "$review_reviewer" && -n "$review_evidence" ]] || fail "external review row is incomplete"
    reviewer_is_independent "$review_reviewer" || fail "external review reviewer is not independent"
    [[ "$review_workflow" == "compiled-workload-review.yml" && "$review_run" =~ ^([1-9][0-9]*|self-check-[1-9][0-9]*)$ && "$review_actor" =~ ^[A-Za-z0-9_.-]+$ && "$review_actor" != *synthetic* ]] \
      || fail "external review provenance is not authenticated"
    [[ "$review_evidence" == *"workflow=$review_workflow"* && "$review_evidence" == *"run=$review_run"* && "$review_evidence" == *"actor=$review_actor"* ]] \
      || fail "external review provenance evidence is incomplete"
    [[ "$reviewed_candidate" == "$current_candidate" && "$reviewed_candidate" =~ ^[0-9a-f]{40}$ ]] \
      || fail "external review candidate is stale or invalid"
    is_nonzero_sha256 "$review_compiler" && [[ "$review_compiler" == "${identity_seen[jet_binary_sha256]}" ]] \
      || fail "external review compiler identity disagrees"
    [[ "$review_contract" == "${identity_seen[contract_sha256]}" ]] || fail "external review contract identity disagrees"
    [[ "$review_source" == "${identity_seen[source_closure_sha256]}" ]] || fail "external review source identity disagrees"
    expected_report_digest="$(hash_report_files "$report_dir")" || fail "report digest could not be recomputed"
    [[ "$review_digest" == "$expected_report_digest" ]] || fail "external review report digest disagrees"
    [[ "$workload_fairness" == pass && "$peer_fairness" == pass && "$authority_fairness" == pass \
      && "$measurement_fairness" == pass && "$tier_fairness" == pass && "$loss_ownership" == pass && "$review_verdict" == pass ]] \
      || fail "external review fairness or status is not pass"
  done < "$review"
  ((review_rows == 1)) || fail "external review must contain exactly one row"
  if ((${#task_metric_loss[@]} > 0 || ${#task_tier_loss[@]} > 0)); then
    local loss_summary="" loss_id
    for loss_id in "${!task_metric_loss[@]}"; do
      loss_summary+="${loss_summary:+;}${loss_id}:metrics=${task_metric_loss_metrics[$loss_id]%,}:owner=${outcome_loss_owner[$loss_id]}"
    done
    for loss_id in "${!task_tier_loss[@]}"; do
      loss_summary+="${loss_summary:+;}${loss_id}:tiers=${task_tier_loss_rows[$loss_id]%,}:owner=${outcome_loss_owner[$loss_id]}"
    done
    fail "compiled workload superiority failed: measured Jet loss ($loss_summary); blocking product card remains open"
  fi
  echo "compiled workload gate: pass report=$report_dir"
}


case "${1:-}" in
  --contract) [[ "$#" -eq 1 ]] || usage; static_contract ;;
  --check) [[ "$#" -eq 2 ]] || usage; static_contract; check_report "$2" ;;
  *) usage ;;
esac
