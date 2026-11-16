set -euo pipefail
total=0
for i in $(seq 0 199); do
  bytes=$(curl -sf -o /dev/null -w '%{size_download}' "http://127.0.0.1:18080/item/$i")
  total=$((total + bytes))
done
echo "requests 200"
echo "bytes $total"
