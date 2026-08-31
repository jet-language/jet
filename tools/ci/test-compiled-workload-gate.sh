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

candidate_revision="$(git rev-parse HEAD)"
review_evidence="candidate=$candidate_revision;reviewer=compiled-workload-gate-self-check;fairness=frozen-task-and-peer-contract;measurements=synthetic-validator-fixture"

report="$tmp/pass"
mkdir -p "$report"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -- "$1" | awk '{print $1}'
  else
    shasum -a 256 -- "$1" | awk '{print $1}'
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

contract_hash="$(hash_relative_files \
  tests/compiled_workloads/manifest.tsv \
  tests/compiled_workloads/domain_contract.tsv \
  tests/compiled_workloads/peer_ledger.tsv \
  tests/compiled_workloads/metric_contract.tsv \
  tests/compiled_workloads/tier_matrix.tsv \
  tests/compiled_workloads/canaries.tsv \
  tests/compiled_workloads/adapter_ledger.tsv \
  tests/compiled_workloads/measurement_policy.tsv \
  docs/reference/core-surface-ledger.json)"
source_closure_hash="$(hash_relative_files \
  tests/compiled_workloads/adapters \
  tests/compiled_workloads/fixtures \
  tests/compiled_workloads/expected \
  tests/compiled_workloads/task-definitions)"

cat >"$report/identity.tsv" <<EOF
version	key	value
1	candidate_commit	$candidate_revision
1	platform	linux
1	environment	os=linux;ci=compiled-workload;locale=C;network=disabled
1	machine	validator-fixture
1	jet_tool_version	jet-fixture
1	contract_sha256	$contract_hash
1	source_closure_sha256	$source_closure_hash
1	samples	5
1	peer_commits	fixture
EOF

declare -A peer_language=() peer_program=() peer_dependency=() peer_boundary=() peer_commit=()
while IFS=$'\t' read -r version task selection language program source_url source_revision build_command run_command dependency boundary targets owner; do
  [[ "$version" == version || -z "$version" ]] && continue
  if [[ "$selection" == best-applicable ]]; then
    peer_language[$task]="$language"
    peer_program[$task]="$program"
    peer_dependency[$task]="$dependency"
    peer_boundary[$task]="$boundary"
  fi
done <"$CORPUS/peer_ledger.tsv"
declare -A jet_source=() jet_hostile=() peer_source=() peer_hostile=()
while IFS=$'\t' read -r version task source hostile selected_source selected_hostile revision; do
  [[ "$version" == version || -z "$version" ]] && continue
  jet_source[$task]="$source"
  jet_hostile[$task]="$hostile"
  peer_source[$task]="$selected_source"
  peer_hostile[$task]="$selected_hostile"
  peer_commit[$task]="$revision"
done <"$CORPUS/adapter_ledger.tsv"

awk -F '\t' -v OFS='\t' \
  -v header='version\ttask_id\tpeer_language\tpeer_program\tinput\texpected\toutcome\ttoolchain_id\tjet_tool_version\tpeer_tool_version\tdependency_rule\tsource_boundary\tjet_status\tpeer_status\tloss_owner\treview_status\treview_evidence' \
  -v candidate="$candidate_revision" \
  -v review_evidence="$review_evidence" \
  'FILENAME == ARGV[1] { if (FNR > 1 && $3 == "best-applicable") { lang[$2] = $4; program[$2] = $5; dep[$2] = $10; boundary[$2] = $11 } next }
   FILENAME == ARGV[2] { if (FNR == 1) print header; else print 1,$2,lang[$2],program[$2],$6,$7,$5,"platform=linux;candidate=" candidate ";fixture=2026-08-24","jet-fixture","peer-fixture",dep[$2],boundary[$2],"pass","pass","-","pass",review_evidence }' \
  "$CORPUS/peer_ledger.tsv" "$CORPUS/manifest.tsv" >"$report/outcomes.tsv"

