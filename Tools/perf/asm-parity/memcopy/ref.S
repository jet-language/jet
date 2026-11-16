# run.jet's work: the same 4096 operations from the same generator, laid
# out the same way in one page-aligned block, each a copy and a move through
# one memmove routine picked once at load (`dispatch`) and called through a
# stack slot. The routine is the best known shape for its size classes:
# below one vector two overlapping scalar or 16-byte loads and stores; up to
# 2, 4 and 8 vectors that many unaligned loads, all before any store, so an
# overlap in either direction is safe; above 8 vectors a 4-vector loop with
# aligned stores, forward unless the target starts inside the source, the
# head and the last 4 vectors loaded before the loop and stored after it;
# and `rep movsb` from 2048 bytes when the ranges do not overlap (fast
# strings). The AVX2 routine needs AVX2 with the OS saving the YMM state and
# ERMS; any other x86-64 gets the same shape on SSE2 vectors without
# `rep movsb`. The kernel's one check, a count of zero or less moves
# nothing, costs nothing on the small paths (zero falls to the byte case's
# early return, a negative count to the large path) and one sign test above
# 8 vectors.
    .include "common.S"
    .text
    .set OPS, 4096
    .set OP, 20                  # size, from, to, move_from, move_to: 4 bytes each
    .set LARGEST, 16384
    .set COPY_ROOM, 64
    .set MOVE_ROOM, 256
    .set TARGET_AT, 18432
    .set MOVED_AT, 36864
    .set BLOCK, MOVED_AT + LARGEST + MOVE_ROOM
    .set MAPPED, 139264          # OPS * OP + BLOCK, in whole pages
    .set REP_FROM, 2048

kernel:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
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
    lea r13, [rax + OPS * OP]    # the block (page-aligned) and the source
    lea r14, [r13 + TARGET_AT]
    lea r15, [r13 + MOVED_AT]
    # The table: state = (state * 1103515245 + 12345) & 0x7fffffff.
    mov rdi, rax
    lea r8, [rip + size_bits]
    mov eax, 12345
    mov r9d, OPS
2:  imul rax, rax, 1103515245
    add rax, 12345
    and eax, 0x7fffffff
    mov edx, eax
    and edx, 7
    movzx ecx, byte ptr [r8 + rdx]
    mov edx, 1
    shl edx, cl
    dec edx
    mov r10d, eax
    shr r10d, 3
    and r10d, edx
    inc r10d
    mov [rdi], r10d              # size
    imul rax, rax, 1103515245
    add rax, 12345
    and eax, 0x7fffffff
    mov edx, eax
    and edx, 63
    mov [rdi + 4], edx           # from
    mov edx, eax
    shr edx, 6
    and edx, 63
    mov [rdi + 8], edx           # to
    mov edx, eax
    shr edx, 12
    and edx, 255
    mov [rdi + 12], edx          # move_from
    mov edx, eax
    shr edx, 20
    and edx, 255
    mov [rdi + 16], edx          # move_to
    add rdi, OP
    dec r9d
    jnz 2b
    xor ecx, ecx                 # source[i] = i * 7 + 3
    mov eax, 3
3:  mov [r13 + rcx], al
    add eax, 7
    inc rcx
    cmp rcx, LARGEST + COPY_ROOM
    jb 3b
    mov rdi, r14                 # the target cleared
    xor eax, eax
    mov ecx, LARGEST + COPY_ROOM
    rep stosb
    xor ecx, ecx                 # moved[i] = i
4:  mov [r15 + rcx], cl
    inc rcx
    cmp rcx, LARGEST + MOVE_ROOM
    jb 4b
    xor ebp, ebp                 # total
5:  mov rbx, [rsp + 8]
6:  mov edx, [rbx]
    mov esi, [rbx + 4]
    add rsi, r13
    mov edi, [rbx + 8]
    add rdi, r14
    call [rsp]
    mov edx, [rbx]
    mov esi, [rbx + 12]
    add rsi, r15
    mov edi, [rbx + 16]
    add rdi, r15
    call [rsp]
    mov eax, [rbx + 8]
    add eax, [rbx]
    movzx eax, byte ptr [r14 + rax - 1]
    add rbp, rax
    mov eax, [rbx + 16]
    movzx eax, byte ptr [r15 + rax]
    add rbp, rax
    add rbx, OP
    cmp rbx, r13                 # the table ends where the block starts
    jb 6b
    dec r12
    jnz 5b
    mov rax, rbp
    add rsp, 24
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret

