# run.jet's work: the same 4096 operations from the same generator over the
# same 256 slots. Each one releases the block its slot holds and takes a fresh
# block of the drawn size. The heap is a minimal hand slab allocator with the
# three things a real heap needs:
# - size classes: one free list per class of 16 bytes (1 to 64 units of 16
#   bytes covers 1 byte to 1 KiB);
# - a page map: one word per 64 KiB unit of one aligned reservation names the
#   slab, so `free` takes no size;
# - a cross-thread free path: a slab records the heap that owns it. A block
#   freed by another heap goes onto the slab's remote list with one
#   `lock cmpxchg`, and the owner takes that whole list back with one `xchg`
#   when its class list runs dry.
# The heap's pointer lives in r15, where a threaded heap would read it from
# thread-local storage. A slab that is replaced as its class's current slab
# keeps its remote list until the next refill of that class reaches it; a
# single heap never fills one. A refused take exits 70, as Jet's panic does.
# The uses are the lines of the two calls and the refusal check.
    .include "common.S"
    .text
    .set OPS, 4096
    .set SLOTS, 256
    .set SLAB, 65536             # a slab, and the page map's unit
    .set RESERVE, 67174400       # 1024 units, plus one for the alignment
    # The heap's own unit, at the reservation's first aligned address:
    .set CUR, 1024               # each class's current slab
    .set NEXT, 2048              # the next fresh slab
    .set LIMIT, 2056             # the last slab start that fits
    .set PMAP, 4096              # the page map: the slab of each unit
    .set LIVE, 16384             # the slots
    .set TABLE, 20480            # the operations: slot, size (4 bytes each)

kernel:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    mov r12, rdi                 # passes: n, or 1 when n is 0
    test r12, r12
    jnz 1f
    mov r12d, 1
1:  xor edi, edi                 # mmap(NULL, RESERVE, RW, PRIVATE|ANON|NORESERVE)
    mov esi, RESERVE
    mov edx, 3
    mov r10d, 0x4022
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja 9f
    lea r15, [rax + SLAB - 1]
    and r15, -SLAB               # the heap
    lea rdx, [r15 + SLAB]
    mov [r15 + NEXT], rdx
    lea rdx, [rax + RESERVE - SLAB]
    mov [r15 + LIMIT], rdx
    lea r14, [r15 + LIVE]
    lea r13, [r15 + TABLE]
    # The table: state = (state * 1103515245 + 12345) & 0x7fffffff.
    mov rdi, r13
    mov eax, 12345
    mov r9d, OPS
2:  imul rax, rax, 1103515245
    add rax, 12345
    and eax, 0x7fffffff
    mov ecx, eax
    shr ecx, 28
    add ecx, 3
    mov edx, 1
    shl edx, cl
    dec edx
    mov r10d, eax
    shr r10d, 12
    and r10d, edx
    inc r10d
    mov [rdi + 4], r10d          # size
    imul rax, rax, 1103515245
    add rax, 12345
    and eax, 0x7fffffff
    mov edx, eax
    shr edx, 16
    and edx, SLOTS - 1
    mov [rdi], edx               # slot
    add rdi, 8
    dec r9d
    jnz 2b
    xor ebp, ebp                 # total
5:  mov rbx, r13
6:  mov eax, [rbx]
    mov rdi, [r14 + rax * 8]
    test rdi, rdi
    jz 7f
    call free                    # the heap
7:  mov edi, [rbx + 4]           # the heap
    call alloc                   # the heap
    test rax, rax                # the heap
    jz 9f                        # the heap
    mov ecx, [rbx]
    mov [r14 + rcx * 8], rax
    mov eax, [rbx + 4]
    add rbp, rax
    add rbx, 8
    lea rax, [r13 + OPS * 8]
    cmp rbx, rax
    jb 6b
    dec r12
    jnz 5b
    xor ebx, ebx                 # release what the slots hold
8:  mov rdi, [r14 + rbx * 8]
    test rdi, rdi
    jz 4f
    call free
4:  inc ebx
    cmp ebx, SLOTS
    jb 8b
    mov rax, rbp
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret
9:  mov edi, 70                  # no memory
    mov eax, 231
    syscall

# A block of rdi bytes (1 to 1024) from the heap at r15, in rax; 0 when the
# reservation is spent. Slab header: class, owner heap, remote list, bump.
# Clobbers rcx, rdx, r8, r9.
    .type alloc, @function
alloc:
    lea rcx, [rdi + 15]
    shr rcx, 4                   # the class
    mov rax, [r15 + rcx * 8]
    test rax, rax
    jz 1f
    mov rdx, [rax]
    mov [r15 + rcx * 8], rdx
    ret
1:  mov r8, [r15 + CUR + rcx * 8]
    test r8, r8
    jz 3f
    xor eax, eax
    xchg rax, [r8 + 16]          # the blocks other heaps freed on the slab
    test rax, rax
    jz 2f
    mov rdx, [rax]
    mov [r15 + rcx * 8], rdx
    ret
2:  mov rax, [r8 + 24]           # the slab's untouched rest
    mov rdx, rcx
    shl rdx, 4
    add rdx, rax
    lea r9, [r8 + SLAB]
    cmp rdx, r9
    ja 3f
    mov [r8 + 24], rdx
    ret
3:  mov r8, [r15 + NEXT]         # a fresh slab
    cmp r8, [r15 + LIMIT]
    ja 4f
    lea rdx, [r8 + SLAB]
    mov [r15 + NEXT], rdx
    mov [r8], rcx
    mov [r8 + 8], r15
    mov qword ptr [r8 + 16], 0
    mov rdx, r8
    sub rdx, r15
    shr rdx, 16
    mov [r15 + PMAP + rdx * 8], r8
    mov [r15 + CUR + rcx * 8], r8
    lea rax, [r8 + 32]
    mov rdx, rcx
    shl rdx, 4
    add rdx, rax
    mov [r8 + 24], rdx
    ret
4:  xor eax, eax
    ret
    .size alloc, . - alloc

# Returns the block at rdi to the heap at r15: onto its class list when the
# heap owns the block's slab, else onto the slab's remote list.
# Clobbers rax, rcx, rdx, r8.
    .type free, @function
free:
    mov rax, rdi
    sub rax, r15
    shr rax, 16
    mov r8, [r15 + PMAP + rax * 8] # the block's slab
    cmp [r8 + 8], r15
    jne 1f
    mov rcx, [r8]
    mov rdx, [r15 + rcx * 8]
    mov [rdi], rdx
    mov [r15 + rcx * 8], rdi
    ret
1:  mov rax, [r8 + 16]
2:  mov [rdi], rax
    lock cmpxchg [r8 + 16], rdi
    jne 2b
    ret
    .size free, . - free

prefix: .ascii "sum"
    .p2align 3
prefix_len: .quad 3
prefix2:
prefix2_len: .quad 0
