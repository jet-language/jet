<?php
$s = "";
for ($i = 0; $i < 2000000; $i++) $s .= "abc,";
echo "len=" . strlen($s) . "\n";