# The routine this CPU runs, in rax: move_avx2 under AVX2 (CPUID.7:EBX[5])
# with OSXSAVE and AVX (CPUID.1:ECX[27,28]) and the OS saving SSE and YMM
# state (XCR0[1,2]), and ERMS (CPUID.7:EBX[9]); else move_sse2.
    .type dispatch, @function
dispatch:
    push rbx
    lea r8, [rip + move_sse2]
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
    lea r8, [rip + move_avx2]
9:  mov rax, r8
    pop rbx
    ret
    .size dispatch, . - dispatch

# memmove(rdi, rsi, rdx) on `vec`-byte vectors (register prefix `v`, `mv`
# the unaligned move, `mva` the aligned store); `rep` adds the fast-string
# path. Clobbers rax, rcx, rdx, rsi, rdi, r8, r9, r11 and vector registers
# 0 to 8.
.macro MOVE name, vec, v, mv, mva, rep
    .type \name, @function
\name:
    cmp rdx, \vec
    jb 20f
    cmp rdx, 2 * \vec
    ja 30f
    \mv \v\()mm0, [rsi]
    \mv \v\()mm1, [rsi + rdx - \vec]
    \mv [rdi], \v\()mm0
    \mv [rdi + rdx - \vec], \v\()mm1
    UPPER \vec
    ret
20:
.if \vec == 32
    cmp edx, 16
    jb 21f
    vmovdqu xmm0, [rsi]
    vmovdqu xmm1, [rsi + rdx - 16]
    vmovdqu [rdi], xmm0
    vmovdqu [rdi + rdx - 16], xmm1
    ret
21:
.endif
    cmp edx, 8
    jb 22f
    mov rcx, [rsi]
    mov rax, [rsi + rdx - 8]
    mov [rdi], rcx
    mov [rdi + rdx - 8], rax
    ret
22: cmp edx, 4
    jb 23f
    mov ecx, [rsi]
    mov eax, [rsi + rdx - 4]
    mov [rdi], ecx
    mov [rdi + rdx - 4], eax
    ret
23: cmp edx, 1
    ja 24f
    jb 25f
    movzx ecx, byte ptr [rsi]
    mov [rdi], cl
25: ret
24: movzx ecx, word ptr [rsi]
    movzx eax, word ptr [rsi + rdx - 2]
    mov [rdi], cx
    mov [rdi + rdx - 2], ax
    ret
30: cmp rdx, 4 * \vec
    ja 40f
    \mv \v\()mm0, [rsi]
    \mv \v\()mm1, [rsi + \vec]
    \mv \v\()mm2, [rsi + rdx - 2 * \vec]
    \mv \v\()mm3, [rsi + rdx - \vec]
    \mv [rdi], \v\()mm0
    \mv [rdi + \vec], \v\()mm1
    \mv [rdi + rdx - 2 * \vec], \v\()mm2
    \mv [rdi + rdx - \vec], \v\()mm3
    UPPER \vec
    ret
40: cmp rdx, 8 * \vec
    ja 50f
    \mv \v\()mm0, [rsi]
    \mv \v\()mm1, [rsi + \vec]
    \mv \v\()mm2, [rsi + 2 * \vec]
    \mv \v\()mm3, [rsi + 3 * \vec]
    \mv \v\()mm4, [rsi + rdx - 4 * \vec]
    \mv \v\()mm5, [rsi + rdx - 3 * \vec]
    \mv \v\()mm6, [rsi + rdx - 2 * \vec]
    \mv \v\()mm7, [rsi + rdx - \vec]
    \mv [rdi], \v\()mm0
    \mv [rdi + \vec], \v\()mm1
    \mv [rdi + 2 * \vec], \v\()mm2
    \mv [rdi + 3 * \vec], \v\()mm3
    \mv [rdi + rdx - 4 * \vec], \v\()mm4
    \mv [rdi + rdx - 3 * \vec], \v\()mm5
    \mv [rdi + rdx - 2 * \vec], \v\()mm6
    \mv [rdi + rdx - \vec], \v\()mm7
    UPPER \vec
    ret
