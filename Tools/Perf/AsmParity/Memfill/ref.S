# run.jet's work: the same 4096 fills from the same generator, laid out the
# same way in one page-aligned block, each through one memset routine picked
# once at load (`dispatch`) and called through a stack slot. The routine is
# the best known shape for its size classes: the byte spread over a word by
# one multiply; below one vector two overlapping scalar or 16-byte stores;
# up to 2, 4 and 8 vectors that many unaligned stores of the broadcast
# vector, from both ends; above 8 vectors 4 unaligned stores at each end and
# a 4-vector loop of aligned stores between them; and `rep stosb` from 2048
# bytes (fast strings). The AVX2 routine needs AVX2 with the OS saving the
# YMM state and ERMS; any other x86-64 gets the same shape on SSE2 vectors
# without `rep stosb`. The kernel's one check, a count of zero or less
# writes nothing, costs nothing on the small paths (zero falls to the byte
# case's early return, a negative count to the large path) and one sign
# test above 8 vectors.
    .include "common.S"
    .text
    .set OPS, 4096
    .set OP, 12                  # size, to, value: 4 bytes each
    .set LARGEST, 16384
    .set FILL_ROOM, 64
    .set MAPPED, 69632           # OPS * OP + LARGEST + FILL_ROOM, in whole pages
    .set REP_FROM, 2048

kernel:
    push rbx
    push rbp
    push r12
    push r13
    sub rsp, 24                  # [rsp]: the routine; [rsp + 8]: the table
    mov r12, rdi                 # passes: n, or 1 when n is 0
    test r12, r12
    jnz 1f
    mov r12d, 1
1:  call dispatch
    mov [rsp], rax
    xor edi, edi                 # mmap(NULL, MAPPED, RW, PRIVATE|ANON)
    mov esi, MAPPED
    mov edx, 3
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    mov [rsp + 8], rax
    lea r13, [rax + OPS * OP]    # the block (page-aligned)
    # The table: state = (state * 1103515245 + 12345) & 0x7fffffff.
    mov rdi, rax
    lea r8, [rip + size_bits]
    mov eax, 12345
    mov r9d, OPS
2:  imul rax, rax, 1103515245
    add rax, 12345
    and eax, 0x7fffffff
    mov edx, eax
    shr edx, 28
    movzx ecx, byte ptr [r8 + rdx]
    mov edx, 1
    shl edx, cl
    dec edx
    mov r10d, eax
    shr r10d, 12
    and r10d, edx
    inc r10d
    mov [rdi], r10d              # size
    imul rax, rax, 1103515245
    add rax, 12345
    and eax, 0x7fffffff
    mov edx, eax
    shr edx, 16
    and edx, 63
    mov [rdi + 4], edx           # to
    mov edx, eax
    shr edx, 22
    and edx, 255
    mov [rdi + 8], edx           # value
    add rdi, OP
    dec r9d
    jnz 2b
    xor ebp, ebp                 # total
5:  mov rbx, [rsp + 8]
6:  mov edx, [rbx]
    mov esi, [rbx + 8]
    mov edi, [rbx + 4]
    add rdi, r13
    call [rsp]
    mov eax, [rbx + 4]
    movzx ecx, byte ptr [r13 + rax]
    add rbp, rcx
    add eax, [rbx]
    movzx eax, byte ptr [r13 + rax - 1]
    add rbp, rax
    add rbx, OP
    cmp rbx, r13                 # the table ends where the block starts
    jb 6b
    dec r12
    jnz 5b
    mov rax, rbp
    add rsp, 24
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret

# The routine this CPU runs, in rax: fill_avx2 under AVX2 (CPUID.7:EBX[5])
# with OSXSAVE and AVX (CPUID.1:ECX[27,28]) and the OS saving SSE and YMM
# state (XCR0[1,2]), and ERMS (CPUID.7:EBX[9]); else fill_sse2.
    .type dispatch, @function
dispatch:
    push rbx
    lea r8, [rip + fill_sse2]
    xor eax, eax
    cpuid
    cmp eax, 7
    jb 9f
    mov eax, 1
    cpuid
    and ecx, 0x18000000
    cmp ecx, 0x18000000
    jne 9f
    xor ecx, ecx
    xgetbv
    and eax, 6
    cmp eax, 6
    jne 9f
    mov eax, 7
    xor ecx, ecx
    cpuid
    and ebx, 0x220
    cmp ebx, 0x220
    jne 9f
    lea r8, [rip + fill_avx2]
9:  mov rax, r8
    pop rbx
    ret
    .size dispatch, . - dispatch

