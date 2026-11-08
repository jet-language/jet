set -euo pipefail
total=0
for ((i = 1; i <= 30000000; i++)); do total=$((total + i * i % 7919)); done
echo "$total"
