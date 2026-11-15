# run.jet's work: the same 4096 operations from the same generator over one
# page-aligned block of 8192 words, each a read of the word at one index and
# a write of it plus the step at another. A raw access is its one
# instruction: the index scaled into the address operand, the word moved in
# or out. Nothing is checked, as nothing is on the Jet side's proved path:
# the indexes are below WORDS by construction.
    .include "common.S"
    .text
    .set OPS, 4096
    .set OP, 12                  # from, to, step: 4 bytes each
    .set WORDS, 8192
    .set MAPPED, 114688          # OPS * OP + WORDS * 8, in whole pages

kernel:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    mov r12, rdi                 # passes: n, or 1 when n is 0
    test r12, r12
    jnz 1f
    mov r12d, 1
1:  xor edi, edi                 # mmap(NULL, MAPPED, RW, PRIVATE|ANON): zeroed
    mov esi, MAPPED
    mov edx, 3
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    mov r14, rax                 # the table
    lea r13, [rax + OPS * OP]    # the words (page-aligned)
    # The table: state = (state * 1103515245 + 12345) & 0x7fffffff.
    mov rdi, rax
    mov eax, 12345
    mov r9d, OPS
2:  imul rax, rax, 1103515245
    add rax, 12345
    and eax, 0x7fffffff
    mov edx, eax
    and edx, WORDS - 1
    mov [rdi], edx               # from
    mov edx, eax
    shr edx, 13
    and edx, WORDS - 1
    mov [rdi + 4], edx           # to
    mov edx, eax
    shr edx, 26
    and edx, 31
    mov [rdi + 8], edx           # step
    add rdi, OP
    dec r9d
    jnz 2b
    xor ebp, ebp                 # total
5:  mov rbx, r14
6:  mov eax, [rbx]
    mov ecx, [rbx + 4]
    mov edx, [rbx + 8]
    mov rax, [r13 + rax * 8]     # the raw read
    add rdx, rax
    mov [r13 + rcx * 8], rdx     # the raw write
    add rbp, rax
    add rbx, OP
    cmp rbx, r13                 # the table ends where the words start
    jb 6b
    dec r12
    jnz 5b
    mov rax, rbp
    pop r14
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
