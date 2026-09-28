#!/usr/bin/env bash
# Private bootstrap front-end entry: assemble one explicit Jet Compiler unit,
# then use the repository's supported `jet check` command. The assembled Jet
# source/provenance and proof receipt stay outside Cargo's Rust target tree.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
scratch="${HOME:?HOME must be set}/.cache/jet-luna/compiler-bootstrap"
project="$scratch/project"
unit="$project/src/compiler.jet"
package_manifest="$project/package.jet"
source_map="$scratch/compiler.map.json"
log="$scratch/jet-check.log"
receipt="$scratch/jet-check.receipt"
source_input_root="${JET_BOOTSTRAP_SOURCE_ROOT:-$repo}"

node "$repo/Compiler/Bootstrap/assemble.mjs"

if [[ ! -f "$package_manifest" ]]; then
  echo "jet-bootstrap: isolated project manifest was not assembled" >&2
  exit 65
fi
check_command=(timeout --foreground 120 "$repo/Tools/agent/jet-env" jet check "$unit")
printf -v exact_command '%q ' "${check_command[@]}"
unit_sha="$(sha256sum "$unit" | cut -d ' ' -f1)"
source_map_sha="$(sha256sum "$source_map" | cut -d ' ' -f1)"
package_sha="$(sha256sum "$package_manifest" | cut -d ' ' -f1)"
{
  printf 'entry-command: Tools/agent/jet-env bash Compiler/Bootstrap/check.sh\n'
  printf 'working-directory: %s\n' "$project"
  printf 'command: %s\n' "${exact_command% }"
  printf 'project-root: %s\n' "$project"
  printf 'project-manifest: %s\n' "$package_manifest"
  printf 'input-source-root: %s\n' "$source_input_root"
  printf 'project-manifest-sha256: %s\n' "$package_sha"
  printf 'source-root: %s/src\n' "$project"
  printf 'module: Compiler\n'
  printf 'fallback-source-file: Compiler\n'
  printf 'unit-sha256: %s\n' "$unit_sha"
  printf 'source-map-sha256: %s\n' "$source_map_sha"
  printf 'source-map: %s\n' "$source_map"
  printf 'log: %s\n' "$log"
} > "$receipt"

set +e
(
  cd "$project"
  "${check_command[@]}"
) > "$log" 2>&1
status=$?
set -e
printf 'exit: %d\n' "$status" >> "$receipt"
cat "$log"
exit "$status"
