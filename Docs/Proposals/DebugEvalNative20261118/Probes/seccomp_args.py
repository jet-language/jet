"""Probe N8: a seccomp filter that checks the arguments of mmap, mprotect,
madvise and futex, as §6.2 now specifies. A forked child installs it (with
SECCOMP_RET_ERRNO(EPERM) for a refused call, so the probe can print what the
fork would trap on) and tries each call. Every other syscall is allowed here
so Python keeps running; the fork's real filter traps them.
Usage: python3 seccomp_args.py"""
import ctypes
import mmap
import os
import struct

libc = ctypes.CDLL(None, use_errno=True)
libc.syscall.restype = ctypes.c_long
SYS = {"mmap": 9, "mprotect": 10, "madvise": 28, "futex": 202}
PROT_WRITE, MAP_SHARED, MAP_PRIVATE, MAP_ANONYMOUS = 2, 1, 2, 32
MADV_DONTNEED, MADV_FREE, MADV_HUGEPAGE, MADV_REMOVE = 4, 8, 14, 9
FUTEX_PRIVATE_FLAG = 128
ALLOW, DENY = 0x7FFF0000, 0x00050000 | 1  # SECCOMP_RET_ALLOW; SECCOMP_RET_ERRNO | EPERM
LD_W_ABS, JEQ_K, JSET_K, RET_K, AND_K = 0x20, 0x15, 0x45, 0x06, 0x54
NR, ARG = 0, lambda n: 16 + 8 * n  # seccomp_data: nr, arch, ip, args[6] (low words)


def ins(code, jt, jf, k):
    return struct.pack("<HBBI", code, jt, jf, k & 0xFFFFFFFF)


def checks():
    """A list of (nr, body) where body returns ALLOW or DENY for that syscall."""
    out = []
    # mmap: only MAP_PRIVATE | MAP_ANONYMOUS (flags arg 3), fd -1 (arg 4).
    out.append((SYS["mmap"], [
        ins(LD_W_ABS, 0, 0, ARG(3)), ins(AND_K, 0, 0, MAP_SHARED | MAP_PRIVATE | MAP_ANONYMOUS),
        ins(JEQ_K, 1, 0, MAP_PRIVATE | MAP_ANONYMOUS), ins(RET_K, 0, 0, DENY),
        ins(LD_W_ABS, 0, 0, ARG(4)), ins(JEQ_K, 1, 0, 0xFFFFFFFF), ins(RET_K, 0, 0, DENY), ins(RET_K, 0, 0, ALLOW)]))
    # mprotect: never PROT_WRITE (arg 2).
    out.append((SYS["mprotect"], [
        ins(LD_W_ABS, 0, 0, ARG(2)), ins(JSET_K, 0, 1, PROT_WRITE), ins(RET_K, 0, 0, DENY), ins(RET_K, 0, 0, ALLOW)]))
    # madvise: only DONTNEED, FREE, HUGEPAGE (arg 2).
    out.append((SYS["madvise"], [
        ins(LD_W_ABS, 0, 0, ARG(2)), ins(JEQ_K, 3, 0, MADV_DONTNEED), ins(JEQ_K, 2, 0, MADV_FREE),
        ins(JEQ_K, 1, 0, MADV_HUGEPAGE), ins(RET_K, 0, 0, DENY), ins(RET_K, 0, 0, ALLOW)]))
    # futex: only with FUTEX_PRIVATE_FLAG (op, arg 1).
    out.append((SYS["futex"], [
        ins(LD_W_ABS, 0, 0, ARG(1)), ins(JSET_K, 1, 0, FUTEX_PRIVATE_FLAG), ins(RET_K, 0, 0, DENY), ins(RET_K, 0, 0, ALLOW)]))
    return out


def program():
    code = [ins(LD_W_ABS, 0, 0, NR)]
    bodies = checks()
    # A dispatch on nr: each check jumps past the remaining dispatches to its body.
    tail = []
    for index, (nr, body) in enumerate(bodies):
        tail.append(body)
    dispatch_len = 2 * len(bodies) + 1
    offsets, at = [], 0
    for body in tail:
        offsets.append(at)
        at += len(body)
    for index, (nr, body) in enumerate(bodies):
        here = 1 + 2 * index
        # JEQ nr -> next instruction (a JA to the body); else skip it.
        code.append(ins(JEQ_K, 0, 1, nr))
        target = (dispatch_len - (here + 2)) + offsets[index] + 1
        code.append(struct.pack("<HBBI", 0x05, 0, 0, target))  # BPF_JMP|BPF_JA
    code.append(ins(RET_K, 0, 0, ALLOW))
    for body in tail:
        code.extend(body)
    return b"".join(code), len(code)


def install():
    raw, count = program()
    buf = ctypes.create_string_buffer(raw)

    class Prog(ctypes.Structure):
        _fields_ = [("len", ctypes.c_ushort), ("filter", ctypes.c_void_p)]

    prog = Prog(count, ctypes.addressof(buf))
    assert libc.prctl(38, 1, 0, 0, 0) == 0  # PR_SET_NO_NEW_PRIVS
    assert libc.syscall(317, 1, 0, ctypes.byref(prog)) == 0, ctypes.get_errno()  # seccomp(SET_MODE_FILTER)


def attempt(label, *args):
    ctypes.set_errno(0)
    result = libc.syscall(*[ctypes.c_long(a) for a in args])
    verdict = "refused (EPERM)" if result == -1 and ctypes.get_errno() == 1 else "allowed"
    os.write(1, "{}: {}\n".format(label, verdict).encode())


page = mmap.mmap(-1, 4096, flags=MAP_PRIVATE | MAP_ANONYMOUS)
address = ctypes.addressof(ctypes.c_char.from_buffer(page))
fd = os.memfd_create("probe")
os.ftruncate(fd, 4096)
word = ctypes.c_int(0)
pid = os.fork()
if pid == 0:
    install()
    attempt("mprotect PROT_READ", SYS["mprotect"], address, 4096, 1)
    attempt("mprotect PROT_READ|PROT_WRITE", SYS["mprotect"], address, 4096, 3)
    attempt("mmap MAP_PRIVATE|MAP_ANONYMOUS fd -1", SYS["mmap"], 0, 4096, 3, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0)
    attempt("mmap MAP_SHARED of an inherited fd", SYS["mmap"], 0, 4096, 3, MAP_SHARED, fd, 0)
    attempt("madvise MADV_DONTNEED", SYS["madvise"], address, 4096, MADV_DONTNEED)
    attempt("madvise MADV_REMOVE", SYS["madvise"], address, 4096, MADV_REMOVE)
    attempt("futex FUTEX_WAKE|FUTEX_PRIVATE_FLAG", SYS["futex"], ctypes.addressof(word), 1 | FUTEX_PRIVATE_FLAG, 1, 0, 0, 0)
    attempt("futex FUTEX_WAKE (shared)", SYS["futex"], ctypes.addressof(word), 1, 1, 0, 0, 0)
    os._exit(0)
os.waitpid(pid, 0)
