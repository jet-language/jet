#!/bin/sh
printf '%s\n' "$@" > $HOME/.cache/jet-dev/dx3/prim-bridges/pkg/rustc-wrapper.args
exec "$@"
