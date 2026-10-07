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
        if [ "${JET_TESTKIT_INPUT:-}" = echo ]; then
            printf 'built: %s/echo.sh\n' "$here"
            exit 0
        fi
        if [ "${JET_TESTKIT_STDERR:-}" = 1 ]; then
            printf 'built: %s/notice.sh\n' "$here"
            exit 0
        fi
        case "$*" in
            *--release*) printf 'built: %s/wrong.sh\n' "$here" ;;
            *) printf 'built: %s/program.sh\n' "$here" ;;
        esac
        ;;
    run) printf 'hello\n' ;;
    *) printf 'unknown fixture command\n' >&2; exit 2 ;;
esac
