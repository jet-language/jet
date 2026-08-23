#!/usr/bin/env bash
# Run the agent workload corpus, write its per-task report, and reject Jet regressions.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

MATRIX="$ROOT/tests/agent_workloads/native_os_matrix.tsv"
MANIFEST="$ROOT/tests/agent_workloads/manifest.tsv"
BASELINE="${JET_AGENT_WORKLOAD_BASELINE:-$ROOT/tests/agent_workloads/jet_baseline.tsv}"
REPORT="${JET_AGENT_WORKLOAD_REPORT:-$ROOT/docs/audits/agent-workload-corpus-report.tsv}"
SCRATCH_ROOT="${JET_AGENT_WORKLOAD_SCRATCH_DIR:-${TMPDIR:-$HOME/.cache/jet-test-scratch}}"

usage() {
  echo "usage: bash tools/ci/agent-workload-gate.sh [--check REPORT]" >&2
  exit 64
}

validate_report() {
  local report="$1"
  if [ ! -f "$report" ]; then
    echo "agent workload gate: missing report $report" >&2
    return 1
  fi
  awk -F '\t' -v baseline="$BASELINE" -v matrix="$MATRIX" -v manifest="$MANIFEST" '
    function fail(message) {
      print "agent workload gate: " message > "/dev/stderr"
      bad = 1
    }
    function owner_ok(value, parts, i, pair) {
      if (value == "") return 0
      count = split(value, parts, ";")
      for (i = 1; i <= count; i++) {
        if (split(parts[i], pair, "=") != 2) return 0
        if (pair[2] !~ /^#/ && pair[2] !~ /^non-goal:/) return 0
      }
      return 1
    }
    BEGIN {
      while ((getline line < baseline) > 0) {
        if (line ~ /^version\t/) continue
        count = split(line, fields, "\t")
        if (count != 4 || fields[1] != "1" || fields[3] != "pass") {
          fail("bad Jet baseline row: " line)
          continue
        }
        baseline_task[++baseline_count] = fields[2]
        baseline_status[fields[2]] = fields[3]
        baseline_owner[fields[2]] = fields[4]
      }
      close(baseline)
      while ((getline line < matrix) > 0) {
        if (line ~ /^version\t/) continue
        count = split(line, fields, "\t")
        if (count != 6 || fields[1] != "1") {
          fail("bad native OS matrix row: " line)
          continue
        }
        matrix_requirement[fields[2]] = fields[4]
        matrix_arch[fields[2]] = fields[3]
        matrix_adapters[fields[2]] = fields[5]
        matrix_count++
      }
      close(matrix)
      while ((getline line < manifest) > 0) {
        if (line ~ /^version\t/) continue
        count = split(line, fields, "\t")
        if (count != 13 || fields[1] != "1") {
          fail("bad corpus manifest row: " line)
          continue
        }
        manifest_domain[fields[2]] = fields[3]
        manifest_case[fields[2]] = fields[4]
        manifest_count++
      }
      close(manifest)
      if (baseline_count == 0) fail("Jet baseline is empty or missing")
      if (matrix_count == 0) fail("native OS matrix is empty or missing")
      if (manifest_count == 0) fail("corpus manifest is empty or missing")
    }
    /^#/ || /^[[:space:]]*$/ { next }
    !header_seen {
      if ($0 != "version\tos\tarch\ttask\tdomain\tcase\tjet\tbash\tpython\tnode\tjet_vs_baselines\tloss_owner\trun_exit\tjet_source_tokens\tbash_source_tokens\tpython_source_tokens\tnode_source_tokens\tjet_cold_ns\tbash_cold_ns\tpython_cold_ns\tnode_cold_ns\tjet_warm_ns\tbash_warm_ns\tpython_warm_ns\tnode_warm_ns") {
        fail("report schema drifted")
      }
      header_seen = 1
      next
    }
    {
      if (NF != 25) {
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
      }
      if (!(task in current)) current_count++
      current[task] = $7
      current_owner[task] = $12
      run_exit = $13
      if ($11 != "pass") fail("task did not pass all adapters: " task)
      if ($7 == "loss" && !owner_ok($12)) fail("Jet loss has no card or ratified non-goal: " task)
      if ($13 != "0") fail("corpus command exited " $13)
    }
    END {
      if (host_os == "") {
        fail("report has no task rows")
      } else if (!(host_os in matrix_requirement)) {
        fail("OS is absent from frozen matrix: " host_os)
      } else {
        if (matrix_requirement[host_os] == "required" && matrix_arch[host_os] != "any" && matrix_arch[host_os] != host_arch) {
          fail("architecture " host_arch " is not allowed for " host_os)
        }
      }
      for (i = 1; i <= baseline_count; i++) {
        task = baseline_task[i]
        if (!(task in current)) {
          fail("report is missing baseline task: " task)
        } else if (baseline_status[task] == "pass" && current[task] != "pass") {
          fail("Jet regression: task=" task " baseline=pass current=" current[task] " owner=" baseline_owner[task])
        }
        if (baseline_owner[task] != current_owner[task]) fail("loss owner drifted: " task)
      }
      for (task in current) if (!(task in manifest_domain)) fail("report has unknown task: " task)
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
      print "version\tos\tarch\ttask\tdomain\tcase\tjet\tbash\tpython\tnode\tjet_vs_baselines\tloss_owner\trun_exit\tjet_source_tokens\tbash_source_tokens\tpython_source_tokens\tnode_source_tokens\tjet_cold_ns\tbash_cold_ns\tpython_cold_ns\tnode_cold_ns\tjet_warm_ns\tbash_warm_ns\tpython_warm_ns\tnode_warm_ns" >> output
    }
    $1 == "machine" {
      host_os = value($0, "os")
      host_arch = value($0, "arch")
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
        printf "1\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n", \
          host_os, host_arch, task_name, domain[task_name], case_name[task_name], jet, bash, python, node, score, owner[task_name], run_exit, \
          source_tokens[task_name SUBSEP "jet"], source_tokens[task_name SUBSEP "bash"], source_tokens[task_name SUBSEP "python"], source_tokens[task_name SUBSEP "node"], \
          cold[task_name SUBSEP "jet"], cold[task_name SUBSEP "bash"], cold[task_name SUBSEP "python"], cold[task_name SUBSEP "node"], \
          warm[task_name SUBSEP "jet"], warm[task_name SUBSEP "bash"], warm[task_name SUBSEP "python"], warm[task_name SUBSEP "node"] >> output
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
mv -f "$generated" "$REPORT"
validate_report "$REPORT"

if [ "$run_exit" -ne 0 ]; then
  exit 1
fi
echo "agent workload gate: pass report=$REPORT"
