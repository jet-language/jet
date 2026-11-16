# textscript (gauntlet text.script): read the file argv[1], trim each line
# of ASCII whitespace (space, tab through carriage return), drop the lines
# left empty, sort the rest bytewise (a shorter prefix first), and print
# "lines <n>\n" and the lines, each followed by "\n" (one more "\n" when
# there are none).
#
# Technique:
# - the file is mapped (MAP_POPULATE); every line is one {pointer, length}
#   entry into it, so that no byte is copied before the output;
# - the entries sort by a stable bottom-up merge between two arrays,
#   comparing eight bytes at a time (bswapped words) and the tail bytewise;
# - the output is built in one buffer and leaves in as few writes as fit
#   (EINTR retried, short writes advanced).
# A file that cannot be opened, read or mapped prints "read failed" on
# stderr and exits 1, as the Jet program's panic does.
    .intel_syntax noprefix
    .text

    .set STAT, 0                 # struct stat (144 bytes)
    .set DIGITS, 160             # the count's digits end here
    .set SRC, 160
    .set DST, 168
    .set OUT, 176
    .set COUNT, 184
    .set WIDTH, 192
    .set FRAME, 256

_start:
    mov rdi, [rsp + 16]          # argv[1]
    sub rsp, FRAME
    test rdi, rdi
    jz fail
    xor esi, esi                 # O_RDONLY
    mov eax, 2                   # open
    syscall
    test rax, rax
    js fail
    mov rbx, rax
    mov edi, eax
    lea rsi, [rsp + STAT]
    mov eax, 5                   # fstat
    syscall
    test rax, rax
    js fail
    mov r13, [rsp + STAT + 48]   # st_size
    xor r12d, r12d
    test r13, r13
    jz 1f
    xor edi, edi                 # mmap(NULL, size, PROT_READ, MAP_PRIVATE | MAP_POPULATE, fd, 0)
    mov rsi, r13
    mov edx, 1
    mov r10d, 0x8002
    mov r8, rbx
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail
    mov r12, rax
1:  # Two entry arrays of size / 2 + 2 entries, then the output buffer.
    mov rbp, r13
    shr rbp, 1
    add rbp, 2
    shl rbp, 4                   # one array's bytes
    lea rsi, [rbp * 2 + 64]
    add rsi, r13
    xor edi, edi
    mov edx, 3                   # PROT_READ | PROT_WRITE
    mov r10d, 0x22               # MAP_PRIVATE | MAP_ANONYMOUS
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail
    mov [rsp + SRC], rax
    lea rdi, [rax + rbp]
    mov [rsp + DST], rdi
    lea rdi, [rdi + rbp]
    mov [rsp + OUT], rdi

    # Scan: one entry per line that is not blank once trimmed.
    mov rdi, [rsp + SRC]
    mov rsi, r12
    lea r8, [r12 + r13]
line:
    cmp rsi, r8
    jae scanned
    mov rdx, rsi                 # the line's start
2:  cmp rsi, r8
    jae 3f
    cmp byte ptr [rsi], 10
    je 3f
    inc rsi
    jmp 2b
3:  mov rcx, rsi                 # its end
4:  cmp rdx, rcx                 # trim the front
    jae 7f
    movzx eax, byte ptr [rdx]
    cmp eax, ' '
    je 5f
    sub eax, 9
    cmp eax, 4
    ja 6f
5:  inc rdx
    jmp 4b
6:  movzx eax, byte ptr [rcx - 1]  # trim the back (a non-blank byte stops it)
    cmp eax, ' '
    je 8f
    sub eax, 9
    cmp eax, 4
    ja 9f
8:  dec rcx
    jmp 6b
9:  mov [rdi], rdx
    sub rcx, rdx
    mov [rdi + 8], rcx
    add rdi, 16
7:  inc rsi                      # past the newline
    jmp line
scanned:
    sub rdi, [rsp + SRC]
    shr rdi, 4
    mov [rsp + COUNT], rdi

    # Sort: merge runs of WIDTH entries from SRC into DST, then swap.
    mov qword ptr [rsp + WIDTH], 1
pass:
    mov rax, [rsp + WIDTH]
    cmp rax, [rsp + COUNT]
    jae sorted
    xor r15d, r15d               # the run pair's first entry
