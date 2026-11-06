#!/bin/sh
# Deliberately controlled CLI peer for driver self-tests, not a real compiler.
set -eu
if [ "${JET_TESTKIT_ICE:-}" = 1 ]; then exit 101; fi
if [ "${JET_TESTKIT_WARNING:-}" = 1 ]; then
    printf 'Warning [jet.BranchArmTable]: unjoined task at function boundary\n' >&2
fi
here=$(dirname "$0")
case "$1" in
    emit) printf 'main: fn() {}\n' ;;
    check) exit 0 ;;
    build)
        if [ "${JET_TESTKIT_REJECT:-}" = 1 ]; then printf 'Error [jet.StrayCharacter]: rejected\n' >&2; exit 1; fi
        directory=$(dirname "$2")
        name=$(basename "$2" .jet)
        artifact="$directory/.jet/build/$name"
        mkdir -p "$directory/.jet/build"
        program="$here/program.sh"
        if [ "${JET_TESTKIT_INPUT:-}" = echo ]; then
            program="$here/echo.sh"
        elif [ "${JET_TESTKIT_STDERR:-}" = 1 ]; then
            program="$here/notice.sh"
        elif [ "${JET_TESTKIT_WHERE:-}" = 1 ]; then
            program="$here/where.sh"
        else
            case "$*" in *--release*) program="$here/wrong.sh" ;; esac
        fi
        cp "$program" "$artifact"
        chmod +x "$artifact"
        ;;
    run) printf 'hello\n' ;;
    test)
        if [ "${JET_TESTKIT_TEST_FAIL:-}" = 1 ]; then printf 'at %s: FAIL\n0 passed, 1 failed, 0 skipped\n' "$2"; exit 1; fi
        printf 'at %s: pass\n1 passed, 0 failed, 0 skipped\n' "$2"
        ;;
    *) printf 'unknown fixture command\n' >&2; exit 2 ;;
esac
