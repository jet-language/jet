"""Run every revision 4 probe (P1-P8, after review 3) in order and print each
command with its exact output, machine paths removed (probes.py). Builds and
IR listings go to a temporary directory.
Usage: python3 run_r4.py <jet> <repo root>"""
import hashlib
import os
import re
import shutil

from probes import debug, jetrun, listing, repo, run, scratch


def image_digest(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()[:16]


print("P1: two takes of one method in one statement")
debug("chain", 9, ["f", "locals", "c", "f", "locals", "c"])
jetrun("check chain_later.jet", ["check", "chain_later.jet"])

print("P2: a take at O2, where the callee is inlined")
shutil.copy("take_arg.jet", scratch)
jetrun("build -O2 take_arg.jet  (in the scratch directory)", ["build", "-O2", "take_arg.jet"], cwd=scratch)
debug("take_arg", 5, ["bt", "locals", "f", "locals", "c"], target=".jet/build/take_arg", cwd=scratch)

print("P3, P5, P7 and P4: the link checks over the runtime, and the entry points journey.jet's peeks reach")
_, journey = listing("journey")
jetrun("run journey.jet", ["run", "journey.jet"])
run("python3 linkchecks.py <repo> journey.ir 'peek|parse_twice|add_parsed'",
    ["python3", "linkchecks.py", repo, journey, "peek|parse_twice|add_parsed"])
_, r7 = listing("r7_scan")
run("python3 r7scan.py r7_scan.ir --readers jet_rt_atomic_bool_load",
    ["python3", "r7scan.py", r7, "--readers", "jet_rt_atomic_bool_load"])

print("P6: one source, two images")
shutil.copy("bounds.jet", scratch)
print("source sha256: {}".format(hashlib.sha256(open("bounds.jet", "rb").read()).hexdigest()[:16]))
print()
for flags in ([], ["--bounds=report"]):
    shown = " ".join(["build"] + flags + ["bounds.jet"])
    jetrun(shown + "  (in the scratch directory)", ["build"] + flags + ["bounds.jet"], cwd=scratch)
    print("image sha256: {}".format(image_digest(os.path.join(scratch, ".jet/build/bounds"))))
    print()
    run(".jet/build/bounds  (built with {})".format(" ".join(flags) or "the stock options"), [os.path.join(scratch, ".jet/build/bounds")], cwd=scratch)

print("P8: two instances of one generic module, and a top-level Shared global")
jetrun("run genmod.jet", ["run", "genmod.jet"])
debug("genmod", 6, ["bt", "c"])
debug("genmod", 16, ["s", "bt", "c"])
text, _ = listing("genmod")
print("the instance functions of genmod.ir, and the body of one:")
body = False
for line in text.splitlines():
    head = re.match(r"^fn (\S+) ", line)
    if head:
        body = head.group(1) == "__jet_small_dbump"
        if "bump" in head.group(1):
            print("  " + line.split(" params")[0])
        continue
    if body:
        print("    " + line.strip())
    elif "#storage" in line:
        print("  run: " + line.strip())
print()
jetrun("check genmod_mutable.jet", ["check", "genmod_mutable.jet"])
print("the control: a breakpoint on a one-line function outside a generic module")
debug("bounds", 4, ["bt", "c", "c"])
