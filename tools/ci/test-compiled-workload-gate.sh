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
manifest_loss_owner="$(awk -F '\t' 'NR > 1 { print $13; exit }' "$CORPUS/manifest.tsv")"
compiler_sha256=1111111111111111111111111111111111111111111111111111111111111111
peer_launcher_path=/usr/bin/validator-peer-launcher
peer_launcher_version=compiled-workload-peer-isolation-v1
peer_launcher_sha256=2222222222222222222222222222222222222222222222222222222222222222
peer_commits="$(awk -F '\t' 'NR > 1 && $3 == "best-applicable" { printf "%s%s:%s", separator, $2, $7; separator = "," }' "$CORPUS/peer_ledger.tsv")"

report="$tmp/pass"
mkdir -p "$report"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -- "$1" | awk '{print $1}'
  else
    shasum -a 256 -- "$1" | awk '{print $1}'
  fi
}
drop_last_data_row() {
  local file="$1"
  local new="${file}.new"
  awk 'NR == 1 { print; next } { rows[NR] = $0 } END { for (i = 2; i < NR; i++) print rows[i] }' "$file" >"$new"
  mv "$new" "$file"
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

hash_report_files() {
  local report_dir="$1"
  node - "$report_dir" identity.tsv samples.tsv statistics.tsv outcomes.tsv measurements.tsv tiers.tsv receipts.tsv tier_receipts.tsv peer_coverage.tsv peer_measurements.tsv <<'NODE'
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const args = process.argv.slice(2);
const root = args.shift();
const digest = crypto.createHash("sha256");
for (const relative of args.sort()) {
  digest.update(relative);
  digest.update("\0");
  digest.update(fs.readFileSync(path.join(root, relative)));
  digest.update("\0");
}
process.stdout.write(digest.digest("hex"));
NODE
}

contract_hash="$(hash_relative_files \
  tests/compiled_workloads/manifest.tsv \
  tests/compiled_workloads/domain_contract.tsv \
  tests/compiled_workloads/peer_ledger.tsv \
  tests/compiled_workloads/peer_adapter_ledger.tsv \
  tests/compiled_workloads/metric_contract.tsv \
  tests/compiled_workloads/tier_matrix.tsv \
  tests/compiled_workloads/canaries.tsv \
  tests/compiled_workloads/adapter_ledger.tsv \
  tests/compiled_workloads/measurement_policy.tsv \
  tests/compiled_workloads/toolchain_contract.tsv)"
source_closure_hash="$(hash_relative_files \
  tests/compiled_workloads/adapters \
  tests/compiled_workloads/fixtures \
  tests/compiled_workloads/expected \
  tests/compiled_workloads/task-definitions)"
jet_self_version="$(awk -F '\t' '$2 == "jet" { print $4 "-self-check" }' "$CORPUS/toolchain_contract.tsv")"

cat >"$report/identity.tsv" <<EOF
version	key	value
1	candidate_commit	$candidate_revision
1	platform	linux
1	environment	os=linux;ci=compiled-workload;locale=C;network=per-task-declared
1	machine	validator-self-check
1	jet_tool_version	$jet_self_version
1	contract_sha256	$contract_hash
1	jet_binary_sha256	$compiler_sha256
1	peer_launcher_path	$peer_launcher_path
1	peer_launcher_version	$peer_launcher_version
1	peer_launcher_sha256	$peer_launcher_sha256
1	source_closure_sha256	$source_closure_hash
1	samples	5
1	peer_commits	$peer_commits
EOF
toolchain_identity_path() {
  local language="$1" command_name path
  command_name="$(awk -F '\t' -v language="$language" '$2 == language { print $3; exit }' "$CORPUS/toolchain_contract.tsv")"
  path="$(command -v "$command_name" 2>/dev/null || true)"
  [[ -n "$path" ]] || path="$ROOT/tools/ci/test-compiled-workload-gate.sh"
  printf '%s' "$path"
}
for toolchain_language in jet rust cxx go zig domain; do
  toolchain_path="$(toolchain_identity_path "$toolchain_language")"
  toolchain_version="$(awk -F '\t' -v language="$toolchain_language" '$2 == language { print $4 "-self-check"; exit }' "$CORPUS/toolchain_contract.tsv")"
  printf '1\ttoolchain_%s_path\t%s\n1\ttoolchain_%s_sha256\t%s\n1\ttoolchain_%s_version\t%s\n' \
    "$toolchain_language" "$toolchain_path" "$toolchain_language" "$(sha256_file "$toolchain_path")" \
    "$toolchain_language" "$toolchain_version" >>"$report/identity.tsv"
done


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
  'FILENAME == ARGV[1] { if (FNR > 1) tool[$2] = $4 "-self-check"; next }
   FILENAME == ARGV[2] { if (FNR > 1 && $3 == "best-applicable") { lang[$2] = $4; program[$2] = $5; dep[$2] = $10; boundary[$2] = $11 } next }
   FILENAME == ARGV[3] { if (FNR == 1) print header; else print 1,$2,lang[$2],program[$2],$6,$7,$5,"platform=linux;candidate=" candidate ";producer=validator-self-check",tool["jet"],tool[lang[$2]],dep[$2],boundary[$2],"pass","pass","-","pending","-" }' \
  "$CORPUS/toolchain_contract.tsv" "$CORPUS/peer_ledger.tsv" "$CORPUS/manifest.tsv" >"$report/outcomes.tsv"

awk -F '\t' -v OFS='\t' \
  -v header='version\ttask_id\tlanguage\tmetric\tvalue\tunit\ttoolchain_id\tevidence\tstatus\tloss_owner\tplatform\ttarget\ttier' \
  'FILENAME == ARGV[1] { if (FNR > 1) tool[$2] = $4 "-self-check"; next }
   FILENAME == ARGV[2] { if (FNR > 1 && $3 == "best-applicable") lang[$2] = $4; next }
   FILENAME == ARGV[3] { if (FNR > 1) { metric[++metric_count] = $2; unit[$2] = $3 } next }
   FILENAME == ARGV[4] { if (FNR > 1) task[++task_count] = $2; next }
   END { print header; for (i = 1; i <= task_count; i++) for (l = 0; l < 2; l++) { language = l == 0 ? "jet" : lang[task[i]]; value = language == "jet" || language == "rust" ? 1 : 2; for (m = 1; m <= metric_count; m++) print 1,task[i],language,metric[m],value,unit[metric[m]],tool[language],"validator-self-check-measurement;comparison-peer=" lang[task[i]] ";comparison-operator=" (lang[task[i]] == "rust" ? ">" : ">=") ";tolerance-ratio=" (lang[task[i]] == "rust" ? "1.05" : "1.00"),"measured","-","linux","native","aot" } }' \
  "$CORPUS/toolchain_contract.tsv" "$CORPUS/peer_ledger.tsv" "$CORPUS/metric_contract.tsv" "$CORPUS/manifest.tsv" >"$report/measurements.tsv"

awk -F '\t' -v OFS='\t' \
  'FILENAME == ARGV[1] { if (FNR > 1 && $3 == "best-applicable") lang[$2] = $4; next }
   FILENAME == ARGV[2] { if (FNR > 1) { metric[++metric_count] = $2; unit[$2] = $3 } next }
   FILENAME == ARGV[3] { if (FNR > 1) count[$2] = $3; next }
   FILENAME == ARGV[4] { if (FNR > 1) task[++task_count] = $2; next }
   END {
     print "version", "task_id", "language", "metric", "sample", "value", "unit", "method", "platform", "target", "tier"
     for (i = 1; i <= task_count; i++) for (l = 0; l < 2; l++) for (m = 1; m <= metric_count; m++) {
       language = l == 0 ? "jet" : lang[task[i]]
       value = language == "jet" || language == "rust" ? 1 : 2
       for (s = 1; s <= count[metric[m]]; s++) print 1, task[i], language, metric[m], s, value, unit[metric[m]], "validator-self-check-measurement;comparison-peer=" lang[task[i]] ";comparison-operator=" (lang[task[i]] == "rust" ? ">" : ">=") ";tolerance-ratio=" (lang[task[i]] == "rust" ? "1.05" : "1.00"), "linux", "native", "aot"
     }
   }' \
  "$CORPUS/peer_ledger.tsv" "$CORPUS/metric_contract.tsv" "$CORPUS/measurement_policy.tsv" "$CORPUS/manifest.tsv" >"$report/samples.tsv"
awk -F '\t' -v OFS='\t' \
  'FILENAME == ARGV[1] { if (FNR > 1 && $3 == "best-applicable") lang[$2] = $4; next }
   FILENAME == ARGV[2] { if (FNR > 1) { metric[++metric_count] = $2; count[$2] = $3; tolerance[$2] = $9; rust_tolerance[$2] = $10 } next }
   FILENAME == ARGV[3] { if (FNR > 1) task[++task_count] = $2; next }
   END {
     print "version", "task_id", "language", "metric", "samples", "median", "min", "max", "relative_stdev", "outliers", "threshold", "tolerance_ratio", "status", "loss_owner", "evidence", "platform", "target", "tier"
     for (i = 1; i <= task_count; i++) for (l = 0; l < 2; l++) for (m = 1; m <= metric_count; m++) {
       language = l == 0 ? "jet" : lang[task[i]]
       value = language == "jet" || language == "rust" ? 1 : 2
       applied = lang[task[i]] == "rust" ? rust_tolerance[metric[m]] : tolerance[metric[m]]
       operator = lang[task[i]] == "rust" ? ">" : ">="
       print 1, task[i], language, metric[m], count[metric[m]], value, value, value, 0, 0, 0, applied, "measured", "-", "samples=" count[metric[m]] ";median=" value ";relative-stdev=0;outliers=0;producer=validator-self-check;comparison-peer=" lang[task[i]] ";comparison-operator=" operator ";tolerance-ratio=" applied, "linux", "native", "aot"
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
   function proof_hash() { return "1111111111111111111111111111111111111111111111111111111111111111" }
   function not_run(scope, availability, reason, rationale) {
     if (scope == "excluded") return "not-run:excluded;availability=" availability ";reason=" reason ";rationale=" rationale
     if (scope == "out-of-scope") return "not-run:platform-scope=linux;availability=" availability ";reason=" reason
     if (scope == "peer-native-only" || scope == "peer-target-not-declared") return "not-run:reason=" scope
     return "not-run:reason=" reason
   }
   FILENAME == ARGV[1] { if (FNR > 1 && $3 == "best-applicable") { lang[$2] = $4; targets[$2] = $12 } next }
   FILENAME == ARGV[2] { if (FNR == 1) print header; else {
     in_scope = ($3 == "linux" || $3 == "cross-target")
     jet_status = ($6 == "excluded" || !in_scope ? "not-applicable" : "pass")
     peer_status = ($5 == "jit" || $6 == "excluded" || !in_scope || !applies(targets[$2], $3, $4) ? "not-applicable" : "pass")
     jet_artifact = jet_status == "pass" && $5 != "jit" ? proof_hash() : "-"
     peer_artifact = peer_status == "pass" ? proof_hash() : "-"
     jet_output = jet_status == "pass" && !($3 == "cross-target" && $4 != "web") ? proof_hash() : "-"
     peer_output = peer_status == "pass" && !($3 == "cross-target" && $4 != "web") ? proof_hash() : "-"
     jet_evidence = jet_status == "pass" ? "artifact=" jet_artifact ";output=" jet_output ";command=validator-self-check-tier" : not_run($6 == "excluded" ? "excluded" : "out-of-scope", $7, $8, $9)
     peer_evidence = peer_status == "pass" ? "artifact=" peer_artifact ";output=" peer_output ";command=validator-self-check-tier" : not_run($6 == "excluded" ? "excluded" : (!in_scope ? "out-of-scope" : ($5 == "jit" ? "peer-native-only" : "peer-target-not-declared")), $7, $8, $9)
     print 1,$2,"jet",$3,$4,$5,jet_status,jet_evidence,"-"
     print 1,$2,lang[$2],$3,$4,$5,peer_status,peer_evidence,"-"
   } }' \
  "$CORPUS/peer_ledger.tsv" "$CORPUS/tier_matrix.tsv" >"$report/tiers.tsv"
declare -A tool_version=()
while IFS=$'\t' read -r version language command version_prefix targets source; do
  [[ "$version" == version || -z "$version" ]] && continue
  tool_version[$language]="${version_prefix}-self-check"
done <"$CORPUS/toolchain_contract.tsv"

{
  printf '%s\n' $'version\ttask_id\tlanguage\tsource_sha256\tinput_sha256\texpected_sha256\toutput_sha256\thostile_input_sha256\thostile_output_sha256\tenvironment\tmachine\ttool_version\tcommand\tpeer_commit\texit_code\thostile_exit_code\tpeer_launcher_path\tpeer_launcher_version\tpeer_launcher_sha256\tauthority'
  while IFS=$'\t' read -r version task domain case_name outcome input expected authority adapters platforms evidence tower loss; do
    [[ "$version" == version || -z "$version" ]] && continue
    peer="${peer_language[$task]}"
    input_sha="$(hash_input_path "$CORPUS/$input")"
    expected_sha="$(sha256_file "$CORPUS/$expected")"
    hostile_output_sha="$(sha256_file "$CORPUS/expected/$task.hostile.out")"
    network=disabled
    [[ "$authority" == *network=loopback-only* ]] && network=loopback-only
    environment="os=linux;ci=compiled-workload;locale=C;network=$network"
    for language in jet "$peer"; do
      if [[ "$language" == jet ]]; then
        source="${jet_source[$task]}"
        hostile="${jet_hostile[$task]}"
      else
        source="${peer_source[$task]}"
        hostile="${peer_hostile[$task]}"
      fi
      source_method=file-bytes-v1
      input_method=file-bytes-v1
      hostile_method=file-bytes-v1
      [[ -d "$CORPUS/$source" ]] && source_method=tree-mode-v1
      [[ -d "$CORPUS/$input" ]] && input_method=tree-mode-v1
      [[ -d "$CORPUS/$hostile" ]] && hostile_method=tree-mode-v1
      printf '1\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t0\t0\t%s\t%s\t%s\t%s\n' \
        "$task" "$language" "$(hash_input_path "$CORPUS/$source")" "$input_sha" "$expected_sha" "$expected_sha" \
        "$(hash_input_path "$CORPUS/$hostile")" "$hostile_output_sha" "$environment" \
        "validator-self-check-machine" "${tool_version[$language]}" \
        "validator-self-check-command;source-hash-method=$source_method;input-hash-method=$input_method;hostile-input-hash-method=$hostile_method" \
        "${peer_commit[$task]}" "$peer_launcher_path" "$peer_launcher_version" "$peer_launcher_sha256" "$authority"
    done
  done <"$CORPUS/manifest.tsv"
} >"$report/receipts.tsv"

awk -F '\t' -v OFS='\t' '
  function field(evidence, name, count, parts, pair, i) {
    count = split(evidence, parts, ";")
    for (i = 1; i <= count; i++) {
      split(parts[i], pair, "=")
      if (pair[1] == name) return pair[2]
    }
    return "-"
  }
  NR == 1 {
    print "version", "task_id", "language", "platform", "target", "tier", "artifact_sha256", "output_sha256", "command", "status"
    next
  }
  {
    artifact = $7 == "pass" || $7 == "loss" ? field($8, "artifact") : "-"
    output = $7 == "pass" || $7 == "loss" ? field($8, "output") : "-"
    command = $7 == "not-applicable" ? "not-run:validator-self-check-tier" : "validator-self-check-tier"
    print 1, $2, $3, $4, $5, $6, artifact, output, command, $7
  }
' "$report/tiers.tsv" >"$report/tier_receipts.tsv"
node - "$report" "$CORPUS" <<'NODE'
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const [report, corpus] = process.argv.slice(2);
const read = file => {
  const lines = fs.readFileSync(file, "utf8").trimEnd().split(/\r?\n/);
  const header = lines.shift().split("\t");
  return lines.filter(Boolean).map(line => Object.fromEntries(header.map((key, index) => [key, line.split("\t")[index]])));
};
const hash = file => crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
const peers = read(path.join(corpus, "peer_ledger.tsv"));
const adapters = read(path.join(corpus, "peer_adapter_ledger.tsv"));
const manifest = read(path.join(corpus, "manifest.tsv"));
const receipts = read(path.join(report, "receipts.tsv"));
const metrics = read(path.join(corpus, "metric_contract.tsv"));
const tools = Object.fromEntries(read(path.join(corpus, "toolchain_contract.tsv")).map(row => [row.language, row.version_prefix + "-self-check"]));
const keyOf = peer => [peer.task_id, peer.selection, peer.language, peer.program].join("|");
const adapterByKey = Object.fromEntries(adapters.map(row => [row.peer_key, row]));
const taskById = Object.fromEntries(manifest.map(row => [row.task_id, row]));
const receiptByTask = Object.fromEntries(receipts.map(row => [row.task_id, row]));
const coverageHeader = [
  "version", "peer_key", "task_id", "selection", "language", "program", "source", "source_revision",
  "source_sha256", "input_sha256", "expected_sha256", "output_sha256", "hostile_input_sha256",
  "hostile_output_sha256", "status", "platform", "target", "tier", "command", "evidence",
];
const measurementHeader = [
  "version", "peer_key", "task_id", "selection", "language", "metric", "samples", "median", "min",
  "max", "relative_stdev", "unit", "status", "evidence", "platform", "target", "tier",
];
const coverage = [coverageHeader.join("\t")];
const measurements = [measurementHeader.join("\t")];
for (const peer of peers) {
  const key = keyOf(peer);
  const adapter = adapterByKey[key];
  const task = taskById[peer.task_id];
  const receipt = receiptByTask[peer.task_id];
  const source = path.join(corpus, adapter.peer_source);
  const applies = peer.applicable_targets.split(",").map(value => value.trim()).includes("linux");
  const status = applies ? "measured" : "not-applicable";
  const sourceSha = hash(source);
  const inputSha = receipt.input_sha256;
  const expectedSha = receipt.expected_sha256;
  const hostileInputSha = receipt.hostile_input_sha256;
  const outputSha = applies ? expectedSha : "-";
  const hostileOutputSha = applies ? receipt.hostile_output_sha256 : "-";
  const command = applies ? "build=validator-self-check;run=validator-self-check;hostile=validator-self-check" : "not-run:host-platform";
  const evidence = applies
    ? `toolchain=${tools[peer.language]};source-sha256=${sourceSha};input-sha256=${inputSha};expected-sha256=${expectedSha};output-sha256=${outputSha};hostile-input-sha256=${hostileInputSha};hostile-output-sha256=${hostileOutputSha};diagnostic=validator-self-check`
    : `peer-targets=${peer.applicable_targets};reason=host-platform`;
  coverage.push([
    "1", key, peer.task_id, peer.selection, peer.language, peer.program, adapter.peer_source, peer.source_revision,
    sourceSha, inputSha, expectedSha, outputSha, hostileInputSha, hostileOutputSha, status, "linux", "-", "native",
    command, evidence,
  ].join("\t"));
  for (const metric of metrics) {
    const peerValue = applies ? (peer.language === "rust" ? "1" : "2") : "-";
    measurements.push([
      "1", key, peer.task_id, peer.selection, peer.language, metric.metric, applies ? "5" : "0",
      peerValue, peerValue, peerValue, applies ? "0" : "-", metric.unit,
      status, applies ? `toolchain=${tools[peer.language]};validator-self-check-peer-measurement` : `peer-targets=${peer.applicable_targets};reason=host-platform`,
      "linux", "-", "native",
    ].join("\t"));
  }
}
fs.writeFileSync(path.join(report, "peer_coverage.tsv"), coverage.join("\n") + "\n");
fs.writeFileSync(path.join(report, "peer_measurements.tsv"), measurements.join("\n") + "\n");
NODE

REVIEW_WORKFLOW=compiled-workload-review.yml \
REVIEW_RUN=self-check-1 \
REVIEW_ACTOR=validator-self-check \
CANDIDATE_SHA="$candidate_revision" \
node tools/ci/compiled-workload-review.mjs "$report"

echo "synthetic external review proof: pass"

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
refresh_review_digest() {
  local directory="$1" digest new="$1/review.tsv.new"
  digest="$(hash_report_files "$directory")"
  awk -F '\t' -v OFS='\t' -v digest="$digest" 'NR == 2 { $7 = digest } { print }' "$directory/review.tsv" >"$new"
  mv "$new" "$directory/review.tsv"
}


expect_command_reject() {
  local name="$1"
  set +e
  output="$(bash tools/ci/compiled-workload-gate.sh --review "$report" 2>&1)"
  status=$?
  set -e
  [[ "$status" -ne 0 ]] || { echo "canary accepted: $name" >&2; exit 1; }
  grep -Fq "usage:" <<<"$output" || { echo "canary drifted: $name" >&2; echo "$output" >&2; exit 1; }
  echo "canary: $name"
}
expect_command_reject removed-self-review-command

cp -R "$report" "$tmp/missing-review"
rm "$tmp/missing-review/review.tsv"
expect_reject missing-review 'report must contain external review.tsv' "$tmp/missing-review"

cp -R "$report" "$tmp/stale-external-review"
awk -F '\t' -v OFS='\t' 'NR == 2 { $3 = "0000000000000000000000000000000000000000" } { print }' "$tmp/stale-external-review/review.tsv" >"$tmp/stale-external-review/new" && mv "$tmp/stale-external-review/new" "$tmp/stale-external-review/review.tsv"
expect_reject stale-external-review 'external review candidate is stale or invalid' "$tmp/stale-external-review"

cp -R "$report" "$tmp/mismatched-external-review"
awk -F '\t' -v OFS='\t' 'NR == 2 { $7 = "0000000000000000000000000000000000000000000000000000000000000000" } { print }' "$tmp/mismatched-external-review/review.tsv" >"$tmp/mismatched-external-review/new" && mv "$tmp/mismatched-external-review/new" "$tmp/mismatched-external-review/review.tsv"
expect_reject mismatched-external-review 'external review report digest disagrees' "$tmp/mismatched-external-review"

cp -R "$report" "$tmp/reviewer-self"
awk -F '\t' -v OFS='\t' 'NR == 2 { $2 = "compiled-workload-gate" } { print }' "$tmp/reviewer-self/review.tsv" >"$tmp/reviewer-self/new" && mv "$tmp/reviewer-self/new" "$tmp/reviewer-self/review.tsv"
expect_reject reviewer-self 'external review reviewer is not independent' "$tmp/reviewer-self"

cp -R "$report" "$tmp/missing-compiler-identity"
awk -F '\t' -v OFS='\t' '$2 != "jet_binary_sha256" { print }' "$tmp/missing-compiler-identity/identity.tsv" >"$tmp/missing-compiler-identity/new" && mv "$tmp/missing-compiler-identity/new" "$tmp/missing-compiler-identity/identity.tsv"
expect_reject missing-compiler-identity 'report identity is missing: jet_binary_sha256' "$tmp/missing-compiler-identity"

cp -R "$report" "$tmp/compiler-identity-drift"
awk -F '\t' -v OFS='\t' '$2 == "jet_binary_sha256" { $3 = "0000000000000000000000000000000000000000000000000000000000000000" } { print }' "$tmp/compiler-identity-drift/identity.tsv" >"$tmp/compiler-identity-drift/new" && mv "$tmp/compiler-identity-drift/new" "$tmp/compiler-identity-drift/identity.tsv"
expect_reject compiler-identity-drift 'report compiler identity is invalid' "$tmp/compiler-identity-drift"

cp -R "$report" "$tmp/missing-peer-launcher-identity"
awk -F '\t' -v OFS='\t' '$2 != "peer_launcher_path" { print }' "$tmp/missing-peer-launcher-identity/identity.tsv" >"$tmp/missing-peer-launcher-identity/new" && mv "$tmp/missing-peer-launcher-identity/new" "$tmp/missing-peer-launcher-identity/identity.tsv"
expect_reject missing-peer-launcher-identity 'report identity is missing: peer_launcher_path' "$tmp/missing-peer-launcher-identity"

cp -R "$report" "$tmp/peer-launcher-contract-drift"
awk -F '\t' -v OFS='\t' '$2 == "peer_launcher_version" { $3 = "wrong-contract" } { print }' "$tmp/peer-launcher-contract-drift/identity.tsv" >"$tmp/peer-launcher-contract-drift/new" && mv "$tmp/peer-launcher-contract-drift/new" "$tmp/peer-launcher-contract-drift/identity.tsv"
expect_reject peer-launcher-contract-drift 'report peer launcher contract is invalid' "$tmp/peer-launcher-contract-drift"

cp -R "$report" "$tmp/peer-launcher-digest-drift"
awk -F '\t' -v OFS='\t' '$2 == "peer_launcher_sha256" { $3 = "0000000000000000000000000000000000000000000000000000000000000000" } { print }' "$tmp/peer-launcher-digest-drift/identity.tsv" >"$tmp/peer-launcher-digest-drift/new" && mv "$tmp/peer-launcher-digest-drift/new" "$tmp/peer-launcher-digest-drift/identity.tsv"
expect_reject peer-launcher-digest-drift 'report peer launcher digest is invalid' "$tmp/peer-launcher-digest-drift"

cp -R "$report" "$tmp/receipt-peer-launcher-drift"
awk -F '\t' -v OFS='\t' 'NR == 2 { $17 = "/usr/bin/other-peer-launcher" } { print }' "$tmp/receipt-peer-launcher-drift/receipts.tsv" >"$tmp/receipt-peer-launcher-drift/new" && mv "$tmp/receipt-peer-launcher-drift/new" "$tmp/receipt-peer-launcher-drift/receipts.tsv"
refresh_review_digest "$tmp/receipt-peer-launcher-drift"
expect_reject receipt-peer-launcher-drift 'receipt peer launcher identity drifted' "$tmp/receipt-peer-launcher-drift"

cp -R "$report" "$tmp/receipt-authority-drift"
awk -F '\t' -v OFS='\t' 'NR == 2 { $20 = "network=forged" } { print }' "$tmp/receipt-authority-drift/receipts.tsv" >"$tmp/receipt-authority-drift/new" && mv "$tmp/receipt-authority-drift/new" "$tmp/receipt-authority-drift/receipts.tsv"
refresh_review_digest "$tmp/receipt-authority-drift"
expect_reject receipt-authority-drift 'receipt authority was not enforced' "$tmp/receipt-authority-drift"

cp -R "$report" "$tmp/report-digest-drift"
awk -F '\t' -v OFS='\t' '$2 == "machine" { $3 = "changed-machine" } { print }' "$tmp/report-digest-drift/identity.tsv" >"$tmp/report-digest-drift/new" && mv "$tmp/report-digest-drift/new" "$tmp/report-digest-drift/identity.tsv"
expect_reject report-digest-drift 'external review report digest disagrees' "$tmp/report-digest-drift"
cp -R "$report" "$tmp/comparison-dependency-drift"
awk -F '\t' -v OFS='\t' 'NR == 2 { $11 = "changed-dependency" } { print }' "$tmp/comparison-dependency-drift/outcomes.tsv" >"$tmp/comparison-dependency-drift/new" && mv "$tmp/comparison-dependency-drift/new" "$tmp/comparison-dependency-drift/outcomes.tsv"
refresh_review_digest "$tmp/comparison-dependency-drift"
expect_reject comparison-dependency-drift 'peer comparison identity drifted' "$tmp/comparison-dependency-drift"
cp -R "$report" "$tmp/comparison-toolchain-drift"
awk -F '\t' -v OFS='\t' 'NR == 2 { $10 = "changed-peer-toolchain" } { print }' "$tmp/comparison-toolchain-drift/outcomes.tsv" >"$tmp/comparison-toolchain-drift/new" && mv "$tmp/comparison-toolchain-drift/new" "$tmp/comparison-toolchain-drift/outcomes.tsv"
refresh_review_digest "$tmp/comparison-toolchain-drift"
expect_reject comparison-toolchain-drift 'peer toolchain identity disagreed with report identity' "$tmp/comparison-toolchain-drift"

cp -R "$report" "$tmp/statistics-drift"
awk -F '\t' -v OFS='\t' 'NR == 2 { $6 = 2 } { print }' "$tmp/statistics-drift/statistics.tsv" >"$tmp/statistics-drift/new" && mv "$tmp/statistics-drift/new" "$tmp/statistics-drift/statistics.tsv"
refresh_review_digest "$tmp/statistics-drift"
expect_reject statistics-drift 'statistics disagree with samples' "$tmp/statistics-drift"

cp -R "$report" "$tmp/receipt-input-drift"
awk -F '\t' -v OFS='\t' 'NR == 2 { $5 = "0000000000000000000000000000000000000000000000000000000000000000" } { print }' "$tmp/receipt-input-drift/receipts.tsv" >"$tmp/receipt-input-drift/new" && mv "$tmp/receipt-input-drift/new" "$tmp/receipt-input-drift/receipts.tsv"
refresh_review_digest "$tmp/receipt-input-drift"
expect_reject receipt-input-drift 'receipt input drifted' "$tmp/receipt-input-drift"

cp -R "$report" "$tmp/missing-samples"
rm "$tmp/missing-samples/samples.tsv"
expect_reject missing-samples 'report must contain the complete release and all-peer coverage tables' "$tmp/missing-samples"

cp -R "$report" "$tmp/missing-statistics"
rm "$tmp/missing-statistics/statistics.tsv"
expect_reject missing-statistics 'report must contain the complete release and all-peer coverage tables' "$tmp/missing-statistics"

cp -R "$report" "$tmp/missing-receipt"
rm "$tmp/missing-receipt/receipts.tsv"
expect_reject missing-receipt 'report must contain the complete release and all-peer coverage tables' "$tmp/missing-receipt"

cp -R "$report" "$tmp/missing-tier-receipt"
rm "$tmp/missing-tier-receipt/tier_receipts.tsv"
expect_reject missing-tier-receipt 'report must contain the complete release and all-peer coverage tables' "$tmp/missing-tier-receipt"

cp -R "$report" "$tmp/missing-outcome"
drop_last_data_row "$tmp/missing-outcome/outcomes.tsv"
refresh_review_digest "$tmp/missing-outcome"
expect_reject missing-outcome 'outcome report does not cover frozen manifest' "$tmp/missing-outcome"

cp -R "$report" "$tmp/unrelated-live-owner"
awk -F '\t' -v OFS='\t' 'NR == 2 { $13 = "loss"; $15 = "#1414" } { print }' "$tmp/unrelated-live-owner/outcomes.tsv" >"$tmp/unrelated-live-owner/new" && mv "$tmp/unrelated-live-owner/new" "$tmp/unrelated-live-owner/outcomes.tsv"
refresh_review_digest "$tmp/unrelated-live-owner"
expect_reject unrelated-live-owner 'loss owner does not match manifest loss_cards' "$tmp/unrelated-live-owner"

cp -R "$report" "$tmp/missing-metric"
drop_last_data_row "$tmp/missing-metric/measurements.tsv"
refresh_review_digest "$tmp/missing-metric"
expect_reject missing-metric 'missing measurement' "$tmp/missing-metric"

cp -R "$report" "$tmp/missing-tier"
drop_last_data_row "$tmp/missing-tier/tiers.tsv"
refresh_review_digest "$tmp/missing-tier"
expect_reject missing-tier 'missing tier proof' "$tmp/missing-tier"

cp -R "$report" "$tmp/changed-input"
awk -F '\t' -v OFS='\t' 'NR == 2 { $5 = "changed" } { print }' "$tmp/changed-input/outcomes.tsv" >"$tmp/changed-input/new" && mv "$tmp/changed-input/new" "$tmp/changed-input/outcomes.tsv"
refresh_review_digest "$tmp/changed-input"
expect_reject changed-input 'input or outcome drifted' "$tmp/changed-input"

cp -R "$report" "$tmp/embedded-review-claim"
awk -F '\t' -v OFS='\t' 'NR == 2 { $16 = "pass"; $17 = "self-claimed" } { print }' "$tmp/embedded-review-claim/outcomes.tsv" >"$tmp/embedded-review-claim/new" && mv "$tmp/embedded-review-claim/new" "$tmp/embedded-review-claim/outcomes.tsv"
refresh_review_digest "$tmp/embedded-review-claim"
expect_reject embedded-review-claim 'producer report already carries review claims' "$tmp/embedded-review-claim"

cp -R "$report" "$tmp/stale-candidate"
awk -F '\t' -v OFS='\t' 'NR == 2 { sub(/candidate=[0-9a-f]+/, "candidate=0000000000000000000000000000000000000000", $8) } { print }' "$tmp/stale-candidate/outcomes.tsv" >"$tmp/stale-candidate/new" && mv "$tmp/stale-candidate/new" "$tmp/stale-candidate/outcomes.tsv"
refresh_review_digest "$tmp/stale-candidate"
expect_reject stale-candidate 'comparison candidate is stale' "$tmp/stale-candidate"

cp -R "$report" "$tmp/unowned-metric-loss"
awk -F '\t' -v OFS='\t' 'NR == 2 { $5 = 2 } { print }' "$tmp/unowned-metric-loss/measurements.tsv" >"$tmp/unowned-metric-loss/new" && mv "$tmp/unowned-metric-loss/new" "$tmp/unowned-metric-loss/measurements.tsv"
refresh_review_digest "$tmp/unowned-metric-loss"
expect_reject unowned-metric-loss 'Jet metric loss is not recorded' "$tmp/unowned-metric-loss"

cp -R "$report" "$tmp/required-tier-loss"
loss_task="$(awk -F '\t' 'NR > 1 && $3 == "jet" && $7 == "pass" { print $2; exit }' "$tmp/required-tier-loss/tiers.tsv")"
awk -F '\t' -v OFS='\t' -v task="$loss_task" -v owner="$manifest_loss_owner" 'NR > 1 && $2 == task && $3 == "jet" && !changed { $7 = "loss"; $9 = owner; changed = 1 } { print }' "$tmp/required-tier-loss/tiers.tsv" >"$tmp/required-tier-loss/new" && mv "$tmp/required-tier-loss/new" "$tmp/required-tier-loss/tiers.tsv"
awk -F '\t' -v OFS='\t' -v task="$loss_task" 'NR > 1 && $2 == task && $3 == "jet" && !changed { $10 = "loss"; changed = 1 } { print }' "$tmp/required-tier-loss/tier_receipts.tsv" >"$tmp/required-tier-loss/new" && mv "$tmp/required-tier-loss/new" "$tmp/required-tier-loss/tier_receipts.tsv"
awk -F '\t' -v OFS='\t' -v task="$loss_task" -v owner="$manifest_loss_owner" 'NR > 1 && $2 == task { $13 = "loss"; $15 = owner } { print }' "$tmp/required-tier-loss/outcomes.tsv" >"$tmp/required-tier-loss/new" && mv "$tmp/required-tier-loss/new" "$tmp/required-tier-loss/outcomes.tsv"
required_tier_digest="$(hash_report_files "$tmp/required-tier-loss")"
awk -F '\t' -v OFS='\t' -v digest="$required_tier_digest" 'NR == 2 { $7 = digest } { print }' "$tmp/required-tier-loss/review.tsv" >"$tmp/required-tier-loss/new" && mv "$tmp/required-tier-loss/new" "$tmp/required-tier-loss/review.tsv"
expect_reject required-tier-loss 'compiled workload superiority failed' "$tmp/required-tier-loss"

cp -R "$report" "$tmp/toolchain-drift"
awk -F '\t' -v OFS='\t' 'NR == 2 { $7 = "drifted-toolchain" } { print }' "$tmp/toolchain-drift/measurements.tsv" >"$tmp/toolchain-drift/new" && mv "$tmp/toolchain-drift/new" "$tmp/toolchain-drift/measurements.tsv"
refresh_review_digest "$tmp/toolchain-drift"
expect_reject toolchain-drift 'measurement toolchain identity disagreed with report identity' "$tmp/toolchain-drift"

cp -R "$report" "$tmp/missing-peer-coverage"
drop_last_data_row "$tmp/missing-peer-coverage/peer_measurements.tsv"
refresh_review_digest "$tmp/missing-peer-coverage"
expect_reject missing-peer-coverage 'missing peer measurement' "$tmp/missing-peer-coverage"
cp -R "$report" "$tmp/candidate-peer-loss"
candidate_peer_key="$(awk -F '\t' 'NR > 1 && $3 == "candidate" && $4 == "cxx" { print $2 "|" $3 "|" $4 "|" $5; exit }' "$CORPUS/peer_ledger.tsv")"
[[ -n "$candidate_peer_key" ]] || { echo "missing cxx candidate fixture" >&2; exit 1; }
awk -F '\t' -v OFS='\t' -v key="$candidate_peer_key" 'NR == 1 { print; next } $2 == key && $6 == "runtime" { $8 = 1; $9 = 1; $10 = 1 } { print }' "$tmp/candidate-peer-loss/peer_measurements.tsv" >"$tmp/candidate-peer-loss/new" && mv "$tmp/candidate-peer-loss/new" "$tmp/candidate-peer-loss/peer_measurements.tsv"
refresh_review_digest "$tmp/candidate-peer-loss"
expect_reject candidate-peer-loss 'peer comparison loss' "$tmp/candidate-peer-loss"
echo 'test result: compiled workload gate self-check: pass'
