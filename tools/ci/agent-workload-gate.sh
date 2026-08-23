#!/usr/bin/env bash
# Run the agent workload corpus, write its per-task report, and reject Jet regressions.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

MATRIX="$ROOT/tests/agent_workloads/native_os_matrix.tsv"
MANIFEST="$ROOT/tests/agent_workloads/manifest.tsv"
POLICY_CONTRACT="$ROOT/tests/agent_workloads/policy.tsv"
BASELINE="${JET_AGENT_WORKLOAD_BASELINE:-$ROOT/tests/agent_workloads/jet_baseline.tsv}"
REPORT="${JET_AGENT_WORKLOAD_REPORT:-$ROOT/docs/audits/agent-workload-corpus-report.tsv}"
SCRATCH_ROOT="${JET_AGENT_WORKLOAD_SCRATCH_DIR:-${TMPDIR:-$HOME/.cache/jet-test-scratch}}"

usage() {
  echo "usage: bash tools/ci/agent-workload-gate.sh [--check REPORT]" >&2
  exit 64
}

hash_stdin() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 | awk '{print $1}'
  else
    echo "agent workload gate: no SHA-256 command available" >&2
    return 1
  fi
}

validate_report() {
  local report="$1"
  if [ ! -f "$report" ]; then
    echo "agent workload gate: missing report $report" >&2
    return 1
  fi
  if [ ! -f "$POLICY_CONTRACT" ]; then
    echo "agent workload gate: missing policy contract $POLICY_CONTRACT" >&2
    return 1
  fi
  local policy_digest
  policy_digest="$(LC_ALL=C awk 'NR > 1 { printf "\n" } { printf "%s", $0 }' "$POLICY_CONTRACT" | hash_stdin)"
  awk -F '\t' -v baseline="$BASELINE" -v matrix="$MATRIX" -v manifest="$MANIFEST" -v expected_policy_digest="$policy_digest" '
    function fail(message) {
      print "agent workload gate: " message > "/dev/stderr"
      bad = 1
    }
    function owner_ok(value, parts, i, pair, target) {
      if (value == "") return 0
      count = split(value, parts, ";")
      for (i = 1; i <= count; i++) {
        if (split(parts[i], pair, "=") != 2) return 0
        target = pair[2]
        if (target !~ /^#[0-9]+$/ && target !~ /^non-goal:[^;]+$/) return 0
      }
      return 1
    }
    function nonnegative_integer(value) {
      return value ~ /^[0-9]+$/
    }
    BEGIN {
      baseline_header = "version\ttask_id\tjet_status\tloss_owner"
      matrix_header = "version\tos\tarch_policy\trequirement\tadapters\treason"
      manifest_header = "version\ttask_id\tdomain\tcase\tdeclared_outcome\tinput\texpected\tauthority\tadapters\tplatforms\tevidence\ttower_card\tloss_cards"
      if ((getline line < baseline) <= 0 || line != baseline_header) fail("baseline schema drifted")
      while ((getline line < baseline) > 0) {
        if (line == "") continue
        count = split(line, fields, "\t")
        if (count != 4 || fields[1] != "1" || fields[3] != "pass") {
          fail("bad Jet baseline row: " line)
          continue
        }
        if (baseline_seen[fields[2]]++) fail("duplicate Jet baseline task: " fields[2])
        if (!owner_ok(fields[4])) fail("Jet baseline loss has no card or ratified non-goal: " fields[2])
        baseline_task[++baseline_count] = fields[2]
        baseline_status[fields[2]] = fields[3]
        baseline_owner[fields[2]] = fields[4]
      }
      close(baseline)
      if ((getline line < matrix) <= 0 || line != matrix_header) fail("native OS matrix schema drifted")
      while ((getline line < matrix) > 0) {
        if (line == "") continue
        count = split(line, fields, "\t")
        if (count != 6 || fields[1] != "1") {
          fail("bad native OS matrix row: " line)
          continue
        }
        if (fields[2] == "" || fields[3] == "" || fields[4] !~ /^(required|excluded)$/ || fields[5] !~ /^(jet|bash|python|node)(,(jet|bash|python|node))*$/ || fields[6] == "") {
          fail("bad native OS matrix metadata: " line)
        }
        if (matrix_seen[fields[2]]++) fail("duplicate native OS matrix row: " fields[2])
        matrix_requirement[fields[2]] = fields[4]
        matrix_arch[fields[2]] = fields[3]
        matrix_adapters[fields[2]] = fields[5]
        matrix_count++
      }
      close(matrix)
      if ((getline line < manifest) <= 0 || line != manifest_header) fail("corpus manifest schema drifted")
      while ((getline line < manifest) > 0) {
        if (line == "") continue
        count = split(line, fields, "\t")
        if (count != 13 || fields[1] != "1") {
          fail("bad corpus manifest row: " line)
          continue
        }
        if (manifest_seen[fields[2]]++) fail("duplicate corpus manifest task: " fields[2])
        manifest_domain[fields[2]] = fields[3]
        manifest_case[fields[2]] = fields[4]
        manifest_owner[fields[2]] = fields[13]
        if (!owner_ok(fields[13])) fail("bad manifest loss owner: " fields[2])
        manifest_count++
      }
      close(manifest)
      if (baseline_count == 0) fail("Jet baseline is empty or missing")
      if (matrix_count == 0) fail("native OS matrix is empty or missing")
      if (manifest_count == 0) fail("corpus manifest is empty or missing")
    }
    /^#/ || /^[[:space:]]*$/ { next }
    !header_seen {
      if ($0 != "version\tos\tarch\ttask\tdomain\tcase\tjet\tbash\tpython\tnode\tjet_vs_baselines\tloss_owner\trun_exit\tjet_source_tokens\tbash_source_tokens\tpython_source_tokens\tnode_source_tokens\tjet_cold_ns\tbash_cold_ns\tpython_cold_ns\tnode_cold_ns\tjet_warm_ns\tbash_warm_ns\tpython_warm_ns\tnode_warm_ns\tpolicy_digest") {
        fail("report schema drifted")
      }
      header_seen = 1
      next
    }
    {
      if (NF != 26 || $1 != "1") {
        fail("bad report row: " $0)
        next
      }
      if (host_os == "") {
        host_os = $2
        host_arch = $3
      }
      if ($2 != host_os || $3 != host_arch) fail("report mixes OS or architecture")
      task = $4
      if (seen[task]++) fail("duplicate report task: " task)
      if (!(task in manifest_domain)) {
        fail("report has unknown task: " task)
      } else {
        if ($5 != manifest_domain[task] || $6 != manifest_case[task]) fail("task metadata drifted: " task)
        if ($12 != manifest_owner[task]) fail("loss owner drifted from manifest: " task)
      }
      if ($7 !~ /^(pass|loss|missing)$/ || $8 !~ /^(pass|loss|missing)$/ || $9 !~ /^(pass|loss|missing)$/ || $10 !~ /^(pass|loss|missing)$/) {
        fail("bad adapter status: " task)
      }
      if ($11 !~ /^(pass|loss)$/) fail("bad corpus score: " task)
      if (!(task in current)) current_count++
      current[task] = $7
      current_owner[task] = $12
      run_exit = $13
      if ($13 != "0") fail("corpus command exited " $13 ": " task)
      expected_score = $13 == "0" && $7 == "pass" && $8 == "pass" && $9 == "pass" && $10 == "pass" ? "pass" : "loss"
      if ($11 != expected_score) fail("corpus score drifted: " task)
      if ($7 == "loss" && !owner_ok($12)) fail("Jet loss has no card or ratified non-goal: " task)
      if (!nonnegative_integer($13)) fail("bad corpus exit code: " task)
      for (metric = 14; metric <= 25; metric++) {
        if (!nonnegative_integer($metric)) fail("bad report metric in " task ": column=" metric)
      }
      if ($26 !~ /^[0-9a-f]{64}$/) fail("bad policy digest: " task)
      else if ($26 != expected_policy_digest) fail("policy digest is not the frozen policy: " task)
      if (report_policy_digest == "") report_policy_digest = $26
      else if ($26 != report_policy_digest) fail("policy digest drifted: " task)
    }
    END {
      if (!header_seen) {
        fail("report has no header")
      } else if (host_os == "") {
        fail("report has no task rows")
      } else if (!(host_os in matrix_requirement)) {
        fail("OS is absent from frozen matrix: " host_os)
      } else if (matrix_requirement[host_os] != "required") {
        fail("OS is excluded by frozen matrix: " host_os)
      } else {
        if (matrix_requirement[host_os] == "required" && matrix_arch[host_os] != "any" && matrix_arch[host_os] != host_arch) {
          fail("architecture " host_arch " is not allowed for " host_os)
        }
      }
      for (i = 1; i <= baseline_count; i++) {
        task = baseline_task[i]
        if (!(task in manifest_domain)) {
          fail("Jet baseline names unknown task: " task)
        } else if (baseline_owner[task] != manifest_owner[task]) {
          fail("Jet baseline loss owner drifted from manifest: " task)
        }
        if (!(task in current)) {
          fail("report is missing baseline task: " task)
        } else if (baseline_status[task] == "pass" && current[task] != "pass") {
          fail("Jet regression: task=" task " baseline=pass current=" current[task] " owner=" baseline_owner[task])
        }
        if (baseline_owner[task] != current_owner[task]) fail("loss owner drifted: " task)
      }
      for (task in current) if (!(task in manifest_domain)) fail("report has unknown task: " task)
      if (baseline_count != manifest_count) fail("Jet baseline task count drifted")
      if (current_count != manifest_count) fail("report task count drifted")
      exit bad
    }
  ' "$report"
}

generate_report() {
  local raw="$1"
  local output="$2"
  local run_exit="$3"
  awk -F '\t' -v manifest="$MANIFEST" -v output="$output" -v run_exit="$run_exit" '
    function value(text, key, parts, i, pair) {
      count = split(text, parts, "\t")
      for (i = 1; i <= count; i++) {
        split(parts[i], pair, "=")
        if (pair[1] == key) return substr(parts[i], length(key) + 2)
      }
      return "not-recorded"
    }
    BEGIN {
      while ((getline line < manifest) > 0) {
        if (line ~ /^version\t/) continue
        count = split(line, fields, "\t")
        if (count != 13) continue
        task[++task_count] = fields[2]
        domain[fields[2]] = fields[3]
        case_name[fields[2]] = fields[4]
        owner[fields[2]] = fields[13]
      }
      close(manifest)
      print "# generated by tools/ci/agent-workload-gate.sh" > output
      print "version\tos\tarch\ttask\tdomain\tcase\tjet\tbash\tpython\tnode\tjet_vs_baselines\tloss_owner\trun_exit\tjet_source_tokens\tbash_source_tokens\tpython_source_tokens\tnode_source_tokens\tjet_cold_ns\tbash_cold_ns\tpython_cold_ns\tnode_cold_ns\tjet_warm_ns\tbash_warm_ns\tpython_warm_ns\tnode_warm_ns\tpolicy_digest" >> output
    }
    $1 == "machine" {
      host_os = value($0, "os")
      host_arch = value($0, "arch")
      policy_digest = value($0, "policy_digest")
      next
    }
    $1 == "result" {
      task_name = value($0, "task")
      adapter = value($0, "adapter")
      status[task_name SUBSEP adapter] = value($0, "success") == "true" ? "pass" : "loss"
      source_tokens[task_name SUBSEP adapter] = value($0, "source_tokens")
      cold[task_name SUBSEP adapter] = value($0, "cold_ns")
      warm[task_name SUBSEP adapter] = value($0, "warm_ns")
      next
    }
    END {
      for (i = 1; i <= task_count; i++) {
        task_name = task[i]
        jet = status[task_name SUBSEP "jet"]
        bash = status[task_name SUBSEP "bash"]
        python = status[task_name SUBSEP "python"]
        node = status[task_name SUBSEP "node"]
        if (jet == "") jet = "missing"
        if (bash == "") bash = "missing"
        if (python == "") python = "missing"
        if (node == "") node = "missing"
        score = run_exit == 0 && jet == "pass" && bash == "pass" && python == "pass" && node == "pass" ? "pass" : "loss"
        printf "1\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n", \
          host_os, host_arch, task_name, domain[task_name], case_name[task_name], jet, bash, python, node, score, owner[task_name], run_exit, \
          source_tokens[task_name SUBSEP "jet"], source_tokens[task_name SUBSEP "bash"], source_tokens[task_name SUBSEP "python"], source_tokens[task_name SUBSEP "node"], \
          cold[task_name SUBSEP "jet"], cold[task_name SUBSEP "bash"], cold[task_name SUBSEP "python"], cold[task_name SUBSEP "node"], \
          warm[task_name SUBSEP "jet"], warm[task_name SUBSEP "bash"], warm[task_name SUBSEP "python"], warm[task_name SUBSEP "node"], policy_digest >> output
      }
    }
  ' "$raw"
}

if [ "${1:-}" = "--check" ]; then
  [ "$#" -eq 2 ] || usage
  validate_report "$2"
  echo "agent workload gate: pass report=$2"
  exit 0
fi
[ "$#" -eq 0 ] || usage

mkdir -p "$SCRATCH_ROOT" "$(dirname "$REPORT")"
run_dir="$(mktemp -d "$SCRATCH_ROOT/agent-workload-gate.XXXXXX")"
trap 'rm -rf "$run_dir"' EXIT
raw="$run_dir/raw.log"
generated="$run_dir/report.tsv"

set +e
JET_NIX_TMP_CLEANED="${JET_NIX_TMP_CLEANED:-1}" TMPDIR="$SCRATCH_ROOT" scripts/agent/jet-env cargo test --test agent_workloads equivalent_adapters_complete_declared_tasks -- --exact --nocapture 2>&1 | tee "$raw"
run_exit="${PIPESTATUS[0]}"
set -e

generate_report "$raw" "$generated" "$run_exit"
if ! validate_report "$generated"; then
  echo "agent workload gate: generated report rejected; existing report preserved: $REPORT" >&2
  exit 1
fi
mv -f "$generated" "$REPORT"

if [ "$run_exit" -ne 0 ]; then
  exit 1
fi
echo "agent workload gate: pass report=$REPORT"
