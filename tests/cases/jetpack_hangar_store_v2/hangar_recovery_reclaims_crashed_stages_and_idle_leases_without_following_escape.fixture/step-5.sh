#!/bin/sh
# rs:1487-1491: the outside bytes survive. rs:1492-1496: the lease root becomes a symlink to outside.
set -eu
test "$(cat outside/must-survive)" = "live data"
rm root/hangar/.archive-stage
rm -rf root/leases
ln -s "$PWD/outside" root/leases
