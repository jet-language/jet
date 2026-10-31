# Total Collatz steps for every start in 1..<n, as run.jet counts them: one
# step per halving, one per 3x + 1. An odd x is always followed by a
# halving, so (3x + 1) / 2 counts two steps at once.
    .include "common.S"
    .text
kernel:
    xor eax, eax
    mov ecx, 1
    cmp rcx, rdi
    jae 9f
1:  mov rdx, rcx
    cmp rdx, 1
    je 4f
2:  test dl, 1
    jnz 3f
    shr rdx, 1
    inc rax
    cmp rdx, 1
    jne 2b
    jmp 4f
3:  lea rdx, [rdx + rdx * 2 + 1]
    shr rdx, 1
    add rax, 2
    cmp rdx, 1
    jne 2b
4:  inc rcx
    cmp rcx, rdi
    jb 1b
9:  ret

prefix: .ascii "steps"
    .p2align 3
prefix_len: .quad 5
prefix2:
prefix2_len: .quad 0
