#!/bin/sh
# A stand-in for `jet` that acts on markers in candidate.jet:
#   // check: fail    `check` fails
#   // out: TEXT      `run` prints TEXT
#   // err: TEXT      `run` writes TEXT to stderr
#   // exit: N        `run` exits N
#   // serve: MODE    `run` serves HTTP on the port argument (serve.mjs)
here=$(cd "$(dirname "$0")" && pwd)
command=$1
case $command in
check)
    if grep -q '^// check: fail' candidate.jet; then
        echo "candidate.jet: error" >&2
        exit 1
    fi
    echo "ok: candidate.jet"
    exit 0 ;;
run)
    shift 3
    [ "$1" = "--" ] && shift
    mode=$(sed -n 's|^// serve: ||p' candidate.jet)
    if [ -n "$mode" ]; then
        exec node "$here/serve.mjs" "$mode" "$1"
    fi
    sed -n 's|^// out: ||p' candidate.jet
    sed -n 's|^// err: ||p' candidate.jet >&2
    code=$(sed -n 's|^// exit: ||p' candidate.jet)
    exit "${code:-0}" ;;
*)
    echo "fake-jet: unknown command $command" >&2
    exit 2 ;;
esac
