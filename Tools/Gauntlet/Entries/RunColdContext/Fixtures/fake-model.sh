#!/bin/sh
# A stand-in model: `fake-model.sh FAMILY [PROMPT]` answers with a candidate
# whose markers (see fake-jet.sh) give each family, context and task a known
# outcome. Without a PROMPT argument the request is the JSON on stdin.
# FAKE_MODEL_FAIL=1 makes it exit 3.
family=$1
if [ "${FAKE_MODEL_FAIL:-}" = 1 ]; then
    echo "fake-model: failing on purpose" >&2
    exit 3
fi
if [ $# -ge 2 ]; then request=$2; else request=$(cat); fi
task=$(printf '%s\n' "$request" | sed -n 's|.*Task ID: \([a-z-]*\).*|\1|p' | sed -n 1p)
case $request in
*"## Jet context capsule"*) context=capsule ;;
*) context=control ;;
esac
fence='```'
case "$family/$context/$task" in
openai/capsule/hello) printf 'run: fn() { }\n// out: Hello, Jet\n' ;;
openai/capsule/cli) printf '{"source": "run: fn() { }\\n// out: hello, Ada\\n"}\n' ;;
openai/capsule/data-transform) printf '%sjet\nrun: fn() { }\n// out: jet\n// out: jet\n// out: rust\n%s\n' "$fence" "$fence" ;;
openai/capsule/http) printf 'run: fn() { }\n// serve: good\n' ;;
openai/control/hello) printf 'print("Hello, Jet")\n' ;;
openai/control/cli) printf 'run: fn() { }\n// check: fail\n' ;;
openai/control/data-transform) printf 'run: fn() { }\n// out: rust\n// out: jet\n' ;;
openai/control/http) printf 'run: fn() { }\n// serve: bad\n' ;;
anthropic/capsule/hello) printf 'run: fn() { }\n// out: Hello, Jet\n// err: note\n' ;;
anthropic/capsule/cli) printf 'run: fn() { }\n// out: hello, Ada\n' ;;
anthropic/capsule/data-transform) printf 'run: fn() { }\n// out: jet\n// out: jet\n// out: rust\n' ;;
anthropic/capsule/http) printf 'run: fn() { }\n// serve: good\n' ;;
anthropic/control/hello) printf 'run: fn() { }\n// out: Hello, Jet\n// exit: 3\n' ;;
anthropic/control/cli) printf '%sjet\nrun: fn() { }\n// out: hello, Ada\n' "$fence" ;;
anthropic/control/data-transform) printf 'run: fn() { }\n// out: jet\n// out: jet\n// out: rust\n' ;;
anthropic/control/http) printf 'run: fn() { }\n// serve: good\n' ;;
*) echo "fake-model: no answer for $family/$context/$task" >&2; exit 4 ;;
esac
