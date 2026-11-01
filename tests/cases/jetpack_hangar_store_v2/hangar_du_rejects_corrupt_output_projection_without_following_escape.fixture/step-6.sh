set -eu
# rs:1674-1678: recover restores the original projection and never touches the outside bytes.
cmp meta.original jetpack/hangar/du-proof-*/meta.json
test "$(cat outside/must-survive)" = "outside bytes"
