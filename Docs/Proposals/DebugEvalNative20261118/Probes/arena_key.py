"""Probe Q4 (review 4): what does R7's arena lookup cost per hit, keyed by
carrier, against a word in each fiber's TCB, and what does the trap cost
that a word still 0 takes? Runs x86-64 code in this process with GS set to
fake TCBs laid out as the runtime's (Thread.jet's TCB, Fiber.jet's header):
- carrier: `gs:[56]` (the fiber on this TCB, 0 on a plain thread), the
  fiber's carrier record (fiber+16), the record's TCB (record+24), then the
  arena word (TCB+760); on a plain thread `gs:[760]` at once;
- fiber: `gs:[760]`, one load.
Each lookup feeds the next one's address, so the figure is the latency of
one lookup, not a throughput. Then a traced child runs `int3` in a loop and
this process resumes it each time: one trap and resume, the least a hit
costs whose word is still 0 (the debugger's mapping of the arena is more).
Usage: python3 arena_key.py"""
import ctypes
import mmap
import os
import signal
import statistics
import struct
import time

libc = ctypes.CDLL(None, use_errno=True)
libc.syscall.argtypes = [ctypes.c_long, ctypes.c_long, ctypes.c_void_p]
libc.ptrace.argtypes = [ctypes.c_long, ctypes.c_long, ctypes.c_void_p, ctypes.c_void_p]
ARCH_SET_GS, SYS_ARCH_PRCTL = 0x1001, 158
PTRACE_TRACEME, PTRACE_CONT = 0, 7
HITS, RUNS, TRAPS = 50_000_000, 5, 20_000

# fn(n): loop n times: rax = arena (carrier keyed), rcx = rax & 0.
CARRIER = bytes.fromhex(
    "31c9"                  # xor ecx, ecx
    "65488b4138"            # loop: mov rax, gs:[rcx+56]
    "4885c0"                # test rax, rax
    "7411"                  # jz plain
    "488b4010"              # mov rax, [rax+16]
    "488b4018"              # mov rax, [rax+24]
    "488b80f8020000"        # mov rax, [rax+760]
    "eb08"                  # jmp got
    "65488b81f8020000"      # plain: mov rax, gs:[rcx+760]
    "4889c1"                # got: mov rcx, rax
    "83e100"                # and ecx, 0
    "48ffcf"                # dec rdi
    "75d2"                  # jnz loop
    "c3")                   # ret
# fn(n): loop n times: rax = gs:[760] (fiber keyed), rcx = rax & 0.
FIBER = bytes.fromhex(
    "31c9"                  # xor ecx, ecx
    "65488b81f8020000"      # loop: mov rax, gs:[rcx+760]
    "4889c1"                # mov rcx, rax
    "83e100"                # and ecx, 0
    "48ffcf"                # dec rdi
    "75ed"                  # jnz loop
    "c3")                   # ret
# fn(n): n traps.
TRAP = bytes.fromhex("cc" "48ffcf" "75fa" "c3")

code = mmap.mmap(-1, 4096, prot=mmap.PROT_READ | mmap.PROT_WRITE | mmap.PROT_EXEC)
data = mmap.mmap(-1, 4 * 4096)
base = ctypes.addressof(ctypes.c_char.from_buffer(code))
words = ctypes.addressof(ctypes.c_char.from_buffer(data))
fiber_tcb, fiber, record, carrier_tcb = (words + 4096 * k for k in range(4))
ARENA = 0x7f0000001000


def word(address, value):
    data[address - words:address - words + 8] = struct.pack("<Q", value)


word(fiber_tcb + 56, fiber)
word(fiber + 16, record)
word(record + 24, carrier_tcb)
word(carrier_tcb + 760, ARENA)
word(fiber_tcb + 760, ARENA)


def function(offset, body):
    code[offset:offset + len(body)] = body
    return ctypes.CFUNCTYPE(ctypes.c_long, ctypes.c_long)(base + offset)


carrier, per_fiber, trap = function(0, CARRIER), function(256, FIBER), function(512, TRAP)


def per_hit(run, gs):
    if libc.syscall(SYS_ARCH_PRCTL, ARCH_SET_GS, ctypes.c_void_p(gs)) != 0:
        raise SystemExit("arch_prctl: errno {}".format(ctypes.get_errno()))
    figures = []
    for _ in range(RUNS):
        start = time.perf_counter_ns()
        run(HITS)
        figures.append((time.perf_counter_ns() - start) / HITS)
    return statistics.median(figures)


print("{} lookups, median of {} runs, ns per lookup:".format(HITS, RUNS))
print("  carrier keyed, on a fiber:       {:.2f}".format(per_hit(carrier, fiber_tcb)))
print("  carrier keyed, on a plain thread: {:.2f}".format(per_hit(carrier, carrier_tcb)))
print("  fiber keyed:                     {:.2f}".format(per_hit(per_fiber, fiber_tcb)))
libc.syscall(SYS_ARCH_PRCTL, ARCH_SET_GS, None)

child = os.fork()
if child == 0:
    libc.ptrace(PTRACE_TRACEME, 0, None, None)
    os.kill(os.getpid(), signal.SIGSTOP)
    trap(TRAPS)
    os._exit(0)
os.waitpid(child, 0)
libc.ptrace(PTRACE_CONT, child, None, None)
start = time.perf_counter_ns()
for _ in range(TRAPS):
    os.waitpid(child, 0)
    libc.ptrace(PTRACE_CONT, child, None, None)
spent = time.perf_counter_ns() - start
os.waitpid(child, 0)
print("{} traps, each stopped and resumed by this process: {:.2f} us per trap".format(TRAPS, spent / TRAPS / 1000))
