#!/bin/sh
# Types the PTY answers the way a person does: the answers before the secret
# first, then, once the program is waiting at its secret prompt, the secret.
# A terminal echoes input when it arrives, so a secret written up front is
# echoed before any program could turn echo off.
binary=$1
{
    printf '\nnot-a-number\n3\n2\n'
    sleep 1
    printf 'secret\n'
} | script -qfec "exec '$binary'" /dev/null
