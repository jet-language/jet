# run.jet's work: the same 1024 pairs from the same generator, each list in
# one mapped block at a 16-byte boundary as the heap places a list's bytes,
# and the same flip of one right list's first byte at the start of each
# pass. Each compare calls one equality routine, picked once at load
# (`dispatch`) and called through a stack slot. The pairs of a cell always
# have equal lengths, so the routine takes the one size. Its shape is the
# best known one for its size classes: below 8 bytes two overlapping word,
# halfword or byte compares; below 16 two overlapping 8-byte xors; up to 2
# vectors one overlapping compare of each end; above that a 2-vector loop
# that stops at the first difference, then the last 2 vectors overlapped.
# The AVX2 routine needs AVX2 with the OS saving the YMM state and does 16
# to 31 bytes with two 16-byte compares; any other x86-64 gets the same shape
# on SSE2 vectors. Each line of the compare's call is a use.
    .include "common.S"
    .text
    .set PAIRS, 1024
    .set OP, 12                  # size, left, right: 4 bytes each, from the table
    .set MAPPED, 33603584        # PAIRS * OP + PAIRS * 2 * 16384, in whole pages

kernel:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    sub rsp, 8                   # [rsp]: the routine
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
    mov r13, rax                 # the table, then the lists
    lea r14, [rax + PAIRS * OP]  # the next list (16-byte aligned)
    # The pairs: state = (state * 1103515245 + 12345) & 0x7fffffff.
    lea r8, [rip + size_bits]
    mov rdi, r13
    mov eax, 12345
    mov r9d, PAIRS
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
    inc r10d                     # size
    mov [rdi], r10d
    mov rdx, r14                 # left
    sub rdx, r13
    mov [rdi + 4], edx
    lea r15, [r10 + 15]
    and r15, -16                 # the size in whole 16-byte units
    lea rbx, [r14 + r15]         # right
    mov rdx, rbx
    sub rdx, r13
    mov [rdi + 8], edx
    xor ecx, ecx                 # at
    mov esi, 3                   # byte at: at * 7 + 3, in a byte
3:  mov [r14 + rcx], sil
    mov [rbx + rcx], sil
    add esi, 7
    inc rcx
    cmp rcx, r10
    jb 3b
    imul rax, rax, 1103515245
    add rax, 12345
    and eax, 0x7fffffff
    mov edx, eax
    shr edx, 30                  # the pair differs?
    jz 4f
    mov edx, eax
    and edx, 0x3fffffff
    imul rdx, r10
    shr rdx, 30                  # where
    xor byte ptr [rbx + rdx], 1
4:  lea r14, [rbx + r15]
    add rdi, OP
    dec r9d
    jnz 2b
    lea r15, [r13 + PAIRS * OP]  # the table's end
    xor ebp, ebp                 # total
    xor r14d, r14d               # pass
5:  mov eax, r14d
    and eax, PAIRS - 1           # the flipped pair
    lea rax, [rax + rax * 2]
    mov edx, [r13 + rax * 4 + 8]
    xor byte ptr [r13 + rdx], 1
    mov rbx, r13
6:  mov edx, [rbx]               # the compare
    mov edi, [rbx + 4]           # the compare
    add rdi, r13                 # the compare
    mov esi, [rbx + 8]           # the compare
    add rsi, r13                 # the compare
    call [rsp]                   # the compare
    add rbp, rax
    add rbx, OP
    cmp rbx, r15
    jb 6b
    inc r14
    dec r12
    jnz 5b
    mov rax, rbp
    add rsp, 8
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret

# The routine this CPU runs, in rax: equal_avx2 under AVX2 (CPUID.7:EBX[5])
# with OSXSAVE and AVX (CPUID.1:ECX[27,28]) and the OS saving SSE and YMM
# state (XCR0[1,2]); else equal_sse2.
    .type dispatch, @function
dispatch:
    push rbx
    lea r8, [rip + equal_sse2]
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
    test ebx, 0x20
    jz 9f
    lea r8, [rip + equal_avx2]
9:  mov rax, r8
    pop rbx
    ret
    .size dispatch, . - dispatch

# Vector register `dst`: 0xff in each byte where `a` and `b` agree. A VEX
# compare reads `b` unaligned; SSE2 loads it into `tmp` first.
.macro SAME vec, dst, tmp, a, b
.if \vec == 32
    vmovdqu \dst, \a
    vpcmpeqb \dst, \dst, \b
