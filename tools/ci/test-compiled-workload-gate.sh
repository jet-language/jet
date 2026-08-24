#!/usr/bin/env bash
# Card #1414: validator self-check. It creates only synthetic scratch reports.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
CORPUS="$ROOT/tests/compiled_workloads"
scratch_root="${TMPDIR:-$HOME/.cache/jet-test-scratch}"
mkdir -p "$scratch_root"
tmp="$(mktemp -d "$scratch_root/compiled-workload-gate.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

bash tools/ci/compiled-workload-gate.sh --contract

report="$tmp/pass"
mkdir -p "$report"

awk -F '\t' -v OFS='\t' \
  -v header='version\ttask_id\tpeer_language\tpeer_program\tinput\texpected\toutcome\ttoolchain_id\tjet_tool_version\tpeer_tool_version\tdependency_rule\tsource_boundary\tjet_status\tpeer_status\tloss_owner\treview_status\treview_evidence' \
  'FILENAME == ARGV[1] { if (FNR > 1 && $3 == "best-applicable") { lang[$2] = $4; program[$2] = $5; dep[$2] = $10; boundary[$2] = $11 } next }
   FILENAME == ARGV[2] { if (FNR == 1) print header; else print 1,$2,lang[$2],program[$2],$6,$7,$5,"fixture-2026-08-24","jet-fixture","peer-fixture",dep[$2],boundary[$2],"pass","pass","-","pass","review-fixture" }' \
  "$CORPUS/peer_ledger.tsv" "$CORPUS/manifest.tsv" >"$report/outcomes.tsv"

awk -F '\t' -v OFS='\t' \
  -v header='version\ttask_id\tlanguage\tmetric\tvalue\tunit\ttoolchain_id\tevidence\tstatus\tloss_owner' \
  'FILENAME == ARGV[1] { if (FNR > 1 && $3 == "best-applicable") lang[$2] = $4; next }
   FILENAME == ARGV[2] { if (FNR > 1) { metric[++metric_count] = $2; unit[$2] = $3 } next }
   FILENAME == ARGV[3] { if (FNR > 1) { task[++task_count] = $2; next } }
   END { print header; for (i = 1; i <= task_count; i++) for (l = 0; l < 2; l++) { language = l == 0 ? "jet" : lang[task[i]]; for (m = 1; m <= metric_count; m++) print 1,task[i],language,metric[m],1,unit[metric[m]],"fixture-2026-08-24","measurement-fixture","measured","-" } }' \
  "$CORPUS/peer_ledger.tsv" "$CORPUS/metric_contract.tsv" "$CORPUS/manifest.tsv" >"$report/measurements.tsv"

awk -F '\t' -v OFS='\t' \
  -v header='version\ttask_id\tlanguage\tplatform\ttarget\ttier\tstatus\tevidence\tloss_owner' \
  'FILENAME == ARGV[1] { if (FNR > 1 && $3 == "best-applicable") lang[$2] = $4; next }
   FILENAME == ARGV[2] { if (FNR == 1) print header; else { print 1,$2,"jet",$3,$4,$5,($6 == "excluded" ? "not-applicable" : "pass"),"tier-fixture","-"; print 1,$2,lang[$2],$3,$4,$5,($6 == "excluded" ? "not-applicable" : "pass"),"tier-fixture","-" } }' \
  "$CORPUS/peer_ledger.tsv" "$CORPUS/tier_matrix.tsv" >"$report/tiers.tsv"

bash tools/ci/compiled-workload-gate.sh --check "$report"

expect_reject() {
  local name="$1" expected="$2" path="$3"
  set +e
  output="$(bash tools/ci/compiled-workload-gate.sh --check "$path" 2>&1)"
  status=$?
  set -e
  [[ "$status" -ne 0 ]] || { echo "canary accepted: $name" >&2; exit 1; }
  grep -Fq "$expected" <<<"$output" || { echo "canary drifted: $name" >&2; echo "$output" >&2; exit 1; }
  echo "canary: $name"
}

cp -R "$report" "$tmp/missing-outcome"
sed -i '$d' "$tmp/missing-outcome/outcomes.tsv"
expect_reject missing-outcome 'outcome report does not cover frozen manifest' "$tmp/missing-outcome"

cp -R "$report" "$tmp/unowned-loss"
awk -F '\t' -v OFS='\t' 'NR == 2 { $13 = "loss"; $15 = "unowned" } { print }' "$tmp/unowned-loss/outcomes.tsv" >"$tmp/unowned-loss/new" && mv "$tmp/unowned-loss/new" "$tmp/unowned-loss/outcomes.tsv"
expect_reject unowned-loss 'unowned Jet loss' "$tmp/unowned-loss"

cp -R "$report" "$tmp/missing-metric"
sed -i '$d' "$tmp/missing-metric/measurements.tsv"
expect_reject missing-metric 'missing measurement' "$tmp/missing-metric"

cp -R "$report" "$tmp/missing-tier"
sed -i '$d' "$tmp/missing-tier/tiers.tsv"
expect_reject missing-tier 'missing tier proof' "$tmp/missing-tier"

cp -R "$report" "$tmp/changed-input"
awk -F '\t' -v OFS='\t' 'NR == 2 { $5 = "changed" } { print }' "$tmp/changed-input/outcomes.tsv" >"$tmp/changed-input/new" && mv "$tmp/changed-input/new" "$tmp/changed-input/outcomes.tsv"
expect_reject changed-input 'input or outcome drifted' "$tmp/changed-input"

cp -R "$report" "$tmp/unreviewed"
awk -F '\t' -v OFS='\t' 'NR == 2 { $16 = "pending" } { print }' "$tmp/unreviewed/outcomes.tsv" >"$tmp/unreviewed/new" && mv "$tmp/unreviewed/new" "$tmp/unreviewed/outcomes.tsv"
expect_reject unreviewed 'independent review missing' "$tmp/unreviewed"

echo 'test result: compiled workload gate self-check: pass'
