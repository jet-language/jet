#!/usr/bin/env bash
# Criterion 4: one scheduling meaning on AOT, jet run (JIT), and the interpreter.
cd /home/nate/Projects/Github/jet || exit 1
pass=0; fail=0
for ex in examples/features/concurrency/*.jet; do
  stem=$(basename "$ex" .jet)
  gold="examples/features/expected/concurrency/$stem.out"
  [ -f "$gold" ] || { echo "SKIP  $stem (no stdout golden)"; continue; }
  jit=$(timeout 120 ./target/debug/jet run "$ex" 2>/dev/null)
  aot=$(timeout 300 ./target/debug/jet run --release "$ex" 2>/dev/null)
  ipt=$(timeout 120 ./target/debug/jet dev --interpret --watch=off "$ex" 2>/dev/null)
  want=$(cat "$gold")
  bad=""
  [ "$jit" = "$want" ] || bad="$bad jit"
  [ "$aot" = "$want" ] || bad="$bad aot"
  [ "$ipt" = "$want" ] || bad="$bad interp"
  if [ -z "$bad" ]; then echo "OK    $stem"; pass=$((pass+1));
  else echo "DIFF  $stem ->$bad"; fail=$((fail+1)); fi
done
echo "pass=$pass fail=$fail"
