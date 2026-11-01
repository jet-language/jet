# The gauntlet bulkrename (Tools/gauntlet/entries/bulkrename/jet/run.jet) by
# hand: list DIR (getdents64), take each non-directory whose name matches
# ^IMG_(\d+)\.jpeg$, order them by that number (ties by name, as the
# name-sorted listing and the stable sort give), rename them in order to
# photo-<0001..>.jpg in DIR, print "renamed <name> -> <target>" for each and
# "renamed <n> skipped <files - n>".
# - One lazy anonymous mapping holds the directory records, two arrays of
#   16-byte {key, name} rows and the output; nothing is copied out of the
#   getdents64 buffer.
# - An entry's type is its d_type; DT_UNKNOWN asks newfstatat without
#   following links (a failed stat counts as not a directory, like
#   files.list_dir).
# - The key is the number when it has at most 19 significant digits (it then
#   fits, and orders like the number), else all ones; rows sort by an LSD
#   radix pass per key byte (stable), then each run of equal keys is put in
#   (number, name) order by insertion with the exact comparison (significant
#   length, digits, name), so numbers of any length order right.
# - The renames are renameat on the directory's descriptor, in order; the
#   lines go out in one buffered write.
# Errors: no DIR, an unlistable DIR or a failed rename -> exit 1 (what was
# printed before a failed rename is written first).
    .intel_syntax noprefix
    .text

.set REGION, 1 << 30             # reserved, touched lazily
.set DIRENTS, 1 << 25            # most directory record bytes
.set COUNTS, 0                   # 256 dword counters (radix)
.set TARGET, 1024                # the rename target's name
.set STAT, 1088                  # newfstatat's buffer
.set FRAME, 1280

_start:
    mov rdi, [rsp + 16]          # argv[1]
    test rdi, rdi
    jz fail
    sub rsp, FRAME
    mov esi, 0x90000             # O_RDONLY | O_DIRECTORY | O_CLOEXEC
    xor edx, edx
    mov eax, 2                   # open
    syscall
    test rax, rax
    js fail
    mov r14, rax                 # the directory
    xor edi, edi
    mov esi, REGION
    mov edx, 3                   # PROT_READ | PROT_WRITE
    mov r10d, 0x4022             # MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE
    mov r8, -1
    xor r9d, r9d
    mov eax, 9                   # mmap
    syscall
    cmp rax, -4096
    ja fail
    mov r15, rax                 # records from here
    xor r13d, r13d               # their bytes
1:  mov edx, DIRENTS
    sub rdx, r13
    cmp rdx, 65536
    jb fail                      # (a directory past DIRENTS)
    mov rdi, r14
    lea rsi, [r15 + r13]
    mov eax, 217                 # getdents64
    syscall
    test rax, rax
    js fail
    jz 2f
    add r13, rax
    jmp 1b

    # Rows: A at r15 + r13 rounded up to 16, B after it (n * 16 < r13, as a
    # record takes at least 24 bytes), the output after B.
2:  lea rbp, [r15 + r13 + 15]
    and rbp, -16                 # A
    xor ebx, ebx                 # n
    xor r12d, r12d               # files
    xor r11d, r11d               # OR of the keys
    mov r10, r15                 # record
3:  lea rax, [r15 + r13]
    cmp r10, rax
    jae 7f
    lea rsi, [r10 + 19]          # d_name
    movzx eax, byte ptr [r10 + 18]
    cmp eax, 4                   # DT_DIR
    je 6f
    test eax, eax                # DT_UNKNOWN
    jnz 4f
    push r10
    push r11
    push rsi
    mov rdi, r14
    lea rdx, [rsp + 24 + STAT]
    mov r10d, 0x100              # AT_SYMLINK_NOFOLLOW
    mov eax, 262                 # newfstatat
    syscall
    pop rsi
    pop r11
    pop r10
    test rax, rax
    jnz 4f                       # failed: not a directory
    mov eax, [rsp + STAT + 24]   # st_mode
    and eax, 0xf000
    cmp eax, 0x4000
    je 6f
4:  inc r12
    cmp dword ptr [rsi], 0x5f474d49   # "IMG_"
    jne 6f
    lea rdi, [rsi + 4]
    xor eax, eax                 # the number (mod 2^64)
    xor ecx, ecx                 # significant digits
5:  movzx edx, byte ptr [rdi]
    sub edx, '0'
    cmp edx, 9
    ja 8f
    inc rdi
    mov r8, rax
    or r8, rdx
    jz 5b                        # a leading zero
    inc ecx
    imul rax, rax, 10
    add rax, rdx
    jmp 5b
