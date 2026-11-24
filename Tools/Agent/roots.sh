# roots.sh: the dev tool roots for shell tools; source it (`. Tools/Agent/roots.sh`).
# The table itself is Tools/Agent/roots.env, the one every repository tool
# reads: this file sources it, roots.mjs asks this file, and the Jet tools read
# it through Tools/Agent/Roots.jet. Repository tools find their state,
# scratch, queues, published compilers and seeds through these and nothing
# else. A machine sets its own values in the environment or in
# ${XDG_CONFIG_HOME:-~/.config}/jet/env, which this file reads first; without
# them the defaults derive from $HOME and the XDG directories, so a fresh clone
# needs no setup. No committed file names one machine's copy
# (the personal-information guard of Tools/Agent/LaneCheck.jet).
#
# The env file and roots.env hold assignments only, so the Jet tools read them
# with no shell (core.sys.env_file and env_text), as `.` sources them: each
# line is blank, a `#` comment, or commands that `;` separates, with an
# optional ` # comment` after them. A command is assignments (NAME=word, or
# NAME+=word to append) or `export` or `readonly` and names, each with an
# optional =word or +=word. A word is one shell word: '...' is literal; "..."
# expands and takes \" \\ \$ \` as escapes; a leading ~ (or one after a `:`)
# is $HOME; $NAME, ${NAME} and ${NAME[:]-word}, ${NAME[:]=word},
# ${NAME[:]+word} and ${NAME[:]?word} expand, the word included; $1 to $9 are
# unset. Nothing else: no other commands, $(...), backticks, ~user or other
# ${...} forms.
#
# JET_DEV_WORKTREE, the checkout the lander commits to, defaults to this
# checkout; a POSIX sh caller that sources this file sets it first. Every root
# is exported but JET_BIN, which `jet test` gives its own children.
#
# Run by bash rather than sourced (`bash Tools/Agent/roots.sh NAME...`), it
# prints `NAME=value` for each root named, and exits 64 at any other name.
# Its Jet twin is Tools/Agent/Roots.jet (gauntlet tool entries RootsSh and
# RootsMjs).

if [ -f "${XDG_CONFIG_HOME:-$HOME/.config}/jet/env" ]; then
  . "${XDG_CONFIG_HOME:-$HOME/.config}/jet/env"
fi
if [ -n "${BASH_VERSION:-}" ]; then
  eval 'jet_roots_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"'
  [ -n "${JET_DEV_WORKTREE:-}" ] || JET_DEV_WORKTREE="$(cd "$jet_roots_dir/../.." && pwd)"
else
  jet_roots_dir="$JET_DEV_WORKTREE/Tools/Agent"
fi
. "$jet_roots_dir/roots.env"
unset jet_roots_dir
export JET_STATE_ROOT JET_SCRATCH_ROOT JET_RUN_ROOT JET_BASE_ROOT JET_TARGET_ROOT JET_SEEDS JET_SEED JET_DEV_WORKTREE
if [ -n "${BASH_VERSION:-}" ]; then
  eval 'if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    for name in "$@"; do
      case "$name" in
        JET_STATE_ROOT|JET_SCRATCH_ROOT|JET_RUN_ROOT|JET_BASE_ROOT|JET_TARGET_ROOT|JET_SEEDS|JET_SEED|JET_BIN|JET_DEV_WORKTREE) printf "%s=%s\n" "$name" "${!name}" ;;
        *) echo "roots: unknown root $name" >&2; exit 64 ;;
      esac
    done
  fi'
fi