# memset(rdi, sil, rdx) on `vec`-byte vectors (register prefix `v`, `mv` the
# unaligned store, `mva` the aligned store); `rep` adds the fast-string path.
# Clobbers rax, rcx, rdi, r8, r11 and vector register 0.
.macro FILL name, vec, v, mv, mva, rep
    .type \name, @function
\name:
    movzx eax, sil
    movabs rcx, 0x0101010101010101
    imul rax, rcx                # the byte in each byte of rax
    cmp rdx, \vec
    jb 20f
    SPREAD \vec
    cmp rdx, 2 * \vec
    ja 30f
    \mv [rdi], \v\()mm0
    \mv [rdi + rdx - \vec], \v\()mm0
    UPPER \vec
    ret
20:
.if \vec == 32
    cmp edx, 16
    jb 21f
    vmovq xmm0, rax
    vpunpcklqdq xmm0, xmm0, xmm0
    vmovdqu [rdi], xmm0
    vmovdqu [rdi + rdx - 16], xmm0
    ret
21:
.endif
    cmp edx, 8
    jb 22f
    mov [rdi], rax
    mov [rdi + rdx - 8], rax
    ret
22: cmp edx, 4
    jb 23f
    mov [rdi], eax
    mov [rdi + rdx - 4], eax
    ret
23: cmp edx, 1
    ja 24f
    jb 25f
    mov [rdi], al
25: ret
24: mov [rdi], ax
    mov [rdi + rdx - 2], ax
    ret
30: cmp rdx, 4 * \vec
    ja 40f
    \mv [rdi], \v\()mm0
    \mv [rdi + \vec], \v\()mm0
    \mv [rdi + rdx - 2 * \vec], \v\()mm0
    \mv [rdi + rdx - \vec], \v\()mm0
    UPPER \vec
    ret
40: cmp rdx, 8 * \vec
    ja 50f
    \mv [rdi], \v\()mm0
    \mv [rdi + \vec], \v\()mm0
    \mv [rdi + 2 * \vec], \v\()mm0
    \mv [rdi + 3 * \vec], \v\()mm0
    \mv [rdi + rdx - 4 * \vec], \v\()mm0
    \mv [rdi + rdx - 3 * \vec], \v\()mm0
    \mv [rdi + rdx - 2 * \vec], \v\()mm0
    \mv [rdi + rdx - \vec], \v\()mm0
    UPPER \vec
    ret
50: test rdx, rdx                # a negative count writes nothing
    js 59f
.if \rep
    cmp rdx, REP_FROM
    jb 60f
    UPPER \vec
    mov rcx, rdx
    rep stosb
    ret
.endif
60: lea r11, [rdi + rdx - 4 * \vec]
    \mv [rdi], \v\()mm0
    \mv [rdi + \vec], \v\()mm0
    \mv [rdi + 2 * \vec], \v\()mm0
    \mv [rdi + 3 * \vec], \v\()mm0
    lea r8, [rdi + 4 * \vec]
    and r8, -\vec                # the last aligned target at or below rdi + 4 vectors
61: \mva [r8], \v\()mm0
    \mva [r8 + \vec], \v\()mm0
    \mva [r8 + 2 * \vec], \v\()mm0
    \mva [r8 + 3 * \vec], \v\()mm0
    sub r8, -4 * \vec
    cmp r8, r11
    jb 61b
    \mv [r11], \v\()mm0
    \mv [r11 + \vec], \v\()mm0
    \mv [r11 + 2 * \vec], \v\()mm0
    \mv [r11 + 3 * \vec], \v\()mm0
59: UPPER \vec
    ret
    .size \name, . - \name
.endm

# The word in rax spread over vector register 0.
.macro SPREAD vec
.if \vec == 32
    vmovq xmm0, rax
    vpbroadcastq ymm0, xmm0
.else
    movq xmm0, rax
    punpcklqdq xmm0, xmm0
.endif
.endm

# Clears the upper YMM halves after a 32-byte path, so later SSE code pays
# no transition.
.macro UPPER vec
.if \vec == 32
    vzeroupper
.endif
.endm

    .p2align 5
    FILL fill_avx2, 32, y, vmovdqu, vmovdqa, 1
    .p2align 5
    FILL fill_sse2, 16, x, movdqu, movdqa, 0

size_bits: .byte 3, 4, 5, 6, 7, 9, 11, 14
prefix: .ascii "sum"
    .p2align 3
prefix_len: .quad 3
prefix2:
prefix2_len: .quad 0
