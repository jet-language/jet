#!/usr/bin/env bash
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
jet=${JET:-jet}
jet_cc=${JET_CC:-jet-cc}
jet_cxx=${JET_CXX:-jet-c++}
cmake=${CMAKE_COMMAND:-cmake}
work=$(mktemp -d "jet-ffi-driver.XXXXXX")
trap 'rm -rf "$work"' EXIT

cp -R "$root" "$work/project"
cd "$work/project"

# Driver adoption is an explicit foreign-project mode.  It does not imply
# that Jet owns the full CMake graph.
"$cmake" -S . -B build \
    -DCMAKE_C_COMPILER="$jet_cc" \
    -DCMAKE_CXX_COMPILER="$jet_cxx" \
    -DCMAKE_EXPORT_COMPILE_COMMANDS=ON \
    >"$work/configure.log" 2>&1
"$cmake" --build build >"$work/foreign-build.log" 2>&1
output=$(build/driver_app)
[[ "$output" == 42 ]]
[[ -f build/generated/native-action.txt ]]

preview=$("$jet" build --import cmake:build --preview 2>&1)
preview_lower=$(printf '%s\n' "$preview" | tr '[:upper:]' '[:lower:]')
for marker in dependencies commands toolchain inputs outputs environment unsupported inner; do
    case "$preview_lower" in
        *"$marker"*) ;;
        *)
            printf 'driver-build: preview omitted %s\n' "$marker" >&2
            exit 1
            ;;
    esac
done

plan_digest=${PLAN_DIGEST:-}
if [[ -z "$plan_digest" ]]; then
    plan_digest=$(printf '%s\n' "$preview" | sed -n 's/.*plan[- ]digest[=: ]\([[:alnum:]_.-]*\).*/\1/p' | sed -n '1p')
fi
if [[ -z "$plan_digest" ]]; then
    printf '%s\n' 'driver-build: preview did not expose a plan digest' >&2
    exit 1
fi
"$jet" build --import cmake:build --accept "$plan_digest" >"$work/accept.log" 2>&1
"$jet" build >"$work/jet-build.log" 2>&1

if "$jet" build --import cmake:build --accept stale-plan-digest >"$work/stale.log" 2>&1; then
    printf '%s\n' 'driver-build: stale plan was accepted' >&2
    exit 1
fi
stale=$(cat "$work/stale.log")
case "$stale" in
    *stale*|*digest*|*input*|*selection*) ;;
    *)
        printf '%s\n' 'driver-build: stale rejection omitted digest/input evidence' >&2
        cat "$work/stale.log" >&2
        exit 1
        ;;
esac
printf '%s\n' 'driver-build: 42 (driver adoption, partial graph, and stale rejection observed)'
