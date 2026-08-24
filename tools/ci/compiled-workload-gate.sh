#!/usr/bin/env bash
# Card #1414: fail closed until complete-work measurements and review exist.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

CORPUS="$ROOT/tests/compiled_workloads"
MANIFEST="$CORPUS/manifest.tsv"
DOMAIN_CONTRACT="$CORPUS/domain_contract.tsv"
PEERS="$CORPUS/peer_ledger.tsv"
METRIC_CONTRACT="$CORPUS/metric_contract.tsv"
TIER_MATRIX="$CORPUS/tier_matrix.tsv"
CANARIES="$CORPUS/canaries.tsv"
CORE_LEDGER="$ROOT/docs/reference/core-surface-ledger.json"

MANIFEST_HEADER=$'version\ttask_id\tdomain\tcase\tdeclared_outcome\tinput\texpected\tauthority\tadapters\tplatforms\tevidence\ttower_card\tloss_cards'
DOMAIN_HEADER=$'version\ttask_id\tallowed_dependencies\tmachine_spec\tvariant\tscoring'
PEER_HEADER=$'version\ttask_id\tselection\tlanguage\tprogram\tsource_url\tsource_revision\tbuild_command\trun_command\tdependency_rule\tsource_boundary\tapplicable_targets\towner'
METRIC_HEADER=$'version\tmetric\tunit\tcomparison\tmissing_policy'
TIER_HEADER=$'version\ttask_id\tplatform\ttarget\ttier\trequirement\trationale'
CANARY_HEADER=$'version\tcanary\tmutation\trequired_failure'
OUTCOME_HEADER=$'version\ttask_id\tpeer_language\tpeer_program\tinput\texpected\toutcome\ttoolchain_id\tjet_tool_version\tpeer_tool_version\tdependency_rule\tsource_boundary\tjet_status\tpeer_status\tloss_owner\treview_status\treview_evidence'
MEASUREMENT_HEADER=$'version\ttask_id\tlanguage\tmetric\tvalue\tunit\ttoolchain_id\tevidence\tstatus\tloss_owner'
TIER_REPORT_HEADER=$'version\ttask_id\tlanguage\tplatform\ttarget\ttier\tstatus\tevidence\tloss_owner'

METRICS=(source_effort build_time edit_time runtime memory artifact_size diagnostics debugging deployment unsafe_burden)
LANGUAGES=(rust cxx go swift zig domain)

