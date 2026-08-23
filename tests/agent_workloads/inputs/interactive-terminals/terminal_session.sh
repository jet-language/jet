#!/bin/sh
printf 'Name: '
IFS= read -r name || exit 3
printf 'Hello %s\n' "$name"
printf 'Choice: '
IFS= read -r choice || exit 3
printf 'Choice %s\n' "$choice"
