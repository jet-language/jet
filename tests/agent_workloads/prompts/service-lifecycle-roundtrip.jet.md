# Task: service-lifecycle-roundtrip

Domain: service-lifecycle
Case: up-health-wait-logs-down
Required outcome: exit=0;stdout=exact

## Input

Your program is given the path to an input directory as its first argument.
Its working directory is a scratch directory you may write to freely; the
input directory itself must be left unchanged.

The input directory contains exactly this, with small files shown inline:

    bin/
    bin/systemctl  (786 bytes)
      | #!/bin/sh
      | set -eu
      | state=${JETPACK_FAKE_SYSTEMD_STATE:-$(dirname "$0")/../state}
      | signal=TERM
      | unit=
      | operation=kill
      | for arg in "$@"; do
      |     case "$arg" in
      |         is-active) operation=active ;;
      |         --signal=*) signal=${arg#--signal=} ;;
      |         *.scope) unit=$arg ;;
      |     esac
      | done
      | [ -n "$unit" ]
      | pid=$(cat "$state/$unit.pid")
      | if [ "$operation" = active ]; then
      |     for stat in /proc/[0-9]*/stat; do
      |         [ -r "$stat" ] || continue
      |         line=$(cat "$stat") || continue
      |         rest=${line##*)}
      |         set -- $rest
      |         state_code=$1
      |         process_group=$3
      |         if [ "$process_group" = "$pid" ] && [ "$state_code" != Z ]; then
      |             exit 0
      |         fi
      |     done
      |     exit 3
      | fi
      | kill "-$signal" -- "-$pid" 2>/dev/null || kill "-$signal" "$pid" 2>/dev/null || true
      | exit 0
      | 
    bin/systemd-run  (980 bytes)
      | #!/bin/sh
      | set -eu
      | state=${JETPACK_FAKE_SYSTEMD_STATE:-$(dirname "$0")/../state}
      | unit=
      | workdir=
      | saw_user=0
      | saw_scope=0
      | saw_collect=0
      | saw_quiet=0
      | saw_delegate=0
      | saw_kill_mode=0
      | while [ "$#" -gt 0 ]; do
      |     case "$1" in
      |         --user) saw_user=1 ;;
      |         --scope) saw_scope=1 ;;
      |         --collect) saw_collect=1 ;;
      |         --quiet) saw_quiet=1 ;;
      |         --property=Delegate=yes) saw_delegate=1 ;;
      |         --property=KillMode=control-group) saw_kill_mode=1 ;;
      |         --unit=*) unit=${1#--unit=} ;;
      |         --working-directory=*) workdir=${1#--working-directory=} ;;
      |         --setenv=*) export "${1#--setenv=}" ;;
      |         --unsetenv=*) unset "${1#--unsetenv=}" ;;
      |         --) shift; break ;;
      |     esac
      |     shift
      | done
      | [ -n "$unit" ]
      | [ "$saw_user" -eq 1 ]
      | [ "$saw_scope" -eq 1 ]
      | [ "$saw_collect" -eq 1 ]
      | [ "$saw_quiet" -eq 1 ]
      | [ "$saw_delegate" -eq 1 ]
      | [ "$saw_kill_mode" -eq 1 ]
      | mkdir -p "$state"
      | printf '%s\n' "$$" > "$state/$unit.pid"
      | [ -z "$workdir" ] || cd "$workdir"
      | exec "$@"
      | 
    env.jet  (331 bytes)
      | module env.dev {
      |     services: {
      |         fixture: {
      |             enable: true,
      |             run: ["sh", "-c", "echo service-started; sleep 30"]
      |         },
      |         timeout: {
      |             enable: true,
      |             run: ["sh", "-c", "sleep 30 & child=$!; echo $child > child.pid; wait"],
      |             ready: "sleep 30"
      |         }
      |     }
      | }
      | 
    run.jet  (12 bytes)
      | fn run() {}
      | 

## Required output

Write to standard output EXACTLY the following bytes, and exit with status 0.
Trailing newline matters. Do not print anything else -- no logging, no
progress, no banner.

----- BEGIN EXPECTED STDOUT -----
service=ready
authority=linux-systemd-user
containment=delegated-cgroup
receipt=health-lifecycle
cleanup=ok
----- END EXPECTED STDOUT -----

## Rules

- Read the input from the directory path given as the first argument.
- Do not hardcode the expected output as a literal string. Compute it from the
  input. A submission that prints a baked-in constant is a failure even though
  the bytes match.
- Do not write into the input directory. Scratch files must go in the working
  directory and must be cleaned up before exit.
- No network access.

## Language

Write your solution in Jet, as a single file named `candidate.jet`.

Check that it compiles or parses with:

    scripts/agent/jet-env jet check candidate.jet

Fix every error the checker reports and check again. When the checker is
clean, stop and report. Do not run the program against the expected output --
you are not given a way to compare, and guessing from the output is not part
of this task.

## Report format

Your final message must be exactly these lines and nothing else:

ROUNDS: <number of edits you made after the first version; 0 if the first version checked clean>
CLEAN: <yes|no>
DIAGNOSTICS: <total count of distinct checker errors you saw across all rounds>
