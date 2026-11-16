<?php
$seen = [];
$handle = fopen("w13.txt", "r");
while (($line = fgets($handle)) !== false) {
    $seen[rtrim($line, "\n")] = true;
}
echo "unique=" . count($seen) . "\n";
