"""Run every revision 3 probe in order and print each command with its exact
output, machine paths removed (probes.py). IR listings go to a temporary
directory. Usage: python3 run_all.py <jet> <repo root>"""
import os

from probes import debug, here, jetrun, repo, run, runtime_calls, scratch

debug("place_moved", 11, ["locals", "print xs", "print box", "c"])
debug("take_arg", 5, ["locals", "f", "locals", "c"])
for name in ("take_arg_later", "moved_read"):
    jetrun("check {}.jet".format(name), ["check", name + ".jet"])
debug("move_then_share", 10, ["locals", "c"])
for name in ("row_compose", "unsafe_callee", "row_forms3", "fx_forms", "fx_line", "shared_denied"):
    jetrun("check {}.jet".format(name), ["check", name + ".jet"])
jetrun("check unsafe_reach.jet", ["check", "unsafe_reach.jet"])
jetrun("run unsafe_reach.jet", ["run", "unsafe_reach.jet"])
listing = jetrun("emit --ir wait_link.jet > wait_link.ir", ["emit", "--ir", "wait_link.jet"], listing=True)
open(os.path.join(scratch, "wait_link.ir"), "w", encoding="utf-8").write(listing)
print("runtime calls of the peek functions in wait_link.ir:")
runtime_calls(listing, "peek")
jetrun("run wait_link.jet", ["run", "wait_link.jet"])
listing = jetrun("emit --ir r7_scan.jet > r7_scan.ir", ["emit", "--ir", "r7_scan.jet"], listing=True)
open(os.path.join(scratch, "r7_scan.ir"), "w", encoding="utf-8").write(listing)
run("python3 r7scan.py r7_scan.ir", ["python3", "r7scan.py", os.path.join(scratch, "r7_scan.ir")])
jetrun("run r7_scan.jet", ["run", "r7_scan.jet"])
run("python3 asmcalls.py <repo>", ["python3", "asmcalls.py", repo])
run("python3 rtindirect.py <repo>/Compiler/JetRuntime/Source", ["python3", "rtindirect.py", os.path.join(repo, "Compiler/JetRuntime/Source")])
inputs = os.path.join(here, "Inputs")
jetrun("build main.jet  (in Inputs/)", ["build", "main.jet"], cwd=inputs)
jetrun("run main.jet  (in Inputs/)", ["run", "main.jet"], cwd=inputs)
run("python3 files.py .jet/build/main  (in Inputs/)", ["python3", "files.py", ".jet/build/main"], cwd=inputs)
sleep = os.path.join(here, "Sleep")
jetrun("build sleeper.jet  (in Sleep/)", ["build", "sleeper.jet"], cwd=sleep)
run("python3 ../tcb.py .jet/build/sleeper  (in Sleep/)", ["python3", "../tcb.py", ".jet/build/sleeper"], cwd=sleep)
run("python3 seccomp_args.py", ["python3", "seccomp_args.py"])
run("python3 dontfork.py", ["python3", "dontfork.py"])
run("python3 globals.py <repo>/Compiler <repo>/Core", ["python3", "globals.py", os.path.join(repo, "Compiler"), os.path.join(repo, "Core")])
for attempt in range(3):
    run("python3 fork_cost.py 64 512 2048  (run {} of 3)".format(attempt + 1), ["python3", "fork_cost.py", "64", "512", "2048"])
