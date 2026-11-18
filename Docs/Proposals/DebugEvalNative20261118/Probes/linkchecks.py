"""Probes P3, P4, P5 and P7 (revision 4): §5's eval link checks, as revision 4
states them, run over the runtime's source and the backend's OS assembly.

Each runtime function and each asm body gets direct facts; a function has a
fact when it or any runtime function or asm body it calls has it:

- state: it names a runtime module global, or an asm body it calls reads or
  writes through `gs:` (the TCB);
- loaded: it names `coll_callN(`, or calls an asm body that transfers to an
  address not fixed at assembly (P5's forms);
- blocks: a `futex` call whose operation is a FUTEX_WAIT form, or any syscall
  outside the fork's §6.2 allow-list;
- writes: it calls an asm body that stores to memory.

Calls are found by name, as in rtindirect.py. This is a source scan that
stands in for the IR test of §5, so its counts are evidence, not the test.
With a `jet emit --ir` listing, it also checks each runtime entry point the
listing calls (P4).
Usage: python3 linkchecks.py <repo root> [<listing.ir>]"""
import os
import re
import sys

root = sys.argv[1]
runtime = os.path.join(root, "Compiler/JetRuntime/Source")
linux = os.path.join(root, "Compiler/JetBackend/Source/OS/Linux.jet")
ALLOWED = {9: "mmap", 10: "mprotect", 11: "munmap", 15: "rt_sigreturn", 28: "madvise", 202: "futex", 231: "exit_group"}
WAITS = {0: "FUTEX_WAIT", 6: "FUTEX_LOCK_PI", 9: "FUTEX_WAIT_BITSET", 11: "FUTEX_WAIT_REQUEUE_PI", 13: "FUTEX_LOCK_PI2"}
head = re.compile(r"^(?:#[A-Za-z]+(?:\([^)]*\))?\s+)*(?:priv\s+)?([a-z_][a-z0-9_]*): fn\b")
binding = re.compile(r"^(?:priv\s+)?([a-z_][a-z0-9_]*)\s*(:=|:)\s*(?!fn\b|struct\b|enum\b|trait\b|module\b|type\b|impl\b)\S")
token = re.compile(r"\b([a-z_][a-z0-9_]*)\(")

# The backend's OS asm bodies and their transfer, store, TCB and syscall forms.
asm = {}
current = None
inside = False
for number, line in enumerate(open(linux, encoding="utf-8"), 1):
    found = head.match(line)
    if found:
        current = found.group(1)
        asm[current] = {"line": number, "text": [], "forms": set(), "store": False, "tcb": False, "syscalls": set()}
        inside = False
        continue
    if current is None:
        continue
    if line.strip() == '"""':
        inside = not inside
        continue
    if line.startswith("}"):
        current = None
        continue
    if inside:
        asm[current]["text"].append(line.split(";")[0].strip())
moved_rsp = re.compile(r"^(mov|lea|xchg|pop)\s+rsp\b|^xchg\s+\S+,\s*rsp\b")
for name, body in asm.items():
    switched = False
    fixed = None
    for text in body["text"]:
        if re.match(r"^(call|jmp)\s+(r[a-z0-9]+|e[a-z]x|qword ptr\s*\[|\[|\{)", text):
            body["forms"].add("{} through a register or memory".format(text.split()[0]))
        if moved_rsp.match(text):
            switched = True
        if text == "ret" and switched:
            body["forms"].add("ret on a stack loaded into rsp")
        if re.match(r"^wr[gf]sbase\b", text):
            body["forms"].add(text.split()[0])
        if "gs:" in text:
            body["tcb"] = True
        if re.match(r"^(lock\s+)?(mov\w*|xchg|cmpxchg\w*|xadd|add|sub|inc|dec|or|and|xor|stos\w*)\s+(byte|word|dword|qword|xmmword|ymmword)?\s*(ptr\s*)?(gs:)?\[", text) or re.match(r"^rep\s+(movs|stos)", text):
            body["store"] = True
        number = re.match(r"^mov\s+[er]ax,\s*(\d+)$", text)
        if number:
            fixed = int(number.group(1))
        if text == "syscall" and fixed is not None:
            body["syscalls"].add(fixed)

# The runtime's functions, globals and exported entry points.
bodies = {}
params = {}
files = {}
exported = set()
globals_ = set()
for name in sorted(os.listdir(runtime)):
    if not name.endswith(".jet"):
        continue
    current = None
    for line in open(os.path.join(runtime, name), encoding="utf-8"):
        found = head.match(line)
        if found:
            current = found.group(1)
            bodies[current] = []
            files[current] = name
            params[current] = set(re.findall(r"([a-z_][a-z0-9_]*):", line.split("fn", 1)[1].split(")")[0]))
            if line.startswith("#Export"):
                exported.add(current)
            continue
        if current is None:
            top = binding.match(line)
            if top:
                globals_.add(top.group(1))
            continue
        code = line.split("//")[0]
        bodies[current].append(code)
        if line.startswith("}"):
            current = None