8:  lea rdx, [rsi + 4]
    cmp rdi, rdx
    je 6f                        # no digits
    cmp dword ptr [rdi], 0x65706a2e   # ".jpe"
    jne 6f
    cmp word ptr [rdi + 4], 0x0067    # "g", end
    jne 6f
    mov rdx, -1
    cmp ecx, 19
    cmova rax, rdx
    mov rdx, rbx
    shl rdx, 4
    mov [rbp + rdx], rax
    mov [rbp + rdx + 8], rsi
    or r11, rax
    inc rbx
6:  movzx eax, word ptr [r10 + 16]    # d_reclen
    add r10, rax
    jmp 3b

    # LSD radix by key byte, A <-> B, as many passes as the largest key has
    # bytes.
7:  lea r9, [rbp + r13]          # B
    and r9, -16
    add r9, 16
    xor ecx, ecx                 # shift
    test r11, r11
    jz 12f
    bsr r11, r11
    shr r11d, 3
    inc r11d                     # passes
9:  lea rdi, [rsp + COUNTS]
    push rcx
    mov ecx, 128
    xor eax, eax
    rep stosq
    pop rcx
    xor esi, esi
10: cmp rsi, rbx
    jae 11f
    mov rax, rsi
    shl rax, 4
    mov rax, [rbp + rax]
    shr rax, cl
    movzx eax, al
    inc dword ptr [rsp + COUNTS + rax * 4]
    inc rsi
    jmp 10b
11: xor eax, eax                 # counts to starts
    xor edx, edx
13: mov edi, [rsp + COUNTS + rdx * 4]
    mov [rsp + COUNTS + rdx * 4], eax
    add eax, edi
    inc edx
    cmp edx, 256
    jb 13b
    xor esi, esi
14: cmp rsi, rbx
    jae 15f
    mov rax, rsi
    shl rax, 4
    mov rdi, [rbp + rax]
    mov r8, [rbp + rax + 8]
    mov rax, rdi
    shr rax, cl
    movzx eax, al
    mov edx, [rsp + COUNTS + rax * 4]
    inc dword ptr [rsp + COUNTS + rax * 4]
    shl rdx, 4
    mov [r9 + rdx], rdi
    mov [r9 + rdx + 8], r8
    inc rsi
    jmp 14b
15: xchg rbp, r9
    add ecx, 8
    dec r11d
    jnz 9b

    # Each run of equal keys into (number, name) order.
12: xor esi, esi                 # run start
16: cmp rsi, rbx
    jae 20f
    mov rax, rsi
    shl rax, 4
    mov rdx, [rbp + rax]
    lea rdi, [rsi + 1]           # run end
17: cmp rdi, rbx
    jae 18f
    mov rax, rdi
    shl rax, 4
    cmp [rbp + rax], rdx
    jne 18f
    inc rdi
    jmp 17b
18: lea rax, [rsi + 1]
    cmp rdi, rax
    jbe 19f
    call settle                  # rows rsi .. rdi
19: mov rsi, rdi
    jmp 16b

    # Renames and lines. r9 is free again (B); the output goes after both.
20: mov rax, rbp                 # the output after both row arrays
    cmp rax, r9
    cmovb rax, r9
    lea rdi, [rax + r13 + 16]
    and rdi, -16
    mov [rsp + TARGET + 32], rdi # the output's start
    xor r10d, r10d               # index
21: cmp r10, rbx
    jae 23f
    mov rax, r10
    shl rax, 4
    mov r11, [rbp + rax + 8]     # name
    # target: "photo-" ordinal (4 digits at least) ".jpg"
    push rdi
    lea rdi, [rsp + 8 + TARGET]
    mov dword ptr [rdi], 0x746f6870   # "phot"
    mov word ptr [rdi + 4], 0x2d6f    # "o-"
    add rdi, 6
    lea rax, [r10 + 1]
    mov ecx, 4
    call decimal
    mov dword ptr [rdi], 0x67706a2e   # ".jpg"
    mov byte ptr [rdi + 4], 0
    lea r8, [rdi + 4]            # the target's end
    pop rdi
    push rdi
    push r8
    push r10
    push r11
    mov rsi, r11
    lea r10, [rsp + 32 + TARGET]
    mov rdi, r14
    mov rdx, r14
    mov eax, 264                 # renameat
    syscall
    pop r11
    pop r10
    pop r8
    pop rdi
    test rax, rax
    jnz 24f
    mov rax, 0x2064656d616e6572  # "renamed "
    mov [rdi], rax
    add rdi, 8
    mov rsi, r11
22: movzx eax, byte ptr [rsi]
    test eax, eax
    jz 25f
    mov [rdi], al
    inc rdi
    inc rsi
    jmp 22b
25: mov dword ptr [rdi], 0x203e2d20   # " -> "
    add rdi, 4
    lea rsi, [rsp + TARGET]
    mov rcx, r8
    sub rcx, rsi
    rep movsb
    mov byte ptr [rdi], 10
    inc rdi
    inc r10
    jmp 21b
