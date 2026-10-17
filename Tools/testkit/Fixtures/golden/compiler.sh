#!/bin/sh
# Deliberately controlled CLI peer for driver self-tests, not a real compiler.
set -eu
if [ "${JET_TESTKIT_ICE:-}" = 1 ]; then exit 101; fi
if [ "${JET_TESTKIT_WARNING:-}" = 1 ]; then
    printf 'Warning [L0507]: unjoined task at function boundary\n' >&2
fi
here=$(dirname "$0")
case "$1" in
    emit) printf 'fn main() {}\n' ;;
    check) exit 0 ;;
    build)
        directory=$(dirname "$2")
        name=$(basename "$2" .jet | tr . _)
        artifact="$directory/.jet/build/$name"
        mkdir -p "$directory/.jet/build"
        program="$here/program.sh"
        if [ "${JET_TESTKIT_INPUT:-}" = echo ]; then
            program="$here/echo.sh"
        elif [ "${JET_TESTKIT_STDERR:-}" = 1 ]; then
            program="$here/notice.sh"
        else
            case "$*" in *--release*) program="$here/wrong.sh" ;; esac
        fi
        cp "$program" "$artifact"
        chmod +x "$artifact"
        ;;
    run) printf 'hello\n' ;;
    *) printf 'unknown fixture command\n' >&2; exit 2 ;;
esac
