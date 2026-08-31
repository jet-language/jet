#!/usr/bin/env bash
# Card #1414: fail closed until complete-work measurements and review exist.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

CORPUS="$ROOT/tests/compiled_workloads"
MANIFEST="$CORPUS/manifest.tsv"
DOMAIN_CONTRACT="$CORPUS/domain_contract.tsv"
PEERS="$CORPUS/peer_ledger.tsv"
ADAPTERS="$CORPUS/adapter_ledger.tsv"
METRIC_CONTRACT="$CORPUS/metric_contract.tsv"
MEASUREMENT_POLICY="$CORPUS/measurement_policy.tsv"
TIER_MATRIX="$CORPUS/tier_matrix.tsv"
CANARIES="$CORPUS/canaries.tsv"
CORE_LEDGER="$ROOT/docs/reference/core-surface-ledger.json"

MANIFEST_HEADER=$'version\ttask_id\tdomain\tcase\tdeclared_outcome\tinput\texpected\tauthority\tadapters\tplatforms\tevidence\ttower_card\tloss_cards'
DOMAIN_HEADER=$'version\ttask_id\tallowed_dependencies\tmachine_spec\tvariant\tscoring'
PEER_HEADER=$'version\ttask_id\tselection\tlanguage\tprogram\tsource_url\tsource_revision\tbuild_command\trun_command\tdependency_rule\tsource_boundary\tapplicable_targets\towner'
ADAPTER_HEADER=$'version\ttask_id\tjet_source\tjet_hostile\tpeer_source\tpeer_hostile\tpeer_commit'
METRIC_HEADER=$'version\tmetric\tunit\tcomparison\tmissing_policy'
POLICY_HEADER=$'version\tmetric\tsamples\tmin_samples\tmin_value\toutlier_mad_multiplier\tmax_relative_stdev\tmax_outliers\ttolerance_ratio'
TIER_HEADER=$'version\ttask_id\tplatform\ttarget\ttier\trequirement\trationale'
CANARY_HEADER=$'version\tcanary\tmutation\trequired_failure'
IDENTITY_HEADER=$'version\tkey\tvalue'
SAMPLE_HEADER=$'version\ttask_id\tlanguage\tmetric\tsample\tvalue\tunit\tmethod'
STATISTICS_HEADER=$'version\ttask_id\tlanguage\tmetric\tsamples\tmedian\tmin\tmax\trelative_stdev\toutliers\tthreshold\ttolerance_ratio\tstatus\tloss_owner\tevidence'
RECEIPT_HEADER=$'version\ttask_id\tlanguage\tsource_sha256\tinput_sha256\texpected_sha256\toutput_sha256\thostile_input_sha256\thostile_output_sha256\tenvironment\tmachine\ttool_version\tcommand\tpeer_commit\texit_code\thostile_exit_code'
TIER_RECEIPT_HEADER=$'version\ttask_id\tlanguage\tplatform\ttarget\ttier\tartifact_sha256\toutput_sha256\tcommand\tstatus'
OUTCOME_HEADER=$'version\ttask_id\tpeer_language\tpeer_program\tinput\texpected\toutcome\ttoolchain_id\tjet_tool_version\tpeer_tool_version\tdependency_rule\tsource_boundary\tjet_status\tpeer_status\tloss_owner\treview_status\treview_evidence'
MEASUREMENT_HEADER=$'version\ttask_id\tlanguage\tmetric\tvalue\tunit\ttoolchain_id\tevidence\tstatus\tloss_owner'
TIER_REPORT_HEADER=$'version\ttask_id\tlanguage\tplatform\ttarget\ttier\tstatus\tevidence\tloss_owner'

METRICS=(source_effort build_time edit_time runtime memory artifact_size diagnostics debugging deployment unsafe_burden)
LANGUAGES=(rust cxx go swift zig domain)
TOWER_CLI="$ROOT/plugins/tower/tower.mjs"
declare -A live_tower_owner_cache=()
declare -gA policy_tolerance=()
declare -gA policy_samples=()
declare -gA policy_min_value=() policy_outlier_mad_multiplier=() policy_max_relative_stdev=() policy_max_outliers=()

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

