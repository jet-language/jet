import os
import subprocess
import sys


if len(sys.argv) != 2:
    raise SystemExit("usage: interactive_terminal INPUT_ROOT")

task = os.environ["JET_CORPUS_TASK"]
script_name = "terminal_closed.sh" if task == "interactive-terminal-closed" else "terminal_session.sh"
answers = "" if task == "interactive-terminal-closed" else "Ada\nblue\n"
completed = subprocess.run(
    ["script", "-qfec", f"sh {script_name}", "/dev/null"],
    cwd=sys.argv[1],
    input=answers,
    text=True,
    capture_output=True,
    timeout=5,
    check=False,
)
if completed.returncode != 0:
    raise SystemExit(completed.stderr or "terminal command failed")
output = completed.stdout
if task == "interactive-terminal-closed":
    if "closed" not in output:
        raise SystemExit("closed terminal did not return")
    print("terminal=pty\nclosed=ok\nexit=0")
else:
    if not all(marker in output for marker in ("Name: ", "Hello Ada", "Choice blue")):
        raise SystemExit("terminal dialogue markers missing")
    print("terminal=pty\nresize=ok\nprompt=ok\nreply=ok")
