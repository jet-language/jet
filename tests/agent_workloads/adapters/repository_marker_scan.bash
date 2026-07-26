#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || {
  printf '%s\n' 'usage: repository_marker_scan INPUT_ROOT' >&2
  exit 2
}

root=${1%/}
shopt -s globstar nullglob
rows=()
for file in "$root"/**/*; do
  [[ -f "$file" ]] || continue
  count=0
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ "$line" == *agent_workload:* ]] && ((count += 1))
  done < "$file"
  if ((count > 0)); then
    relative=${file#"$root"}
    relative=${relative//\\//}
    rows+=("$relative|$count")
  fi
done

for ((i = 0; i < ${#rows[@]}; i += 1)); do
  for ((j = i + 1; j < ${#rows[@]}; j += 1)); do
    if [[ ${rows[j]} < ${rows[i]} ]]; then
      swap=${rows[i]}
      rows[i]=${rows[j]}
      rows[j]=$swap
    fi
  done
done

printf '%s\n' "${rows[@]}"
