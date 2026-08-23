#!/usr/bin/env bash
# Check one agent-workload report for every required native OS.
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: bash tools/ci/agent-workload-matrix.sh REPORT_DIR" >&2
  exit 64
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
report_dir="$1"
matrix="$ROOT/tests/agent_workloads/native_os_matrix.tsv"
status=0
required=0

while IFS=$'\t' read -r version os arch_policy requirement adapters reason; do
  [ "$version" = "1" ] || continue
  [ "$requirement" = "required" ] || continue
  required=$((required + 1))
  report="$report_dir/$os.tsv"
  if [ ! -f "$report" ]; then
    echo "agent workload matrix: missing required report $report" >&2
    status=1
    continue
  fi
  report_os="$(awk -F '\t' '$1 !~ /^#/ && $1 != "version" { print $2; exit }' "$report")"
  if [ "$report_os" != "$os" ]; then
    echo "agent workload matrix: $report declares OS '$report_os', expected '$os'" >&2
    status=1
    continue
  fi
  if ! bash tools/ci/agent-workload-gate.sh --check "$report"; then
    status=1
  fi
done < "$matrix"

if [ "$required" -eq 0 ]; then
  echo "agent workload matrix: frozen matrix has no required OS" >&2
  exit 1
fi
if [ "$status" -ne 0 ]; then
  exit 1
fi
echo "agent workload matrix: pass required_os=$required report_dir=$report_dir"
