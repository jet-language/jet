#!/bin/sh
printf '%s\n' "$@" > /home/nate/.cache/jet-dev/dx3/prim-bridges/pkg/rustc-wrapper.args
exec "$@"