awk -F '\t' -v OFS='\t' \
  -v header='version\ttask_id\tlanguage\tmetric\tvalue\tunit\ttoolchain_id\tevidence\tstatus\tloss_owner' \
  'FILENAME == ARGV[1] { if (FNR > 1 && $3 == "best-applicable") lang[$2] = $4; next }
   FILENAME == ARGV[2] { if (FNR > 1) { metric[++metric_count] = $2; unit[$2] = $3 } next }
   FILENAME == ARGV[3] { if (FNR > 1) { task[++task_count] = $2; next } }
   END { print header; for (i = 1; i <= task_count; i++) for (l = 0; l < 2; l++) { language = l == 0 ? "jet" : lang[task[i]]; toolchain = l == 0 ? "jet-fixture" : "peer-fixture"; for (m = 1; m <= metric_count; m++) print 1,task[i],language,metric[m],1,unit[metric[m]],toolchain,"measurement-fixture","measured","-" } }' \
  "$CORPUS/peer_ledger.tsv" "$CORPUS/metric_contract.tsv" "$CORPUS/manifest.tsv" >"$report/measurements.tsv"

awk -F '\t' -v OFS='\t' \
  'FILENAME == ARGV[1] { if (FNR > 1 && $3 == "best-applicable") lang[$2] = $4; next }
   FILENAME == ARGV[2] { if (FNR > 1) { metric[++metric_count] = $2; unit[$2] = $3 } next }
   FILENAME == ARGV[3] { if (FNR > 1) count[$2] = $3; next }
   FILENAME == ARGV[4] { if (FNR > 1) task[++task_count] = $2; next }
   END {
     print "version", "task_id", "language", "metric", "sample", "value", "unit", "method"
     for (i = 1; i <= task_count; i++) for (l = 0; l < 2; l++) for (m = 1; m <= metric_count; m++) {
       language = l == 0 ? "jet" : lang[task[i]]
       for (s = 1; s <= count[metric[m]]; s++) print 1, task[i], language, metric[m], s, 1, unit[metric[m]], "measurement-fixture"
     }
   }' \
  "$CORPUS/peer_ledger.tsv" "$CORPUS/metric_contract.tsv" "$CORPUS/measurement_policy.tsv" "$CORPUS/manifest.tsv" >"$report/samples.tsv"

awk -F '\t' -v OFS='\t' \
  'FILENAME == ARGV[1] { if (FNR > 1 && $3 == "best-applicable") lang[$2] = $4; next }
   FILENAME == ARGV[2] { if (FNR > 1) { metric[++metric_count] = $2; count[$2] = $3; tolerance[$2] = $9 } next }
   FILENAME == ARGV[3] { if (FNR > 1) task[++task_count] = $2; next }
   END {
     print "version", "task_id", "language", "metric", "samples", "median", "min", "max", "relative_stdev", "outliers", "threshold", "tolerance_ratio", "status", "loss_owner", "evidence"
     for (i = 1; i <= task_count; i++) for (l = 0; l < 2; l++) for (m = 1; m <= metric_count; m++) {
       language = l == 0 ? "jet" : lang[task[i]]
       print 1, task[i], language, metric[m], count[metric[m]], 1, 1, 1, 0, 0, 0, tolerance[metric[m]], "measured", "-", "samples=" count[metric[m]] ";median=1;relative-stdev=0;outliers=0"
     }
   }' \
  "$CORPUS/peer_ledger.tsv" "$CORPUS/measurement_policy.tsv" "$CORPUS/manifest.tsv" >"$report/statistics.tsv"

awk -F '\t' -v OFS='\t' \
  -v header='version\ttask_id\tlanguage\tplatform\ttarget\ttier\tstatus\tevidence\tloss_owner' \
   'function applies(csv, platform, target, parts, i, count, alias) {
     count = split(csv, parts, ",")
     alias = target
     sub(/-.*/, "", alias)
     for (i = 1; i <= count; i++) if (parts[i] == platform || parts[i] == target || parts[i] == alias) return 1
     return 0
   }
   function zero_hash() { return "0000000000000000000000000000000000000000000000000000000000000000" }
   FILENAME == ARGV[1] { if (FNR > 1 && $3 == "best-applicable") { lang[$2] = $4; targets[$2] = $12 } next }
   FILENAME == ARGV[2] { if (FNR == 1) print header; else {
     in_scope = ($3 == "linux" || $3 == "cross-target")
     jet_status = ($6 == "excluded" || !in_scope ? "not-applicable" : "pass")
     peer_status = ($5 == "jit" || $6 == "excluded" || !in_scope || !applies(targets[$2], $3, $4) ? "not-applicable" : "pass")
     jet_artifact = jet_status == "pass" ? zero_hash() : "-"
     peer_artifact = peer_status == "pass" ? zero_hash() : "-"
     jet_output = jet_status == "pass" && !($3 == "cross-target" && $4 != "web") ? zero_hash() : "-"
     peer_output = peer_status == "pass" && !($3 == "cross-target" && $4 != "web") ? zero_hash() : "-"
     jet_evidence = jet_status == "pass" ? "artifact=" jet_artifact ";output=" jet_output ";command=tier-fixture" : "tier-fixture"
     peer_evidence = peer_status == "pass" ? "artifact=" peer_artifact ";output=" peer_output ";command=tier-fixture" : "tier-fixture"
     print 1,$2,"jet",$3,$4,$5,jet_status,jet_evidence,"-"
     print 1,$2,lang[$2],$3,$4,$5,peer_status,peer_evidence,"-"
   } }' \
  "$CORPUS/peer_ledger.tsv" "$CORPUS/tier_matrix.tsv" >"$report/tiers.tsv"