23: mov rax, 0x2064656d616e6572  # "renamed "
    mov [rdi], rax
    add rdi, 8
    mov rax, rbx
    xor ecx, ecx
    call decimal
    mov rax, 0x2064657070696b73  # "skipped "
    mov byte ptr [rdi], ' '
    mov [rdi + 1], rax
    add rdi, 9
    mov rax, r12
    sub rax, rbx
    xor ecx, ecx
    call decimal
    mov byte ptr [rdi], 10
    inc rdi
    call flush
    xor edi, edi
    mov eax, 231
    syscall
24: call flush                   # a failed rename
fail:
    mov edi, 1
    mov eax, 231
    syscall

# flush: the output from its start ([rsp + 8 + TARGET + 32]) to rdi.
flush:
    mov rsi, [rsp + 8 + TARGET + 32]
    mov rdx, rdi
    sub rdx, rsi
    jz 2f
1:  mov edi, 1
    mov eax, 1
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    jle fail
    add rsi, rax
    sub rdx, rax
    jnz 1b
2:  ret

# decimal: rax in decimal at rdi (advanced), at least ecx digits (zeros in
# front). Renders below rsp. Clobbers rax, rcx, rdx, rsi, r8.
decimal:
    lea rsi, [rsp - 8]
    mov r8d, 10
1:  xor edx, edx
    div r8
    add dl, '0'
    dec rsi
    mov [rsi], dl
    dec ecx
    test rax, rax
    jnz 1b
2:  test ecx, ecx
    jle 3f
    dec rsi
    mov byte ptr [rsi], '0'
    dec ecx
    jmp 2b
3:  lea rcx, [rsp - 8]
    sub rcx, rsi
    rep movsb
    ret

# settle: rows rsi .. rdi of A (rbp) into (number, name) order by insertion.
# Keeps rsi, rdi, rbx, rbp, r9, r12-r15.
settle:
    push rsi
    push rdi
    push rbx
    mov rbx, rdi                 # end
    lea r10, [rsi + 1]           # next to insert
    mov r11, rsi                 # start
1:  cmp r10, rbx
    jae 4f
    mov rax, r10
    shl rax, 4
    mov rdx, [rbp + rax]         # its key
    mov rcx, [rbp + rax + 8]     # its name
    mov r8, r10                  # hole
2:  cmp r8, r11
    jbe 3f
    mov rax, r8
    shl rax, 4
    mov rsi, [rbp + rax - 8]     # the name before the hole
    call before                  # rcx before rsi?
    jae 3f
    mov rax, r8
    shl rax, 4
    mov rsi, [rbp + rax - 16]
    mov [rbp + rax], rsi
    mov rsi, [rbp + rax - 8]
    mov [rbp + rax + 8], rsi
    dec r8
    jmp 2b
3:  mov rax, r8
    shl rax, 4
    mov [rbp + rax], rdx
    mov [rbp + rax + 8], rcx
    inc r10
    jmp 1b
4:  pop rbx
    pop rdi
    pop rsi
    ret

# before: CF set when name rcx orders before name rsi: fewer significant
# digits, else smaller digits, else the smaller name (bytes, a prefix first).
# Keeps rcx, rdx, r8-r11.
before:
    push rcx
    push rdx
    push rsi
    lea rdi, [rcx + 4]
    add rsi, 4
1:  cmp byte ptr [rdi], '0'
    jne 2f
    inc rdi
    jmp 1b
2:  cmp byte ptr [rsi], '0'
    jne 3f
    inc rsi
    jmp 2b
3:  mov rax, rdi                 # the significant lengths
4:  movzx edx, byte ptr [rax]
    sub edx, '0'
    cmp edx, 9
    ja 5f
    inc rax
    jmp 4b
5:  sub rax, rdi
    mov rcx, rsi
6:  movzx edx, byte ptr [rcx]
    sub edx, '0'
    cmp edx, 9
    ja 7f
    inc rcx
    jmp 6b
7:  sub rcx, rsi
    cmp rax, rcx
    jne 9f                       # CF: rcx's is shorter
    mov rcx, rax
    test rcx, rcx
    jz 8f
    xchg rsi, rdi                # cmpsb sets CF for [rsi] < [rdi]
    repe cmpsb
    jne 9f                       # CF: rcx's digit is smaller
8:  mov rdi, [rsp + 16]          # the whole names
    mov rsi, [rsp]
10: movzx eax, byte ptr [rdi]
    movzx edx, byte ptr [rsi]
    cmp eax, edx
    jne 9f                       # CF: rcx's byte is smaller
    test eax, eax
    jz 9f                        # equal: CF clear
    inc rdi
    inc rsi
    jmp 10b
9:  pop rsi
    pop rdx
    pop rcx
    ret
