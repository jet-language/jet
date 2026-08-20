#!/usr/bin/env bash
# disk-report.sh — what Jet development is costing in RAM and disk, and the
# exact command to give each piece back.
#
# Run this before and after a long session. A session that ended in an OOM kill
# had 517G in one build cache, 8.6G of test scratch sitting in RAM-backed /tmp,
# and 12G in swap. Every one of those is visible here in under a second.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1

line() { printf '%-46s %8s   %s\n' "$1" "$2" "${3:-}"; }

echo "── memory ──────────────────────────────────────────────────────────────"
free -h | sed -n '1,3p'
echo
echo "── RAM-backed filesystems (anything here is RAM) ───────────────────────"
df -h -t tmpfs 2>/dev/null | awk 'NR==1 || $3+0 > 0'
echo
echo "── disk ────────────────────────────────────────────────────────────────"
df -h . | sed -n '1,2p'
echo
echo "── the usual occupants ─────────────────────────────────────────────────"
for p in target target/debug/incremental target/debug/deps target/tmp build \
         "$HOME/.cache/jet-test-scratch" "$HOME/.cache/jet/runtime"; do
  [ -e "$p" ] || continue
  line "$p" "$(du -sh "$p" 2>/dev/null | cut -f1)"
done
for p in target-*; do
  [ -e "$p" ] || continue
  line "$p" "$(du -sh "$p" 2>/dev/null | cut -f1)" "stale session tree — delete"
done
if [ -d .claude/worktrees ]; then
  for w in .claude/worktrees/*/; do
    [ -d "$w" ] || continue
    line "$w" "$(du -sh "$w" 2>/dev/null | cut -f1)" "worktree"
  done
fi
echo
echo "── give it back ────────────────────────────────────────────────────────"
cat <<'TXT'
  rm -rf target/debug/incremental          # regenerates; usually the biggest slice
  rm -rf target-*                          # stale per-session build trees
  rm -rf ~/.cache/jet-test-scratch/*       # test scratch (proof-parallel clears this)
  cargo clean                              # last resort: a full cold rebuild follows
  sudo swapoff -a && sudo swapon -a        # zero swap; needs root, safe when RAM is free
TXT
