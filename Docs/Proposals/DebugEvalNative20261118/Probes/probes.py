"""The shared helpers of run_all.py (revision 3) and run_r4.py (revision 4):
each prints a command and its exact output, machine paths removed. IR
listings and builds go to a temporary directory. A runner is called as
`python3 <runner> <jet> <repo root>`."""
import os
import re
import subprocess
import sys
import tempfile

jet, repo = sys.argv[1], sys.argv[2]
here = os.path.dirname(os.path.abspath(__file__))
scratch = tempfile.mkdtemp(prefix="debug-eval-probes-")
env = dict(os.environ, NO_COLOR="1", JET_COMPILER_IDENTITY="jet@0.1.0#probe", JET_TOOLCHAIN_ROOT=repo)
nice = ["taskset", "-c", "8-15", "nice", "-n", "19", "timeout", "600"]


def scrub(text):
    return text.replace(scratch + "/", "").replace(scratch, ".").replace(here + "/", "").replace(repo + "/", "").replace(repo, "<repo>")


def run(shown, argv, cwd=here, stdin=None, listing=False):
    print("$ " + shown)
    done = subprocess.run(nice + argv, cwd=cwd, env=env, input=stdin, capture_output=True, text=True)
    if listing:
        print("(an IR listing of {} lines; stderr: {!r})".format(done.stdout.count("\n"), scrub(done.stderr)))
    else:
        print(scrub(done.stdout + done.stderr).rstrip("\n"))
    print()
    return done.stdout


def jetrun(shown, args, cwd=here, stdin=None, listing=False):
    return run("jet " + shown, [jet] + args, cwd, stdin, listing)


def debug(name, line, commands, target=None, cwd=here):
    """`jet debug` with a breakpoint at `name`.jet:`line`, on the source or on
    a built image, the commands fed one per line."""
    program = target or name + ".jet"
    jetrun("debug --break {0}.jet:{1} {2}  <<< '{3}'".format(name, line, program, "; ".join(commands)),
           ["debug", "--break", "{}.jet:{}".format(name, line), program], cwd=cwd, stdin="\n".join(commands) + "\n")


def listing(name):
    """`jet emit --ir` of `name`.jet, saved in the scratch directory."""
    text = jetrun("emit --ir {0}.jet > {0}.ir".format(name), ["emit", "--ir", name + ".jet"], listing=True)
    path = os.path.join(scratch, name + ".ir")
    open(path, "w", encoding="utf-8").write(text)
    return text, path


def runtime_calls(listing, part):
    """Each function of an IR listing whose name holds `part`, and the runtime
    entries it calls."""
    current = None
    for line in listing.splitlines():
        head = re.match(r"^fn (\S+) ", line)
        if head:
            current = head.group(1) if part in head.group(1) else None
            continue
        target = re.search(r"call_runtime .*?(jet_\w+) \(", line)
        if current and target:
            print("  {} -> {}".format(current, target.group(1)))
    print()
