"""Probe N4: which runtime functions call a code address they load, and which
exported entry points reach one through runtime calls. The runtime calls a
loaded address only through the backend's `coll_callN` stubs, so a function
calls one when its body names `coll_callN(`. Calls between runtime functions
are found by name. Usage: python3 rtindirect.py <Compiler/JetRuntime/Source>"""
import os
import re
import sys

root = sys.argv[1]
head = re.compile(r"^(?:#[A-Za-z]+(?:\([^)]*\))?\s+)*([a-z_][a-z0-9_]*): fn\b")
exported = set()
bodies = {}
files = {}
for name in sorted(os.listdir(root)):
    if not name.endswith(".jet"):
        continue
    current = None
    for line in open(os.path.join(root, name), encoding="utf-8"):
        found = head.match(line)
        if found:
            current = found.group(1)
            bodies[current] = []
            files[current] = name
            if line.startswith("#Export"):
                exported.add(current)
        if current is not None:
            bodies[current].append(line)
        if line.startswith("}"):
            current = None

indirect = re.compile(r"\bcoll_call[0-9]+\(")
token = re.compile(r"\b([a-z_][a-z0-9_]*)\(")
direct = {name for name, lines in bodies.items() if any(indirect.search(line) for line in lines)}
callees = {name: {t for line in lines for t in token.findall(line) if t in bodies and t != name} for name, lines in bodies.items()}

reaches = set(direct)
changed = True
while changed:
    changed = False
    for name, called in callees.items():
        if name not in reaches and called & reaches:
            reaches.add(name)
            changed = True

print("runtime functions: {}".format(len(bodies)))
print("call a loaded code address (coll_callN): {}".format(len(direct)))
by_file = {}
for name in sorted(direct):
    by_file.setdefault(files[name], []).append(name)
for name in sorted(by_file):
    print("  {}: {}".format(name, ", ".join(by_file[name])))
print("reach one through runtime calls: {}".format(len(reaches)))
print("exported entry points that reach one: {} of {}".format(len(reaches & exported), len(exported)))
