#!/usr/bin/env bash
# D-CI1: run one required gate and leave a candidate-bound receipt, including
# the failure path. Callers upload the report even when the gate is red.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"

usage() {
  echo "usage: bash tools/ci/ci-evidence.sh --report-dir DIR -- COMMAND [ARG ...]" >&2
  exit 64
}

[ "${1:-}" = "--report-dir" ] && [ "$#" -ge 4 ] && [ "${3:-}" = "--" ] || usage
report_dir="$2"
shift 3
[ "$#" -gt 0 ] || usage

mkdir -p -- "$report_dir" || exit 1
stdout_file="$report_dir/command.stdout"
stderr_file="$report_dir/command.stderr"
receipt="$report_dir/receipt.txt"
toolchain="$report_dir/toolchain.txt"
candidate_manifest="$report_dir/candidate.txt"
: >"$stdout_file" || exit 1

hash_file() {
  local path="$1"
  if [ -f "$path" ] && command -v sha256sum >/dev/null 2>&1; then
    sha256sum -- "$path" | awk '{print $1}'
  elif [ -f "$path" ] && command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -- "$path" | awk '{print $1}'
  else
    printf 'unavailable'
  fi
}

candidate="${JET_CI_CANDIDATE_COMMIT:-${GITHUB_SHA:-}}"
checkout_candidate=""
if command -v git >/dev/null 2>&1; then
  checkout_candidate="$(git -C "$ROOT" rev-parse --verify HEAD 2>/dev/null || true)"
fi
if [ -z "$candidate" ]; then
  candidate="$checkout_candidate"
fi

runner_os="${RUNNER_OS:-${GITHUB_RUNNER_OS:-}}"
runner_arch="${RUNNER_ARCH:-${GITHUB_RUNNER_ARCH:-}}"
[ -n "$runner_os" ] || runner_os="$(uname -s 2>/dev/null || printf 'unknown')"
[ -n "$runner_arch" ] || runner_arch="$(uname -m 2>/dev/null || printf 'unknown')"

{
  echo '=== rustc ==='
  if command -v rustc >/dev/null 2>&1; then rustc -vV; else echo '(unavailable)'; fi
  echo
  echo '=== cargo ==='
  if command -v cargo >/dev/null 2>&1; then cargo -vV; else echo '(unavailable)'; fi
  echo
  echo '=== nix ==='
  if command -v nix >/dev/null 2>&1; then nix --version; else echo '(unavailable)'; fi
} >"$toolchain"

source_manifest="$ROOT/Cargo.lock"
workflow="$ROOT/.github/workflows/ci.yml"
source_hash="$(hash_file "$source_manifest")"
workflow_hash="$(hash_file "$workflow")"
toolchain_hash="$(hash_file "$toolchain")"
artifact_name="${JET_CI_ARTIFACT_NAME:-not-published}"
signature_candidate="${JET_CI_SIGNATURE_CANDIDATE:-$candidate}"
release_candidate="${JET_CI_RELEASE_CANDIDATE:-$candidate}"
started_at="$(date -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || printf 'unknown')"
status="started"
command_exit="not-finished"
command_args=("$@")
export JET_CI_EVIDENCE_DIR="$report_dir"

identity_failure=""
fail_identity() {
  [ -n "$identity_failure" ] || identity_failure="$1"
}

case "$candidate" in
  ''|*[!0-9a-fA-F]*) fail_identity "missing or invalid candidate commit identity" ;;
esac
[ "${#candidate}" -eq 40 ] || fail_identity "candidate commit identity must be a 40-character SHA-1"
if [ -n "${GITHUB_SHA:-}" ] && [ "$candidate" != "$GITHUB_SHA" ]; then
  fail_identity "candidate commit does not match GITHUB_SHA: $candidate != $GITHUB_SHA"
fi
if [ -n "$checkout_candidate" ] && [ "$candidate" != "$checkout_candidate" ]; then
  fail_identity "candidate commit does not match checked-out revision: $candidate != $checkout_candidate"
fi
if [ "$artifact_name" != "not-published" ]; then
  case "$artifact_name" in
    *-"$candidate") ;;
    *) fail_identity "artifact name does not identify candidate: $artifact_name" ;;
  esac
fi
[ "$signature_candidate" = "$candidate" ] || \
  fail_identity "signature candidate does not match candidate commit: $signature_candidate != $candidate"
[ "$release_candidate" = "$candidate" ] || \
  fail_identity "release metadata candidate does not match candidate commit: $release_candidate != $candidate"
for identity_hash in "$source_hash" "$workflow_hash" "$toolchain_hash"; do
  [ "$identity_hash" != "unavailable" ] || fail_identity "candidate identity hash is unavailable"
done

