set -euo pipefail
grep -oE 'status=5[0-9][0-9] ms=[0-9]+' data/log100m.txt | awk -F'ms=' '
  { count++; total += $2 }
  END { print "matches " count; print "ms_sum " total }'
