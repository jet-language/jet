<?php
$command = $argv[1] ?? "";
$opts = [];
for ($i = 2; $i < $argc; $i++) {
    $key = substr($argv[$i], 2);
    if ($i + 1 < $argc && !str_starts_with($argv[$i + 1], "--")) $opts[$key] = $argv[++$i];
    else $opts[$key] = true;
}
if ($command === "greet") {
    if (!isset($opts["name"])) exit("--name is required\n");
    $text = "Hello, {$opts["name"]}!";
    for ($i = 0; $i < (int)($opts["times"] ?? 1); $i++) echo (isset($opts["shout"]) ? strtoupper($text) : $text), "\n";
} elseif ($command === "add") {
    echo (int)$opts["a"] + (int)$opts["b"], "\n";
} else {
    fwrite(STDERR, "usage: main <greet|add> [options]\n");
    exit(2);
}
