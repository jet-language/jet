#!/usr/bin/env bash
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
for workflow in owned-source external-package guest-module driver-build; do
    bash "$root/$workflow/run.sh"
done
