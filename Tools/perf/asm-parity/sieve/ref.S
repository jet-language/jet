# The gauntlet sieve (Tools/gauntlet/entries/sieve/jet/run.jet) by hand:
# one byte per odd number (slot i is 2i + 1; slot 0 stands in for 2), so
# `count` is the number of 1 bytes and `largest` is the last one's number.
    .include "common.S"
    .text
kernel:
    push rbx
    push r12
    push r13
    push r14
    mov r12, rdi                 # n
    mov r13, rdi
    shr r13, 1                   # slots
    # mmap(NULL, slots + 128 rounded to pages, RW, PRIVATE|ANON): the zero
    # bytes past `slots` let the count read whole 128-byte blocks.
    lea rsi, [r13 + 128 + 4095]
    and rsi, -4096
    xor edi, edi
    mov edx, 3
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    mov r14, rax
    xor eax, eax
    cmp r12, 2
    seta al                      # every slot starts as n > 2
    mov rdi, r14
    mov rcx, r13
    rep stosb
    cmp r12, 2
    jbe 9f
    mov ebx, 3                   # p
1:  mov rax, rbx
    imul rax, rbx                # p * p
    cmp rax, r12
    jae 4f
    mov rcx, rbx
    shr rcx, 1
    cmp byte ptr [r14 + rcx], 1
    jne 3f
    shr rax, 1                   # first slot to clear: p * p / 2
    cmp rax, r13
    jae 3f
2:  mov byte ptr [r14 + rax], 0
    add rax, rbx
    cmp rax, r13
    jb 2b
3:  add rbx, 2
    jmp 1b
    # count: sum of the 0/1 bytes, 128 at a time (vpsadbw into four
    # accumulators of 64-bit sums).
4:  vpxor ymm0, ymm0, ymm0
    vpxor ymm1, ymm1, ymm1
    vpxor ymm2, ymm2, ymm2
    vpxor ymm3, ymm3, ymm3
    vpxor ymm7, ymm7, ymm7
    xor ecx, ecx
5:  vpsadbw ymm4, ymm7, [r14 + rcx]
    vpaddq ymm0, ymm0, ymm4
    vpsadbw ymm5, ymm7, [r14 + rcx + 32]
    vpaddq ymm1, ymm1, ymm5
    vpsadbw ymm4, ymm7, [r14 + rcx + 64]
    vpaddq ymm2, ymm2, ymm4
    vpsadbw ymm5, ymm7, [r14 + rcx + 96]
    vpaddq ymm3, ymm3, ymm5
    sub rcx, -128
    cmp rcx, r13
    jb 5b
    vpaddq ymm0, ymm0, ymm1
    vpaddq ymm2, ymm2, ymm3
    vpaddq ymm0, ymm0, ymm2
    vextracti128 xmm1, ymm0, 1
    vpaddq xmm0, xmm0, xmm1
    vpshufd xmm1, xmm0, 0x4e
    vpaddq xmm0, xmm0, xmm1
    vmovq rax, xmm0              # count
    vzeroupper
    # largest: the last 1 byte, scanning back from the end.
    mov rcx, r13
6:  dec rcx
    js 7f
    cmp byte ptr [r14 + rcx], 1
    jne 6b
    lea rdx, [rcx + rcx + 1]
    jmp 8f
7:  mov edx, 1                   # no 1 byte: (0) * 2 + 1
8:  cmp rdx, 2
    ja 10f
    mov edx, 2
    jmp 10f
9:  xor eax, eax                 # n <= 2: every slot is 0
    xor edx, edx
10: pop r14
    pop r13
    pop r12
    pop rbx
    ret

prefix: .ascii "count"
    .p2align 3
prefix_len: .quad 5
prefix2: .ascii "largest"
    .p2align 3
prefix2_len: .quad 7
