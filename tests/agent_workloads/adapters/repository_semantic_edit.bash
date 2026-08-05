#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || {
  printf '%s\n' 'usage: repository_semantic_edit INPUT_ROOT' >&2
  exit 2
}

project=project
trap 'rm -rf -- "$project"' EXIT
cp -R -- "$1" "$project"
source="$project/project/examples/main.jet"
temporary="$source.tmp"
awk '
  index($0, "fn prepare()") == 1 { sub("fn prepare", "fn configure") }
  $0 == "    prepare()" { $0 = "    configure()" }
  { print }
' "$source" >"$temporary"
mv -- "$temporary" "$source"
cat "$source"
