#!/bin/sh
# rs:1498-1502: the outside bytes survive; drop the escape link.
set -eu
test "$(cat outside/must-survive)" = "live data"
rm root/leases
