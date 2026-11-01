#!/bin/sh
# rs:1728-1734,1759-1760,1774: one payload per quota fixture.
set -eu
mkdir -p quota-source-oldest quota-source-newer quota-source-admitted
awk 'BEGIN { for (i = 0; i < 4194304; i++) printf "a" }' > quota-source-oldest/payload
awk 'BEGIN { for (i = 0; i < 4194304; i++) printf "b" }' > quota-source-newer/payload
awk 'BEGIN { for (i = 0; i < 65536; i++) printf "c" }' > quota-source-admitted/payload
