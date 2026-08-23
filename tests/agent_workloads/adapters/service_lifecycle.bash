#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || { printf '%s\n' 'usage: service_lifecycle INPUT_ROOT' >&2; exit 2; }
input=$1
task=${JET_CORPUS_TASK:?missing task identity}
jetpack=${JET_CORPUS_JETPACK:?missing jetpack binary}
project="$PWD/service-project"
home="$PWD/service-home"
root="$PWD/service-root"
state="$project/systemd-state"
path="$project/bin:$PATH"
rm -rf -- "$project" "$home" "$root"
trap 'rm -rf -- "$project" "$home" "$root"' EXIT
mkdir -p -- "$home" "$root"
cp -R -- "$input"/. "$project"/
chmod +x "$project/bin/systemd-run" "$project/bin/systemctl"

run_jp() {
  local output=$1
  local errors=$2
  local timeout_ms=$3
  shift 3
  set +e
  HOME="$home" JETPACK_ROOT="$root" JETPACK_FAKE_SYSTEMD_STATE="$state" \
    JETPACK_SERVICE_HEALTH_TIMEOUT_MS="$timeout_ms" PATH="$path" \
    "$jetpack" "$@" >"$output" 2>"$errors"
  local status=$?
  set -e
  return "$status"
}

if [[ $task == service-lifecycle-readiness-timeout ]]; then
  if run_jp timeout.out timeout.err 200 services up timeout --no-color; then
    printf '%s\n' 'readiness timeout unexpectedly succeeded' >&2
    exit 1
  fi
  grep -q 'E1261' timeout.err || grep -q 'E1261' timeout.out || {
    printf '%s\n' 'readiness timeout lost E1261' >&2
    exit 1
  }
  lifecycle="$project/.jet/services/timeout/lifecycle"
  grep -q 'phase=failed' "$lifecycle"
  grep -q 'recovery=startup-failed' "$lifecycle"
  [[ ! -e "$project/.jet/services/timeout/pid" ]] || {
    printf '%s\n' 'failed service retained pid' >&2
    exit 1
  }
  child_file="$project/.jet/services/timeout/data/child.pid"
  if [[ -f $child_file ]]; then
    child=$(<"$child_file")
    if [[ -r /proc/$child/stat ]]; then
      state_code=$(awk '{print $3}' "/proc/$child/stat")
      [[ $state_code == Z ]] || {
        printf '%s\n' 'failed service retained descendant' >&2
        exit 1
      }
    fi
  fi
  [[ -f "$project/.jet/services/timeout/supervisor.error" ]] || {
    printf '%s\n' 'failed service lost supervisor receipt' >&2
    exit 1
  }
  printf '%s\n' 'service=failed' 'error=E1261' 'limit=bounded' 'descendants=contained' 'receipt=startup-failed'
else
  run_jp up.out up.err 5000 services up fixture --no-color || {
    cat up.err >&2
    exit 1
  }
  run_jp health.out health.err 5000 services health fixture --json --no-color || {
    cat health.err >&2
    exit 1
  }
  grep -q '"health":"healthy"' health.out
  grep -q 'linux-systemd-user' health.out
  grep -q 'delegated-cgroup' health.out
  run_jp wait.out wait.err 5000 services wait fixture --no-color || {
    cat wait.err >&2
    exit 1
  }
  grep -q 'service `fixture` is ready' wait.err
  run_jp logs.out logs.err 5000 services logs fixture --no-color || {
    cat logs.err >&2
    exit 1
  }
  grep -q 'service-started' logs.out
  run_jp down.out down.err 5000 services down fixture --no-color || {
    cat down.err >&2
    exit 1
  }
  lifecycle="$project/.jet/services/fixture/lifecycle"
  grep -q 'phase=stopped' "$lifecycle"
  grep -q 'recovery=down' "$lifecycle"
  [[ ! -e "$project/.jet/services/fixture/pid" ]] || {
    printf '%s\n' 'stopped service retained pid' >&2
    exit 1
  }
  printf '%s\n' 'service=ready' 'authority=linux-systemd-user' 'containment=delegated-cgroup' 'receipt=health-lifecycle' 'cleanup=ok'
fi
