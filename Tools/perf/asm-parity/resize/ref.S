# run.jet's work: the same 4096 appends from the same generator over the same
# 256 byte lists, with the same reset at 1 KiB. A list is its buffer, its
# length and its capacity; a full buffer grows to twice its capacity, or to
# the new length when that is more, as a Jet List does.
# The heap is ../allocfree/ref.S's minimal slab, with classes up to 2 KiB:
# 16-byte size classes, a page map over one aligned reservation, and the
# remote list for a block another heap frees. A buffer's capacity is its
# class's size, so the resize is that slab's: an append that fits the block's
# class grows in place, and one that does not moves to a block of the larger
# class and releases the old one.
# The append's bytes are copied by `copy`, which moves 16 bytes at a time with
# an overlapping last move, and small counts with two overlapping words.
# A refused growth exits 70. The uses are the lines from the capacity check
# through the length update.
    .include "common.S"
    .text
    .set OPS, 4096
    .set SLOTS, 256
    .set LIMIT_BYTES, 1024
    .set SLAB, 65536             # a slab, and the page map's unit
    .set RESERVE, 67174400       # 1024 units, plus one for the alignment
    # The heap's own unit, at the reservation's first aligned address:
    .set CUR, 2048               # each class's current slab
    .set NEXT, 4096              # the next fresh slab
    .set LIMIT, 4104             # the last slab start that fits
    .set PASSES, 4112            # the passes left
    .set PMAP, 8192              # the page map: the slab of each unit
    .set LIVE, 16384             # the lists: buffer, length, capacity
    .set TABLE, 24576            # the operations: slot, count (4 bytes each)
    .set SOURCE, 57344           # the 64 bytes the appends read

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
    mov [r15 + PASSES], r12
    lea r14, [r15 + LIVE]
    lea r13, [r15 + TABLE]
    xor eax, eax                 # the source: 0, 1, ..., 63
2:  mov [r15 + SOURCE + rax], al
    inc eax
    cmp eax, 64
    jb 2b
    # The table: state = (state * 1103515245 + 12345) & 0x7fffffff.
    mov rdi, r13
    mov eax, 12345
    mov r9d, OPS
3:  imul rax, rax, 1103515245
    add rax, 12345
    and eax, 0x7fffffff
    mov ecx, eax
    shr ecx, 28
    and ecx, 3
    add ecx, 3
    mov edx, 1
    shl edx, cl
    dec edx
    mov r10d, eax
    shr r10d, 12
    and r10d, edx
    inc r10d
    mov [rdi + 4], r10d          # count
    imul rax, rax, 1103515245
    add rax, 12345
    and eax, 0x7fffffff
    mov edx, eax
    shr edx, 16
    and edx, SLOTS - 1
    mov [rdi], edx               # slot
    add rdi, 8
    dec r9d
    jnz 3b
    xor ebp, ebp                 # total
5:  mov rbx, r13
6:  mov eax, [rbx]
    lea rax, [rax + rax * 2]
    lea r12, [r14 + rax * 8]     # the list
    mov edx, [rbx + 4]
    mov rcx, [r12 + 8]
    add rcx, rdx                 # the new length
    cmp rcx, LIMIT_BYTES
    jbe 7f
    mov rdi, [r12]               # start over: release the buffer
    call free
    xor eax, eax
    mov [r12], rax
    mov [r12 + 8], rax
    mov [r12 + 16], rax
    mov ecx, [rbx + 4]
7:  cmp rcx, [r12 + 16]          # the resize
    jbe 8f                       # the resize
    call grow                    # the resize
    test rax, rax                # the resize
    jz 9f                        # the resize
8:  mov rdi, [r12]               # the resize
    add rdi, [r12 + 8]           # the resize
    lea rsi, [r15 + SOURCE]      # the resize
    mov edx, [rbx + 4]           # the resize
    call copy                    # the resize
    mov edx, [rbx + 4]           # the resize
    add [r12 + 8], rdx           # the resize
    add rbp, [r12 + 8]
    add rbx, 8
    lea rax, [r13 + OPS * 8]
    cmp rbx, rax
    jb 6b
    dec qword ptr [r15 + PASSES]
    jnz 5b
    xor ebx, ebx                 # release the buffers
4:  mov rdi, [r14 + rbx * 8]
    test rdi, rdi
    jz 3f
    call free
3:  add ebx, 3
    cmp ebx, SLOTS * 3
    jb 4b
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

# Moves the list at r12 to a block for at least rcx bytes: twice its
# capacity, or rcx when that is more. Gives the block in rax, or 0 with the
# list as it was. Clobbers rcx, rdx, rsi, rdi, r8 to r11, xmm0, xmm1.
    .type grow, @function
grow:
    mov rdi, [r12 + 16]
    add rdi, rdi
    cmp rdi, rcx
    cmovb rdi, rcx
    call alloc
    test rax, rax
    jz 1f
    mov r10, rax
    mov r11, rcx
    shl r11, 4                   # the class's size: the new capacity
    mov rdi, rax
    mov rsi, [r12]
    mov rdx, [r12 + 8]
    call copy
    mov rdi, [r12]
    test rdi, rdi
    jz 2f
    call free
2:  mov [r12], r10
    mov [r12 + 16], r11
    mov rax, r10
1:  ret
    .size grow, . - grow

# A block of rdi bytes (1 to 2048) from the heap at r15, in rax, with its
# class in rcx; 0 when the reservation is spent. Slab header: class, owner
# heap, remote list, bump. Clobbers rdx, r8, r9.
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

# Copies rdx bytes from rsi to rdi; the ranges do not overlap.
# Clobbers rax, rcx, r8, xmm0, xmm1.
    .type copy, @function
copy:
    cmp rdx, 16
    jb 3f
    movdqu xmm1, [rsi + rdx - 16]
    lea r8, [rdi + rdx - 16]
    xor ecx, ecx
1:  movdqu xmm0, [rsi + rcx]
    movdqu [rdi + rcx], xmm0
    add rcx, 16
    lea rax, [rcx + 16]
    cmp rax, rdx
    jbe 1b
    movdqu [r8], xmm1
    ret
3:  cmp edx, 8
    jb 4f
    mov rax, [rsi]
    mov rcx, [rsi + rdx - 8]
    mov [rdi], rax
    mov [rdi + rdx - 8], rcx
    ret
4:  cmp edx, 4
    jb 5f
    mov eax, [rsi]
    mov ecx, [rsi + rdx - 4]
    mov [rdi], eax
    mov [rdi + rdx - 4], ecx
    ret
5:  test edx, edx
    jz 6f
    movzx eax, byte ptr [rsi]
    mov [rdi], al
    cmp edx, 1
    je 6f
    movzx ecx, word ptr [rsi + rdx - 2]
    mov [rdi + rdx - 2], cx
6:  ret
    .size copy, . - copy

prefix: .ascii "sum"
    .p2align 3
prefix_len: .quad 3
prefix2:
prefix2_len: .quad 0
