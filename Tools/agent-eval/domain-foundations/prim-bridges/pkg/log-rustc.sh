#!/bin/sh
printf '%s\n' "$@" > /home/nate/.cache/jet-luna/dx3/prim-bridges/pkg/rustc-wrapper.args
exec "$@"
