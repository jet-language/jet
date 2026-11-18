"""Probe N9: a MADV_DONTFORK range is a hole in a fork. Can the fork's own
mmap land in it, and does reserving the hole PROT_NONE with
MAP_FIXED_NOREPLACE first keep it out? A read is tried three ways: by the
eval code in the fork itself, by the debugger with process_vm_readv, and by
the debugger through /proc/<pid>/mem (which reads through page protections,
as PTRACE_PEEKDATA does).
Usage: python3 dontfork.py"""
import ctypes
import os
import signal
import time

libc = ctypes.CDLL(None, use_errno=True)
libc.mmap.restype = ctypes.c_void_p
libc.mmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_long]
libc.process_vm_readv.restype = ctypes.c_ssize_t
PROT_NONE, PROT_RW = 0, 3
MAP_PRIVATE, MAP_ANONYMOUS, MAP_FIXED_NOREPLACE = 2, 32, 0x100000
MADV_DONTFORK = 10
SIZE = 1 << 20


class IOVec(ctypes.Structure):
    _fields_ = [("base", ctypes.c_void_p), ("len", ctypes.c_size_t)]


hole = libc.mmap(None, SIZE, PROT_RW, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0)
assert libc.madvise(ctypes.c_void_p(hole), SIZE, MADV_DONTFORK) == 0
ctypes.memset(hole, 0x5A, SIZE)
print("target: {:#x} holds 0x5a bytes, MADV_DONTFORK".format(hole))


def debugger_vm_read(pid):
    out = ctypes.create_string_buffer(1)
    local = IOVec(ctypes.addressof(out), 1)
    remote = IOVec(hole, 1)
    got = libc.process_vm_readv(pid, ctypes.byref(local), 1, ctypes.byref(remote), 1, 0)
    return "EFAULT, a fault" if got < 0 else "bytes {!r}, no fault".format(out.raw)


def debugger_mem_read(pid):
    try:
        with open("/proc/{}/mem".format(pid), "rb", buffering=0) as mem:
            mem.seek(hole)
            return "bytes {!r}, no fault".format(mem.read(1))
    except OSError as error:
        return "{}, a fault".format(error.strerror)


def debugger_peek(pid):
    libc.ptrace.restype = ctypes.c_long
    libc.ptrace.argtypes = [ctypes.c_long, ctypes.c_long, ctypes.c_void_p, ctypes.c_void_p]
    if libc.ptrace(16, pid, None, None) != 0:  # PTRACE_ATTACH
        return "attach failed"
    os.waitpid(pid, 0)
    ctypes.set_errno(0)
    word = libc.ptrace(2, pid, ctypes.c_void_p(hole), None)  # PTRACE_PEEKDATA
    error = ctypes.get_errno()
    libc.ptrace(17, pid, None, None)  # PTRACE_DETACH
    return "errno {}, a fault".format(error) if error else "word {:#x}, no fault".format(word & 0xFFFFFFFFFFFFFFFF)

for reserve in (False, True):
    print("fork, reserve={}".format(reserve))
    ready_r, ready_w = os.pipe()
    pid = os.fork()
    if pid == 0:
        if reserve:
            got = libc.mmap(ctypes.c_void_p(hole), SIZE, PROT_NONE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0)
            os.write(1, "  reserved the hole PROT_NONE: {}\n".format(got == hole).encode())
        arena = libc.mmap(ctypes.c_void_p(hole), SIZE, PROT_RW, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0)
        os.write(1, "  the arena's mmap (hinted at the hole) landed in it: {}\n".format(arena == hole).encode())
        os.write(ready_w, b"x")
        time.sleep(0.5)
        value = ctypes.string_at(hole, 1)
        os.write(1, "  the eval code's own read gives: bytes {!r}, no fault\n".format(value).encode())
        os._exit(0)
    os.read(ready_r, 1)
    print("  the debugger's process_vm_readv gives: {}".format(debugger_vm_read(pid)))
    print("  the debugger's /proc/<pid>/mem read gives: {}".format(debugger_mem_read(pid)))
    print("  the debugger's PTRACE_PEEKDATA gives: {}".format(debugger_peek(pid)))
    _, status = os.waitpid(pid, 0)
    if os.WIFSIGNALED(status):
        print("  the eval code's own read gives: {}, a fault".format(signal.Signals(os.WTERMSIG(status)).name))
