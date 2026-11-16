<?php
$count = 0;
$qty = 0;
$value = 0;
$handle = fopen("w05.csv", "r");
fgetcsv($handle, null, ",", "\"", "");
while (($row = fgetcsv($handle, null, ",", "\"", "")) !== false) {
    $q = (int)$row[3];
    $count += 1;
    $qty += $q;
    $value += $q * (int)$row[4];
}
echo "rows=$count qty=$qty value=$value\n";