fail() { echo "compiled workload gate: $*" >&2; exit 1; }
usage() { echo "usage: bash tools/ci/compiled-workload-gate.sh --contract|--check REPORT_DIR" >&2; exit 64; }

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
  [[ "$value" =~ ^#[0-9]+$ || "$value" =~ ^non-goal:.+$ ]]
}

has_word() { [[ ";$1;" == *";$2;"* || ",$1," == *",$2,"* ]]; }

static_contract() {
  local file header
  for file in "$MANIFEST" "$DOMAIN_CONTRACT" "$PEERS" "$METRIC_CONTRACT" "$TIER_MATRIX" "$CANARIES"; do
    [[ -f "$file" ]] || fail "missing frozen contract: ${file#$ROOT/}"
  done
  [[ -f "$CORE_LEDGER" ]] || fail "missing Core competitor ledger"
  node -e 'const fs = require("node:fs"); const ledger = JSON.parse(fs.readFileSync(process.argv[1], "utf8")); if (ledger.summary?.languageCount !== 11 || !ledger.competitors?.Rust || !ledger.competitors?.Go || !ledger.competitors?.Swift) process.exit(1)' "$CORE_LEDGER" || fail "Core competitor ledger language set drifted"
  [[ "$(head -n 1 "$MANIFEST")" == "$MANIFEST_HEADER" ]] || fail "manifest schema drifted"
  [[ "$(head -n 1 "$DOMAIN_CONTRACT")" == "$DOMAIN_HEADER" ]] || fail "domain contract schema drifted"
  [[ "$(head -n 1 "$PEERS")" == "$PEER_HEADER" ]] || fail "peer ledger schema drifted"
  [[ "$(head -n 1 "$METRIC_CONTRACT")" == "$METRIC_HEADER" ]] || fail "metric contract schema drifted"
  [[ "$(head -n 1 "$TIER_MATRIX")" == "$TIER_HEADER" ]] || fail "tier matrix schema drifted"
  [[ "$(head -n 1 "$CANARIES")" == "$CANARY_HEADER" ]] || fail "removal canary schema drifted"
  local expected_fields
  for expected_fields in "$MANIFEST:13" "$DOMAIN_CONTRACT:6" "$PEERS:13" "$METRIC_CONTRACT:5" "$TIER_MATRIX:7" "$CANARIES:4"; do
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

  declare -gA selected_language selected_program selected_dependency selected_boundary
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
      selected_seen[$id]=1; selected_language[$id]="$language"; selected_program[$id]="$program"
      selected_dependency[$id]="$dependency_rule"; selected_boundary[$id]="$source_boundary"
    fi
  done < "$PEERS"
  for id in "${!task_domain[@]}"; do [[ -n "${selected_seen[$id]+x}" ]] || fail "no best applicable peer: $id"; done
  for language in "${LANGUAGES[@]}"; do [[ -n "${peer_languages_seen[$language]+x}" ]] || fail "peer language missing: $language"; done

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

  declare -gA tier_requirement tier_target tier_tier tier_seen
  local platform target tier requirement rationale key
  declare -A global_platform=() global_tier=()
  while IFS=$'\t' read -r version id platform target tier requirement rationale; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "${task_domain[$id]+x}" ]] || fail "bad tier matrix row: $id"
    [[ "$tier" == aot || "$tier" == jit ]] || fail "bad execution tier: $id/$tier"
    [[ "$requirement" == required || "$requirement" == excluded ]] || fail "bad tier requirement: $id/$tier"
    key="$id|$platform|$target|$tier"; [[ -z "${tier_seen[$key]+x}" ]] || fail "duplicate tier matrix row: $key"
    tier_seen[$key]=1; tier_requirement[$key]="$requirement"; tier_target[$key]="$target"; tier_tier[$key]="$tier"
    global_platform[$platform]=1; global_tier[$tier]=1
    [[ -n "$rationale" ]] || fail "tier rationale missing: $key"
  done < "$TIER_MATRIX"
  for platform in linux macos windows cross-target; do [[ -n "${global_platform[$platform]+x}" ]] || fail "tier platform missing: $platform"; done
  for tier in aot jit; do [[ -n "${global_tier[$tier]+x}" ]] || fail "execution tier missing: $tier"; done

  local canary_count=0 canary
  while IFS=$'\t' read -r version canary mutation required_failure; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "$canary" && -n "$mutation" && -n "$required_failure" ]] || fail "bad removal canary row"
    canary_count=$((canary_count + 1))
  done < "$CANARIES"
  ((canary_count >= 4)) || fail "removal canary set is incomplete"
  echo "compiled workload contract: pass tasks=${#task_domain[@]} peers=${#peer_languages_seen[@]} metrics=${#metric_seen[@]} tiers=${#tier_seen[@]}"
}

