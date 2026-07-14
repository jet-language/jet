<?php
$counter = 0;

function transform(array $input): array {
    global $counter;
    $counter += 1;
    return [
        'count' => $counter,
        'nested' => $input['nested'],
        'list' => $input['list'],
        'scalar' => $input['scalar'],
        'nothing' => null,
    ];
}

function fail_call($input) {
    throw new RuntimeException('raw secret failure detail');
}

function sleep_call($input) {
    sleep(30);
    return $input;
}

function pooled_sleep($input) {
    usleep(1000000);
    return $input;
}
