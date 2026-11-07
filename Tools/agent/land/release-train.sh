#!/usr/bin/env bash
# release-train.sh SHA: the lander hook of the a0_NNN release train (install beside batch-land.sh).
# Records SHA as the newest pending dev HEAD and returns at once. One train runs at a time (flock):
# it gates the newest pending HEAD with `Tools/release/Main.jet -- admit` (run by the current
# release), and when landings arrived meanwhile it
# gates the newest of them next; intermediate HEADs are skipped. The gate releases only with at least
# RELEASE_MIN_GAINS gained rows; a "gains FAIL" just means wait for more landings. A pass writes the
# next a0_NNN and the commands, and says RELEASE READY on the lander log for Main; nothing is
# merged, tagged, pushed or made current here.
set -uo pipefail
sha=${1:?usage: release-train.sh SHA}
. "$(dirname "${BASH_SOURCE[0]}")/../roots.sh"
dev=${RELEASE_DEV:-$JET_DEV_WORKTREE}
root=${JET_RELEASE_WORK:-$JET_STATE_ROOT/release}
log=${RELEASE_LOG:-$JET_RUN_ROOT/batch-land.log}
mkdir -p "$root"
printf '%s\n' "$sha" > "$root/pending.new" && mv "$root/pending.new" "$root/pending"
# Main pauses the train while a known blocker fails every gate; the pending HEAD stays recorded.
[ -e "$root/paused" ] && exit 0
if [ -z "${RELEASE_TRAIN_INNER:-}" ]; then
    RELEASE_TRAIN_INNER=1 setsid -f "$0" "$sha" > /dev/null 2>&1 < /dev/null
    exit 0
fi
exec 9> "$root/train.flock"
flock -n 9 || exit 0
while [ -s "$root/pending" ]; do
    head=$(< "$root/pending")
    rm -f "$root/pending"
    out="$root/train-${head:0:12}.log"
    seed=$(readlink -f "$JET_SEED")
    (cd "$dev" && JET_COMPILER_IDENTITY=$(< "$seed/IDENTITY") JET_TOOLCHAIN_ROOT=$dev timeout 43200 "$seed/jet" run Tools/release/Main.jet -- admit "$head" --min-gains "${RELEASE_MIN_GAINS:-10}") > "$out" 2>&1
    rc=$?
    case $rc in
        0) verdict="RELEASE READY $(awk '/^PASS:/{print $2}' "$out") from ${head:0:10}; run the commands in $out" ;;
        1) verdict="release gate ${head:0:10}: not released ($(awk '/^FAIL:/{sub(/^FAIL: /, ""); print}' "$out" | cut -c1-200))" ;;
        *) verdict="release gate ${head:0:10}: could not start rc=$rc ($(awk 'END{print}' "$out" | cut -c1-200))" ;;
    esac
    printf '%s\n' "$verdict" >> "$log"
done
