# run.jet's work: each pass maps 64 KiB of fresh private pages with one
# mmap and unmaps them with one munmap. The mapping keeps the kernel's one
# check, the refusal test on mmap's result, at its smallest: a refused
# mapping exits 70 as Jet's panic does. Like Jet's region, each mapping is
# registered in a page map, a flat table with one word per 64 KiB unit of
# the 47-bit address space: one store of the base at map, one clear at
# unmap. The table is one NORESERVE mapping made once at start, as a
# runtime makes its page map before the first region, so it is not a use.
# Every line of the two calls (their arguments, the syscalls, the check and
# the page-map store and clear) is a use.
    .include "common.S"
    .text
    .set BYTES, 65536
    .set TABLE_BYTES, 1 << 34    # 2^31 units of 64 KiB, 8 bytes each

kernel:
    push rbx
    push rbp
    push r12
    mov r12, rdi                 # passes: n, or 1 when n is 0
    test r12, r12
    jnz 1f
    mov r12d, 1
1:  xor edi, edi                 # the page map's table
    movabs rsi, TABLE_BYTES
    mov edx, 3                   # PROT_READ | PROT_WRITE
    mov r10d, 0x4022             # MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE
    or r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja 3f
    mov rbx, rax                 # the table
    xor ebp, ebp                 # total
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
    mov rdi, rax                 # the mapping
    shr rax, 16                  # the mapping: its unit
    mov [rbx + rax*8], rdi       # the mapping: registered
    add rbp, BYTES
    mov qword ptr [rbx + rax*8], 0 # the mapping: unregistered
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
