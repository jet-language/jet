<?php
$files = 0;
$bytes = 0;
$it = new RecursiveIteratorIterator(new RecursiveDirectoryIterator("w02", FilesystemIterator::SKIP_DOTS));
foreach ($it as $entry) {
    if (!$entry->isFile()) continue;
    $files += 1;
    $bytes += $entry->getSize();
}
echo "files=$files bytes=$bytes\n";