global_use = re.compile(r"\b(" + "|".join(sorted(globals_)) + r")\b(?!\s*\()")
futex = re.compile(r"os_syscall_i64\(I64\{202\},\s*[^,]+,\s*I64\{(\d+)\}")
syscall = re.compile(r"os_syscall_i64\(I64\{(\d+)\}")
direct = {}
for name, lines in bodies.items():
    text = "".join(lines)
    called = {t for t in token.findall(text) if (t in bodies or t in asm) and t != name}
    facts = {"state": set(), "loaded": set(), "blocks": set(), "writes": set(), "param_store": False}
    for used in sorted(set(global_use.findall(text)))[:1]:
        facts["state"].add("global {}".format(used))
    for used in sorted(set(re.findall(r"\b(coll_call[0-9]+)\(", text)))[:1]:
        facts["loaded"].add(used)
    for op in futex.findall(text):
        if int(op) & 127 in WAITS:
            facts["blocks"].add("{} ({})".format(name, WAITS[int(op) & 127]))
    for number in syscall.findall(text):
        if int(number) not in ALLOWED:
            facts["blocks"].add("{} (syscall {})".format(name, number))
    for callee in called & set(asm):
        body = asm[callee]
        if body["tcb"]:
            facts["state"].add("{} (gs:)".format(callee))
        if body["forms"]:
            facts["loaded"].add(callee)
        for number in body["syscalls"]:
            if number not in ALLOWED:
                facts["blocks"].add("{} (syscall {})".format(callee, number))
        if body["store"]:
            facts["writes"].add(callee)
            for site in re.findall(r"\b" + callee + r"\(\s*(?:wrapping\()?\s*([a-z_][a-z0-9_]*)", text):
                if site in params[name]:
                    facts["param_store"] = True
    direct[name] = (facts, called & set(bodies))


def closure(overrides=None, stubs=frozenset()):
    """Each runtime function's facts, its callees' included. A stubbed
    function is the eval stub that replaces it: it has no fact and calls none
    of the runtime."""
    facts = {}
    for name, (own, _) in direct.items():
        facts[name] = {key: set() if name in stubs else set(value) for key, value in own.items() if key != "param_store"}
    for name, keys in (overrides or {}).items():
        for key in keys:
            facts[name][key] = set()
    changed = True
    while changed:
        changed = False
        for name, (_, called) in direct.items():
            if name in stubs:
                continue
            for callee in called:
                for key in ("state", "loaded", "blocks", "writes"):
                    extra = facts[callee][key] - facts[name][key]
                    if extra:
                        facts[name][key] |= extra
                        changed = True
    return facts


facts = closure()
print("runtime functions: {}, exported entry points: {}, module globals: {}".format(len(bodies), len(exported), len(globals_)))
print("asm bodies in OS/Linux.jet: {}".format(len(asm)))
print()

print("P5: asm bodies that transfer to an address not fixed at assembly")
for name in sorted(asm):
    if asm[name]["forms"]:
        users = sorted({files[f] for f, (_, called) in direct.items() if name in token.findall("".join(bodies[f]))})
        print("  {} (Linux.jet:{}): {}; runtime callers in {}".format(name, asm[name]["line"], ", ".join(sorted(asm[name]["forms"])), ", ".join(users) or "none"))
print()


def chain(table, name, key):
    """One call path from `name` to a function or asm body that has `key` itself."""
    path = [name]
    seen = {name}
    while not direct[path[-1]][0][key]:
        step = next((c for c in sorted(direct[path[-1]][1]) if table[c][key] and c not in seen), None)
        if step is None:
            break
        path.append(step)
        seen.add(step)
    return " -> ".join(path + sorted(direct[path[-1]][0][key])[:1])


def passing(table, keys, names):
    return {name for name in names if not any(table[name][key] for key in keys)}


def futex_waits(table, name):
    return {reason for reason in table[name]["blocks"] if "FUTEX" in reason}


print("P3: futex waits issued directly, and the runtime functions that issue a syscall outside the §6.2 allow-list directly")
for name in sorted(direct):
    for reason in sorted(futex_waits({name: direct[name][0]}, name)):
        print("  {}:{}; {}".format(files[name], reason, "touches state" if facts[name]["state"] else "no runtime state, no loaded address" if not facts[name]["loaded"] else "no runtime state"))
