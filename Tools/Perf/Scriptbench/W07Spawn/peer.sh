ok=0
for _ in $(seq 100); do
  if /usr/bin/env true; then ok=$((ok + 1)); fi
done
echo "children ok=$ok"
