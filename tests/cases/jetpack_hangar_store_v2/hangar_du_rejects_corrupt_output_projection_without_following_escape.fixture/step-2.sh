set -eu
# rs:1609-1625: point the entry's output projection at an outside directory and hide the closure journal.
meta=$(ls -d jetpack/hangar/du-proof-*/meta.json)
chmod -R u+w "$(dirname "$meta")"
mkdir -p outside
printf 'outside bytes' > outside/must-survive
out=$(awk -F'"' '$2 == "out" {print $4}' "$meta")
test -n "$out"
cp "$meta" meta.original
awk -v old="$out" -v new="$PWD/outside" '{
  line = ""
  rest = $0
  while ((at = index(rest, old)) > 0) {
    line = line substr(rest, 1, at - 1) new
    rest = substr(rest, at + length(old))
  }
  print line rest
}' meta.original > "$meta"
! cmp -s meta.original "$meta"
mv jetpack/hangar/closure-db/journal jetpack/hangar/closure-db/journal.corrupt-backup
