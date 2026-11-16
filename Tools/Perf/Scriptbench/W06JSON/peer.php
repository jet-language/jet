<?php
$people = json_decode(file_get_contents("w06.json"), true, 512, JSON_THROW_ON_ERROR);
$active = 0;
$salary = 0;
foreach ($people as $person) {
    if (!$person["active"]) continue;
    $active += 1;
    $salary += $person["salary"];
}
echo "records=" . count($people) . " active=$active salary=$salary\n";
