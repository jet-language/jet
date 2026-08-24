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
if [ -d "$tower_dir" ]; then
  tower_dir="$(cd "$tower_dir" && pwd -P)"
  tower_file="$tower_dir/$(basename "$tower_file")"
fi

report_path="${JET_TOWER_HYGIENE_REPORT:-}"
if [ -z "$report_path" ]; then
  shard="${JET_TEST_SHARD:-local}"
  report_path="$ROOT/.tmp/tower-hygiene/$candidate-$shard.txt"
elif [[ "$report_path" != /* ]]; then
  report_path="$ROOT/$report_path"
fi

case "$report_path" in
  "$ROOT/plugins/tower"|"$ROOT/plugins/tower"/*|"$tower_dir"|"$tower_dir"/*)
    errors+=("audit report may not be written under Tower data")
    report_path="$ROOT/.tmp/tower-hygiene/unsafe-report.txt"
    ;;
esac

if ! node_path="$(command -v node 2>/dev/null)"; then
  errors+=("node runner is unavailable")
  node_version="unavailable"
else
  node_version="$("$node_path" --version 2>/dev/null || printf 'unavailable')"
  if [ "$node_version" = "unavailable" ]; then
    node_path=""
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

lint_blocked=0
repair_journal="$tower_dir/backups/repair-transaction.json"
store_lock="$tower_file.lock"
if [ -e "$repair_journal" ] || [ -L "$repair_journal" ]; then
  lint_blocked=1
  errors+=("Tower store has a pending repair transaction: $repair_journal")
fi
if [ -e "$store_lock" ] || [ -L "$store_lock" ]; then
  lint_blocked=1
  errors+=("Tower store has an active or stale write lock: $store_lock")
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

hash_tree() {
  local path="$1"
  if [ ! -d "$path" ]; then
    printf 'missing'
    return 0
  fi
  "$node_path" - "$path" <<'NODE'
const { createHash } = require('node:crypto');
const { lstatSync, readFileSync, readlinkSync, readdirSync } = require('node:fs');
const { relative, resolve, join } = require('node:path');

const root = resolve(process.argv[2]);
const hash = createHash('sha256');
function walk(dir) {
  for (const name of readdirSync(dir).sort()) {
    const path = join(dir, name);
    const rel = relative(root, path);
    const stat = lstatSync(path);
    const kind = stat.isDirectory() ? 'd' : stat.isFile() ? 'f' : stat.isSymbolicLink() ? 'l' : 'o';
    hash.update(`${kind}\0${stat.mode}\0${rel}\0`);
    if (stat.isDirectory()) walk(path);
    else if (stat.isFile()) hash.update(readFileSync(path));
    else if (stat.isSymbolicLink()) hash.update(readlinkSync(path));
  }
}
walk(root);
process.stdout.write(hash.digest('hex'));
NODE
}

tower_before="unavailable"
if [ -n "${node_path:-}" ]; then
  if ! tower_before="$(hash_tree "$tower_dir")"; then
    tower_before="unavailable"
    errors+=("Tower store could not be fingerprinted before lint")
  fi
fi

lint_json_status="not-run"
lint_repeat_json_status="not-run"
if [ -n "${node_path:-}" ] && [ -f "$TOWER" ] && [ -d "$spec_root" ] && [ "$lint_blocked" -eq 0 ]; then
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
  if [ "$lint_blocked" -eq 1 ]; then
    errors+=("Tower lint was not run because the Tower store is not in a safe read-only state")
  else
    errors+=("Tower lint was not run because a required runner, entrypoint, or scope is missing")
  fi
fi

tower_after="unavailable"
if [ -n "${node_path:-}" ]; then
  if ! tower_after="$(hash_tree "$tower_dir")"; then
    tower_after="unavailable"
    errors+=("Tower store could not be fingerprinted after lint")
  fi
  if [ "$tower_before" = "unavailable" ] || [ "$tower_after" = "unavailable" ] ||
     [ "$tower_before" = "missing" ] || [ "$tower_after" = "missing" ]; then
    read_only="fail"
    errors+=("Tower store fingerprint is unavailable; read-only state is unproven")
  elif [ "$lint_blocked" -eq 1 ]; then
    read_only="blocked"
  elif [ "$tower_before" != "$tower_after" ]; then
    read_only="fail"
    errors+=("Tower store changed during read-only lint")
  else
    read_only="pass"
  fi
fi

validate_lint_json() {
  local path="$1"
  "$node_path" - "$path" <<'NODE'
const { readFileSync } = require('node:fs');
const value = JSON.parse(readFileSync(process.argv[2], 'utf8'));
if (!Array.isArray(value) || value.some((finding) =>
  !finding || typeof finding !== 'object' ||
  typeof finding.rule !== 'string' || typeof finding.ref !== 'string' ||
  typeof finding.msg !== 'string')) process.exit(1);
NODE
}

if [ "$lint_status" != "not-run" ]; then
  if validate_lint_json "$lint_output"; then
    lint_json_status="pass"
  else
    lint_json_status="fail"
    errors+=("tower lint did not emit a valid JSON finding array")
  fi
fi
if [ "$lint_repeat_status" != "not-run" ]; then
  if validate_lint_json "$lint_repeat_output"; then
    lint_repeat_json_status="pass"
  else
    lint_repeat_json_status="fail"
    errors+=("repeat tower lint did not emit a valid JSON finding array")
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
  printf 'read_only=%s\n' "$read_only"
  printf 'lint_exit=%s\n' "$lint_status"
  printf 'lint_repeat_exit=%s\n' "$lint_repeat_status"
  printf 'lint_json=%s\n' "$lint_json_status"
  printf 'lint_repeat_json=%s\n' "$lint_repeat_json_status"
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
