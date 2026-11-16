#!/usr/bin/env sh
set -eu

output=${1:?output path is required}
printf '%s\n' 'native action output' >"$output"
