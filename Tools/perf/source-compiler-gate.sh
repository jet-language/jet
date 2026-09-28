#!/usr/bin/env bash
# Criterion11 executable Source-compiler implementation gate.
#
# This is deliberately a separate lane from compiler-speed's language-peer
# policy.  It compares the generated Source implementation with the native
# Rust-reference implementation on the same compiler input, workflow, actual
# execution tier, target, optimization, and source-bound artifact.  The ratio
# is Source/Rust-reference and MUST be strictly below 1.00 for every cell and
# metric.  Missing, unavailable, inconclusive, synthetic, or mismatched rows
# are rejected before a ratio is considered.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PERF_DIR="$ROOT/Tools/perf"
CONTRACT="$PERF_DIR/source-compiler-contract.tsv"
POLICY="$PERF_DIR/source-compiler-policy.tsv"

CONTRACT_HEADER=$'version\tcell_id\tcase\tworkflow\texecution_tier\tinput\texpected\tsource_entry\tsource_manifest\toutcome\ttarget\tprofile\toptimization\tartifact_kind\tmetrics'
POLICY_HEADER=$'version\tmetric\tunit\tcomparison\tsamples\tmin_samples\tmin_value\tmax_relative_stdev\tmax_outliers\tratio_limit'
IDENTITY_HEADER=$'version\tkey\tvalue'
SAMPLE_HEADER=$'version\tcell_id\tcase\tworkflow\texecution_tier\timplementation\tmetric\tsample\tpair_id\tvalue\tunit\treceipt_key\tevidence'
RECEIPT_HEADER=$'version\tcell_id\tcase\tworkflow\texecution_tier\timplementation\tstatus\tsource_input_sha256\tsource_closure_sha256\texpected_sha256\toutput_sha256\tdiagnostic_sha256\tdiagnostic_count\tsemantic_expected_sha256\tsemantic_output_sha256\tsemantic_equivalence\tartifact_sha256\tgenerated_source_sha256\tcompiler_sha256\ttarget\tprofile\toptimization\tartifact_kind\tcorrectness\tsource_boundary\tcommand\tadapter_sha256\tpairing\tevidence'

fail() { echo "source compiler gate: $*" >&2; exit 1; }
usage() { echo "usage: bash Tools/perf/source-compiler-gate.sh --contract|--check REPORT_DIR" >&2; exit 64; }

sha256_file() {
  [[ -f "$1" ]] || return 1
  sha256sum -- "$1" | awk '{ print $1 }'
}

is_sha() { [[ "$1" =~ ^[0-9a-f]{64}$ ]]; }
is_nonzero_sha() { [[ "$1" =~ ^[0-9a-f]{64}$ && ! "$1" =~ ^0{64}$ ]]; }
is_commit() { [[ "$1" =~ ^[0-9a-fA-F]{40}$ ]]; }

safe_path() {
  [[ -n "$1" && "$1" != /* && "$1" != *$'\n'* && "$1" != *$'\r'* && "$1" != *'\\'* ]]
  [[ "$1" != ../* && "$1" != */../* && "$1" != */.. ]]
}

contract_hash() {
  {
    printf 'Tools/perf/source-compiler-contract.tsv\t%s\n' "$(sha256_file "$CONTRACT")"
    printf 'Tools/perf/source-compiler-policy.tsv\t%s\n' "$(sha256_file "$POLICY")"
  } | sha256sum | awk '{ print $1 }'
}

current_candidate() {
  local candidate="${JET_CI_CANDIDATE_COMMIT:-${GITHUB_SHA:-}}"
  if [[ -z "$candidate" ]]; then
    candidate="$(git -C "$ROOT" rev-parse --verify HEAD 2>/dev/null || true)"
  fi
  is_commit "$candidate" || fail "missing or invalid candidate commit identity"
  printf '%s' "$candidate"
}

