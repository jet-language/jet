# run.jet's work: each pass maps 64 KiB of fresh private pages with one
# mmap and unmaps them with one munmap. The mapping keeps the kernel's one
# check, the refusal test on mmap's result, at its smallest: a refused
# mapping exits 70 as Jet's panic does. Every line of the two calls (their
# arguments, the syscalls and the check) is a use.
    .include "common.S"
    .text
    .set BYTES, 65536

kernel:
    push rbx
    push rbp
    push r12
    mov r12, rdi                 # passes: n, or 1 when n is 0
    test r12, r12
    jnz 1f
    mov r12d, 1
1:  xor ebp, ebp                 # total
2:  xor edi, edi                 # the mapping
    mov esi, BYTES               # the mapping
    mov edx, 3                   # the mapping: PROT_READ | PROT_WRITE
    mov r10d, 0x22               # the mapping: MAP_PRIVATE | MAP_ANONYMOUS
    or r8, -1                    # the mapping: no file
    xor r9d, r9d                 # the mapping
    mov eax, 9                   # the mapping: mmap
    syscall                      # the mapping
    cmp rax, -4096               # the mapping: refused?
    ja 3f                        # the mapping
    add rbp, BYTES
    mov rdi, rax                 # the mapping
    mov esi, BYTES               # the mapping
    mov eax, 11                  # the mapping: munmap
    syscall                      # the mapping
    dec r12
    jnz 2b
    mov rax, rbp
    pop r12
    pop rbp
    pop rbx
    ret
3:  mov edi, 70                  # the mapping: refused
    mov eax, 231                 # the mapping
    syscall                      # the mapping

prefix: .ascii "sum"
    .p2align 3
prefix_len: .quad 3
prefix2:
prefix2_len: .quad 0