hash_relative_files() {
  node - "$ROOT" "$@" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const args = process.argv.slice(1);
const root = args.shift();
const files = [];
function walk(file) {
  const stat = fs.statSync(file);
  if (stat.isDirectory()) {
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

safe_path() {
  local value="$1"
  [[ -n "$value" && "$value" != /* && "$value" != *:* && "$value" != *\\* ]] || return 1
  local part
  IFS=/ read -ra parts <<< "$value"
  for part in "${parts[@]}"; do
    [[ -n "$part" && "$part" != . && "$part" != .. ]] || return 1
  done
}

owner_ok() {
  local value="$1"
  [[ "$value" =~ ^#[0-9]+$ || "$value" =~ ^non-goal:D-[A-Za-z0-9][A-Za-z0-9-]*$ ]]
}

has_word() { [[ ";$1;" == *";$2;"* || ",$1," == *",$2,"* ]]; }

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

review_evidence_ok() {
  local evidence="$1"
  local candidate_re='(^|;)candidate=[0-9a-f]{40}(;|$)'
  local reviewer_re='(^|;)reviewer=[^;[:space:]]+(;|$)'
  local fairness_re='(^|;)fairness=[^;[:space:]]+(;|$)'
  local measurements_re='(^|;)measurements=[^;[:space:]]+(;|$)'
  [[ "$evidence" =~ $candidate_re ]] \
    && [[ "$evidence" =~ $reviewer_re ]] \
    && [[ "$evidence" =~ $fairness_re ]] \
    && [[ "$evidence" =~ $measurements_re ]]
}

static_contract() {
  local file header
  for file in "$MANIFEST" "$DOMAIN_CONTRACT" "$PEERS" "$ADAPTERS" "$METRIC_CONTRACT" "$MEASUREMENT_POLICY" "$TIER_MATRIX" "$CANARIES"; do
    [[ -f "$file" ]] || fail "missing frozen contract: ${file#$ROOT/}"
  done
  [[ -f "$CORE_LEDGER" ]] || fail "missing Core competitor ledger"
  node -e 'const fs = require("node:fs"); const ledger = JSON.parse(fs.readFileSync(process.argv[1], "utf8")); if (ledger.summary?.languageCount !== 11 || !ledger.competitors?.Rust || !ledger.competitors?.Go || !ledger.competitors?.Swift) process.exit(1)' "$CORE_LEDGER" || fail "Core competitor ledger language set drifted"
  [[ "$(head -n 1 "$MANIFEST")" == "$MANIFEST_HEADER" ]] || fail "manifest schema drifted"
  [[ "$(head -n 1 "$DOMAIN_CONTRACT")" == "$DOMAIN_HEADER" ]] || fail "domain contract schema drifted"
  [[ "$(head -n 1 "$PEERS")" == "$PEER_HEADER" ]] || fail "peer ledger schema drifted"
  [[ "$(head -n 1 "$ADAPTERS")" == "$ADAPTER_HEADER" ]] || fail "adapter ledger schema drifted"
  [[ "$(head -n 1 "$METRIC_CONTRACT")" == "$METRIC_HEADER" ]] || fail "metric contract schema drifted"
  [[ "$(head -n 1 "$MEASUREMENT_POLICY")" == "$POLICY_HEADER" ]] || fail "measurement policy schema drifted"
  [[ "$(head -n 1 "$TIER_MATRIX")" == "$TIER_HEADER" ]] || fail "tier matrix schema drifted"
  [[ "$(head -n 1 "$CANARIES")" == "$CANARY_HEADER" ]] || fail "removal canary schema drifted"
  local expected_fields
  for expected_fields in "$MANIFEST:13" "$DOMAIN_CONTRACT:6" "$PEERS:13" "$ADAPTERS:7" "$METRIC_CONTRACT:5" "$MEASUREMENT_POLICY:9" "$TIER_MATRIX:7" "$CANARIES:4"; do
    file="${expected_fields%:*}"
    header="${expected_fields##*:}"
    awk -F '\t' -v expected="$header" 'NR > 1 && NF != expected { bad = 1; print NR ": expected " expected " fields, got " NF > "/dev/stderr" } END { exit bad }' "$file" || fail "row width drifted: ${file#$ROOT/}"
  done

  declare -gA task_domain task_input task_expected task_outcome task_adapter task_platform task_tower task_loss
  local line version id domain case_name outcome input expected authority adapters platforms evidence tower loss expected_authority
  while IFS=$'\t' read -r version id domain case_name outcome input expected authority adapters platforms evidence tower loss; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "$id" && -z "${task_domain[$id]+x}" ]] || fail "bad or duplicate manifest row: $id"
    safe_path "$input" && safe_path "$expected" || fail "unsafe manifest fixture path: $id"
    [[ -f "$CORPUS/$input" && -f "$CORPUS/$expected" ]] || fail "manifest fixture is missing: $id"
    expected_authority="argv=input-root;cwd=scratch;host=ambient;network=disabled;external-write=disabled"
    [[ "$id" == service-json-http ]] && expected_authority="argv=input-root;cwd=scratch;host=ambient;network=loopback-only;external-write=disabled"
    [[ "$authority" == "$expected_authority" ]] || fail "authority drifted: $id"
    has_word "$adapters" jet || fail "Jet adapter missing: $id"
    [[ "$platforms" == *linux=* && "$platforms" == *macos=* && "$platforms" == *windows=* && "$platforms" == *cross-target=* ]] || fail "platform matrix incomplete: $id"
    [[ "$evidence" == docs/research/card-1414-compiled-peer-task-definitions.md#* ]] || fail "public task evidence missing: $id"
    [[ "$tower" == "#1414" ]] || fail "manifest owner drifted: $id"
    owner_ok "$loss" || fail "manifest loss owner is not auditable: $id"
    task_domain[$id]="$domain"; task_input[$id]="$input"; task_expected[$id]="$expected"
    task_outcome[$id]="$outcome"; task_adapter[$id]="$adapters"; task_platform[$id]="$platforms"
    task_tower[$id]="$tower"; task_loss[$id]="$loss"
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
  for id in "${!task_domain[@]}"; do [[ -n "${domain_seen[$id]+x}" ]] || fail "domain contract missing task: $id"; done

  declare -gA selected_language selected_program selected_dependency selected_boundary selected_targets selected_revision
  declare -gA peer_languages_seen
  local selection language program source_url source_revision build_command run_command dependency_rule source_boundary targets owner
  declare -A selected_seen=()
  while IFS=$'\t' read -r version id selection language program source_url source_revision build_command run_command dependency_rule source_boundary targets owner; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "${task_domain[$id]+x}" ]] || fail "peer ledger names unknown task: $id"
    [[ "$selection" == best-applicable || "$selection" == candidate ]] || fail "bad peer selection: $id"
    [[ "$language" =~ ^(rust|cxx|go|swift|zig|domain)$ ]] || fail "bad peer language: $id/$language"
    [[ "$source_url" == https://* && "$source_revision" =~ ^(tag:[^[:space:]]+|[0-9a-f]{40})$ ]] || fail "peer source is not pinned: $id/$language"
    [[ -n "$program" && -n "$build_command" && -n "$run_command" && -n "$dependency_rule" && -n "$source_boundary" && -n "$targets" ]] || fail "incomplete peer row: $id/$language"
    owner_ok "$owner" || fail "peer owner is not auditable: $id/$language"
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

  declare -gA adapter_jet_source adapter_jet_hostile adapter_peer_source adapter_peer_hostile adapter_peer_commit
  declare -A adapter_seen=()
  local adapter_version adapter_id jet_source jet_hostile peer_source peer_hostile peer_commit
  while IFS=$'\t' read -r adapter_version adapter_id jet_source jet_hostile peer_source peer_hostile peer_commit; do
    [[ -z "$adapter_version" || "$adapter_version" == version ]] && continue
    [[ "$adapter_version" == 1 && -n "${task_domain[$adapter_id]+x}" ]] || fail "bad adapter ledger row: $adapter_id"
    [[ -z "${adapter_seen[$adapter_id]+x}" ]] || fail "duplicate adapter ledger row: $adapter_id"
    safe_path "$jet_source" && safe_path "$jet_hostile" && safe_path "$peer_source" && safe_path "$peer_hostile" || fail "unsafe adapter ledger path: $adapter_id"
    [[ -f "$CORPUS/$jet_source" && -f "$CORPUS/$jet_hostile" && -f "$CORPUS/$peer_source" && -f "$CORPUS/$peer_hostile" && -f "$CORPUS/expected/$adapter_id.hostile.out" ]] || fail "adapter ledger fixture is missing: $adapter_id"
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
  local policy_version policy_metric samples min_samples min_value outlier_mad_multiplier max_relative_stdev max_outliers tolerance_ratio
  while IFS=$'\t' read -r policy_version policy_metric samples min_samples min_value outlier_mad_multiplier max_relative_stdev max_outliers tolerance_ratio; do
    [[ -z "$policy_version" || "$policy_version" == version ]] && continue
    [[ "$policy_version" == 1 && -n "${metric_seen[$policy_metric]+x}" ]] || fail "bad measurement policy row: $policy_metric"
    [[ -z "${policy_seen[$policy_metric]+x}" ]] || fail "duplicate measurement policy row: $policy_metric"
    [[ "$samples" =~ ^[1-9][0-9]*$ && "$min_samples" =~ ^[1-9][0-9]*$ && "$min_value" =~ ^[0-9]+([.][0-9]+)?$ && "$outlier_mad_multiplier" =~ ^[0-9]+([.][0-9]+)?$ && "$max_relative_stdev" =~ ^[0-9]+([.][0-9]+)?$ && "$max_outliers" =~ ^[0-9]+$ && "$tolerance_ratio" =~ ^[0-9]+([.][0-9]+)?$ ]] || fail "measurement policy value is invalid: $policy_metric"
    ((min_samples <= samples)) || fail "measurement policy minimum exceeds samples: $policy_metric"
    policy_seen[$policy_metric]=1
    policy_tolerance[$policy_metric]="$tolerance_ratio"
    policy_samples[$policy_metric]="$samples"
    policy_min_value[$policy_metric]="$min_value"
    policy_outlier_mad_multiplier[$policy_metric]="$outlier_mad_multiplier"
    policy_max_relative_stdev[$policy_metric]="$max_relative_stdev"
    policy_max_outliers[$policy_metric]="$max_outliers"
  done < "$MEASUREMENT_POLICY"
  ((${#policy_seen[@]} == ${#METRICS[@]})) || fail "measurement policy count drifted"
  for metric in "${METRICS[@]}"; do [[ -n "${policy_seen[$metric]+x}" ]] || fail "measurement policy missing: $metric"; done

  declare -gA tier_requirement tier_target tier_tier tier_rationale tier_seen
  local platform target tier requirement rationale key
  declare -A global_platform=() global_tier=()
  while IFS=$'\t' read -r version id platform target tier requirement rationale; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "${task_domain[$id]+x}" ]] || fail "bad tier matrix row: $id"
    [[ "$tier" == aot || "$tier" == jit ]] || fail "bad execution tier: $id/$tier"
    [[ "$requirement" == required || "$requirement" == excluded ]] || fail "bad tier requirement: $id/$tier"
    key="$id|$platform|$target|$tier"; [[ -z "${tier_seen[$key]+x}" ]] || fail "duplicate tier matrix row: $key"
    tier_seen[$key]=1; tier_requirement[$key]="$requirement"; tier_target[$key]="$target"; tier_tier[$key]="$tier"; tier_rationale[$key]="$rationale"
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
  ((canary_count == 17)) || fail "removal canary set is incomplete: expected 17, got $canary_count"
  local required_canary
  for required_canary in \
    missing-outcome unowned-loss missing-metric missing-tier changed-input unreviewed stale-candidate \
    comparison-dependency-drift comparison-toolchain-drift statistics-drift receipt-input-drift \
    unowned-metric-loss toolchain-drift missing-samples missing-statistics missing-receipt missing-tier-receipt; do
    [[ -n "${canary_seen[$required_canary]+x}" ]] || fail "required removal canary missing: $required_canary"
  done
  echo "compiled workload contract: pass tasks=${#task_domain[@]} peers=${#peer_languages_seen[@]} metrics=${#metric_seen[@]} tiers=${#tier_seen[@]}"
}

check_report() {
  local report_dir="$1"
  [[ -d "$report_dir" ]] || fail "missing report directory: $report_dir"
  local identity="$report_dir/identity.tsv" samples="$report_dir/samples.tsv" statistics="$report_dir/statistics.tsv"
  local outcomes="$report_dir/outcomes.tsv" measurements="$report_dir/measurements.tsv" tiers="$report_dir/tiers.tsv"
  local receipts="$report_dir/receipts.tsv" tier_receipts="$report_dir/tier_receipts.tsv"
  [[ -f "$identity" && -f "$samples" && -f "$statistics" && -f "$outcomes" && -f "$measurements" && -f "$tiers" && -f "$receipts" && -f "$tier_receipts" ]] \
    || fail "report must contain identity.tsv, samples.tsv, statistics.tsv, outcomes.tsv, measurements.tsv, tiers.tsv, receipts.tsv, and tier_receipts.tsv"
  local current_candidate
  current_candidate="$(git rev-parse HEAD 2>/dev/null)" || fail "current candidate commit is unavailable"
  [[ "$current_candidate" =~ ^[0-9a-f]{40}$ ]] || fail "current candidate commit is not immutable"
  [[ "$(head -n 1 "$identity")" == "$IDENTITY_HEADER" ]] || fail "identity report schema drifted"
  [[ "$(head -n 1 "$samples")" == "$SAMPLE_HEADER" ]] || fail "sample report schema drifted"
  [[ "$(head -n 1 "$statistics")" == "$STATISTICS_HEADER" ]] || fail "statistics report schema drifted"
  [[ "$(head -n 1 "$outcomes")" == "$OUTCOME_HEADER" ]] || fail "outcome report schema drifted"
  [[ "$(head -n 1 "$measurements")" == "$MEASUREMENT_HEADER" ]] || fail "measurement report schema drifted"
  [[ "$(head -n 1 "$tiers")" == "$TIER_REPORT_HEADER" ]] || fail "tier report schema drifted"
  [[ "$(head -n 1 "$receipts")" == "$RECEIPT_HEADER" ]] || fail "receipt report schema drifted"
  [[ "$(head -n 1 "$tier_receipts")" == "$TIER_RECEIPT_HEADER" ]] || fail "tier receipt report schema drifted"
  local expected_fields file header
  for expected_fields in "$identity:3" "$samples:8" "$statistics:15" "$outcomes:17" "$measurements:10" "$tiers:9" "$receipts:16" "$tier_receipts:10"; do
    file="${expected_fields%:*}"
    header="${expected_fields##*:}"
    awk -F '\t' -v expected="$header" 'NF != expected { bad = 1; print NR ": expected " expected " fields, got " NF > "/dev/stderr" } END { exit bad }' "$file" \
      || fail "report row width drifted: ${file#$report_dir/}"
  done

  declare -A identity_seen=()
  local identity_version identity_key identity_value identity_platform identity_candidate
  while IFS=$'\t' read -r identity_version identity_key identity_value; do
    [[ -z "$identity_version" || "$identity_version" == version ]] && continue
    [[ "$identity_version" == 1 && -n "$identity_key" && -n "$identity_value" ]] || fail "incomplete report identity"
    [[ -z "${identity_seen[$identity_key]+x}" ]] || fail "duplicate report identity: $identity_key"
    identity_seen[$identity_key]="$identity_value"
  done < "$identity"
  for identity_key in candidate_commit platform environment machine jet_tool_version contract_sha256 source_closure_sha256 samples peer_commits; do
    [[ -n "${identity_seen[$identity_key]+x}" ]] || fail "report identity is missing: $identity_key"
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
  [[ "${identity_seen[samples]}" =~ ^[1-9][0-9]*$ ]] || fail "report sample identity is invalid"
  [[ "${identity_seen[samples]}" == "${policy_samples[source_effort]}" ]] || fail "report sample identity disagrees with policy"
  [[ "${identity_seen[environment]}" == "os=$identity_platform;"* && -n "${identity_seen[machine]}" && -n "${identity_seen[jet_tool_version]}" ]] \
    || fail "report environment identity is incomplete"
  local expected_contract_hash expected_source_closure_hash expected_peer_commit
  expected_contract_hash="$(hash_relative_files \
    tests/compiled_workloads/manifest.tsv \
    tests/compiled_workloads/domain_contract.tsv \
    tests/compiled_workloads/peer_ledger.tsv \
    tests/compiled_workloads/metric_contract.tsv \
    tests/compiled_workloads/tier_matrix.tsv \
    tests/compiled_workloads/canaries.tsv \
    tests/compiled_workloads/adapter_ledger.tsv \
    tests/compiled_workloads/measurement_policy.tsv \
    docs/reference/core-surface-ledger.json)" \
    || fail "report contract identity could not be recomputed"
  [[ "${identity_seen[contract_sha256]}" == "$expected_contract_hash" ]] || fail "report contract identity is stale"
  expected_source_closure_hash="$(hash_relative_files \
    tests/compiled_workloads/adapters \
    tests/compiled_workloads/fixtures \
    tests/compiled_workloads/expected \
    tests/compiled_workloads/task-definitions)" \
    || fail "report source identity could not be recomputed"
  [[ "${identity_seen[source_closure_sha256]}" == "$expected_source_closure_hash" ]] || fail "report source identity is stale"
  for id in "${!task_domain[@]}"; do
    expected_peer_commit="$id:${selected_revision[$id]}"
    [[ ",${identity_seen[peer_commits]}," == *",$expected_peer_commit,"* ]] || fail "report peer identity is incomplete: $id"
  done

  declare -A outcome_seen=() metric_report_seen=() tier_report_seen=() tier_report_status=() tier_report_artifact=() tier_report_output=()
  declare -A outcome_jet_version=() outcome_peer_version=() outcome_jet_status=() outcome_loss_owner=()
  local version id peer_language peer_program input expected outcome toolchain_id jet_tool_version peer_tool_version dependency_rule source_boundary jet_status peer_status loss_owner review_status review_evidence
  local report_platform="" report_candidate="" declared_platform declared_candidate review_candidate
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
    [[ "$dependency_rule" == "${selected_dependency[$id]}" && "$source_boundary" == "${selected_boundary[$id]}" ]] || fail "peer comparison identity drifted: $id"
    [[ "$jet_status" == pass || "$jet_status" == loss ]] || fail "Jet result is not measured: $id"
    [[ "$peer_status" == pass ]] || fail "peer result is not measured: $id"
    if [[ "$jet_status" == loss ]]; then
      require_live_loss_owner "$loss_owner" "$id"
    else
      [[ "$loss_owner" == - ]] || fail "unexpected Jet loss owner: $id"
    fi
    [[ "$review_status" == pass && -n "$review_evidence" ]] || fail "independent review missing: $id"
    review_evidence_ok "$review_evidence" || fail "fresh review evidence is incomplete: $id"
    [[ "$review_evidence" =~ (^|;)candidate=([0-9a-f]{40})(;|$) ]] || fail "fresh review candidate is missing: $id"
    review_candidate="${BASH_REMATCH[2]}"
    [[ "$review_candidate" == "$declared_candidate" ]] || fail "fresh review candidate is stale: $id"
    outcome_jet_version[$id]="$jet_tool_version"
    outcome_peer_version[$id]="$peer_tool_version"
    outcome_jet_status[$id]="$jet_status"
    outcome_loss_owner[$id]="$loss_owner"
    outcome_seen[$id]=1
  done < "$outcomes"
  ((${#outcome_seen[@]} == ${#task_domain[@]})) || fail "outcome report does not cover frozen manifest"

  declare -A sample_report_seen=() sample_group_count=() sample_group_values=()
  local sample_version sample_id sample_language sample_metric sample_number sample_value sample_unit sample_method sample_key sample_group_key
  while IFS=$'\t' read -r sample_version sample_id sample_language sample_metric sample_number sample_value sample_unit sample_method; do
    [[ -z "$sample_version" || "$sample_version" == version ]] && continue
    [[ "$sample_version" == 1 && -n "${task_domain[$sample_id]+x}" ]] || fail "sample names unknown task: $sample_id"
    [[ "$sample_language" == jet || "$sample_language" == "${selected_language[$sample_id]}" ]] || fail "sample language is not Jet or selected peer: $sample_id/$sample_language"
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

  local language metric value unit evidence status metric_loss expected_unit
  declare -A metric_value_report=() metric_status_report=() metric_owner_report=()
  while IFS=$'\t' read -r version id language metric value unit toolchain_id evidence status metric_loss; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 ]] || fail "measurement version drifted: $id"
    [[ -n "${task_domain[$id]+x}" ]] || fail "measurement names unknown task: $id"
    [[ "$language" == jet || "$language" == "${selected_language[$id]}" ]] || fail "measurement language is not Jet or selected peer: $id/$language"
    expected_unit="${metric_unit[$metric]-}"
    [[ -n "$expected_unit" && "$unit" == "$expected_unit" && -n "$value" && -n "$toolchain_id" && -n "$evidence" ]] || fail "incomplete measurement: $id/$language/$metric"
    [[ "$value" =~ ^[0-9]+([.][0-9]+)?$ ]] || fail "measurement value is invalid: $id/$language/$metric"
    if [[ "$language" == jet ]]; then
      [[ "$toolchain_id" == "${outcome_jet_version[$id]-}" ]] || fail "Jet toolchain identity drifted: $id/$metric"
    else
      [[ "$toolchain_id" == "${outcome_peer_version[$id]-}" ]] || fail "peer toolchain identity drifted: $id/$metric"
    fi
    [[ "$status" == measured || "$status" == loss ]] || fail "measurement is not complete: $id/$language/$metric"
    if [[ "$language" == "${selected_language[$id]}" ]]; then
      [[ "$status" == measured && "$metric_loss" == - ]] || fail "peer measurement cannot be a loss: $id/$metric"
    fi
    if [[ "$status" == loss ]]; then
      require_live_loss_owner "$metric_loss" "$id/$language/$metric"
    else
      [[ "$metric_loss" == - ]] || fail "unexpected metric loss owner: $id/$language/$metric"
    fi
    key="$id|$language|$metric"; [[ -z "${metric_report_seen[$key]+x}" ]] || fail "duplicate measurement: $key"; metric_report_seen[$key]=1
    metric_value_report[$key]="$value"
    metric_status_report[$key]="$status"
    metric_owner_report[$key]="$metric_loss"
  done < "$measurements"
  local id2 language2 metric2
  declare -A task_metric_loss=()
  for id2 in "${!task_domain[@]}"; do
    for language2 in jet "${selected_language[$id2]}"; do
      for metric2 in "${METRICS[@]}"; do
        [[ -n "${metric_report_seen[$id2|$language2|$metric2]+x}" ]] || fail "missing measurement: $id2/$language2/$metric2"
      done
    done
    for metric2 in "${METRICS[@]}"; do
      local jet_key peer_key jet_value peer_value tolerance jet_loses
      jet_key="$id2|jet|$metric2"
      peer_key="$id2|${selected_language[$id2]}|$metric2"
      jet_value="${metric_value_report[$jet_key]}"
      peer_value="${metric_value_report[$peer_key]}"
      tolerance="${policy_tolerance[$metric2]}"
      jet_loses=0
      if awk -v jet="$jet_value" -v peer="$peer_value" -v tolerance="$tolerance" \
        'BEGIN { if (peer == 0) exit !(jet > 0); exit !(jet > peer * tolerance) }'; then
        jet_loses=1
      fi
      if ((jet_loses)); then
        task_metric_loss[$id2]=1
        [[ "${metric_status_report[$jet_key]}" == loss ]] || fail "Jet metric loss is not recorded: $id2/$metric2"
        require_live_loss_owner "${metric_owner_report[$jet_key]}" "$id2/$metric2"
        [[ "${metric_owner_report[$jet_key]}" == "${outcome_loss_owner[$id2]}" ]] || fail "Jet loss owners disagree: $id2/$metric2"
      else
        [[ "${metric_status_report[$jet_key]}" == measured ]] || fail "Jet metric loss is unsupported by values: $id2/$metric2"
        [[ "${metric_owner_report[$jet_key]}" == - ]] || fail "unexpected Jet metric loss owner: $id2/$metric2"
      fi
    done
    if [[ -n "${task_metric_loss[$id2]+x}" ]]; then
      [[ "${outcome_jet_status[$id2]}" == loss ]] || fail "Jet outcome hides metric loss: $id2"
      require_live_loss_owner "${outcome_loss_owner[$id2]}" "$id2"
    else
      [[ "${outcome_jet_status[$id2]}" == pass && "${outcome_loss_owner[$id2]}" == - ]] || fail "Jet outcome contradicts measured metrics: $id2"
    fi
  done

  declare -A statistics_report_seen=() statistics_median=() statistics_status=() statistics_owner=()
  local stat_version stat_id stat_language stat_metric stat_sample_count stat_median stat_min stat_max stat_relative stat_outliers stat_threshold stat_tolerance stat_status stat_owner stat_evidence stat_key
  while IFS=$'\t' read -r stat_version stat_id stat_language stat_metric stat_sample_count stat_median stat_min stat_max stat_relative stat_outliers stat_threshold stat_tolerance stat_status stat_owner stat_evidence; do
    [[ -z "$stat_version" || "$stat_version" == version ]] && continue
    [[ "$stat_version" == 1 && -n "${task_domain[$stat_id]+x}" ]] || fail "statistics names unknown task: $stat_id"
    [[ "$stat_language" == jet || "$stat_language" == "${selected_language[$stat_id]}" ]] || fail "statistics language is not Jet or selected peer: $stat_id/$stat_language"
    [[ -n "${metric_unit[$stat_metric]+x}" && "$stat_sample_count" == "${policy_samples[$stat_metric]}" ]] || fail "statistics sample count is invalid: $stat_id/$stat_language/$stat_metric"
    [[ "$stat_median" =~ ^[0-9]+([.][0-9]+)?$ && "$stat_min" =~ ^[0-9]+([.][0-9]+)?$ && "$stat_max" =~ ^[0-9]+([.][0-9]+)?$ && "$stat_relative" =~ ^[0-9]+([.][0-9]+)?$ && "$stat_outliers" =~ ^[0-9]+$ && "$stat_threshold" =~ ^[0-9]+([.][0-9]+)?$ && "$stat_tolerance" =~ ^[0-9]+([.][0-9]+)?$ ]] \
      || fail "statistics values are invalid: $stat_id/$stat_language/$stat_metric"
    [[ "$stat_status" == measured || "$stat_status" == loss ]] || fail "statistics status is invalid: $stat_id/$stat_language/$stat_metric"
    if [[ "$stat_language" == "${selected_language[$stat_id]}" ]]; then
      [[ "$stat_status" == measured && "$stat_owner" == - ]] || fail "peer statistics cannot be a loss: $stat_id/$stat_metric"
    elif [[ "$stat_status" == loss ]]; then
      require_live_loss_owner "$stat_owner" "$stat_id/$stat_metric"
    else
      [[ "$stat_owner" == - ]] || fail "unexpected statistics loss owner: $stat_id/$stat_metric"
    fi
    [[ "$stat_tolerance" == "${policy_tolerance[$stat_metric]}" && -n "$stat_evidence" ]] || fail "statistics policy evidence drifted: $stat_id/$stat_language/$stat_metric"
    sample_group_key="$stat_id|$stat_language|$stat_metric"
    [[ -n "${sample_group_values[$sample_group_key]+x}" ]] || fail "statistics samples are missing: $sample_group_key"
    if ! awk \
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

  local report_id platform target tier tier_status tier_evidence tier_loss
  while IFS=$'\t' read -r version id language platform target tier tier_status tier_evidence tier_loss; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 ]] || fail "tier report version drifted: $id"
    key="$id|$platform|$target|$tier"
    [[ -n "${tier_seen[$key]+x}" ]] || fail "tier report names undeclared row: $key"
    [[ "$language" == jet || "$language" == "${selected_language[$id]}" ]] || fail "tier report language is not Jet or selected peer: $key"
    [[ "$tier_status" == pass || "$tier_status" == loss || "$tier_status" == not-applicable ]] || fail "bad tier status: $key"
    local expected_tier_status in_scope
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
    else
      [[ "$tier_status" == pass || "$tier_status" == loss ]] || fail "tier proof missing: $key/$language"
    fi
    [[ -n "$tier_evidence" ]] || fail "tier evidence missing: $key"
    if [[ "$tier_status" == loss ]]; then
      [[ "$language" == jet ]] || fail "peer tier cannot be a loss: $key/$language"
      require_live_loss_owner "$tier_loss" "$key"
    else
      [[ "$tier_loss" == - ]] || fail "unexpected tier loss owner: $key/$language"
    fi
    report_id="$key|$language"
    if [[ "$tier_status" == pass ]]; then
      [[ "$tier_evidence" =~ (^|;)artifact=([0-9a-f]{64})(;|$) ]] || fail "tier artifact evidence is missing: $report_id"
      tier_report_artifact[$report_id]="${BASH_REMATCH[2]}"
      [[ "$tier_evidence" =~ (^|;)output=(-|[0-9a-f]{64})(;|$) ]] || fail "tier output evidence is missing: $report_id"
      tier_report_output[$report_id]="${BASH_REMATCH[2]}"
      if [[ "$platform" != cross-target || "$target" == web ]]; then
        [[ "${tier_report_output[$report_id]}" =~ ^[0-9a-f]{64}$ ]] || fail "tier output evidence is required: $report_id"
      fi
    fi
    [[ -z "${tier_report_seen[$report_id]+x}" ]] || fail "duplicate tier report: $report_id"
    tier_report_seen[$report_id]=1
    tier_report_status[$report_id]="$tier_status"
  done < "$tiers"
  local req_key
  for req_key in "${!tier_seen[@]}"; do
    for language2 in jet "${selected_language[${req_key%%|*}]}"; do
      [[ -n "${tier_report_seen[$req_key|$language2]+x}" ]] || fail "missing tier proof: $req_key/$language2"
    done
  done

  declare -A receipt_seen=()
  local receipt_version receipt_id receipt_language source_sha input_sha expected_sha output_sha hostile_input_sha hostile_output_sha receipt_environment receipt_machine receipt_tool receipt_command receipt_peer_commit exit_code hostile_exit_code receipt_key expected_source expected_hostile
  while IFS=$'\t' read -r receipt_version receipt_id receipt_language source_sha input_sha expected_sha output_sha hostile_input_sha hostile_output_sha receipt_environment receipt_machine receipt_tool receipt_command receipt_peer_commit exit_code hostile_exit_code; do
    [[ -z "$receipt_version" || "$receipt_version" == version ]] && continue
    [[ "$receipt_version" == 1 && -n "${task_domain[$receipt_id]+x}" ]] || fail "receipt names unknown task: $receipt_id"
    [[ "$receipt_language" == jet || "$receipt_language" == "${selected_language[$receipt_id]}" ]] || fail "receipt language is not Jet or selected peer: $receipt_id/$receipt_language"
    [[ "$source_sha" =~ ^[0-9a-f]{64}$ && "$input_sha" =~ ^[0-9a-f]{64}$ && "$expected_sha" =~ ^[0-9a-f]{64}$ && "$output_sha" =~ ^[0-9a-f]{64}$ && "$hostile_input_sha" =~ ^[0-9a-f]{64}$ && "$hostile_output_sha" =~ ^[0-9a-f]{64}$ ]] || fail "receipt hash is invalid: $receipt_id/$receipt_language"
    [[ "$receipt_environment" == "os=$report_platform;"* && -n "$receipt_machine" && -n "$receipt_tool" && -n "$receipt_command" ]] || fail "receipt identity is incomplete: $receipt_id/$receipt_language"
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
    [[ "$source_sha" == "$(file_sha256 "$CORPUS/$expected_source")" ]] || fail "receipt source drifted: $receipt_id/$receipt_language"
    [[ "$input_sha" == "$(file_sha256 "$CORPUS/${task_input[$receipt_id]}")" ]] || fail "receipt input drifted: $receipt_id/$receipt_language"
    [[ "$expected_sha" == "$(file_sha256 "$CORPUS/${task_expected[$receipt_id]}")" && "$output_sha" == "$expected_sha" ]] || fail "receipt normal output drifted: $receipt_id/$receipt_language"
    [[ "$hostile_input_sha" == "$(file_sha256 "$CORPUS/$expected_hostile")" ]] || fail "receipt hostile input drifted: $receipt_id/$receipt_language"
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
    [[ "$tier_receipt_result" == pass || "$tier_receipt_result" == not-applicable ]] || fail "tier receipt status is invalid: $report_tier_key"
    [[ "$tier_artifact_sha" == - || "$tier_artifact_sha" =~ ^[0-9a-f]{64}$ ]] || fail "tier receipt artifact hash is invalid: $report_tier_key"
    [[ "$tier_output_sha" == - || "$tier_output_sha" =~ ^[0-9a-f]{64}$ ]] || fail "tier receipt output hash is invalid: $report_tier_key"
    if [[ "$tier_receipt_result" == pass ]]; then
      [[ "$tier_artifact_sha" =~ ^[0-9a-f]{64}$ ]] || fail "tier receipt artifact hash is missing: $report_tier_key/$tier_receipt_language"
      [[ "$tier_artifact_sha" == "${tier_report_artifact[$report_tier_key|$tier_receipt_language]}" ]] || fail "tier artifact receipt disagrees: $report_tier_key/$tier_receipt_language"
      if [[ "$tier_receipt_platform" != cross-target || "$tier_receipt_target" == web ]]; then
        [[ "$tier_output_sha" =~ ^[0-9a-f]{64}$ ]] || fail "tier receipt output hash is missing: $report_tier_key/$tier_receipt_language"
        [[ "$tier_output_sha" == "${tier_report_output[$report_tier_key|$tier_receipt_language]}" ]] || fail "tier output receipt disagrees: $report_tier_key/$tier_receipt_language"
      else
        [[ "$tier_output_sha" == - && "${tier_report_output[$report_tier_key|$tier_receipt_language]}" == - ]] || fail "cross-target output receipt is invalid: $report_tier_key/$tier_receipt_language"
      fi
    else
      [[ "$tier_artifact_sha" == - && "$tier_output_sha" == - ]] || fail "not-applicable tier receipt has artifacts: $report_tier_key/$tier_receipt_language"
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
  echo "compiled workload gate: pass report=$report_dir"
}

case "${1:-}" in
  --contract) [[ "$#" -eq 1 ]] || usage; static_contract ;;
  --check) [[ "$#" -eq 2 ]] || usage; static_contract; check_report "$2" ;;
  *) usage ;;
esac
