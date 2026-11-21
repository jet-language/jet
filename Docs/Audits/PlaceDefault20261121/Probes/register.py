#!/usr/bin/env python3
"""How an x86-64 call would carry its destination: in no place (plain), in
the next argument register (arg: rdx, as a third parameter), or in RAX, a
register no argument uses and every call already clobbers (reg). Jet's x64
calls load arguments into rdi, rsi, rdx, rcx, r8, r9 and an indirect
target into r10 (JetIR X64Select.jet), so RAX is free at every call.

The machine code is written here byte by byte and run from an executable
mapping through ctypes, so the probe needs nothing but python3. The loop
calls the builder through a register, as a function value calls; the
builder adds the destination to its first item, as returns.jet's `arg`
row reads its word once, stores two items and returns the count. Each
convention's three changed bytes are the same length, so the code is laid
out alike. Run: register.py [calls] [repeats]
"""
import ctypes
import mmap
import struct
import sys
import time

calls = int(sys.argv[1]) if len(sys.argv) > 1 else 200_000_000
repeats = int(sys.argv[2]) if len(sys.argv) > 2 else 7

NOP3 = bytes([0x0F, 0x1F, 0x00])
# What the caller does before the call, and what the builder does first.
CALLER = {"plain": NOP3, "arg": bytes([0x4C, 0x89, 0xE2]), "reg": bytes([0x4C, 0x89, 0xE0])}  # mov rdx, r12 / mov rax, r12
CALLEE = {"plain": NOP3, "arg": bytes([0x48, 0x01, 0xD7]), "reg": bytes([0x48, 0x01, 0xC7])}  # add rdi, rdx / add rdi, rax


def program(conv: str) -> bytes:
    """bench(n=rdi, dest=rsi) -> the sum of n builder results, then the
    builder at offset 128 and its two-word buffer at offset 192."""
    head = bytes([
        0x53,                    # push rbx
        0x41, 0x54,              # push r12
        0x41, 0x55,              # push r13
        0x41, 0x56,              # push r14
        0x48, 0x83, 0xEC, 0x08,  # sub rsp, 8 (16-aligned at the call)
        0x48, 0x89, 0xFB,        # mov rbx, rdi
        0x49, 0x89, 0xF4,        # mov r12, rsi
    ])
    lea_at = len(head)
    head += bytes([0x4C, 0x8D, 0x2D]) + struct.pack("<i", 128 - (lea_at + 7))  # lea r13, [rip -> builder]
    head += bytes([0x4D, 0x31, 0xF6])  # xor r14, r14
    loop = bytes([0x89, 0xDF, 0xBE, 0x02, 0x00, 0x00, 0x00])  # mov edi, ebx; mov esi, 2
    loop += CALLER[conv]
    loop += bytes([0x41, 0xFF, 0xD5, 0x49, 0x01, 0xC6, 0x48, 0xFF, 0xCB])  # call r13; add r14, rax; dec rbx
    loop += bytes([0x75, (-(len(loop) + 2)) & 0xFF])  # jnz loop
    tail = bytes([0x4C, 0x89, 0xF0, 0x48, 0x83, 0xC4, 0x08, 0x41, 0x5E, 0x41, 0x5D, 0x41, 0x5C, 0x5B, 0xC3])
    code = head + loop + tail
    assert len(code) <= 128
    code += b"\xCC" * (128 - len(code))
    builder = CALLEE[conv]
    lea_at = 128 + len(builder)
    builder += bytes([0x4C, 0x8D, 0x05]) + struct.pack("<i", 192 - (lea_at + 7))  # lea r8, [rip -> buffer]
    builder += bytes([
        0x49, 0x89, 0x38,        # mov [r8], rdi
        0x4C, 0x8D, 0x4F, 0x01,  # lea r9, [rdi + 1]
        0x4D, 0x89, 0x48, 0x08,  # mov [r8 + 8], r9
        0x48, 0x89, 0xF0,        # mov rax, rsi
        0xC3,                    # ret
    ])
    code += builder
    assert len(code) <= 192
    return code + b"\xCC" * (192 - len(code)) + bytes(64)


benches = {}
keep = []
for conv in CALLER:
    page = mmap.mmap(-1, 4096, prot=mmap.PROT_READ | mmap.PROT_WRITE | mmap.PROT_EXEC)
    page.write(program(conv))
    address = ctypes.addressof(ctypes.c_char.from_buffer(page))
    keep.append(page)
    benches[conv] = ctypes.CFUNCTYPE(ctypes.c_int64, ctypes.c_int64, ctypes.c_int64)(address)

taken = {conv: [] for conv in benches}
for run in benches.values():
    run(1_000_000, 0)
# The conventions alternate within each repeat, so a busy host's drift
# lands on all of them alike.
for _ in range(repeats):
    for conv, run in benches.items():
        started = time.perf_counter_ns()
        total = run(calls, 0)
        ns = (time.perf_counter_ns() - started) / calls
        assert total == 2 * calls, (conv, total)
        taken[conv].append(ns)
for conv, ns in taken.items():
    ns.sort()
    print(f"x64 value {conv}: {calls} calls, min {ns[0]:.3f} ns, median {ns[len(ns) // 2]:.3f} ns a call")
