#!/bin/sh
# measured_bytes (tests/jetpack_criterion4.rs:16-30): st_blocks * 512 per inode, each inode once, over
# shared-cas, jet-omp/hangar and jet-omp2/hangar. `du -scB1` counts the same across its arguments.
set -eu
du -scB1 shared-cas jet-omp/hangar jet-omp2/hangar | awk 'END { print $1 }'
