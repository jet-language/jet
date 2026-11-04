#!/usr/bin/env bash
# personal-info-guard.sh [DIR]: refuse personal or machine information in a checkout (#6564 c5).
# Committed files name no home path, user name, host name, personal email or
# machine mount. Write a neutral placeholder instead: ~ or $HOME, $USER,
# $JET_SCRATCH_ROOT, $JET_STATE_ROOT, or an example account such as `user` on
# `devhost`. There is no allowlist: recorded evidence, the Tower board
# included, is redacted the same way (`tower redact`).
#
# The patterns come from the machine running the check, through the rule table
# Tower uses (Tools/tower/app/personal-info.mjs), so neither file holds personal
# data and a leak is caught on the machine that would write it: the home
# directory, /home/$USER, /Users/$USER, /etc/profiles/per-user/$USER,
# $JET_SCRATCH_ROOT and every mount under /mnt, /media and /run/media, git's
# user.email, and, as whole words in any case, git's user.name, the GitHub
# handle in a noreply email, the login name and the host name (generic names
# such as `runner` are skipped).
# Every file in DIR (default: this checkout) is searched except .git, paths the
# checkout's .gitignore excludes, and chain build state (.chain*).
# Exit 0 when clean, 65 with the offending files when not, 64 when it cannot run.
set -uo pipefail

rules_module="$(cd "$(dirname "$0")/../tower/app" && pwd)/personal-info.mjs"
dir="${1:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$dir" || exit 64
command -v git >/dev/null 2>&1 || { echo "personal-info-guard: git is required" >&2; exit 64; }
table="$(node "$rules_module" patterns)" || { echo "personal-info-guard: cannot read the rule table from $rules_module" >&2; exit 64; }

path_args=()
word_args=()
while IFS=$'\t' read -r kind source; do
  case "$kind" in
    path) path_args+=(-e "$source") ;;
    word) word_args+=(-e "$source") ;;
  esac
done <<< "$table"

# Pass 1 lists files (-l stays fast on multi-megabyte lines); pass 2 shows the
# first hit in each of the first 20 files. git grep exits 1 for no match; any
# other status means the check did not run.
files=""
search() {
  local out rc
  out="$(git grep --no-index --exclude-standard -I -P "$@" -- . ':!.git' ':!.chain' ':!.chain-base' ':!.chain-lock')"
  rc=$?
  if [ "$rc" -gt 1 ]; then echo "personal-info-guard: git grep failed (rc=$rc)" >&2; exit 64; fi
  files+="${files:+$'\n'}$out"
}
[ "${#path_args[@]}" -eq 0 ] || search -l "${path_args[@]}"
[ "${#word_args[@]}" -eq 0 ] || search -l -i "${word_args[@]}"
files="$(printf '%s\n' "$files" | awk 'NF && !seen[$0]++')"
[ -z "$files" ] && exit 0

count="$(printf '%s\n' "$files" | awk 'END { print NR }')"
echo "personal-info-guard: $count file(s) name this machine's home, user, host, email or mounts." >&2
echo "Write ~ or \$HOME, \$USER, \$JET_SCRATCH_ROOT, \`user\` or \`devhost\` instead (#6564 c5). First hits:" >&2
printf '%s\n' "$files" | awk 'NR <= 20' | while IFS= read -r file; do
  line=""
  [ "${#path_args[@]}" -gt 0 ] && line="$(git grep --no-index -I -P -n -m 1 "${path_args[@]}" -- "$file" 2>/dev/null | awk 'NR == 1')"
  [ -z "$line" ] && [ "${#word_args[@]}" -gt 0 ] && line="$(git grep --no-index -I -P -n -m 1 -i "${word_args[@]}" -- "$file" 2>/dev/null | awk 'NR == 1')"
  echo "  ${line:0:200}" >&2
done
exit 65