run:
    mov rbx, [rsp + COUNT]
    cmp r15, rbx
    jae passed
    mov rax, [rsp + WIDTH]
    lea rcx, [r15 + rax]
    cmp rcx, rbx
    cmova rcx, rbx               # mid
    lea rdx, [rcx + rax]
    cmp rdx, rbx
    cmova rdx, rbx               # end
    mov rax, [rsp + SRC]
    mov rsi, r15
    shl rsi, 4
    mov rdi, [rsp + DST]
    add rdi, rsi
    add rsi, rax                 # the left run
    shl rcx, 4
    lea r12, [rax + rcx]         # its end, the right run's start
    mov r13, r12
    mov r15, rdx                 # the next pair
    shl rdx, 4
    lea r14, [rax + rdx]         # the right run's end
merge:
    cmp rsi, r12
    jae rest
    cmp r13, r14
    jae rest
    call later
    test eax, eax
    jnz 1f
    movups xmm0, [rsi]
    movups [rdi], xmm0
    add rsi, 16
    add rdi, 16
    jmp merge
1:  movups xmm0, [r13]
    movups [rdi], xmm0
    add r13, 16
    add rdi, 16
    jmp merge
rest:
    cmp rsi, r12
    jae 2f
    movups xmm0, [rsi]
    movups [rdi], xmm0
    add rsi, 16
    add rdi, 16
    jmp rest
2:  cmp r13, r14
    jae run
    movups xmm0, [r13]
    movups [rdi], xmm0
    add r13, 16
    add rdi, 16
    jmp 2b
passed:
    mov rax, [rsp + SRC]
    mov rcx, [rsp + DST]
    mov [rsp + SRC], rcx
    mov [rsp + DST], rax
    shl qword ptr [rsp + WIDTH], 1
    jmp pass

sorted:
    # "lines <n>\n"
    mov rdi, [rsp + OUT]
    mov rax, 0x2073656e696c      # "lines "
    mov [rdi], rax
    add rdi, 6
    mov rax, [rsp + COUNT]
    lea r8, [rsp + DIGITS]
    mov r9d, 10
3:  xor edx, edx
    div r9
    add dl, '0'
    dec r8
    mov [r8], dl
    test rax, rax
    jnz 3b
    mov rsi, r8
    lea rcx, [rsp + DIGITS]
    sub rcx, r8
    rep movsb
    mov byte ptr [rdi], 10
    inc rdi
    # Each line and its "\n"; one "\n" alone for none.
    mov rbx, [rsp + SRC]
    mov rdx, [rsp + COUNT]
    test rdx, rdx
    jnz 4f
    mov byte ptr [rdi], 10
    inc rdi
    jmp 5f
4:  mov rsi, [rbx]
    mov rcx, [rbx + 8]
    rep movsb
    mov byte ptr [rdi], 10
    inc rdi
    add rbx, 16
    dec rdx
    jnz 4b
5:  mov rsi, [rsp + OUT]
    mov rdx, rdi
    sub rdx, rsi
6:  mov edi, 1
    mov eax, 1                   # write
    syscall
    cmp rax, -4
    je 6b
    test rax, rax
    js fail
    add rsi, rax
    sub rdx, rax
    jnz 6b
    xor edi, edi
    mov eax, 231                 # exit_group
    syscall

# later: 1 in eax when the entry at r13 sorts before the one at rsi (so a
# merge takes it first), else 0. Clobbers rax, rcx, rdx, r8 through r11.
later:
    push rbx
    mov r10, [rsi]
    mov r11, [r13]
    mov rbx, [rsi + 8]
    mov rdx, [r13 + 8]
    mov rcx, rbx
    cmp rdx, rcx
    cmovb rcx, rdx               # the common length
1:  cmp rcx, 8
    jb 2f
    mov r8, [r10]
    mov r9, [r11]
    cmp r8, r9
    jne 4f
    add r10, 8
    add r11, 8
    sub rcx, 8
    jmp 1b
2:  test rcx, rcx
    jz 3f
    movzx r8d, byte ptr [r10]
    movzx r9d, byte ptr [r11]
    cmp r8d, r9d
    jne 5f
    inc r10
    inc r11
    dec rcx
    jmp 2b
3:  xor eax, eax                 # one is the other's prefix: the shorter first
    cmp rdx, rbx
    setb al
    pop rbx
    ret
4:  bswap r8
    bswap r9
5:  xor eax, eax
    cmp r9, r8
    setb al
    pop rbx
    ret

fail:
    mov edi, 2
    lea rsi, [rip + failed]
    mov edx, 12
    mov eax, 1                   # write
    syscall
    mov edi, 1
    mov eax, 231
    syscall
failed:
    .ascii "read failed\n"
