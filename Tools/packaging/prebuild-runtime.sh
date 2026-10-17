#!/usr/bin/env bash
# Package the full checked Jet runtime beside a hosted compiler. The native
# library hook lowers Compiler/JetRuntime/sources.list and lifecycle.list as
# one object; Jet's linker binds only that object and permitted C platform
# archives. No rustc, Rust runtime, std rlibs or system linker is invoked.
# usage: JET_PACK_MUSL_LIB=<static musl directory> prebuild-runtime.sh <jet> [dir]
set -euo pipefail
exec node "$(dirname "$(realpath "$0")")/prebuild-runtime.mjs" "$@"
