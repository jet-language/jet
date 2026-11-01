#!/bin/sh
# rs:1468-1473: an abandoned archive stage and a dead lease (pid 4294967295), plus outside bytes for the escape checks.
set -eu
mkdir -p root/hangar/.archive-stage/abandoned root/leases/4294967295-0-abandoned outside
printf 'partial archive' > root/hangar/.archive-stage/abandoned/payload
printf 'lease snapshot' > root/leases/4294967295-0-abandoned/payload
printf 'live data' > outside/must-survive
