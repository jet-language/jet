<?php
$total = 0;
for ($i = 0; $i < 200; $i++) {
    $total += strlen(file_get_contents("http://127.0.0.1:18080/item/$i"));
}
echo "requests 200\n";
echo "bytes $total\n";
