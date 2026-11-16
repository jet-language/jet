# The gauntlet tensor-map-4k (Tools/gauntlet/entries/tensor-map-4k/jet/run.jet)
# by hand: two 4096 x 4096 tensors of doubles, all 1.0 and all 2.0, their
# elementwise sum, its corner and the sum of all its elements, checked
# (corner 3.0, checksum 50331648) and printed as run.jet prints them.
# The three tensors are materialized, every element of each written, and
# every sum element is its own 1.0 + 2.0 (one vaddpd per four); what goes
# is the reading back:
# - one pass writes left, right and their sum together from registers with
#   non-temporal stores (no read for ownership) and adds each sum vector
#   into one of four accumulators. The sum's partial sums are integers
#   below 2^53, so this order gives run.jet's exact total (axis 0, then the
#   row of column totals);
# - the corner is read back from the sum tensor;
# - one anonymous mapping holds the 384 MiB, advised to huge pages.
# Errors: no memory, or a check failing -> exit 1.
    .intel_syntax noprefix
    .text

.set SIDE, 4096
.set BYTES, SIDE * SIDE * 8      # one tensor

_start:
    xor edi, edi                 # mmap(NULL, 3 tensors, RW, PRIVATE|ANON)
    mov rsi, 3 * BYTES
    mov edx, 3
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja 9f
    mov rbx, rax
    mov rdi, rax                 # madvise(MADV_HUGEPAGE), advisory
    mov rsi, 3 * BYTES
    mov edx, 14
    mov eax, 28
    syscall
    vbroadcastsd ymm0, [rip + one]
    vbroadcastsd ymm1, [rip + two]
    vxorpd xmm2, xmm2, xmm2
    vxorpd xmm3, xmm3, xmm3
    vxorpd xmm4, xmm4, xmm4
    vxorpd xmm5, xmm5, xmm5
    mov rdi, rbx                 # left, then right and the sum at BYTES apart
    lea rcx, [rbx + BYTES]
1:  vmovntpd [rdi], ymm0
    vmovntpd [rdi + 32], ymm0
    vmovntpd [rdi + 64], ymm0
    vmovntpd [rdi + 96], ymm0
    vmovntpd [rdi + BYTES], ymm1
    vmovntpd [rdi + BYTES + 32], ymm1
    vmovntpd [rdi + BYTES + 64], ymm1
    vmovntpd [rdi + BYTES + 96], ymm1
    vaddpd ymm6, ymm0, ymm1
    vaddpd ymm7, ymm0, ymm1
    vaddpd ymm8, ymm0, ymm1
    vaddpd ymm9, ymm0, ymm1
    vmovntpd [rdi + 2 * BYTES], ymm6
    vmovntpd [rdi + 2 * BYTES + 32], ymm7
    vmovntpd [rdi + 2 * BYTES + 64], ymm8
    vmovntpd [rdi + 2 * BYTES + 96], ymm9
    vaddpd ymm2, ymm2, ymm6
    vaddpd ymm3, ymm3, ymm7
    vaddpd ymm4, ymm4, ymm8
    vaddpd ymm5, ymm5, ymm9
    sub rdi, -128
    cmp rdi, rcx
    jb 1b
    sfence
    vaddpd ymm2, ymm2, ymm3
    vaddpd ymm4, ymm4, ymm5
    vaddpd ymm2, ymm2, ymm4
    vextractf128 xmm3, ymm2, 1
    vaddpd xmm2, xmm2, xmm3
    vunpckhpd xmm3, xmm2, xmm2
    vaddsd xmm2, xmm2, xmm3      # checksum
    vmovsd xmm4, [rbx + 3 * BYTES - 8]   # corner
    vzeroupper
    ucomisd xmm4, [rip + three]
    jne 9f
    jp 9f
    ucomisd xmm2, [rip + checksum]
    jne 9f
    jp 9f
    mov edi, 1
    lea rsi, [rip + line]
    mov edx, line_end - line
2:  mov eax, 1
    syscall
    cmp rax, -4
    je 2b
    test rax, rax
    jle 9f
    add rsi, rax
    sub rdx, rax
    jnz 2b
    xor edi, edi
    mov eax, 231
    syscall
9:  mov edi, 1
    mov eax, 231
    syscall

    .p2align 3
one: .double 1.0
two: .double 2.0
three: .double 3.0
checksum: .double 50331648.0
line: .ascii "shape:[4096, 4096] numel:16777216 corner:3.0 checksum:50331648\n"
line_end:
