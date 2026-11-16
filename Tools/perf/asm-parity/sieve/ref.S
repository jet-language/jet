# The gauntlet sieve (Tools/gauntlet/entries/sieve/jet/run.jet) by hand.
# run.jet keeps one byte per odd number below n (slot i is 2i + 1; slot 0,
# the number 1, stands in for 2), clears the odd multiples of each odd p
# with p * p < n from p * p, and prints the number of 1 slots and the last
# one's number (at least 2). Every odd composite c < n has a prime factor q
# with q * q <= c < n, so the 1 slots are exactly 1 and the odd primes below
# n: any correct sieve of the odd numbers gives the same two answers.
# This one:
# - is segmented: one 32 KiB block of slots (an L1 data cache) at a time,
#   each base prime p >= 17 carrying its next multiple from block to block;
# - pre-sieves 3, 5, 7, 11 and 13: a block starts as a copy of their
#   periodic pattern (period 15015 slots) at the block's phase, and the
#   first block puts those five primes back;
# - clears a prime's multiples four stores to an iteration;
# - counts a block with vpsadbw (its tail past the last slot zeroed to a
#   whole 128 bytes) and keeps the last 1 slot of the last nonempty block.
# The base primes up to isqrt(n - 1) come from a plain byte sieve. n <= 2:
# both answers 0, as run.jet prints.
    .include "common.S"
    .text

.set SEG, 32768                  # slots per block
.set PERIOD, 15015               # 3 * 5 * 7 * 11 * 13
.set PATTERN_LEN, PERIOD + SEG   # a block's copy never wraps
.set BLOCK, 49152                # the block, after the pattern
.set BASE, BLOCK + SEG           # the base sieve, then the {p, next} rows

kernel:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    xor eax, eax
    xor edx, edx
    cmp rdi, 2
    jbe 99f
    mov r12, rdi                 # n
    mov r13, rdi
    shr r13, 1                   # slots
    # r = isqrt(n - 1): the largest p with p * p < n
    lea rax, [rdi - 1]
    cvtsi2sd xmm0, rax
    sqrtsd xmm0, xmm0
    cvttsd2si rbp, xmm0
1:  mov rcx, rbp
    imul rcx, rbp
    cmp rcx, rax
    jbe 2f
    dec rbp
    jmp 1b
2:  lea rcx, [rbp + 1]
    imul rcx, rcx
    cmp rcx, rax
    ja 3f
    inc rbp
    jmp 2b
    # mmap(NULL, BASE + base sieve + rows, RW, PRIVATE|ANON)
3:  mov rcx, rbp
    shr rcx, 1
    inc rcx                      # base sieve bytes: 1, 3, .. r
    lea rsi, [rcx + 63]
    and rsi, -64
    shl rcx, 4
    add rsi, rcx
    add rsi, BASE + 4095
    and rsi, -4096
    xor edi, edi
    mov edx, 3
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    mov r14, rax
    # the pattern: 0 at the odd multiples of 3, 5, 7, 11 and 13
    mov rdi, r14
    mov ecx, PATTERN_LEN
    mov al, 1
    rep stosb
    lea rsi, [rip + small]
4:  movzx ecx, byte ptr [rsi]
    test ecx, ecx
    jz 6f
    mov eax, ecx
    shr eax, 1                   # the slot of q itself
5:  mov byte ptr [r14 + rax], 0
    add eax, ecx
    cmp eax, PATTERN_LEN
    jb 5b
    inc rsi
    jmp 4b
    # the base sieve: byte i for 2i + 1 <= r
6:  lea r9, [r14 + BASE]
    mov r8, rbp
    shr r8, 1
    inc r8
    mov rdi, r9
    mov rcx, r8
    mov al, 1
    rep stosb
    mov byte ptr [r9], 0         # 1
    mov ecx, 1
7:  lea rdx, [rcx + rcx + 1]     # p
    mov rax, rdx
    imul rax, rdx
    cmp rax, rbp
    ja 9f
    cmp byte ptr [r9 + rcx], 0
    je 8f
    shr rax, 1
71: cmp rax, r8
    jae 8f
    mov byte ptr [r9 + rax], 0
    add rax, rdx
    jmp 71b
8:  inc rcx
    jmp 7b
    # rows {p, next slot to clear} for the base primes from 17
9:  lea r15, [r8 + 63]
    and r15, -64
    add r15, r9
    mov rbx, r15
    mov ecx, 8                   # 17
10: cmp rcx, r8
    jae 12f
    cmp byte ptr [r9 + rcx], 0
    je 11f
    lea rdx, [rcx + rcx + 1]
    mov [rbx], rdx
    mov rax, rdx
    imul rax, rdx
    shr rax, 1
    mov [rbx + 8], rax
    add rbx, 16
11: inc rcx
    jmp 10b
    # the blocks: r12 its first slot, rbp the rows begun, r11 the count,
    # r10 the last 1 slot
12: mov rbp, r15
    xor r12d, r12d
    xor r11d, r11d
    xor r10d, r10d
    vpxor xmm7, xmm7, xmm7
13: mov rax, r12
    xor edx, edx
    mov ecx, PERIOD
    div rcx                      # the block's phase
    lea rsi, [r14 + rdx]
    lea rdi, [r14 + BLOCK]
    mov ecx, SEG
    rep movsb
    test r12, r12
    jnz 14f
    mov byte ptr [r14 + BLOCK + 1], 1    # 3
    mov word ptr [r14 + BLOCK + 2], 0x0101   # 5, 7
    mov word ptr [r14 + BLOCK + 5], 0x0101   # 11, 13
14: lea r9, [r12 + SEG]
15: cmp rbp, rbx                 # rows whose p * p falls in this block
    jae 16f
    cmp [rbp + 8], r9
    jae 16f
    add rbp, 16
    jmp 15b
16: mov r8, r15
    lea rdi, [r14 + BLOCK]
    lea rsi, [rdi + SEG]
17: cmp r8, rbp
    jae 21f
    mov rdx, [r8]                # p
    mov rax, [r8 + 8]
    sub rax, r12
    add rax, rdi
    lea rcx, [rdx + rdx * 2]
    mov r9, rsi
    sub r9, rcx                  # four stores fit below this
    cmp rax, r9
    jae 19f
18: mov byte ptr [rax], 0
    mov byte ptr [rax + rdx], 0
    mov byte ptr [rax + rdx * 2], 0
    mov byte ptr [rax + rcx], 0
    lea rax, [rax + rdx * 4]
    cmp rax, r9
    jb 18b
19: cmp rax, rsi
    jae 20f
    mov byte ptr [rax], 0
    add rax, rdx
    jmp 19b
20: sub rax, rdi
    add rax, r12
    mov [r8 + 8], rax
    add r8, 16
    jmp 17b
    # the slots in this block, the rest of a 128-byte run zeroed
21: mov r8, r13
    sub r8, r12
    mov eax, SEG
    cmp r8, rax
    cmova r8, rax
    lea rdx, [r8 + 127]
    and rdx, -128
    lea rdi, [r14 + BLOCK + r8]
    mov rcx, rdx
    sub rcx, r8
    xor eax, eax
    rep stosb
    lea rsi, [r14 + BLOCK]
    vpxor xmm0, xmm0, xmm0
    vpxor xmm1, xmm1, xmm1
    vpxor xmm2, xmm2, xmm2
    vpxor xmm3, xmm3, xmm3
    xor ecx, ecx
22: vpsadbw ymm4, ymm7, [rsi + rcx]
    vpaddq ymm0, ymm0, ymm4
    vpsadbw ymm5, ymm7, [rsi + rcx + 32]
    vpaddq ymm1, ymm1, ymm5
    vpsadbw ymm4, ymm7, [rsi + rcx + 64]
    vpaddq ymm2, ymm2, ymm4
    vpsadbw ymm5, ymm7, [rsi + rcx + 96]
    vpaddq ymm3, ymm3, ymm5
    sub rcx, -128
    cmp rcx, rdx
    jb 22b
    vpaddq ymm0, ymm0, ymm1
    vpaddq ymm2, ymm2, ymm3
    vpaddq ymm0, ymm0, ymm2
    vextracti128 xmm1, ymm0, 1
    vpaddq xmm0, xmm0, xmm1
    vpshufd xmm1, xmm0, 0x4e
    vpaddq xmm0, xmm0, xmm1
    vmovq rax, xmm0
    add r11, rax
    test rax, rax
    jz 24f
    mov rcx, r8                  # its last 1 slot
23: dec rcx
    cmp byte ptr [rsi + rcx], 0
    je 23b
    lea r10, [r12 + rcx]
24: add r12, SEG
    cmp r12, r13
    jb 13b
    vzeroupper
    mov rax, r11                 # count
    lea rdx, [r10 + r10 + 1]     # largest, at least 2
    cmp rdx, 2
    ja 99f
    mov edx, 2
99: pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret

small: .byte 3, 5, 7, 11, 13, 0
prefix: .ascii "count"
    .p2align 3
prefix_len: .quad 5
prefix2: .ascii "largest"
    .p2align 3
prefix2_len: .quad 7