50: test rdx, rdx                # a negative count moves nothing
    js 59f
    mov rcx, rdi
    sub rcx, rsi
    cmp rcx, rdx
    jb 70f                       # the target starts inside the source
.if \rep
    cmp rdx, REP_FROM
    jb 60f
    mov rax, rsi
    sub rax, rdi
    cmp rax, rdx
    jb 60f                       # the source starts inside the target
    mov rcx, rdx
    rep movsb
    ret
.endif
60: \mv \v\()mm4, [rsi]
    \mv \v\()mm5, [rsi + rdx - \vec]
    \mv \v\()mm6, [rsi + rdx - 2 * \vec]
    \mv \v\()mm7, [rsi + rdx - 3 * \vec]
    \mv \v\()mm8, [rsi + rdx - 4 * \vec]
    lea r11, [rdi + rdx - 4 * \vec]
    mov r9, rdi
    lea r8, [rdi + \vec]
    and r8, -\vec                # the first aligned target above rdi
    sub rsi, rdi
    add rsi, r8
61: \mv \v\()mm0, [rsi]
    \mv \v\()mm1, [rsi + \vec]
    \mv \v\()mm2, [rsi + 2 * \vec]
    \mv \v\()mm3, [rsi + 3 * \vec]
    \mva [r8], \v\()mm0
    \mva [r8 + \vec], \v\()mm1
    \mva [r8 + 2 * \vec], \v\()mm2
    \mva [r8 + 3 * \vec], \v\()mm3
    sub rsi, -4 * \vec
    sub r8, -4 * \vec
    cmp r8, r11
    jb 61b
    \mv [r11 + 3 * \vec], \v\()mm5
    \mv [r11 + 2 * \vec], \v\()mm6
    \mv [r11 + \vec], \v\()mm7
    \mv [r11], \v\()mm8
    \mv [r9], \v\()mm4
    UPPER \vec
59: ret
70: \mv \v\()mm4, [rsi + rdx - \vec]
    \mv \v\()mm5, [rsi]
    \mv \v\()mm6, [rsi + \vec]
    \mv \v\()mm7, [rsi + 2 * \vec]
    \mv \v\()mm8, [rsi + 3 * \vec]
    lea r9, [rdi + rdx - \vec]
    lea r8, [rdi + rdx]
    and r8, -\vec                # the last aligned target end at or below the end
    sub rsi, rdi
    add rsi, r8
    lea r11, [rdi + 4 * \vec]
71: \mv \v\()mm0, [rsi - \vec]
    \mv \v\()mm1, [rsi - 2 * \vec]
    \mv \v\()mm2, [rsi - 3 * \vec]
    \mv \v\()mm3, [rsi - 4 * \vec]
    \mva [r8 - \vec], \v\()mm0
    \mva [r8 - 2 * \vec], \v\()mm1
    \mva [r8 - 3 * \vec], \v\()mm2
    \mva [r8 - 4 * \vec], \v\()mm3
    add rsi, -4 * \vec
    add r8, -4 * \vec
    cmp r8, r11
    ja 71b
    \mv [rdi], \v\()mm5
    \mv [rdi + \vec], \v\()mm6
    \mv [rdi + 2 * \vec], \v\()mm7
    \mv [rdi + 3 * \vec], \v\()mm8
    \mv [r9], \v\()mm4
    UPPER \vec
    ret
    .size \name, . - \name
.endm

# Clears the upper YMM halves after a 32-byte path, so later SSE code pays
# no transition.
.macro UPPER vec
.if \vec == 32
    vzeroupper
.endif
.endm

    .p2align 5
    MOVE move_avx2, 32, y, vmovdqu, vmovdqa, 1
    .p2align 5
    MOVE move_sse2, 16, x, movdqu, movdqa, 0

size_bits: .byte 3, 4, 5, 6, 7, 9, 11, 14
prefix: .ascii "sum"
    .p2align 3
prefix_len: .quad 3
prefix2:
prefix2_len: .quad 0