node -e '
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const [corpus, report] = process.argv.slice(1);
const readRows = file => {
  const lines = fs.readFileSync(file, "utf8").trim().split(/\r?\n/);
  const header = lines.shift().split("\t");
  return [header, ...lines.filter(Boolean).map(line => line.split("\t"))];
};
const table = file => {
  const rows = readRows(path.join(corpus, file));
  const header = rows.shift();
  return rows.map(values => Object.fromEntries(values.map((value, index) => [header[index], value])));
};
const hash = relative => crypto.createHash("sha256").update(fs.readFileSync(path.join(corpus, relative))).digest("hex");
const manifest = table("manifest.tsv");
const peers = new Map(table("peer_ledger.tsv").filter(row => row.selection === "best-applicable").map(row => [row.task_id, row]));
const adapters = new Map(table("adapter_ledger.tsv").map(row => [row.task_id, row]));
const write = (file, rows) => fs.writeFileSync(path.join(report, file), rows.map(row => row.join("\t")).join("\n") + "\n");

const receipts = [["version", "task_id", "language", "source_sha256", "input_sha256", "expected_sha256", "output_sha256", "hostile_input_sha256", "hostile_output_sha256", "environment", "machine", "tool_version", "command", "peer_commit", "exit_code", "hostile_exit_code"]];
for (const task of manifest) {
  const peer = peers.get(task.task_id);
  const adapter = adapters.get(task.task_id);
  const expectedOutput = hash(task.expected);
  for (const [language, source, hostile, tool] of [
    ["jet", adapter.jet_source, adapter.jet_hostile, "jet-fixture"],
    [peer.language, adapter.peer_source, adapter.peer_hostile, "peer-fixture"],
  ]) {
    receipts.push(["1", task.task_id, language, hash(source), hash(task.input), expectedOutput, expectedOutput, hash(hostile), hash("expected/" + task.task_id + ".hostile.out"), "os=linux;fixture=compiled-workload-gate", "fixture-machine", tool, "fixture-command", peer.source_revision, "0", "0"]);
  }
}
write("receipts.tsv", receipts);

const tierRows = readRows(path.join(report, "tiers.tsv"));
const tierReceipts = [["version", "task_id", "language", "platform", "target", "tier", "artifact_sha256", "output_sha256", "command", "status"]];
for (const row of tierRows.slice(1)) {
  tierReceipts.push(["1", row[1], row[2], row[3], row[4], row[5], "-", "-", "tier-fixture", row[6]]);
}
write("tier_receipts.tsv", tierReceipts);
' "$CORPUS" "$report"

echo "synthetic receipt proof: pass"


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
cp -R "$report" "$tmp/comparison-dependency-drift"
awk -F '\t' -v OFS='\t' 'NR == 2 { $11 = "changed-dependency" } { print }' "$tmp/comparison-dependency-drift/outcomes.tsv" >"$tmp/comparison-dependency-drift/new" && mv "$tmp/comparison-dependency-drift/new" "$tmp/comparison-dependency-drift/outcomes.tsv"
expect_reject comparison-dependency-drift 'peer comparison identity drifted' "$tmp/comparison-dependency-drift"
cp -R "$report" "$tmp/comparison-toolchain-drift"
awk -F '\t' -v OFS='\t' 'NR == 2 { $10 = "changed-peer-toolchain" } { print }' "$tmp/comparison-toolchain-drift/outcomes.tsv" >"$tmp/comparison-toolchain-drift/new" && mv "$tmp/comparison-toolchain-drift/new" "$tmp/comparison-toolchain-drift/outcomes.tsv"
expect_reject comparison-toolchain-drift 'peer toolchain identity drifted' "$tmp/comparison-toolchain-drift"


