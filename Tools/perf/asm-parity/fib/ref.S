# Naive doubly recursive Fibonacci, the algorithm of run.jet: the kernel
# measures calls, so the recursion stays.
    .include "common.S"
    .text
kernel:
    cmp rdi, 2
    jb 1f
    push rbx
    push r12
    mov rbx, rdi
    lea rdi, [rbx - 1]
    call kernel
    mov r12, rax
    lea rdi, [rbx - 2]
    call kernel
    add rax, r12
    pop r12
    pop rbx
    ret
1:  mov rax, rdi
    ret

prefix: .ascii "fib"
    .p2align 3
prefix_len: .quad 3
prefix2:
prefix2_len: .quad 0
