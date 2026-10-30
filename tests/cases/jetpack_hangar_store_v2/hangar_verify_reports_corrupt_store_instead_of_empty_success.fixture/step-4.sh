set -eu
# rs:1556 and rs:1575-1578: restore the journal; the source payload is unchanged.
cp journal.original "$(cat journal.path)"
test "$(cat source/payload)" = "live bytes"
