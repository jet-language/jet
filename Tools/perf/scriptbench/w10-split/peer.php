<?php
$lines = [];
for ($i = 0; $i < 20000; $i++) $lines[] = "alpha beta gamma delta epsilon $i";
$text = implode("\n", $lines);
$count = 0;
$long = 0;
$bytes = 0;
foreach (explode("\n", $text) as $line) {
    foreach (explode(" ", $line) as $word) {
        $count += 1;
        $bytes += strlen($word);
        if (strlen($word) > 4) $long += 1;
    }
}
echo "words=$count long=$long bytes=$bytes\n";
