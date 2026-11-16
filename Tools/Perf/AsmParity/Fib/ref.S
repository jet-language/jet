# Doubly recursive Fibonacci, the algorithm of run.jet, written the way a
# hand coder (and GCC -O2) writes it: the kernel measures calls, so the
# recursion stays, but
# - the leaf test sits at the call site: `body` is entered only with n >= 2,
#   so no call is made for a leaf (as Jet's run_fib tests n < 2 before each
#   call);
# - the second call, fib(n - 2), is the tail of the sum, so it becomes the
#   loop: fib(n) = fib(n - 1) + fib(n - 3) + ... + fib(n - 2k - 1) + fib(r),
#   with r = n - 2k in {0, 1, 2}; only fib(n - 1) of each step is a call.
    .include "common.S"
    .text
kernel:
    cmp rdi, 2
    jb 3f
body:                           # rdi = n >= 2: rax = fib(n)
    push rbx
    push r12
    mov rbx, rdi
    xor r12d, r12d              # the sum of the fib(n - 1) of each step
1:  cmp rbx, 3
    jb 2f                       # n = 2: fib(1) + fib(0) = 1
    lea rdi, [rbx - 1]          # n - 1 >= 2: a call
    call body
    add r12, rax
    sub rbx, 2
    cmp rbx, 2
    jae 1b
    lea rax, [r12 + rbx]        # r = 0 or 1: fib(r) = r
    pop r12
    pop rbx
    ret
2:  lea rax, [r12 + 1]
    pop r12
    pop rbx
    ret
3:  mov rax, rdi
    ret

prefix: .ascii "fib"
    .p2align 3
prefix_len: .quad 3
prefix2:
prefix2_len: .quad 0
