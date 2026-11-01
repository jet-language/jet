#!/bin/sh
# Two sibling Hangar roots and one shared CAS directory (tests/jetpack_criterion4.rs:39-44).
set -eu
mkdir -p jet-omp/hangar/objects/one jet-omp2/hangar/objects/two shared-cas
awk 'BEGIN { for (i = 0; i < 4096; i++) printf "a" }' > jet-omp/hangar/objects/one/payload
awk 'BEGIN { for (i = 0; i < 8192; i++) printf "b" }' > jet-omp2/hangar/objects/two/payload
awk 'BEGIN { for (i = 0; i < 2048; i++) printf "c" }' > shared-cas/shared-payload
