#!/usr/bin/env bash
# Focused CODE canary for the criterion11 gate.  This file is intentionally
# not run by the implementation worker: the owner runs it after Foundation's
# real private adapter is integrated.  The fixture is complete across every
# frozen cell, workflow, tier, implementation, metric, and sample; only the
# Source values mutate, proving that parity and the general 1.05 noise band
# are both losses in this lane.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PERF_DIR="$ROOT/Tools/perf"
GATE="$PERF_DIR/source-compiler-gate.sh"
CONTRACT="$PERF_DIR/source-compiler-contract.tsv"
POLICY="$PERF_DIR/source-compiler-policy.tsv"
SCRATCH_ROOT="${JET_PERF_SCRATCH_ROOT:-$HOME/.cache/jet-dev/perf-bench}"
mkdir -p "$SCRATCH_ROOT"
case "$(realpath -m -- "$SCRATCH_ROOT")" in
  /tmp|/tmp/*|*/target|*/target/*) echo "source compiler canary scratch must be disk-backed and outside target" >&2; exit 1 ;;
esac
REPORT="$(mktemp -d "$SCRATCH_ROOT/source-compiler-gate-canary.XXXXXX")"
trap 'rm -rf "$REPORT"' EXIT

candidate="$(git -C "$ROOT" rev-parse --verify HEAD)"
export JET_CI_CANDIDATE_COMMIT="$candidate"
adapter_path=/bin/sh
adapter_sha256="$(sha256sum "$adapter_path" | awk '{ print $1 }')"
compiler_sha256=cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
artifact_sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
generated_sha256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb

sha256_file() { sha256sum -- "$1" | awk '{ print $1 }'; }
contract_sha256() {
  {
    printf 'Tools/perf/source-compiler-contract.tsv\t%s\n' "$(sha256_file "$CONTRACT")"
    printf 'Tools/perf/source-compiler-policy.tsv\t%s\n' "$(sha256_file "$POLICY")"
  } | sha256sum | awk '{ print $1 }'
}
source_manifest_sha256="$(sha256_file "$ROOT/Compiler/Bootstrap/sources.list")"

write_identity() {
  cat >"$REPORT/identity.tsv" <<EOF
version	key	value
1	contract_sha256	$(contract_sha256)
1	candidate_commit	$candidate
1	machine	canary-machine
1	environment	os=linux;target=x86_64-unknown-linux-gnu,web;locale=C;network=disabled
1	adapter_path	$adapter_path
1	adapter_version	source-compiler-performance-adapter-v2
1	adapter_sha256	$adapter_sha256
1	sample_count	5
1	source_manifest_sha256	$source_manifest_sha256
1	target_set	x86_64-unknown-linux-gnu,web
1	implementation_pair	source,rust-reference
1	workflows	factory,runner
1	execution_tiers	aot,cranelift-jit,source-interpreter-deopt
1	execution_scope	generated-target-program
1	pairing	paired-same-run-v1
1	run_id	canary-run
1	ratio_policy	strict-lt-1.00
EOF
}

make_report() {
  local source_value="$1" execution_harness
  write_identity
  printf '%s\n' 'version	cell_id	case	workflow	execution_tier	implementation	metric	sample	pair_id	value	unit	receipt_key	evidence' >"$REPORT/samples.tsv"
  printf '%s\n' 'version	cell_id	case	workflow	execution_tier	implementation	status	source_input_sha256	source_closure_sha256	expected_sha256	output_sha256	diagnostic_sha256	diagnostic_count	semantic_expected_sha256	semantic_output_sha256	semantic_equivalence	artifact_sha256	generated_source_sha256	compiler_sha256	target	profile	optimization	artifact_kind	correctness	source_boundary	command	adapter_sha256	pairing	evidence' >"$REPORT/receipts.tsv"
  while IFS=$'\t' read -r version cell case_name workflow tier input expected entry source_manifest outcome target profile optimization artifact_kind metrics; do
    [[ -z "$version" || "$version" == version || "$version" == \#* ]] && continue
    input_sha256="$(sha256_file "$ROOT/$input")"
    closure_sha256="$(sha256_file "$ROOT/$source_manifest")"
    semantic_sha256=dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd
    if [[ "$case_name" == selfcompile ]]; then
      expected_sha256=-
      output_sha256=-
      diagnostic_sha256=-
      diagnostic_count=0
      semantic_expected_sha256="$semantic_sha256"
      semantic_output_sha256="$semantic_sha256"
      semantic_equivalence=verified
      cell_artifact="$artifact_sha256"
      cell_generated="$generated_sha256"
    elif [[ "$case_name" == invalid ]]; then
      expected_sha256="$(sha256_file "$ROOT/$expected")"
      output_sha256=-
      diagnostic_sha256="$expected_sha256"
      diagnostic_count=1
      semantic_expected_sha256=-
      semantic_output_sha256=-
      semantic_equivalence=-
      cell_artifact=-
      cell_generated=-
    else
      expected_sha256="$(sha256_file "$ROOT/$expected")"
      output_sha256="$expected_sha256"
      diagnostic_sha256=-
      diagnostic_count=0
      semantic_expected_sha256=-
      semantic_output_sha256=-
      semantic_equivalence=-
      cell_artifact="$artifact_sha256"
      cell_generated="$generated_sha256"
    fi
    for implementation in source rust-reference; do
      receipt_key="$cell|$implementation"
      pair_id="canary-run|$cell|1"
      stdout_evidence_path=-
      [[ "$case_name" == valid ]] && stdout_evidence_path=canary/stdout.bin
      diagnostics_evidence_path=-
      [[ "$case_name" == invalid ]] && diagnostics_evidence_path=canary/diagnostics.bin
      generated_evidence_path=-
      artifact_evidence_path=-
      [[ "$case_name" != invalid ]] && generated_evidence_path=canary/generated-source.bin && artifact_evidence_path=canary/artifact.bin
      semantic_probe_evidence=-
      [[ "$case_name" == selfcompile ]] && semantic_probe_evidence=valid-hello=valid,invalid-missing-return=invalid
      execution_harness=-
      [[ "$target" == web ]] && execution_harness=node-app-js-browser-runtime-v1
      boundary="implementation=$implementation;workflow=$workflow;execution_tier=$tier;execution_scope=generated-target-program;pairing=paired-same-run-v1;target=$target;execution_harness=$execution_harness;profile=$profile;optimization=$optimization;source_entry=$entry;source_input_sha256=$input_sha256;source_manifest_sha256=$closure_sha256;expected_sha256=$expected_sha256;diagnostic_sha256=$diagnostic_sha256;diagnostic_count=$diagnostic_count;semantic_expected_sha256=$semantic_expected_sha256;semantic_output_sha256=$semantic_output_sha256;semantic_equivalence=$semantic_equivalence;artifact_sha256=$cell_artifact;generated_source_sha256=$cell_generated"
      evidence="evidence_version=byte-hash-v1;run_id=canary-run;pair_id=$pair_id;paired=source,rust-reference;pairing=paired-same-run-v1;execution_harness=$execution_harness;request_path=canary/request.json;result_dir=/disk-backed/canary;stdout_path=$stdout_evidence_path;diagnostics_path=$diagnostics_evidence_path;generated_source_path=$generated_evidence_path;artifact_path=$artifact_evidence_path;compiler_path=/bin/sh;output_sha256=$output_sha256;diagnostic_sha256=$diagnostic_sha256;diagnostic_count=$diagnostic_count;semantic_expected_sha256=$semantic_expected_sha256;semantic_output_sha256=$semantic_output_sha256;semantic_equivalence=$semantic_equivalence;artifact_sha256=$cell_artifact;generated_source_sha256=$cell_generated;semantic_probes=$semantic_probe_evidence;semantic_probe_paths=$semantic_probe_evidence"
      printf '1\t%s\t%s\t%s\t%s\t%s\tmeasured\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\tverified\t%s\tadapter-request-v2\t%s\tpaired-same-run-v1\t%s\n' \
        "$cell" "$case_name" "$workflow" "$tier" "$implementation" "$input_sha256" "$closure_sha256" "$expected_sha256" "$output_sha256" "$diagnostic_sha256" "$diagnostic_count" "$semantic_expected_sha256" "$semantic_output_sha256" "$semantic_equivalence" "$cell_artifact" "$cell_generated" "$compiler_sha256" "$target" "$profile" "$optimization" "$artifact_kind" "$boundary" "$adapter_sha256" "$evidence" >>"$REPORT/receipts.tsv"
      IFS=',' read -ra metric_names <<< "$metrics"
      for metric in "${metric_names[@]}"; do
        unit="$(awk -F '\t' -v metric="$metric" 'NR > 1 && $2 == metric { print $3; exit }' "$POLICY")"
        value=100
        [[ "$implementation" == source ]] && value="$source_value"
        for sample in 1 2 3 4 5; do
          pair_id="canary-run|$cell|$sample"
          printf '1\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\tevidence_version=byte-hash-v1;adapter=canary;run_id=canary-run;pair_id=%s;paired=source,rust-reference;pairing=paired-same-run-v1\n' \
            "$cell" "$case_name" "$workflow" "$tier" "$implementation" "$metric" "$sample" "$pair_id" "$value" "$unit" "$receipt_key" "$pair_id" >>"$REPORT/samples.tsv"
        done
      done
    done
  done <"$CONTRACT"
}

expect_status() {
  local expected="$1" label="$2" needle="${3:-}"
  set +e
  output=$(bash "$GATE" --check "$REPORT" 2>&1)
  status=$?
  set -e
  if [[ "$status" -ne "$expected" ]]; then
    printf 'criterion11 canary %s: expected status %s, got %s\n%s\n' "$label" "$expected" "$status" "$output" >&2
    exit 1
  fi
  if [[ -n "$needle" && "$output" != *"$needle"* ]]; then
    printf 'criterion11 canary %s: expected diagnostic %s\n%s\n' "$label" "$needle" "$output" >&2
    exit 1
  fi
}

# 0.99 is a strict win and proves that every frozen cell is accounted for.
make_report 99
expect_status 0 'strict Source win across all cells'

# 1.00 is parity and MUST fail; this is not the general Rust 1.05 policy.
make_report 100
expect_status 1 'parity ratio 1.00' 'required <1.00'

# 1.05 is the general language noise band and MUST also fail this lane.
make_report 105
expect_status 1 'noise-band ratio 1.05' 'required <1.00'

printf '%s\n' 'source compiler gate canary: CODE covers strict ratio and complete matrix'
