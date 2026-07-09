#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-real-jetos.sh --host <name> --disk <path> [--config <path>] [--dry-run]

Runs the real JetOS replacement gate:
  jet os vm prove <host> --disk <path> --real

This script does not create fake tools, does not accept harness-only proof, and
does not mark replacement acceptance. It exits non-zero unless the real guest
proof command succeeds. --dry-run only prints the command; it is not proof.
EOF
}

host=""
disk=""
config=""
dry_run=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --host)
      host="${2:-}"
      shift 2
      ;;
    --disk)
      disk="${2:-}"
      shift 2
      ;;
    --config)
      config="${2:-}"
      shift 2
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$host" || -z "$disk" ]]; then
  echo "error: --host and --disk are required for real JetOS proof" >&2
  usage >&2
  exit 2
fi

cmd=(jet os vm prove "$host" --disk "$disk" --real)
if [[ -n "$config" ]]; then
  cmd+=(--config "$config")
fi

printf 'real JetOS proof command:'
printf ' %q' "${cmd[@]}"
printf '\n'

if [[ "$dry_run" -eq 1 ]]; then
  echo "dry-run only: no JetOS replacement proof was executed" >&2
  exit 0
fi

"${cmd[@]}"
