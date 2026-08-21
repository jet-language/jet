#!/usr/bin/env bash
# Compose per-shard corpus-gate reports into one checked-in ledger.
#
# Each report is an observed classification. Composition owns the only write to
# tests/jit_corpus_gate.txt, after proving one exact, non-overlapping stem set.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: compose-corpus-gate.sh REPORT_DIR SHARD_COUNT" >&2
  exit 64
fi
report_dir="$1"
shard_count="$2"
case "$shard_count" in
  ''|*[!0-9]*) echo "error: SHARD_COUNT must be a positive integer, got '$shard_count'" >&2; exit 64 ;;
esac
if [ "$shard_count" -lt 1 ]; then
  echo "error: SHARD_COUNT must be >= 1, got '$shard_count'" >&2
  exit 64
fi

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo"
scratch_root="${JET_CORPUS_GATE_SCRATCH_DIR:-${JET_TEST_SCRATCH:-$HOME/.cache/jet-test-scratch}}"
mkdir -p "$scratch_root"
tmp="$(mktemp -d "$scratch_root/compose-corpus-gate.XXXXXX")"
ledger_tmp=
trap 'rm -rf "$tmp"; [ -z "$ledger_tmp" ] || rm -f "$ledger_tmp"' EXIT
records="$tmp/records.tsv"
: >"$records"

for ((shard = 0; shard < shard_count; shard++)); do
  file="$report_dir/shard-$shard/cases.txt"
  if [ ! -f "$file" ]; then
    echo "error: missing corpus-gate shard report: $file" >&2
    exit 1
  fi
  awk '
    /^#/ || /^[[:space:]]*$/ { next }
    /^[a-z_]+:$/ { section = $0; sub(/:$/, "", section); next }
    /^[[:space:]]+/ {
      entry = $0
      sub(/^[[:space:]]+/, "", entry)
      stem = entry
      sub(/: .*/, "", stem)
      if (section == "" || stem == "") exit 65
      print stem "\t" section "\t" entry
      next
    }
    { exit 65 }
  ' "$file" >>"$records" || {
    echo "error: malformed corpus-gate report: $file" >&2
    exit 65
  }
done

if [ ! -s "$records" ]; then
  echo "error: corpus-gate shard reports contain no records" >&2
  exit 1
fi

while IFS=$'\t' read -r stem section entry; do
  case "$section" in
    frontend_rejected|gate_excluded|non_runnable|oracle_unavailable|expected_exit|aot_broken|resident_jit|deopt_interp|run_tier_broken|tier_divergent) ;;
    *) echo "error: unknown corpus-gate section '$section' for '$stem'" >&2; exit 65 ;;
  esac
done <"$records"

sort -t$'\t' -k1,1 -k2,2 "$records" >"$tmp/sorted.tsv"
duplicates="$(awk -F'\t' 'NR > 1 && $1 == previous { print $1; found = 1 } { previous = $1 } END { exit found ? 0 : 1 }' "$tmp/sorted.tsv" || true)"
if [ -n "$duplicates" ]; then
  echo "error: corpus-gate shard reports overlap stems:" >&2
  printf '%s\n' "$duplicates" >&2
  exit 1
fi

manifest_stems() {
  awk '
    /^[a-z_][a-z_0-9]*:$/ { section = $0; next }
    /^[[:space:]]+/ && section != "" {
      entry = $0
      sub(/^[[:space:]]+/, "", entry)
      stem = entry
      sub(/: .*/, "", stem)
      print stem
    }
  ' "$1" | sort -u
}

