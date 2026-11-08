<?php
$total = 0;
for ($i = 1; $i <= 30000000; $i++) $total += $i * $i % 7919;
echo "$total\n";
