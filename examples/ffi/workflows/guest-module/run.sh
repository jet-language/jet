#!/usr/bin/env bash
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
jet=${JET:-jet}
cmake=${CMAKE_COMMAND:-cmake}
toolchain=${JET_TOOLCHAIN_FILE:-"$root/../../../../tools/foreign-build-hosts/cmake/JetToolchain.cmake"}
work=$(mktemp -d "jet-ffi-guest.XXXXXX")
trap 'rm -rf "$work"' EXIT

project="$work/project"
cp -R "$root" "$project"
cd "$project"
"$jet" bind rules.h --pkg rules >"$work/bind.log" 2>&1
"$jet" bind rules --shape automatic --freeze >"$work/plan.log" 2>&1
"$cmake" -S . -B "$work/build" \
    -DJet_EXECUTABLE="$jet" \
    -DCMAKE_TOOLCHAIN_FILE="$toolchain" \
    >"$work/configure.log" 2>&1
"$cmake" --build "$work/build" --target game_native >"$work/native-build.log" 2>&1
native_output=$("$work/build/game_native")
[[ "$native_output" == 42 ]]

"$cmake" --build "$work/build" --target game >"$work/guest-build.log" 2>&1
guest_output=$("$work/build/game")
[[ "$guest_output" == 42 ]]

sed -i 's/int64_t/int32_t/g' rules.h
if "$jet" bind rules --shape automatic --freeze >"$work/rejection.log" 2>&1; then
    printf '%s\n' 'guest-module: incompatible replacement was accepted' >&2
    exit 1
fi
rejection=$(cat "$work/rejection.log")
case "$rejection" in
    *drift*|*Drift*|*width*|*Width*|*identity*|*Identity*|*contract*|*Contract*) ;;
    *)
        printf '%s\n' 'guest-module: rejection omitted width/layout/identity/failure contract' >&2
        cat "$work/rejection.log" >&2
        exit 1
        ;;
esac
printf '%s\n' 'guest-module: 42 (native call site preserved; incompatible contract rejected)'