static_contract() {
  [[ -f "$CONTRACT" ]] || fail "missing Source compiler contract: ${CONTRACT#$ROOT/}"
  [[ -f "$POLICY" ]] || fail "missing Source compiler policy: ${POLICY#$ROOT/}"
  [[ "$(awk 'substr($0,1,1) != "#" && length($0) > 0 { print; exit }' "$CONTRACT")" == "$CONTRACT_HEADER" ]] || fail "Source compiler contract schema drifted"
  [[ "$(awk 'substr($0,1,1) != "#" && length($0) > 0 { print; exit }' "$POLICY")" == "$POLICY_HEADER" ]] || fail "Source compiler policy schema drifted"
  awk -F '\t' -v expected=15 'substr($0,1,1) != "#" && NR > 2 && NF != expected { bad=1; print NR ": expected " expected " fields, got " NF > "/dev/stderr" } END { exit bad }' "$CONTRACT" \
    || fail "Source compiler contract row width drifted"
  awk -F '\t' -v expected=10 'substr($0,1,1) != "#" && NR > 1 && NF != expected { bad=1; print NR ": expected " expected " fields, got " NF > "/dev/stderr" } END { exit bad }' "$POLICY" \
    || fail "Source compiler policy row width drifted"

  declare -gA policy_unit=() policy_samples=() policy_min_value=() policy_max_relative=() policy_max_outliers=() policy_seen=()
  local version metric unit comparison samples min_samples min_value max_relative max_outliers ratio
  while IFS=$'\t' read -r version metric unit comparison samples min_samples min_value max_relative max_outliers ratio; do
    [[ -z "$version" || "$version" == version || "$version" == \#* ]] && continue
    [[ "$version" == 1 && -n "$metric" && -z "${policy_seen[$metric]+x}" ]] || fail "bad or duplicate Source compiler policy row: $metric"
    [[ "$comparison" == lower && "$ratio" == 1.00 ]] || fail "Source compiler policy is not strict: $metric"
    [[ "$samples" =~ ^[1-9][0-9]*$ && "$min_samples" =~ ^[1-9][0-9]*$ && "$min_value" =~ ^[1-9][0-9]*([.][0-9]+)?$ && "$max_relative" =~ ^[0-9]+([.][0-9]+)?$ && "$max_outliers" =~ ^[0-9]+$ ]] \
      || fail "invalid Source compiler policy values: $metric"
    (( min_samples <= samples )) || fail "Source compiler policy minimum exceeds samples: $metric"
    policy_seen[$metric]=1
    policy_unit[$metric]="$unit"
    policy_samples[$metric]="$samples"
    policy_min_value[$metric]="$min_value"
    policy_max_relative[$metric]="$max_relative"
    policy_max_outliers[$metric]="$max_outliers"
  done < "$POLICY"
  (( ${#policy_seen[@]} == 5 )) || fail "Source compiler policy metric count drifted"

  declare -gA cell_case=() cell_workflow=() cell_tier=() cell_input=() cell_expected=() cell_entry=() cell_manifest=() cell_target=() cell_profile=() cell_optimization=() cell_artifact=() cell_metrics=() cell_seen=()
  local cell case_name workflow tier input expected entry source_manifest outcome target profile optimization artifact metrics key metric_name
  local count=0
  while IFS=$'\t' read -r version cell case_name workflow tier input expected entry source_manifest outcome target profile optimization artifact metrics; do
    [[ -z "$version" || "$version" == version || "$version" == \#* ]] && continue
    [[ "$version" == 1 && -n "$cell" && -z "${cell_seen[$cell]+x}" ]] || fail "bad or duplicate Source compiler cell: $cell"
    [[ "$case_name" == valid || "$case_name" == invalid || "$case_name" == selfcompile ]] || fail "unknown Source compiler workload: $cell"
    [[ "$workflow" == factory || "$workflow" == runner ]] || fail "unknown Source compiler workflow: $cell"
    [[ "$tier" == aot || "$tier" == cranelift-jit || "$tier" == source-interpreter-deopt ]] || fail "unknown Source compiler execution tier: $cell"
    [[ "$outcome" == accepted || "$outcome" == rejected ]] || fail "unknown Source compiler outcome: $cell"
    [[ "$target" == x86_64-unknown-linux-gnu || "$target" == web ]] || fail "Source compiler target is not pinned: $cell"
    case "$target:$tier:$profile:$optimization:$artifact" in
      x86_64-unknown-linux-gnu:aot:release:'opt-level=3;debug=0;lto=thin':native-executable) ;;
      web:aot:release:'opt-level=3;debug=0;lto=thin':web-bundle-browser-runtime) ;;
      x86_64-unknown-linux-gnu:cranelift-jit:dev:'opt-level=0;debug=1;lto=off':cranelift-jit-code) ;;
      x86_64-unknown-linux-gnu:source-interpreter-deopt:dev:'opt-level=0;debug=1;lto=off':source-interpreter-state) ;;
      *) fail "Source compiler target/tier/profile/optimization/artifact mismatch: $cell" ;;
    esac
    safe_path "$input" && safe_path "$entry" && safe_path "$source_manifest" || fail "unsafe Source compiler contract path: $cell"
    [[ "$entry" == Compiler/Bootstrap/Host/Entry.jet && "$source_manifest" == Compiler/Bootstrap/sources.list ]] || fail "Source compiler entry/source-manifest identity drifted: $cell"
    [[ -f "$ROOT/$input" && -f "$ROOT/$entry" && -f "$ROOT/$source_manifest" ]] || fail "Source compiler contract input/source is missing: $cell"
    if [[ "$case_name" == selfcompile ]]; then
      [[ "$expected" == canonical-stage2-probes ]] || fail "selfcompile must use canonical stage2 probes, not an inventory echo: $cell"
    else
      safe_path "$expected" && [[ -f "$ROOT/$expected" ]] || fail "Source compiler expected output is missing: $cell"
    fi
    [[ "$case_name" == invalid && "$outcome" == rejected || "$case_name" != invalid && "$outcome" == accepted ]] || fail "Source compiler outcome does not match workload: $cell"
    case "$case_name:$tier" in
      valid:aot|selfcompile:aot) [[ "$metrics" == compile_ns,peak_rss_bytes,generated_source_bytes,artifact_bytes,run_ns ]] || fail "AOT accepted metric set drifted: $cell" ;;
      valid:*|selfcompile:*) [[ "$metrics" == compile_ns,peak_rss_bytes,generated_source_bytes,run_ns ]] || fail "non-AOT accepted metric set drifted: $cell" ;;
      invalid:*) [[ "$metrics" == compile_ns,peak_rss_bytes ]] || fail "rejected metric set drifted: $cell" ;;
    esac
    IFS=',' read -ra metric_names <<< "$metrics"
    ((${#metric_names[@]} > 0)) || fail "Source compiler cell has no metrics: $cell"
    for metric_name in "${metric_names[@]}"; do
      [[ -n "${policy_seen[$metric_name]+x}" ]] || fail "Source compiler cell names unknown metric: $cell/$metric_name"
    done
    cell_seen[$cell]=1
    cell_case[$cell]="$case_name"; cell_workflow[$cell]="$workflow"; cell_tier[$cell]="$tier"
    cell_input[$cell]="$input"; cell_expected[$cell]="$expected"; cell_entry[$cell]="$entry"; cell_manifest[$cell]="$source_manifest"
    cell_target[$cell]="$target"; cell_profile[$cell]="$profile"; cell_optimization[$cell]="$optimization"; cell_artifact[$cell]="$artifact"; cell_metrics[$cell]="$metrics"
    count=$((count + 1))
  done < "$CONTRACT"
  (( count == 20 )) || fail "Source compiler matrix must contain 20 required cells, got $count"
  local expected_key
  for case_name in valid invalid selfcompile; do
    for workflow in factory runner; do
      for tier in aot cranelift-jit source-interpreter-deopt; do
        expected_key="$case_name-$workflow-$tier"
        [[ -n "${cell_seen[$expected_key]+x}" ]] || fail "Source compiler matrix omitted required cell: $expected_key"
      done
    done
  done
  for expected_key in valid-web-factory-aot valid-web-runner-aot; do
    [[ "${cell_case[$expected_key]}" == valid && "${cell_tier[$expected_key]}" == aot && "${cell_target[$expected_key]}" == web ]] || fail "Source compiler Web AOT matrix omitted or drifted: $expected_key"
  done
  echo "source compiler contract: pass cells=$count metrics=${#policy_seen[@]} tiers=3 workflows=2 web=aot"
}

stats_for_values() {
  local values="$1" multiplier="$2"
  awk -v csv="$values" -v multiplier="$multiplier" '
    function abs(v) { return v < 0 ? -v : v }
    function sort(a, n, i, j, t) { for (i = 1; i <= n; i++) for (j = i + 1; j <= n; j++) if (a[j] < a[i]) { t=a[i]; a[i]=a[j]; a[j]=t } }
    function med(a, n) { if (n % 2) return a[(n + 1) / 2]; return (a[n / 2] + a[n / 2 + 1]) / 2 }
    BEGIN {
      n = split(csv, raw, ",")
      if (raw[n] == "") n--
      if (n < 1) exit 2
      for (i=1; i<=n; i++) a[i] = raw[i] + 0
      sort(a, n); median = med(a, n)
      sum = 0; for (i=1; i<=n; i++) sum += a[i]
      mean = sum / n; variance = 0; for (i=1; i<=n; i++) variance += (a[i] - mean) * (a[i] - mean)
      relative = mean == 0 ? 0 : sqrt(variance / n) / abs(mean)
      for (i=1; i<=n; i++) deviation[i] = abs(a[i] - median)
      sort(deviation, n); mad = med(deviation, n); limit = mad * multiplier; outliers = 0
      for (i=1; i<=n; i++) if (mad == 0 ? a[i] != median : abs(a[i] - median) > limit) outliers++
      printf "%.12g\t%.12g\t%d\n", median, relative, outliers
    }
  '
}

check_identity() {
  local report="$1" identity="$report/identity.tsv"
  [[ -f "$identity" ]] || fail "report is missing identity.tsv"
  [[ "$(sed -n '1p' "$identity")" == "$IDENTITY_HEADER" ]] || fail "Source compiler identity schema drifted"
  awk -F '\t' -v expected=3 'NR > 1 && NF != expected { bad=1 } END { exit bad }' "$identity" || fail "Source compiler identity row width drifted"
  declare -gA identity_seen=()
  local version key value
  while IFS=$'\t' read -r version key value; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "$key" && -n "$value" && -z "${identity_seen[$key]+x}" ]] || fail "invalid or duplicate Source compiler identity: $key"
    case "$key" in
      contract_sha256|candidate_commit|machine|environment|adapter_path|adapter_version|adapter_sha256|sample_count|source_manifest_sha256|target_set|implementation_pair|workflows|execution_tiers|execution_scope|pairing|run_id|ratio_policy) ;;
      *) fail "unknown Source compiler identity: $key" ;;
    esac
    identity_seen[$key]="$value"
  done < "$identity"
  local required
  for required in contract_sha256 candidate_commit machine environment adapter_path adapter_version adapter_sha256 sample_count source_manifest_sha256 target_set implementation_pair workflows execution_tiers execution_scope pairing run_id ratio_policy; do
    [[ -n "${identity_seen[$required]+x}" ]] || fail "Source compiler identity is missing: $required"
  done
  is_sha "${identity_seen[contract_sha256]}" || fail "Source compiler contract identity is invalid"
  [[ "${identity_seen[contract_sha256]}" == "$(contract_hash)" ]] || fail "Source compiler contract identity is stale"
  is_commit "${identity_seen[candidate_commit]}" || fail "Source compiler candidate identity is invalid"
  [[ "${identity_seen[candidate_commit]}" == "$(current_candidate)" ]] || fail "Source compiler report candidate is stale"
  [[ "${identity_seen[adapter_path]}" == /* && "${identity_seen[adapter_version]}" == source-compiler-performance-adapter-v2 ]] || fail "Source compiler adapter identity is invalid"
  is_nonzero_sha "${identity_seen[adapter_sha256]}" || fail "Source compiler adapter digest is invalid"
  [[ "${identity_seen[sample_count]}" == 5 && "${identity_seen[target_set]}" == x86_64-unknown-linux-gnu,web && "${identity_seen[implementation_pair]}" == source,rust-reference && "${identity_seen[workflows]}" == factory,runner && "${identity_seen[execution_tiers]}" == aot,cranelift-jit,source-interpreter-deopt && "${identity_seen[execution_scope]}" == generated-target-program && "${identity_seen[pairing]}" == paired-same-run-v1 && "${identity_seen[ratio_policy]}" == strict-lt-1.00 && -n "${identity_seen[run_id]}" ]] || fail "Source compiler matrix identity is not pinned"
  [[ "${identity_seen[run_id]}" =~ ^[A-Za-z0-9_.:-]+$ ]] || fail "Source compiler run identity is invalid"
  [[ "${identity_seen[environment]}" == os=linux\;target=x86_64-unknown-linux-gnu,web\;locale=C\;network=disabled ]] || fail "Source compiler environment identity is invalid"
  is_nonzero_sha "${identity_seen[source_manifest_sha256]}" || fail "Source compiler source manifest identity is invalid"
  [[ "${identity_seen[source_manifest_sha256]}" == "$(sha256_file "$ROOT/Compiler/Bootstrap/sources.list")" ]] || fail "Source compiler source manifest is stale"
}

check_report() {
  local report="$1"
  [[ -d "$report" ]] || fail "missing report directory: $report"
  local identity="$report/identity.tsv" samples="$report/samples.tsv" receipts="$report/receipts.tsv"
  [[ -f "$identity" && -f "$samples" && -f "$receipts" ]] || fail "report must contain identity.tsv, samples.tsv, and receipts.tsv"
  check_identity "$report"
  [[ "$(sed -n '1p' "$samples")" == "$SAMPLE_HEADER" ]] || fail "Source compiler sample schema drifted"
  [[ "$(sed -n '1p' "$receipts")" == "$RECEIPT_HEADER" ]] || fail "Source compiler receipt schema drifted"
  awk -F '\t' -v expected=13 'NR > 1 && NF != expected { bad=1 } END { exit bad }' "$samples" || fail "Source compiler sample row width drifted"
  awk -F '\t' -v expected=29 'NR > 1 && NF != expected { bad=1 } END { exit bad }' "$receipts" || fail "Source compiler receipt row width drifted"

  declare -gA receipt_seen=() receipt_diag_hash=() receipt_diag_count=() receipt_semantic_expected=() receipt_semantic_output=() receipt_semantic_equivalence=()
  local version cell case_name workflow tier implementation status source_input source_closure expected_sha output_sha diagnostic_sha diagnostic_count semantic_expected semantic_output semantic_equivalence artifact_sha generated_sha compiler_sha target profile optimization artifact_kind correctness boundary command adapter_sha pairing evidence key expected_source expected_closure expected_output expected_harness
  local receipt_count=0
  while IFS=$'\t' read -r version cell case_name workflow tier implementation status source_input source_closure expected_sha output_sha diagnostic_sha diagnostic_count semantic_expected semantic_output semantic_equivalence artifact_sha generated_sha compiler_sha target profile optimization artifact_kind correctness boundary command adapter_sha pairing evidence; do
    [[ -z "$version" || "$version" == version ]] && continue
    [[ "$version" == 1 && -n "${cell_seen[$cell]+x}" && ( "$implementation" == source || "$implementation" == rust-reference ) ]] || fail "unknown Source compiler receipt identity: $cell/$implementation"
    [[ "$case_name" == "${cell_case[$cell]}" && "$workflow" == "${cell_workflow[$cell]}" && "$tier" == "${cell_tier[$cell]}" ]] || fail "Source compiler receipt cell identity drifted: $cell/$implementation"
    [[ "$status" == measured && "$correctness" == verified && "$pairing" == "${identity_seen[pairing]}" && "$evidence" == *"run_id=${identity_seen[run_id]}"* && "$evidence" == *"pairing=${identity_seen[pairing]}"* ]] || fail "Source compiler receipt is unavailable or unpaired: $cell/$implementation"
    expected_source="$(sha256_file "$ROOT/${cell_input[$cell]}")" || fail "Source compiler input cannot be hashed: $cell"
    expected_closure="$(sha256_file "$ROOT/${cell_manifest[$cell]}")" || fail "Source compiler source manifest cannot be hashed: $cell"
    [[ "$source_input" == "$expected_source" && "$source_closure" == "$expected_closure" ]] || fail "Source compiler source-bound input drifted: $cell/$implementation"
    is_nonzero_sha "$source_input" && is_nonzero_sha "$source_closure" || fail "Source compiler receipt source identity is invalid: $cell/$implementation"
    [[ "$diagnostic_count" =~ ^[0-9]+$ ]] || fail "Source compiler diagnostic count is invalid: $cell/$implementation"
    if [[ "${cell_case[$cell]}" == selfcompile ]]; then
      [[ "$expected_sha" == - && "$output_sha" == - && "$diagnostic_sha" == - && "$diagnostic_count" == 0 && "$semantic_equivalence" == verified ]] || fail "selfcompile semantic receipt is incomplete: $cell/$implementation"
      is_nonzero_sha "$semantic_expected" && is_nonzero_sha "$semantic_output" && [[ "$semantic_expected" == "$semantic_output" ]] || fail "selfcompile semantic roundtrip is not exact: $cell/$implementation"
      [[ "$artifact_sha" != - && "$generated_sha" != - ]] && is_nonzero_sha "$artifact_sha" && is_nonzero_sha "$generated_sha" || fail "selfcompile artifact identity is missing: $cell/$implementation"
    elif [[ "${cell_case[$cell]}" == invalid ]]; then
      expected_output="$(sha256_file "$ROOT/${cell_expected[$cell]}")" || fail "Source compiler expected diagnostics cannot be hashed: $cell"
      [[ "$expected_sha" == "$expected_output" && "$output_sha" == - && "$diagnostic_sha" == "$expected_output" && "$diagnostic_count" =~ ^[1-9][0-9]*$ && "$semantic_expected" == - && "$semantic_output" == - && "$semantic_equivalence" == - && "$artifact_sha" == - && "$generated_sha" == - ]] || fail "invalid Source compiler semantics are not proved: $cell/$implementation"
    else
      expected_output="$(sha256_file "$ROOT/${cell_expected[$cell]}")" || fail "Source compiler expected output cannot be hashed: $cell"
      [[ "$expected_sha" == "$expected_output" && "$output_sha" == "$expected_output" && "$diagnostic_sha" == - && "$diagnostic_count" == 0 && "$semantic_expected" == - && "$semantic_output" == - && "$semantic_equivalence" == - ]] || fail "valid Source compiler semantics are not exact: $cell/$implementation"
      [[ "$artifact_sha" != - && "$generated_sha" != - ]] && is_nonzero_sha "$artifact_sha" && is_nonzero_sha "$generated_sha" || fail "valid Source compiler artifact identity is missing: $cell/$implementation"
    fi
    is_nonzero_sha "$compiler_sha" || fail "Source compiler implementation identity is invalid: $cell/$implementation"
    [[ "$target" == "${cell_target[$cell]}" && "$profile" == "${cell_profile[$cell]}" && "$optimization" == "${cell_optimization[$cell]}" && "$artifact_kind" == "${cell_artifact[$cell]}" ]] || fail "Source compiler target/optimization/artifact identity drifted: $cell/$implementation"
    [[ "$adapter_sha" == "${identity_seen[adapter_sha256]}" ]] || fail "Source compiler adapter identity drifted: $cell/$implementation"
    expected_harness="-"
    [[ "$target" == web ]] && expected_harness="node-app-js-browser-runtime-v1"
    [[ "$boundary" == *"execution_harness=$expected_harness"* ]] || fail "Source compiler execution harness evidence is incomplete: $cell/$implementation"
    [[ "$boundary" == *"implementation=$implementation"* && "$boundary" == *"workflow=$workflow"* && "$boundary" == *"execution_tier=$tier"* && "$boundary" == *"execution_scope=${identity_seen[execution_scope]}"* && "$boundary" == *"pairing=${identity_seen[pairing]}"* && "$boundary" == *"target=$target"* && "$boundary" == *"profile=$profile"* && "$boundary" == *"optimization=$optimization"* && "$boundary" == *"source_entry=${cell_entry[$cell]}"* && "$boundary" == *"source_input_sha256=$source_input"* && "$boundary" == *"source_manifest_sha256=$source_closure"* && "$boundary" == *"expected_sha256=$expected_sha"* && "$boundary" == *"diagnostic_sha256=$diagnostic_sha"* && "$boundary" == *"diagnostic_count=$diagnostic_count"* && "$boundary" == *"semantic_expected_sha256=$semantic_expected"* && "$boundary" == *"semantic_output_sha256=$semantic_output"* && "$boundary" == *"semantic_equivalence=$semantic_equivalence"* && "$boundary" == *"artifact_sha256=$artifact_sha"* && "$boundary" == *"generated_source_sha256=$generated_sha"* ]] || fail "Source compiler source-bound artifact evidence is incomplete: $cell/$implementation"
    [[ "$evidence" == *"evidence_version=byte-hash-v1"* && "$evidence" == *"result_dir="* && "$evidence" == *"request_path="* && "$evidence" == *"stdout_path="* && "$evidence" == *"diagnostics_path="* && "$evidence" == *"generated_source_path="* && "$evidence" == *"artifact_path="* && "$evidence" == *"compiler_path="* && "$evidence" == *"output_sha256=$output_sha"* && "$evidence" == *"diagnostic_sha256=$diagnostic_sha"* && "$evidence" == *"diagnostic_count=$diagnostic_count"* && "$evidence" == *"semantic_expected_sha256=$semantic_expected"* && "$evidence" == *"semantic_output_sha256=$semantic_output"* && "$evidence" == *"semantic_equivalence=$semantic_equivalence"* && "$evidence" == *"artifact_sha256=$artifact_sha"* && "$evidence" == *"generated_source_sha256=$generated_sha"* && "$evidence" == *"semantic_probes="* && "$evidence" == *"semantic_probe_paths="* ]] || fail "Source compiler byte-hash evidence is incomplete: $cell/$implementation"
    [[ "$evidence" == *"execution_harness=$expected_harness"* ]] || fail "Source compiler execution harness evidence is incomplete: $cell/$implementation"
    key="$cell|$implementation"
    [[ -z "${receipt_seen[$key]+x}" ]] || fail "duplicate Source compiler receipt: $key"
    receipt_seen[$key]=1; receipt_diag_hash[$key]="$diagnostic_sha"; receipt_diag_count[$key]="$diagnostic_count"; receipt_semantic_expected[$key]="$semantic_expected"; receipt_semantic_output[$key]="$semantic_output"; receipt_semantic_equivalence[$key]="$semantic_equivalence"
    receipt_count=$((receipt_count + 1))
  done < "$receipts"
  (( receipt_count == 40 )) || fail "Source compiler receipts must cover 20 cells and two implementations (got $receipt_count)"
  local cell_key implementation_key source_receipt_key reference_receipt_key
  for cell_key in "${!cell_seen[@]}"; do
    for implementation_key in source rust-reference; do
      key="$cell_key|$implementation_key"
      [[ -n "${receipt_seen[$key]+x}" ]] || fail "missing Source compiler receipt: $key"
    done
    source_receipt_key="$cell_key|source"
    reference_receipt_key="$cell_key|rust-reference"
    [[ "${receipt_diag_hash[$source_receipt_key]}" == "${receipt_diag_hash[$reference_receipt_key]}" && "${receipt_diag_count[$source_receipt_key]}" == "${receipt_diag_count[$reference_receipt_key]}" && "${receipt_semantic_expected[$source_receipt_key]}" == "${receipt_semantic_expected[$reference_receipt_key]}" && "${receipt_semantic_output[$source_receipt_key]}" == "${receipt_semantic_output[$reference_receipt_key]}" && "${receipt_semantic_equivalence[$source_receipt_key]}" == "${receipt_semantic_equivalence[$reference_receipt_key]}" ]] || fail "Source/Rust-reference correctness facts disagree: $cell_key"
  done

  declare -gA sample_seen=() sample_group_count=() sample_group_values=() sample_value_by_key=()
  local sample_version sample_cell sample_case sample_workflow sample_tier sample_impl sample_metric sample_number sample_pair sample_value sample_unit sample_receipt sample_evidence sample_key group_key expected_pair
  while IFS=$'\t' read -r sample_version sample_cell sample_case sample_workflow sample_tier sample_impl sample_metric sample_number sample_pair sample_value sample_unit sample_receipt sample_evidence; do
    [[ -z "$sample_version" || "$sample_version" == version ]] && continue
    [[ "$sample_version" == 1 && -n "${cell_seen[$sample_cell]+x}" && ( "$sample_impl" == source || "$sample_impl" == rust-reference ) ]] || fail "unknown Source compiler sample identity: $sample_cell/$sample_impl"
    [[ "$sample_case" == "${cell_case[$sample_cell]}" && "$sample_workflow" == "${cell_workflow[$sample_cell]}" && "$sample_tier" == "${cell_tier[$sample_cell]}" ]] || fail "Source compiler sample cell identity drifted: $sample_cell/$sample_impl"
    [[ "$sample_receipt" == "$sample_cell|$sample_impl" && -n "${receipt_seen[$sample_receipt]+x}" ]] || fail "Source compiler sample receipt is missing: $sample_cell/$sample_impl"
    expected_pair="${identity_seen[run_id]}|$sample_cell|$sample_number"
    [[ "$sample_pair" == "$expected_pair" ]] || fail "Source compiler pair identity drifted: $sample_cell/$sample_impl/$sample_number"
    [[ "$sample_evidence" == *"evidence_version=byte-hash-v1"* && "$sample_evidence" == *"pair_id=$sample_pair"* && "$sample_evidence" == *"paired=source,rust-reference"* && "$sample_evidence" == *"pairing=${identity_seen[pairing]}"* ]] || fail "Source compiler paired-run evidence is incomplete: $sample_cell/$sample_impl/$sample_number"
    [[ -n "$sample_metric" ]] || fail "Source compiler sample metric is empty"
    IFS=',' read -ra metric_names <<< "${cell_metrics[$sample_cell]}"
    local metric_allowed=0 metric_name2
    for metric_name2 in "${metric_names[@]}"; do [[ "$metric_name2" == "$sample_metric" ]] && metric_allowed=1; done
    (( metric_allowed == 1 )) || fail "Source compiler sample names undeclared metric: $sample_cell/$sample_metric"
    [[ -n "${policy_seen[$sample_metric]+x}" && "$sample_unit" == "${policy_unit[$sample_metric]}" ]] || fail "Source compiler sample unit is invalid: $sample_cell/$sample_metric"
    [[ "$sample_number" =~ ^[1-9][0-9]*$ && "$sample_number" -le "${policy_samples[$sample_metric]}" ]] || fail "Source compiler sample number is invalid: $sample_cell/$sample_impl/$sample_metric"
    [[ "$sample_value" =~ ^[1-9][0-9]*([.][0-9]+)?$ ]] || fail "Source compiler sample value is invalid: $sample_cell/$sample_impl/$sample_metric"
    awk -v value="$sample_value" -v minimum="${policy_min_value[$sample_metric]}" 'BEGIN { exit !(value >= minimum) }' || fail "Source compiler sample is below policy minimum: $sample_cell/$sample_impl/$sample_metric"
    sample_key="$sample_cell|$sample_impl|$sample_metric|$sample_number"
    [[ -z "${sample_seen[$sample_key]+x}" ]] || fail "duplicate Source compiler sample: $sample_key"
    sample_seen[$sample_key]=1; sample_value_by_key[$sample_key]="$sample_value"
    group_key="$sample_cell|$sample_impl|$sample_metric"
    sample_group_count[$group_key]=$(( ${sample_group_count[$group_key]:-0} + 1 ))
    sample_group_values[$group_key]="${sample_group_values[$group_key]-}${sample_value},"
  done < "$samples"

  declare -gA median=()
  local metric_name3 group values stats median_value relative outliers expected_count
  for cell_key in "${!cell_seen[@]}"; do
    for implementation_key in source rust-reference; do
      IFS=',' read -ra metric_names <<< "${cell_metrics[$cell_key]}"
      for metric_name3 in "${metric_names[@]}"; do
        group="$cell_key|$implementation_key|$metric_name3"
        expected_count="${policy_samples[$metric_name3]}"
        [[ "${sample_group_count[$group]:-0}" == "$expected_count" ]] || fail "Source compiler samples do not cover required group: $group"
        values="${sample_group_values[$group]}"
        IFS=$'\t' read -r median_value relative outliers <<< "$(stats_for_values "$values" "${policy_max_relative[$metric_name3]}")"
        awk -v value="$relative" -v maximum="${policy_max_relative[$metric_name3]}" 'BEGIN { exit !(value <= maximum + 1e-12) }' || fail "Source compiler sample variance is inconclusive: $group"
        (( outliers <= ${policy_max_outliers[$metric_name3]} )) || fail "Source compiler sample outliers exceed policy: $group"
        median["$group"]="$median_value"
      done
    done
  done
  local pair_number pair_source pair_reference pair_source_key pair_reference_key
  for cell_key in "${!cell_seen[@]}"; do
    IFS=',' read -ra metric_names <<< "${cell_metrics[$cell_key]}"
    for metric_name3 in "${metric_names[@]}"; do
      for (( pair_number=1; pair_number <= ${policy_samples[$metric_name3]}; pair_number++ )); do
        pair_source_key="$cell_key|source|$metric_name3|$pair_number"
        pair_reference_key="$cell_key|rust-reference|$metric_name3|$pair_number"
        pair_source="${sample_value_by_key[$pair_source_key]-}"
        pair_reference="${sample_value_by_key[$pair_reference_key]-}"
        [[ -n "$pair_source" && -n "$pair_reference" ]] || fail "missing paired Source/Rust-reference sample: $cell_key/$metric_name3/$pair_number"
        awk -v source="$pair_source" -v reference="$pair_reference" 'BEGIN { exit !(source < reference) }' || fail "strict paired Source compiler loss: $cell_key/$metric_name3/$pair_number (Source=$pair_source Rust-reference=$pair_reference; required <1.00)"
      done
    done
  done


  local source_median reference_median ratio source_group_key reference_group_key
  for cell_key in "${!cell_seen[@]}"; do
    IFS=',' read -ra metric_names <<< "${cell_metrics[$cell_key]}"
    for metric_name3 in "${metric_names[@]}"; do
      source_group_key="$cell_key|source|$metric_name3"
      reference_group_key="$cell_key|rust-reference|$metric_name3"
      source_median="${median[$source_group_key]-}"
      reference_median="${median[$reference_group_key]-}"
      [[ -n "$source_median" && -n "$reference_median" ]] || fail "missing Source/Rust-reference comparison cell: $cell_key/$metric_name3"
      ratio=$(awk -v source="$source_median" -v reference="$reference_median" 'BEGIN { if (reference == 0) exit 2; printf "%.12g", source / reference }') || fail "invalid Source/Rust-reference comparison: $cell_key/$metric_name3"
      awk -v source="$source_median" -v reference="$reference_median" 'BEGIN { exit !(source < reference) }' || fail "strict Source compiler implementation loss: $cell_key/$metric_name3 ratio=$ratio (Source=$source_median Rust-reference=$reference_median; required <1.00)"
    done
  done
  echo "source compiler gate: pass report=$report"
}

case "${1:-}" in
  --contract) [[ "$#" -eq 1 ]] || usage; static_contract ;;
  --check) [[ "$#" -eq 2 ]] || usage; static_contract; check_report "$2" ;;
  *) usage ;;
esac
