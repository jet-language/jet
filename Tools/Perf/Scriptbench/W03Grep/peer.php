<?php
$count = 0;
$total = 0;
$fh = fopen("data/log100m.txt", "r");
while (($line = fgets($fh)) !== false) {
    if (preg_match('/status=5\d\d ms=(\d+)/', $line, $m)) {
        $count++;
        $total += (int)$m[1];
    }
}
echo "matches $count\n";
echo "ms_sum $total\n";
