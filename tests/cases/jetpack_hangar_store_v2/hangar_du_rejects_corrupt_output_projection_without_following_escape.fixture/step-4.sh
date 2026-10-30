set -eu
# rs:1633 and rs:1651-1656: restore the journal; the real output and the outside bytes are untouched.
mv jetpack/hangar/closure-db/journal.corrupt-backup jetpack/hangar/closure-db/journal
out=$(awk -F'"' '$2 == "out" {print $4}' meta.original)
case "$out" in "$PWD/jetpack"/*) ;; *) exit 1 ;; esac
test -d "$out"
test "$(cat outside/must-survive)" = "outside bytes"
