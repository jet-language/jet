#!/usr/bin/env bash
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
for workflow in OwnedSource ExternalPackage GuestModule DriverBuild; do
    bash "$root/$workflow/Run.sh"
done
