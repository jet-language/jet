#!/bin/sh
# rs:1476-1477: both reclaimed. rs:1481-1484: the archive-stage root becomes a symlink to outside.
set -eu
test ! -e root/hangar/.archive-stage/abandoned
test ! -e root/leases/4294967295-0-abandoned
rm -rf root/hangar/.archive-stage
ln -s "$PWD/outside" root/hangar/.archive-stage
