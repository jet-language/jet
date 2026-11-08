set -euo pipefail
counts=$(tr 'A-Z' 'a-z' < data/words.txt | grep -oE '[a-z]+' | sort | uniq -c)
echo "total $(awk '{ t += $1 } END { print t }' <<< "$counts")"
echo "distinct $(wc -l <<< "$counts")"
sort -k1,1nr -k2,2 <<< "$counts" | awk 'NR <= 10 { print $1 " " $2 }'
