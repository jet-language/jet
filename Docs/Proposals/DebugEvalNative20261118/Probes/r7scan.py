"""Probe G13/N6: classify each lambda of a `jet emit --ir` listing by what R7's
in-program rule must refuse. A lambda is placeable when every function it
reaches (its own body and the lambdas and direct callees it calls) makes no
runtime call other than the result's allocation (`jet_rt_alloc_value*`, which
the eval link points at an arena the debugger maps) and the runtime's
readers, no atomic store, no retain or release, and no store except into a
block it allocated itself. READERS stands in for §5's runtime test, which
proves a runtime function writes no memory; here it is the two this listing
reaches, as revision 3 assumed. Revision 4 passes the entry points
linkchecks.py marks "writes no memory" instead (`--readers a,b`): jet_char_len
is not one, as it fills its string's count cache word.
Usage: python3 r7scan.py <listing.ir> [--readers a,b]"""
import re
import sys

ALLOC = re.compile(r"^jet_rt_alloc_value")
functions = {}
READERS = set(sys.argv[3].split(",")) if sys.argv[2:3] == ["--readers"] else {"jet_char_len", "jet_rt_atomic_bool_load"}
order = []
current = None
for line in open(sys.argv[1], encoding="utf-8"):
    head = re.match(r"^fn (\S+) \[function:[^\]]*::([^\]]+)\]", line)
    if head:
        current = {"name": head.group(1), "id": head.group(2), "runtime": [], "calls": [], "stores": [], "allocs": set()}
        functions[head.group(2)] = current
        order.append(current)
        continue
    if current is None:
        continue
    op = re.match(r"^\s+\d+: (\S+) (.*)$", line)
    if not op:
        continue
    kind, rest = op.groups()
    if kind == "call_runtime":
        target = re.search(r"(?:<- )?(jet_\w+) \(", rest)
        name = target.group(1) if target else rest
        current["runtime"].append(name)
        result = re.match(r"s(\d+) <-", rest)
        if result and ALLOC.match(name):
            current["allocs"].add(result.group(1))
    elif kind in ("call", "call_indirect", "call_direct"):
        current["calls"].append(rest)
    elif kind == "store":
        current["stores"].append(rest.split()[0])
    elif "atomic" in kind:
        current["runtime"].append(kind)


def reasons(fn, seen):
    if fn["id"] in seen:
        return []
    seen.add(fn["id"])
    found = []
    for name in fn["runtime"]:
        if not ALLOC.match(name) and name not in READERS:
            found.append("{}: runtime {}".format(fn["name"], name))
    for base in fn["stores"]:
        if base not in fn["allocs"]:
            found.append("{}: store through s{}".format(fn["name"], base))
    for call in fn["calls"]:
        callee = re.search(r"::([^\]\s]+)\]?", call)
        target = functions.get(callee.group(1)) if callee else None
        if target is None:
            found.append("{}: call {}".format(fn["name"], call.strip()[:60]))
        else:
            found.extend(reasons(target, seen))
    return found


for fn in order:
    if ".lambda@" not in fn["name"] or fn["name"].count("lambda@") > 1:
        continue
    why = reasons(fn, set())
    verdict = "placeable" if not why else "fork per hit"
    print("{} {}: {}".format(fn["name"], verdict, "; ".join(sorted(set(why))) or "allocates its result only"))
