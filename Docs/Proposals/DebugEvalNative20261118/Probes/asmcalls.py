"""Probe N4: every instruction that calls a register in the backend's OS
assembly, the function it sits in, and the runtime files that call that
function. Usage: python3 asmcalls.py <repo root>"""
import os
import re
import sys

root = sys.argv[1]
linux = os.path.join(root, "Compiler/JetBackend/Source/OS/Linux.jet")
head = re.compile(r"^([a-z_0-9]+): fn\b")
call = re.compile(r"^\s+call (r[a-z0-9]+|e[a-z]x)\b")
owners = {}
current = None
for number, line in enumerate(open(linux, encoding="utf-8"), 1):
    found = head.match(line)
    if found:
        current = found.group(1)
    if call.match(line) and current:
        owners.setdefault(current, []).append(number)
runtime = os.path.join(root, "Compiler/JetRuntime/Source")
users = {name: [] for name in owners}
for name in sorted(os.listdir(runtime)):
    if not name.endswith(".jet"):
        continue
    for number, line in enumerate(open(os.path.join(runtime, name), encoding="utf-8"), 1):
        for primitive in owners:
            if re.search(r"\b{}\(".format(primitive), line) and not line.lstrip().startswith("//"):
                users[primitive].append("{}:{}".format(name, number))
print("asm functions with a register call: {}".format(len(owners)))
for primitive in sorted(owners):
    print("  {} (Linux.jet:{}): {}".format(primitive, ",".join(map(str, owners[primitive])), ", ".join(users[primitive]) or "no runtime caller"))
