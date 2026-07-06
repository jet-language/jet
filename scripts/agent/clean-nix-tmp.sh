#!/usr/bin/env bash
# SessionStart: clear stale `nix develop` tmp dirs — they accumulate ~200M each
# and fill the tmpfs, causing phantom ENOSPC test failures.
rm -rf /tmp/nix-shell.* 2>/dev/null
use=$(df --output=pcent /tmp 2>/dev/null | tail -1 | tr -dc '0-9')
if [ -n "$use" ] && [ "$use" -ge 80 ]; then
  echo "WARNING: /tmp at ${use}% full — treat test failures as possible phantom ENOSPC; check df -h /tmp first."
fi
exit 0