print("  outside the allow-list: {} functions".format(sum(1 for name in direct if any("syscall" in reason for reason in direct[name][0]["blocks"]))))
everything = set(bodies)
for label, table in (("today", facts), ("with sync_futex_wait's timed scratch moved off the TCB", closure({"sync_futex_wait": ("state",)}))):
    two_all = passing(table, ("state", "loaded"), everything)
    waiting = sorted(name for name in two_all if futex_waits(table, name))
    two_exported = two_all & exported
    print("{}: {} runtime functions pass the first two checks and reach a futex wait: {}".format(label, len(waiting), ", ".join(waiting[:6]) + (", …" if len(waiting) > 6 else "")))
    print("  exported entry points passing the first two checks: {}; reaching a futex wait: {}; reaching a syscall outside the allow-list: {}".format(
        len(two_exported), sum(1 for name in two_exported if futex_waits(table, name)), sum(1 for name in two_exported if table[name]["blocks"] - futex_waits(table, name))))
    for name in sorted(n for n in two_exported if table[n]["blocks"]):
        print("    {}".format(chain(table, name, "blocks")))
print("the third check, no blocking syscall, refuses each of them by the path above")
print()

print("P7: what the first three checks admit that writes memory")
three_now = passing(facts, ("state", "loaded", "blocks"), exported)
writing = {name for name in three_now if facts[name]["writes"]}
through = sorted(name for name in writing if direct[name][0]["param_store"])
print("exported entry points passing the three checks: {}; of them, reaching a store: {}; storing directly through a parameter: {}".format(len(three_now), len(writing), len(through)))
for name in through[:8]:
    print("  {}:{} stores through a parameter with {}".format(files[name], name, ", ".join(sorted(direct[name][0]["writes"]))))
for name in ("jet_char_len", "jet_rt_atomic_bool_load"):
    verdict = "writes memory: " + chain(facts, name, "writes") if facts[name]["writes"] else "writes no memory"
    print("  {} (r7scan.py's READERS): {}{}".format(name, verdict, "" if name in three_now else " (fails a check)"))

# §5's stub table: the files whose functions an eval stub replaces, the
# names, and a name prefix. Revision 3's rows, then revision 4's: the journey
# (Journey.jet, JourneyTLS.jet), the whole stop report (the Trap files and the
# stop's publication), and the task rows by name (the cleanup shield and the
# stop propagation a failed operation polls).
STUBS_R3 = ({"Alloc.jet", "Refcount.jet", "Shared.jet"}, {"jet_ir_stop_turn"}, "jet_rt_panic_")
STUBS_R4 = (STUBS_R3[0] | {"Journey.jet", "JourneyTLS.jet", "Trap.jet", "TrapMessages.jet", "TrapDefectMessages.jet"},
            STUBS_R3[1] | {"jet_rt_cleanup_enter", "jet_rt_cleanup_leave", "jet_scheduler_native_propagate", "rt_task_panic_publish"}, STUBS_R3[2])


def stubbed(rule):
    return frozenset(name for name in bodies if files[name] in rule[0] or name in rule[1] or name.startswith(rule[2]))


if len(sys.argv) > 2:
    # Only the functions named by the third argument (a regex), such as the
    # peeks a user types at a stop, and not `run`'s own entry calls.
    keep = re.compile(sys.argv[3] if len(sys.argv) > 3 else ".")
    called = set()
    current = None
    for line in open(sys.argv[2], encoding="utf-8"):
        opened = re.match(r"^fn (\S+) ", line)
        if opened:
            current = opened.group(1) if keep.search(opened.group(1)) else None
            continue
        target = re.search(r"call_runtime .*?(jet_\w+) \(", line)
        if current and target:
            called.add(target.group(1))
    print()
    print("P4: the runtime entry points the eval functions of {} call, by the three checks, under each stub table".format(os.path.basename(sys.argv[2])))
    for label, rule in (("revision 3", STUBS_R3), ("revision 4", STUBS_R4)):
        stubs = stubbed(rule)
        table = closure(stubs=stubs)
        refused = 0
        print("{}'s stub table ({} runtime functions stubbed):".format(label, len(stubs)))
        for name in sorted(called):
            if name in stubs:
                print("  {}: stubbed".format(name))
                continue
            failed = [key for key in ("state", "loaded", "blocks") if table[name][key]]
            refused += 1 if failed else 0
            print("  {}: {}".format(name, "passes" if not failed else "; ".join("{}: {}".format(key, chain(table, name, key)) for key in failed)))
        print("  the link refuses {} of the {} entry points".format(refused, len(called)))