cp -R "$report" "$tmp/statistics-drift"
awk -F '\t' -v OFS='\t' 'NR == 2 { $6 = 2 } { print }' "$tmp/statistics-drift/statistics.tsv" >"$tmp/statistics-drift/new" && mv "$tmp/statistics-drift/new" "$tmp/statistics-drift/statistics.tsv"
expect_reject statistics-drift 'statistics median disagrees with measurement' "$tmp/statistics-drift"

cp -R "$report" "$tmp/receipt-input-drift"
awk -F '\t' -v OFS='\t' 'NR == 2 { $5 = "0000000000000000000000000000000000000000000000000000000000000000" } { print }' "$tmp/receipt-input-drift/receipts.tsv" >"$tmp/receipt-input-drift/new" && mv "$tmp/receipt-input-drift/new" "$tmp/receipt-input-drift/receipts.tsv"
expect_reject receipt-input-drift 'receipt input drifted' "$tmp/receipt-input-drift"

cp -R "$report" "$tmp/missing-samples"
rm "$tmp/missing-samples/samples.tsv"
expect_reject missing-samples 'report must contain identity.tsv' "$tmp/missing-samples"

cp -R "$report" "$tmp/missing-statistics"
rm "$tmp/missing-statistics/statistics.tsv"
expect_reject missing-statistics 'report must contain identity.tsv' "$tmp/missing-statistics"

cp -R "$report" "$tmp/missing-receipt"
rm "$tmp/missing-receipt/receipts.tsv"
expect_reject missing-receipt 'report must contain identity.tsv' "$tmp/missing-receipt"

cp -R "$report" "$tmp/missing-tier-receipt"
rm "$tmp/missing-tier-receipt/tier_receipts.tsv"
expect_reject missing-tier-receipt 'report must contain identity.tsv' "$tmp/missing-tier-receipt"


cp -R "$report" "$tmp/missing-outcome"
sed -i '$d' "$tmp/missing-outcome/outcomes.tsv"
expect_reject missing-outcome 'outcome report does not cover frozen manifest' "$tmp/missing-outcome"

cp -R "$report" "$tmp/unowned-loss"
awk -F '\t' -v OFS='\t' 'NR == 2 { $13 = "loss"; $15 = "#1173" } { print }' "$tmp/unowned-loss/outcomes.tsv" >"$tmp/unowned-loss/new" && mv "$tmp/unowned-loss/new" "$tmp/unowned-loss/outcomes.tsv"
expect_reject unowned-loss 'loss owner is not a live Tower card' "$tmp/unowned-loss"

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

cp -R "$report" "$tmp/stale-candidate"
awk -F '\t' -v OFS='\t' 'NR == 2 { sub(/candidate=[0-9a-f]+/, "candidate=0000000000000000000000000000000000000000", $8) } { print }' "$tmp/stale-candidate/outcomes.tsv" >"$tmp/stale-candidate/new" && mv "$tmp/stale-candidate/new" "$tmp/stale-candidate/outcomes.tsv"
expect_reject stale-candidate 'comparison candidate is stale' "$tmp/stale-candidate"

cp -R "$report" "$tmp/unowned-metric-loss"
awk -F '\t' -v OFS='\t' 'NR == 2 { $5 = 2 } { print }' "$tmp/unowned-metric-loss/measurements.tsv" >"$tmp/unowned-metric-loss/new" && mv "$tmp/unowned-metric-loss/new" "$tmp/unowned-metric-loss/measurements.tsv"
expect_reject unowned-metric-loss 'Jet metric loss is not recorded' "$tmp/unowned-metric-loss"

cp -R "$report" "$tmp/toolchain-drift"
awk -F '\t' -v OFS='\t' 'NR == 2 { $7 = "drifted-toolchain" } { print }' "$tmp/toolchain-drift/measurements.tsv" >"$tmp/toolchain-drift/new" && mv "$tmp/toolchain-drift/new" "$tmp/toolchain-drift/measurements.tsv"
expect_reject toolchain-drift 'Jet toolchain identity drifted' "$tmp/toolchain-drift"

echo 'test result: compiled workload gate self-check: pass'
