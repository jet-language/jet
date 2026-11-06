#!/bin/sh
printf '%s\n' "$@" > $JET_STATE_ROOT/dx3/prim-bridges/pkg/rustc-wrapper.args
exec "$@"
