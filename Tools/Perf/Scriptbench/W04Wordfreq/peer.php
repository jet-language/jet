<?php
$counts = [];
$total = 0;
$fh = fopen("data/words.txt", "r");
while (($line = fgets($fh)) !== false) {
    preg_match_all('/[a-z]+/', strtolower($line), $m);
    foreach ($m[0] as $word) {
        $counts[$word] = ($counts[$word] ?? 0) + 1;
        $total++;
    }
}
uksort($counts, fn($a, $b) => $counts[$b] <=> $counts[$a] ?: strcmp($a, $b));
echo "total $total\n";
echo "distinct " . count($counts) . "\n";
foreach (array_slice($counts, 0, 10, true) as $word => $n) echo "$n $word\n";
