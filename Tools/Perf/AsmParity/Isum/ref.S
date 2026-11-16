# run.jet's work: xs[i] = i / 3 for i < n, then 20 passes summing xs. The
# fill keeps the quotient in a counter; the sum uses four AVX2
# accumulators. No bounds checks: the loop bound is the length.
    .include "common.S"
    .text
kernel:
    push rbx
    mov rbx, rdi                 # n
    lea rsi, [rdi * 8 + 4095]    # mmap(NULL, n * 8 rounded, RW, PRIVATE|ANON)
    and rsi, -4096
    xor edi, edi
    mov edx, 3
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    mov r11, rax                 # xs
    xor ecx, ecx                 # i
    xor edx, edx                 # i / 3
    xor esi, esi                 # i % 3
    test rbx, rbx
    jz 3f
1:  mov [r11 + rcx * 8], rdx
    inc rcx
    inc esi
    cmp esi, 3
    jne 2f
    xor esi, esi
    inc rdx
2:  cmp rcx, rbx
    jb 1b
3:  vpxor ymm0, ymm0, ymm0
    vpxor ymm1, ymm1, ymm1
    vpxor ymm2, ymm2, ymm2
    vpxor ymm3, ymm3, ymm3
    xor r9d, r9d                 # the scalar tail's total
    mov r8d, 20
4:  xor ecx, ecx
    mov rdx, rbx
    and rdx, -16
    jz 6f
5:  vpaddq ymm0, ymm0, [r11 + rcx * 8]
    vpaddq ymm1, ymm1, [r11 + rcx * 8 + 32]
    vpaddq ymm2, ymm2, [r11 + rcx * 8 + 64]
    vpaddq ymm3, ymm3, [r11 + rcx * 8 + 96]
    add rcx, 16
    cmp rcx, rdx
    jb 5b
6:  cmp rcx, rbx
    jae 7f
    add r9, [r11 + rcx * 8]
    inc rcx
    jmp 6b
7:  dec r8d
    jnz 4b
    vpaddq ymm0, ymm0, ymm1
    vpaddq ymm2, ymm2, ymm3
    vpaddq ymm0, ymm0, ymm2
    vextracti128 xmm1, ymm0, 1
    vpaddq xmm0, xmm0, xmm1
    vpshufd xmm1, xmm0, 0x4e
    vpaddq xmm0, xmm0, xmm1
    vmovq rax, xmm0
    add rax, r9
    vzeroupper
    pop rbx
    ret

prefix: .ascii "sum"
    .p2align 3
prefix_len: .quad 3
prefix2:
prefix2_len: .quad 0
