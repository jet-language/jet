# run.jet's work: 4096 rounds per pass on one counter word. Each round does
# a lock xadd of the step (its old value read back), a load, and a lock
# cmpxchg of the loaded value for one more, whose ZF is the exchange won.
# Each pass ends with an xchg store of zero. An atomic is its one
# instruction on the word, sequentially consistent as x86-64 maps it, with
# the word's address in a register. Nothing is checked: Jet's atomics check
# nothing either.
    .include "common.S"
    .text
    .set ROUNDS, 4096

kernel:
    push rbx
    push rbp
    push r12
    push r13
    push 0                       # the counter word
    mov rbx, rsp
    mov r12, rdi                 # passes: n, or 1 when n is 0
    test r12, r12
    jnz 1f
    mov r12d, 1
1:  xor ebp, ebp                 # total
2:  xor r13d, r13d               # round
3:  mov edx, r13d
    and edx, 31                  # step
    lock xadd [rbx], rdx         # the atomic add
    add rbp, rdx
    mov rax, [rbx]               # the atomic load
    lea rcx, [rax + 1]
    lock cmpxchg [rbx], rcx      # the atomic compare_exchange
    jne 4f
    inc rbp
4:  inc r13d
    cmp r13d, ROUNDS
    jb 3b
    xor eax, eax
    xchg [rbx], rax              # the atomic store
    dec r12
    jnz 2b
    mov rax, rbp
    pop rcx
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret

prefix: .ascii "sum"
    .p2align 3
prefix_len: .quad 3
prefix2:
prefix2_len: .quad 0
