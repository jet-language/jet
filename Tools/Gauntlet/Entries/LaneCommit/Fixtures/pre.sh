#!/usr/bin/env bash
# The stand-in precommit: lists the paths it checks and fails when one of them is bad.
echo "precommit: $*"
rc=0
for p in "$@"; do case $p in *bad*) echo "precommit: $p is bad"; rc=1;; esac; done
exit $rc
