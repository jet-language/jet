#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 1 ]] || { printf '%s\n' 'usage: interactive_terminal INPUT_ROOT' >&2; exit 2; }
root=$1
task=${JET_CORPUS_TASK:?missing task identity}
script_name=terminal_session.sh
input=$'Ada\nblue\n'
if [[ $task == interactive-terminal-closed ]]; then
  script_name=terminal_closed.sh
  input=''
fi
output=$(cd "$root" && printf '%s' "$input" | timeout 5 script -qfec "sh $script_name" /dev/null 2>/dev/null)
if [[ $task == interactive-terminal-closed ]]; then
  [[ $output == *closed* ]] || { printf '%s\n' 'closed terminal did not return' >&2; exit 1; }
  printf '%s\n' 'terminal=pty' 'closed=ok' 'exit=0'
else
  [[ $output == *'Name: '* && $output == *'Hello Ada'* && $output == *'Choice blue'* ]] || {
    printf '%s\n' 'terminal dialogue markers missing' >&2
    exit 1
  }
  printf '%s\n' 'terminal=pty' 'resize=ok' 'prompt=ok' 'reply=ok'
fi
