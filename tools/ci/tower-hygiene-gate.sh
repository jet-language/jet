#!/usr/bin/env bash
# #805 / D-ONCE-LEDGER1=A: read-only Tower hygiene gate.
#
# The Tower store owns decisions; docs/spec is the rendered surface. Keep the
# scope as one configured input so the ledger home can change without another
# directory list in CI.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
TOWER="$ROOT/plugins/tower/tower.mjs"
scope_input="${JET_TOWER_LINT_SCOPE:-docs}"
candidate="unknown"
runner_os="${RUNNER_OS:-}"
runner_arch="${RUNNER_ARCH:-}"
if [ -z "$runner_os" ]; then
  runner_os="$(uname -s 2>/dev/null || printf 'unknown')"
fi
if [ -z "$runner_arch" ]; then
  runner_arch="$(uname -m 2>/dev/null || printf 'unknown')"
fi
result="pass"
lint_status="not-run"
lint_repeat_status="not-run"
read_only="not-checked"
errors=()

if candidate="$(git -C "$ROOT" rev-parse --verify HEAD 2>/dev/null)"; then
  :
else
  candidate="unknown"
  errors+=("candidate commit is unavailable")
fi

if [[ "$scope_input" = /* ]]; then
  docs_root="$scope_input"
else
  docs_root="$ROOT/$scope_input"
fi

data_input="${TOWER_DATA:-$ROOT/plugins/tower/.tower}"
if [[ "$data_input" = /* ]]; then
  data_path="$data_input"
else
  data_path="$ROOT/$data_input"
fi
if [[ "$data_path" = *.json ]]; then
  tower_file="$data_path"
  tower_dir="$(dirname "$data_path")"
else
  tower_dir="$data_path"
  tower_file="$tower_dir/tower.json"
fi
history_file="$tower_dir/history.json"

report_path="${JET_TOWER_HYGIENE_REPORT:-}"
if [ -z "$report_path" ]; then
  shard="${JET_TEST_SHARD:-local}"
  report_path="$ROOT/.tmp/tower-hygiene/$candidate-$shard.txt"
elif [[ "$report_path" != /* ]]; then
  report_path="$ROOT/$report_path"
fi

case "$report_path" in
  "$ROOT/plugins/tower"|"$ROOT/plugins/tower"/*)
    errors+=("audit report may not be written under plugins/tower")
    report_path="$ROOT/.tmp/tower-hygiene/unsafe-report.txt"
    ;;
esac

if ! node_path="$(command -v node 2>/dev/null)"; then
  errors+=("node runner is unavailable")
  node_version="unavailable"
else
  node_version="$("$node_path" --version 2>/dev/null || printf 'unavailable')"
  if [ "$node_version" = "unavailable" ]; then
    errors+=("node runner did not report a version")
  fi
fi

if [ ! -f "$TOWER" ]; then
  errors+=("Tower entrypoint is missing: $TOWER")
fi
if [ ! -d "$docs_root" ]; then
  errors+=("configured Tower lint scope is missing: $docs_root")
fi
spec_root="$docs_root/spec"
if [ ! -d "$spec_root" ]; then
  errors+=("configured Tower lint scope has no docs/spec directory: $spec_root")
fi
if [ ! -f "$tower_file" ]; then
  errors+=("Tower store is missing: $tower_file")
fi

if [ -d "$docs_root" ]; then
  docs_root="$(cd "$docs_root" && pwd -P)"
  spec_root="$docs_root/spec"
fi
report_parent="$(dirname "$report_path")"
mkdir -p -- "$report_parent"

lint_output="$(mktemp "$report_parent/.tower-lint-output.XXXXXX")"
lint_error="$(mktemp "$report_parent/.tower-lint-error.XXXXXX")"
lint_repeat_output="$(mktemp "$report_parent/.tower-lint-repeat-output.XXXXXX")"
lint_repeat_error="$(mktemp "$report_parent/.tower-lint-repeat-error.XXXXXX")"
tmp_report="$(mktemp "$report_path.tmp.XXXXXX")"
cleanup() {
  rm -f -- "$lint_output" "$lint_error" "$lint_repeat_output" "$lint_repeat_error"
  if [ -n "$tmp_report" ]; then
    rm -f -- "$tmp_report"
  fi
}
trap cleanup EXIT

hash_file() {
  local path="$1"
  if [ ! -e "$path" ]; then
    printf 'missing'
    return 0
  fi
  if [ ! -f "$path" ]; then
    printf 'not-regular'
    return 0
  fi
  "$node_path" -e 'const {createHash}=require("node:crypto");const {readFileSync}=require("node:fs");process.stdout.write(createHash("sha256").update(readFileSync(process.argv[1])).digest("hex"));' "$path"
}

tower_before="unavailable"
history_before="unavailable"
if [ -n "${node_path:-}" ]; then
  tower_before="$(hash_file "$tower_file")"
  history_before="$(hash_file "$history_file")"
fi

if [ -n "${node_path:-}" ] && [ -f "$TOWER" ] && [ -d "$spec_root" ]; then
  set +e
  (
    cd "$ROOT"
    "$node_path" "$TOWER" lint --docs --docs-root "$docs_root" --json
  ) >"$lint_output" 2>"$lint_error"
  lint_status=$?
  (
    cd "$ROOT"
    "$node_path" "$TOWER" lint --docs --docs-root "$docs_root" --json
  ) >"$lint_repeat_output" 2>"$lint_repeat_error"
  lint_repeat_status=$?
  set -e
else
  errors+=("Tower lint was not run because a required runner, entrypoint, or scope is missing")
fi

tower_after="unavailable"
history_after="unavailable"
if [ -n "${node_path:-}" ]; then
  tower_after="$(hash_file "$tower_file")"
  history_after="$(hash_file "$history_file")"
  if [ "$tower_before" != "$tower_after" ] || [ "$history_before" != "$history_after" ]; then
    read_only="fail"
    errors+=("Tower store changed during read-only lint")
  else
    read_only="pass"
  fi
fi

if [ "$lint_status" != "0" ]; then
  result="fail"
  if [ "$lint_status" != "not-run" ]; then
    errors+=("tower lint exited $lint_status")
  fi
fi
if [ "$lint_repeat_status" != "0" ]; then
  result="fail"
  if [ "$lint_repeat_status" != "not-run" ]; then
    errors+=("repeat tower lint exited $lint_repeat_status")
  fi
fi
if [ "$lint_status" = "0" ] && [ "$lint_repeat_status" = "0" ]; then
  if ! cmp -s "$lint_output" "$lint_repeat_output" || ! cmp -s "$lint_error" "$lint_repeat_error"; then
    result="fail"
    errors+=("tower lint output changed between identical runs")
  fi
fi
if ((${#errors[@]})); then
  result="fail"
fi

{
  printf '%s\n' 'schema=jet.tower-hygiene.v1'
  printf 'status=%s\n' "$result"
  printf 'candidate_commit=%s\n' "$candidate"
  printf 'runner_os=%s\n' "$runner_os"
  printf 'runner_arch=%s\n' "$runner_arch"
  printf 'node=%s\n' "$node_version"
  printf 'scope_input=%s\n' "$scope_input"
  printf 'docs_root=%s\n' "$docs_root"
  printf 'spec_root=%s\n' "$spec_root"
  printf 'tower_store=%s\n' "$tower_file"
  printf 'tower_store_before_sha256=%s\n' "$tower_before"
  printf 'tower_store_after_sha256=%s\n' "$tower_after"
  printf 'history_before_sha256=%s\n' "$history_before"
  printf 'history_after_sha256=%s\n' "$history_after"
  printf 'read_only=%s\n' "$read_only"
  printf 'lint_exit=%s\n' "$lint_status"
  printf 'lint_repeat_exit=%s\n' "$lint_repeat_status"
  printf 'errors=%s\n' "${#errors[@]}"
  for error in "${errors[@]}"; do
    printf 'error=%s\n' "$error"
  done
  printf '%s\n' 'lint_stdout_begin'
  cat "$lint_output"
  printf '%s\n' 'lint_stdout_end'
  printf '%s\n' 'lint_stderr_begin'
  cat "$lint_error"
  printf '%s\n' 'lint_stderr_end'
  printf '%s\n' 'lint_repeat_stdout_begin'
  cat "$lint_repeat_output"
  printf '%s\n' 'lint_repeat_stdout_end'
  printf '%s\n' 'lint_repeat_stderr_begin'
  cat "$lint_repeat_error"
  printf '%s\n' 'lint_repeat_stderr_end'
} >"$tmp_report"
mv -f -- "$tmp_report" "$report_path"
tmp_report=""

cat "$report_path"
if [ "$result" != "pass" ]; then
  exit 1
fi
