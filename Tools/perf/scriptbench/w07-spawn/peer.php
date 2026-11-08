<?php
$ok = 0;
for ($i = 0; $i < 100; $i++) {
    $process = proc_open(["true"], [], $pipes);
    if (proc_close($process) === 0) $ok += 1;
}
echo "children ok=$ok\n";
