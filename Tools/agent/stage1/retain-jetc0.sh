#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
STAGE1_TOOLS="$JET_AGENT_TOOLS/stage1"
# Fills the stage-zero keep dir from a stage-zero test run that did not set
# JET_STAGE_ZERO_KEEP (Compiler/Bootstrap/Tests.rs retain_stage_zero), so the
# stage-one scripts can drive its jetc0. Writes the same files the test does:
# jetc0, jetc0.env, backend-Cargo.toml, backend-build.rs, artifact-main.rs.
#
# usage: retain-jetc0.sh [SESSION_DIR] [KEEP_DIR]
#   SESSION_DIR  ~/.cache/jet-dev/scratch/bootstrap-self-<pid>-<ns> (default:
#                the newest one with a built stage-zero project)
#   KEEP_DIR     default ~/.cache/jet-dev/stage0
# The binary is <repo>/target/<dir>/jet_bootstrap_stage_zero (build_backend_artifact),
# where <repo> is the `jet` path dependency in the session's Cargo.toml and <dir>
# is debug, or jet-stage-release when the manifest carries that profile
# (JET_STAGE_ZERO_PROFILE=release).
set -u
here=$(cd "$(dirname "$0")" && pwd)
. "$here/lib.sh"

session=${1:-}
keep=${2:-$KEEP}
if [ -z "$session" ]; then
  for candidate in $(ls -dt "$SCRATCH"/bootstrap-self-* 2>/dev/null); do
    [ -f "$candidate/stage-zero/Cargo.toml" ] && [ -f "$candidate/stage-zero/.bootstrap-artifact-id" ] && { session=$candidate; break; }
  done
fi
[ -n "$session" ] && [ -f "$session/stage-zero/Cargo.toml" ] || die "no stage-zero session with a built backend project under $SCRATCH"
project=$session/stage-zero
repo=$(sed -n 's/^jet = { path = "\(.*\)" }$/\1/p;T;q' "$project/Cargo.toml")
[ -d "$repo" ] || die "cannot read the jet path dependency from $project/Cargo.toml"
profile_dir=debug
grep -q '^\[profile\.jet-stage-release\]' "$project/Cargo.toml" && profile_dir=jet-stage-release
binary=$repo/target/$profile_dir/jet_bootstrap_stage_zero
[ -x "$binary" ] || die "no $binary"
[ "$binary" -nt "$project/src/main.rs" ] || die "$binary is older than $project/src/main.rs: not this session's build"
task_roots=false
grep -q '^// bootstrap:task-roots-begin' "$project/src/main.rs" && task_roots=true
mkdir -p "$keep"
cp "$binary" "$keep/jetc0.partial" && mv "$keep/jetc0.partial" "$keep/jetc0"
cp "$project/Cargo.toml" "$keep/backend-Cargo.toml"
cp "$project/build.rs" "$keep/backend-build.rs"
extract_artifact_main "$repo" "$task_roots" "$keep/artifact-main.rs" || die "cannot extract GENERATED_ARTIFACT_MAIN from $repo"
printf 'repo=%s\nsession=%s\nid=%s\ntask_roots=%s\npackage=jet_bootstrap_stage_zero\n' \
  "$repo" "$session" "$(tr -d '\n' < "$project/.bootstrap-artifact-id")" "$task_roots" > "$keep/jetc0.env"
echo "retained $binary -> $keep/jetc0 (repo $repo, task_roots $task_roots)"
JETC0_KEEP=$keep "$here/pin-inputs.sh"