.else
    movdqu \dst, \a
    movdqu \tmp, \b
    pcmpeqb \dst, \tmp
.endif
.endm

# ecx: the byte mask of vector registers 0 and 1 both agreeing.
.macro BOTH vec, v
.if \vec == 32
    vpand \v\()mm0, \v\()mm0, \v\()mm1
    vpmovmskb ecx, \v\()mm0
.else
    pand \v\()mm0, \v\()mm1
    pmovmskb ecx, \v\()mm0
.endif
.endm

# Clears the upper YMM halves after a 32-byte path, so later SSE code pays
# no transition.
.macro UPPER vec
.if \vec == 32
    vzeroupper
.endif
.endm

# rax = 1 when the rdx bytes at rdi and rsi agree, else 0, on `vec`-byte
# vectors (register prefix `v`; `full` is the mask of a whole agreeing
# vector). Clobbers rcx, rdi, r8, r9 and vector registers 0 to 2.
.macro EQUAL name, vec, v, full
    .type \name, @function
\name:
    cmp rdx, \vec
    jb 20f
    cmp rdx, 2 * \vec
    ja 30f
    SAME \vec, \v\()mm0, \v\()mm2, [rdi], [rsi]
    SAME \vec, \v\()mm1, \v\()mm2, [rdi + rdx - \vec], [rsi + rdx - \vec]
    BOTH \vec, \v
    xor eax, eax
    cmp ecx, \full
    sete al
    UPPER \vec
    ret
30: lea r8, [rdi + rdx - 2 * \vec] # the last 2 vectors
    mov r9, rsi
    sub r9, rdi                  # the right list, from the left
31: SAME \vec, \v\()mm0, \v\()mm2, [rdi], [rdi + r9]
    SAME \vec, \v\()mm1, \v\()mm2, [rdi + \vec], [rdi + r9 + \vec]
    BOTH \vec, \v
    cmp ecx, \full
    jne 39f
    add rdi, 2 * \vec
    cmp rdi, r8
    jb 31b
    SAME \vec, \v\()mm0, \v\()mm2, [r8], [r8 + r9]
    SAME \vec, \v\()mm1, \v\()mm2, [r8 + \vec], [r8 + r9 + \vec]
    BOTH \vec, \v
    xor eax, eax
    cmp ecx, \full
    sete al
    UPPER \vec
    ret
39: xor eax, eax
    UPPER \vec
    ret
20:
.if \vec == 32
    cmp edx, 16
    jb 21f
    vmovdqu xmm0, [rdi]
    vpcmpeqb xmm0, xmm0, [rsi]
    vmovdqu xmm1, [rdi + rdx - 16]
    vpcmpeqb xmm1, xmm1, [rsi + rdx - 16]
    vpand xmm0, xmm0, xmm1
    vpmovmskb ecx, xmm0
    xor eax, eax
    cmp ecx, 0xffff
    sete al
    ret
21:
.endif
    cmp edx, 8
    jb 22f
    mov rcx, [rdi]
    xor rcx, [rsi]
    mov r8, [rdi + rdx - 8]
    xor r8, [rsi + rdx - 8]
    xor eax, eax
    or rcx, r8
    sete al
    ret
22: cmp edx, 4
    jb 23f
    mov ecx, [rdi]
    xor ecx, [rsi]
    mov r8d, [rdi + rdx - 4]
    xor r8d, [rsi + rdx - 4]
    xor eax, eax
    or ecx, r8d
    sete al
    ret
23: cmp edx, 2
    jb 24f
    movzx ecx, word ptr [rdi]
    movzx r8d, word ptr [rsi]
    xor ecx, r8d
    movzx r8d, word ptr [rdi + rdx - 2]
    movzx r9d, word ptr [rsi + rdx - 2]
    xor r8d, r9d
    xor eax, eax
    or ecx, r8d
    sete al
    ret
24: mov eax, 1
    test edx, edx
    jz 25f
    movzx ecx, byte ptr [rdi]
    xor eax, eax
    cmp cl, [rsi]
    sete al
25: ret
    .size \name, . - \name
.endm

    .p2align 5
    EQUAL equal_avx2, 32, y, -1
    .p2align 5
    EQUAL equal_sse2, 16, x, 0xffff

size_bits: .byte 3, 4, 5, 6, 7, 9, 11, 14
prefix: .ascii "sum"
    .p2align 3
prefix_len: .quad 3
prefix2:
prefix2_len: .quad 0
