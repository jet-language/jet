"""Probe N7: which base register holds a running Jet image's TCB? Starts the
image, attaches with ptrace, reads fs_base and gs_base from the registers and
names the mapping each points into. Usage: python3 tcb.py <image>"""
import ctypes
import os
import subprocess
import sys
import time

PTRACE_GETREGS, PTRACE_ATTACH, PTRACE_DETACH = 12, 16, 17
FIELDS = ["r15", "r14", "r13", "r12", "rbp", "rbx", "r11", "r10", "r9", "r8", "rax", "rcx", "rdx", "rsi", "rdi", "orig_rax", "rip", "cs", "eflags", "rsp", "ss", "fs_base", "gs_base", "ds", "es", "fs", "gs"]


class Regs(ctypes.Structure):
    _fields_ = [(name, ctypes.c_ulonglong) for name in FIELDS]


libc = ctypes.CDLL(None, use_errno=True)
libc.ptrace.argtypes = [ctypes.c_long, ctypes.c_long, ctypes.c_void_p, ctypes.c_void_p]
child = subprocess.Popen([sys.argv[1]], stdout=subprocess.DEVNULL)
time.sleep(0.5)
if libc.ptrace(PTRACE_ATTACH, child.pid, None, None) != 0:
    raise SystemExit("attach: errno {}".format(ctypes.get_errno()))
os.waitpid(child.pid, 0)
regs = Regs()
libc.ptrace(PTRACE_GETREGS, child.pid, None, ctypes.byref(regs))
maps = open("/proc/{}/maps".format(child.pid)).read().splitlines()
libc.ptrace(PTRACE_DETACH, child.pid, None, None)
child.kill()


def mapping(address):
    for line in maps:
        span, rest = line.split(" ", 1)
        low, high = (int(part, 16) for part in span.split("-"))
        if low <= address < high:
            return line
    return "no mapping"


for name in ("fs_base", "gs_base"):
    value = getattr(regs, name)
    print("{} = {:#x}: {}".format(name, value, mapping(value) if value else "unset"))