find examples/features -type f -name '*.jet' ! -name 'package.jet' -print |
  while IFS= read -r file; do
    stem="${file#examples/features/}"
    case "$stem" in
      expected/*|*/*/*) continue ;;
    esac
    printf '%s\n' "${stem%.jet}"
  done | sort -u >"$tmp/corpus"
cut -f1 "$tmp/sorted.tsv" | sort -u >"$tmp/after"
manifest_stems tests/jit_corpus_gate.txt >"$tmp/before"

comm -23 "$tmp/corpus" "$tmp/after" >"$tmp/uncovered"
comm -13 "$tmp/corpus" "$tmp/after" >"$tmp/ghosts"
if [ -s "$tmp/uncovered" ] || [ -s "$tmp/ghosts" ]; then
  echo "error: composed reports do not cover exact corpus" >&2
  [ ! -s "$tmp/uncovered" ] || { echo "uncovered:" >&2; cat "$tmp/uncovered" >&2; }
  [ ! -s "$tmp/ghosts" ] || { echo "ghosts:" >&2; cat "$tmp/ghosts" >&2; }
  exit 1
fi

# Compare only real corpus stems. A stale ledger ghost is not an example that
# composition is allowed to preserve; the generated manifest removes it.
comm -12 "$tmp/before" "$tmp/corpus" >"$tmp/before_covered"
comm -13 "$tmp/corpus" "$tmp/before" >"$tmp/before_ghosts"
comm -23 "$tmp/before_covered" "$tmp/after" >"$tmp/coverage_missing"
comm -13 "$tmp/before_covered" "$tmp/after" >"$tmp/coverage_added"
before_count="$(wc -l <"$tmp/before_covered" | tr -d ' ')"
before_ghost_count="$(wc -l <"$tmp/before_ghosts" | tr -d ' ')"
after_count="$(wc -l <"$tmp/after" | tr -d ' ')"
before_rows="$(manifest_stems tests/jit_corpus_gate.txt | wc -l | tr -d ' ')"
after_rows="$(wc -l <"$tmp/sorted.tsv" | tr -d ' ')"
missing_count="$(wc -l <"$tmp/coverage_missing" | tr -d ' ')"
added_count="$(wc -l <"$tmp/coverage_added" | tr -d ' ')"
echo "corpus gate coverage: before=$before_count after=$after_count missing=$missing_count added=$added_count stale_ledger_ghosts=$before_ghost_count"
printf 'before_rows=%s\nafter_rows=%s\nbefore_stems=%s\nafter_stems=%s\nmissing_stems=%s\nadded_stems=%s\nstale_ledger_ghosts=%s\n' \
  "$before_rows" "$after_rows" "$before_count" "$after_count" "$missing_count" "$added_count" "$before_ghost_count" \
  >"$report_dir/coverage.txt"
cp "$tmp/before_covered" "$report_dir/coverage_before.txt"
cp "$tmp/after" "$report_dir/coverage_after.txt"
cp "$tmp/coverage_missing" "$report_dir/coverage_missing.txt"
cp "$tmp/coverage_added" "$report_dir/coverage_added.txt"
cp "$tmp/before_ghosts" "$report_dir/coverage_ghosts.txt"
if [ -s "$tmp/coverage_missing" ]; then
  echo "error: composed reports dropped stems already covered by the ledger" >&2
  cat "$tmp/coverage_missing" >&2
  exit 1
fi
# New stems are allowed: the exact live-corpus check above requires every
# current example to appear in the composed reports. Existing coverage may only
# stay or grow.

awk '/^frontend_rejected:$/ { exit } { print }' tests/jit_corpus_gate.txt >"$tmp/manifest"
for section in frontend_rejected gate_excluded non_runnable oracle_unavailable expected_exit aot_broken resident_jit deopt_interp run_tier_broken tier_divergent; do
  printf '%s:\n' "$section" >>"$tmp/manifest"
  awk -F'\t' -v want="$section" '$2 == want { print "  " $3 }' "$tmp/sorted.tsv" >>"$tmp/manifest"
  printf '\n' >>"$tmp/manifest"
done
cp "$tmp/manifest" "$report_dir/cases.txt"

trace="$report_dir/backend_trace.txt"
awk '/^#/' "$report_dir/shard-0/backend_trace.txt" >"$trace"
for ((shard = 0; shard < shard_count; shard++)); do
  awk '!/^#/' "$report_dir/shard-$shard/backend_trace.txt" >>"$trace"
done

if [ "${JET_WRITE_CORPUS_GATE:-0}" = "1" ]; then
  # Keep the canonical ledger whole if the process is interrupted mid-copy.
  ledger_tmp="$(mktemp tests/.jit_corpus_gate.txt.XXXXXX)"
  cp "$tmp/manifest" "$ledger_tmp"
  mv -f "$ledger_tmp" tests/jit_corpus_gate.txt
  echo "corpus gate ledger: wrote tests/jit_corpus_gate.txt"
fi
