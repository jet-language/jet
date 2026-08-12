#!/usr/bin/env bash
# Time every root integration-test binary independently.
# The in-process guard remains the authority: no binary may run past 900s.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo"

budget="${JET_TEST_DEADLINE_SECS:-900}"
external="${JET_TEST_TIMEOUT_SECS:-1000}"
case "$budget" in
  ''|*[!0-9]*) echo "error: JET_TEST_DEADLINE_SECS must be a positive integer" >&2; exit 64 ;;
esac
case "$external" in
  ''|*[!0-9]*) echo "error: JET_TEST_TIMEOUT_SECS must be a positive integer" >&2; exit 64 ;;
esac
if [ "$budget" -lt 1 ] || [ "$budget" -gt 900 ]; then
  echo "error: test-binary budget must be between 1 and 900 seconds, got $budget" >&2
  exit 64
fi
if [ "$external" -lt "$budget" ]; then
  echo "error: external timeout ($external) must cover the $budget-second test guard" >&2
  exit 64
fi

if [ "$#" -eq 0 ]; then
  mapfile -t targets < <(
    find tests -maxdepth 1 -type f -name '*.rs' -printf '%f\n' |
      sed 's/\.rs$//' |
      sort
  )
else
  targets=("$@")
fi
if [ "${#targets[@]}" -eq 0 ]; then
  echo "error: no root integration-test targets found" >&2
  exit 1
fi

status=0
for target in "${targets[@]}"; do
  echo "time-suites: $target (guard ${budget}s)"
  started=$SECONDS
  target_status=0
  if JET_TEST_DEADLINE_SECS="$budget" timeout "${external}s" \
    scripts/agent/jet-env cargo test --test "$target"; then
    :
  else
    target_status=$?
  fi
  elapsed=$((SECONDS - started))
  echo "time-suites: $target elapsed=${elapsed}s status=${target_status}"
  if [ "$target_status" -ne 0 ] && [ "$status" -eq 0 ]; then
    status="$target_status"
  fi
done
exit "$status"