write_candidate_manifest() {
  local tmp="$candidate_manifest.tmp.$$"
  {
    echo 'schema=jet.ci-candidate.v1'
    echo "candidate_commit=$candidate"
    echo "source_candidate_commit=$candidate"
    echo "toolchain_candidate_commit=$candidate"
    echo "artifact_candidate_commit=$candidate"
    echo "signature_candidate_commit=$signature_candidate"
    echo "test_candidate_commit=$candidate"
    echo "support_matrix_candidate_commit=$candidate"
    echo "provenance_candidate_commit=$candidate"
    echo "release_metadata_candidate_commit=$release_candidate"
    echo "source_revision=${checkout_candidate:-unknown}"
    echo "source_manifest_sha256=$source_hash"
    echo "workflow_sha256=$workflow_hash"
    echo "toolchain_sha256=$toolchain_hash"
    echo "artifact_name=$artifact_name"
    echo "signature=not-required-for-ci-test-gate"
    echo "support_matrix=${JET_CI_SUPPORT_MATRIX:-$runner_os/$runner_arch}"
    echo "provenance=github-actions:${GITHUB_RUN_ID:-local}/${GITHUB_RUN_ATTEMPT:-1}"
    echo "release_ref=${GITHUB_REF:-local}"
  } >"$tmp" && mv -f -- "$tmp" "$candidate_manifest"
}

write_receipt() {
  local result="$1"
  local tmp="$receipt.tmp.$$"
  write_candidate_manifest || return 1
  {
    echo 'schema=jet.ci-evidence.v1'
    echo "status=$result"
    echo "candidate_commit=${candidate:-unknown}"
    echo "candidate_manifest=$candidate_manifest"
    echo "source_candidate_commit=$candidate"
    echo "toolchain_candidate_commit=$candidate"
    echo "artifact_candidate_commit=$candidate"
    echo "signature_candidate_commit=$signature_candidate"
    echo "test_candidate_commit=$candidate"
    echo "support_matrix_candidate_commit=$candidate"
    echo "provenance_candidate_commit=$candidate"
    echo "release_metadata_candidate_commit=$release_candidate"
    echo "runner_os=$runner_os"
    echo "runner_arch=$runner_arch"
    echo "workflow=${GITHUB_WORKFLOW:-CI}"
    echo "job=${GITHUB_JOB:-local}"
    echo "run_id=${GITHUB_RUN_ID:-local}"
    echo "run_attempt=${GITHUB_RUN_ATTEMPT:-1}"
    echo "event=${GITHUB_EVENT_NAME:-local}"
    echo "ref=${GITHUB_REF:-local}"
    echo "test_shard=${JET_TEST_SHARD:-not-applicable}"
    echo "test_shard_count=${JET_TEST_SHARD_COUNT:-not-applicable}"
    echo "support_matrix=${JET_CI_SUPPORT_MATRIX:-$runner_os/$runner_arch}"
    echo "artifact_name=$artifact_name"
    echo "provenance=github-actions:${GITHUB_RUN_ID:-local}/${GITHUB_RUN_ATTEMPT:-1}"
    echo "release_ref=${GITHUB_REF:-local}"
    echo "source_manifest_sha256=$source_hash"
    echo "workflow_sha256=$workflow_hash"
    echo "toolchain_sha256=$toolchain_hash"
    echo "started_at=$started_at"
    echo "command_exit=$command_exit"
    printf 'command='
    printf '%q ' "${command_args[@]}"
    printf '\n'
    echo "stdout=$stdout_file"
    echo "stderr=$stderr_file"
    echo "signature=not-required-for-ci-test-gate"
    echo "publication=github-actions-artifact"
  } >"$tmp" && mv -f -- "$tmp" "$receipt"
}

finish() {
  local rc=$?
  if [ "$status" = "started" ]; then
    command_exit="$rc"
    status="fail"
    write_receipt "$status" "${command_args[@]}"
  fi
  exit "$rc"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

if [ -n "$identity_failure" ]; then
  printf '%s\n' "$identity_failure" >"$stderr_file"
  cat -- "$stderr_file" >&2
  command_exit=78
  status="fail"
  write_receipt "$status" "${command_args[@]}"
  trap - EXIT HUP INT TERM
  exit "$command_exit"
fi

"$@" >"$stdout_file" 2>"$stderr_file"
command_exit=$?
cat -- "$stdout_file"
cat -- "$stderr_file" >&2
if [ "$command_exit" -eq 0 ] && [ -n "$candidate" ] && [ "$candidate" != "unknown" ]; then
  status="pass"
else
  status="fail"
  if [ "$command_exit" -eq 0 ]; then
    command_exit=78
  fi
fi
write_receipt "$status" "${command_args[@]}"
trap - EXIT HUP INT TERM
exit "$command_exit"