check_report() {
  local report_dir="$1"
  [[ -d "$report_dir" ]] || fail "missing report directory: $report_dir"
  local outcomes="$report_dir/outcomes.tsv" measurements="$report_dir/measurements.tsv" tiers="$report_dir/tiers.tsv"
  [[ -f "$outcomes" && -f "$measurements" && -f "$tiers" ]] || fail "report must contain outcomes.tsv, measurements.tsv, and tiers.tsv"
  [[ "$(head -n 1 "$outcomes")" == "$OUTCOME_HEADER" ]] || fail "outcome report schema drifted"
  [[ "$(head -n 1 "$measurements")" == "$MEASUREMENT_HEADER" ]] || fail "measurement report schema drifted"
  [[ "$(head -n 1 "$tiers")" == "$TIER_REPORT_HEADER" ]] || fail "tier report schema drifted"

  declare -A outcome_seen=() metric_report_seen=() tier_report_seen=()
  local version id peer_language peer_program input expected outcome toolchain_id jet_tool_version peer_tool_version dependency_rule source_boundary jet_status peer_status loss_owner review_status review_evidence
  while IFS=$'\t' read -r version id peer_language peer_program input expected outcome toolchain_id jet_tool_version peer_tool_version dependency_rule source_boundary jet_status peer_status loss_owner review_status review_evidence; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "${task_domain[$id]+x}" ]] || fail "outcome names unknown task: $id"
    [[ -z "${outcome_seen[$id]+x}" ]] || fail "duplicate outcome: $id"
    [[ "$peer_language" == "${selected_language[$id]}" && "$peer_program" == "${selected_program[$id]}" ]] || fail "outcome peer is not selected peer: $id"
    [[ "$input" == "${task_input[$id]}" && "$expected" == "${task_expected[$id]}" && "$outcome" == "${task_outcome[$id]}" ]] || fail "input or outcome drifted: $id"
    [[ -n "$toolchain_id" && -n "$jet_tool_version" && -n "$peer_tool_version" && -n "$dependency_rule" && -n "$source_boundary" ]] || fail "comparison identity incomplete: $id"
    [[ "$dependency_rule" == "${selected_dependency[$id]}" && "$source_boundary" == "${selected_boundary[$id]}" ]] || fail "peer comparison identity drifted: $id"
    [[ "$jet_status" == pass || "$jet_status" == loss ]] || fail "Jet result is not measured: $id"
    [[ "$peer_status" == pass || "$peer_status" == loss ]] || fail "peer result is not measured: $id"
    if [[ "$jet_status" == loss ]]; then owner_ok "$loss_owner" || fail "unowned Jet loss: $id"; fi
    [[ "$review_status" == pass && -n "$review_evidence" ]] || fail "independent review missing: $id"
    outcome_seen[$id]=1
  done < "$outcomes"
  ((${#outcome_seen[@]} == ${#task_domain[@]})) || fail "outcome report does not cover frozen manifest"

  local language metric value unit evidence status metric_loss expected_unit
  while IFS=$'\t' read -r version id language metric value unit toolchain_id evidence status metric_loss; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "${task_domain[$id]+x}" ]] || fail "measurement names unknown task: $id"
    [[ "$language" == jet || "$language" == "${selected_language[$id]}" ]] || fail "measurement language is not Jet or selected peer: $id/$language"
    expected_unit="${metric_unit[$metric]-}"
    [[ -n "$expected_unit" && "$unit" == "$expected_unit" && -n "$value" && -n "$toolchain_id" && -n "$evidence" ]] || fail "incomplete measurement: $id/$language/$metric"
    [[ "$status" == measured || "$status" == loss ]] || fail "measurement is not complete: $id/$language/$metric"
    if [[ "$status" == loss ]]; then owner_ok "$metric_loss" || fail "unowned metric loss: $id/$language/$metric"; fi
    key="$id|$language|$metric"; [[ -z "${metric_report_seen[$key]+x}" ]] || fail "duplicate measurement: $key"; metric_report_seen[$key]=1
  done < "$measurements"
  local id2 language2 metric2
  for id2 in "${!task_domain[@]}"; do
    for language2 in jet "${selected_language[$id2]}"; do
      for metric2 in "${METRICS[@]}"; do
        [[ -n "${metric_report_seen[$id2|$language2|$metric2]+x}" ]] || fail "missing measurement: $id2/$language2/$metric2"
      done
    done
  done

  local report_id platform target tier tier_status tier_evidence tier_loss
  while IFS=$'\t' read -r version id language platform target tier tier_status tier_evidence tier_loss; do
    [[ -z "$version" || "$version" == version ]] && continue
    key="$id|$platform|$target|$tier"
    [[ -n "${tier_seen[$key]+x}" ]] || fail "tier report names undeclared row: $key"
    [[ "$language" == jet || "$language" == "${selected_language[$id]}" ]] || fail "tier report language is not Jet or selected peer: $key"
    [[ "$tier_status" == pass || "$tier_status" == loss || "$tier_status" == not-applicable ]] || fail "bad tier status: $key"
    [[ "${tier_requirement[$key]}" == excluded && "$tier_status" == not-applicable || "${tier_requirement[$key]}" == required && "$tier_status" != not-applicable ]] || fail "tier applicability drifted: $key"
    [[ -n "$tier_evidence" ]] || fail "tier evidence missing: $key"
    if [[ "$tier_status" == loss ]]; then owner_ok "$tier_loss" || fail "unowned tier loss: $key"; fi
    report_id="$key|$language"; [[ -z "${tier_report_seen[$report_id]+x}" ]] || fail "duplicate tier report: $report_id"; tier_report_seen[$report_id]=1
  done < "$tiers"
  local req_key
  for req_key in "${!tier_seen[@]}"; do
    for language2 in jet "${selected_language[${req_key%%|*}]}"; do
      if [[ "${tier_requirement[$req_key]}" == required ]]; then
        [[ -n "${tier_report_seen[$req_key|$language2]+x}" ]] || fail "missing tier proof: $req_key/$language2"
      fi
    done
  done
  echo "compiled workload gate: pass report=$report_dir"
}

case "${1:-}" in
  --contract) [[ "$#" -eq 1 ]] || usage; static_contract ;;
  --check) [[ "$#" -eq 2 ]] || usage; static_contract; check_report "$2" ;;
  *) usage ;;
esac
