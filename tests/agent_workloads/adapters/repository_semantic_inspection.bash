#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || {
  printf '%s\n' 'usage: repository_semantic_inspection INPUT_ROOT' >&2
  exit 2
}

awk '
  /^[A-Za-z_][A-Za-z0-9_]*: fn\(/ { definitions += 1 }
  /^[[:space:]]*(print|prepare)\(/ { references += 1; calls += 1 }
  END {
    printf "definitions=%d\nreferences=%d\ncalls=%d\n", definitions, references, calls
  }
' "$1/project/examples/main.jet"
