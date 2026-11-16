# wordfreq (gauntlet text.kernel): read the file argv[1], split it on ASCII
# whitespace (space, tab through carriage return), count each exact token,
# print the top 20 by count descending with ties by bytes ascending as
# "<count> <token>", then "distinct <D> total <T>".
#
# Technique:
# - the file is mapped (MAP_POPULATE) into a reservation one page longer,
#   so that the vector loads and the 8-byte token loads past its end read
#   zeros instead of faulting;
# - AVX2 classifies 64 bytes at a time into a whitespace mask; token starts
#   and ends are the mask's edges (carried across chunks), walked with
#   tzcnt/blsr, and the token count is the popcount of the ends;
# - a token of up to 8 bytes is its zero-padded first word (bzhi), so that
#   one compare of word and length is a whole match; its hash is one crc32
#   of that word seeded with the length; a longer token hashes all its
#   words and compares the bytes past the first word;
# - the table is open addressing of 32-byte entries
#   {word, length | hash << 32, count, pointer}, doubled (rehashed from the
#   stored hash) when half full;
# - the top 20 is one pass of insertion into a sorted array of 20, most
#   entries rejected by one count compare;
# - output goes out through one buffer, in as few writes as fit.
# Exit 1 when the file cannot be opened, read or mapped.
    .intel_syntax noprefix
    .text

    .set STAT, 0                 # struct stat (144 bytes)
    .set TOTAL, 160
    .set DISTINCT, 168
    .set TOP, 192                # 20 entry pointers
    .set OUT, 512
    .set OUTSZ, 4096
    .set FRAME, OUT + OUTSZ
    .set INITCAP, 1 << 14

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
    mov rbx, rax                 # fd
    mov edi, eax
    lea rsi, [rsp + STAT]
    mov eax, 5                   # fstat
    syscall
    test rax, rax
    js fail
    mov r13, [rsp + STAT + 48]   # st_size
    # Reserve size + one page readable, then map the file over its start.
    xor edi, edi
    lea rsi, [r13 + 4096]
    mov edx, 1                   # PROT_READ
    mov r10d, 0x22               # MAP_PRIVATE | MAP_ANONYMOUS
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail
    mov r12, rax
    test r13, r13
    jz 1f
    mov rdi, r12
    mov rsi, r13
    mov edx, 1
    mov r10d, 0x8012             # MAP_PRIVATE | MAP_FIXED | MAP_POPULATE
    mov r8, rbx
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail
1:  # The table.
    xor edi, edi
    mov esi, INITCAP * 32
    mov edx, 3                   # PROT_READ | PROT_WRITE
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail
    mov r14, rax
    mov r15d, INITCAP - 1        # mask
    xor eax, eax
    mov [rsp + TOTAL], rax
    mov [rsp + DISTINCT], rax

    mov eax, 0x20202020
    vmovd xmm8, eax
    vpbroadcastd ymm8, xmm8      # ' '
    mov eax, 0x09090909
    vmovd xmm9, eax
    vpbroadcastd ymm9, xmm9      # '\t'
    mov eax, 0x04040404
    vmovd xmm10, eax
    vpbroadcastd ymm10, xmm10    # '\r' - '\t'
    xor ebx, ebx                 # chunk offset
    xor r10d, r10d               # 1 when the byte before the chunk is a token's
    mov rbp, r12                 # the open token's start

chunk:
    cmp rbx, r13
    jae scanned
    vmovdqu ymm0, [r12 + rbx]
    vmovdqu ymm1, [r12 + rbx + 32]
    vpcmpeqb ymm2, ymm0, ymm8
    vpsubb ymm3, ymm0, ymm9
    vpminub ymm4, ymm3, ymm10
    vpcmpeqb ymm3, ymm3, ymm4
    vpor ymm2, ymm2, ymm3
    vpcmpeqb ymm5, ymm1, ymm8
    vpsubb ymm6, ymm1, ymm9
    vpminub ymm7, ymm6, ymm10
    vpcmpeqb ymm6, ymm6, ymm7
    vpor ymm5, ymm5, ymm6
    vpmovmskb eax, ymm2
    vpmovmskb ecx, ymm5
    shl rcx, 32
    or rax, rcx                  # whitespace
    mov rcx, r13
    sub rcx, rbx
    cmp rcx, 64
    jae 1f
    mov rdx, -1                  # the bytes past the end are whitespace
    shlx rdx, rdx, rcx
    or rax, rdx
1:  mov rdx, rax
    not rdx                      # token bytes
    lea rcx, [rdx + rdx]
    or rcx, r10                  # token byte before each
    mov r10, rdx
    shr r10, 63
    andn r9, rcx, rdx            # starts
    and rax, rcx                 # ends
    popcnt rcx, rax
    add [rsp + TOTAL], rcx
    mov r8, r9
    or r8, rax                   # edges
    jz next

edge:
    tzcnt rcx, r8
    blsr r8, r8
    bt r9, rcx
    jnc token
    lea rbp, [r12 + rbx]
    add rbp, rcx
    test r8, r8
    jnz edge
    jmp next

token:
    lea rcx, [rbx + rcx]
    add rcx, r12
    mov rsi, rbp
    sub rcx, rsi                 # length
    call count
    test r8, r8
    jnz edge
next:
    add rbx, 64
    jmp chunk

scanned:
    test r10, r10                # a token runs to the end of the file
    jz top
    lea rcx, [r12 + r13]
    mov rsi, rbp
    sub rcx, rsi
    inc qword ptr [rsp + TOTAL]
    call count

# The top 20: r8 entries in TOP, best first.
top:
    xor r8d, r8d
    mov r12, r14
    lea r13, [r15 + 1]
    shl r13, 5
    add r13, r14                 # table end
2:  cmp r12, r13
    jae print
    cmp dword ptr [r12 + 8], 0
    je 6f
    mov rdi, r12
    cmp r8, 20
    jb 3f
    mov rsi, [rsp + TOP + 19 * 8]
    call better
    test eax, eax
    jz 6f
    mov ebx, 19
    jmp 4f
3:  mov rbx, r8
    inc r8
4:  test rbx, rbx                # sift up
    jz 5f
    mov rsi, [rsp + TOP + rbx * 8 - 8]
    call better
    test eax, eax
    jz 5f
    mov [rsp + TOP + rbx * 8], rsi
    dec rbx
    jmp 4b
5:  mov [rsp + TOP + rbx * 8], r12
6:  add r12, 32
    jmp 2b

print:
    lea r15, [rsp + OUT]         # the output buffer (the mask is done)
    mov rbx, r15                 # output position
    xor r12d, r12d
7:  cmp r12, r8
    jae 8f
    mov r13, [rsp + TOP + r12 * 8]
    mov rax, [r13 + 16]
    call number
    mov byte ptr [rbx], ' '
    inc rbx
    mov rsi, [r13 + 24]
    mov ecx, [r13 + 8]
    call put
    call room
    mov byte ptr [rbx], 10
    inc rbx
    inc r12
    jmp 7b
8:  call room
    lea rsi, [rip + distinct_text]
    mov ecx, 9
    mov rdi, rbx
    rep movsb
    mov rbx, rdi
    mov rax, [rsp + DISTINCT]
    call number
    lea rsi, [rip + total_text]
    mov ecx, 7
    mov rdi, rbx
    rep movsb
    mov rbx, rdi
    call room
    mov rax, [rsp + TOTAL]
    call number
    mov byte ptr [rbx], 10
    inc rbx
    call flush
    xor edi, edi
    mov eax, 231
    syscall

fail:
    mov edi, 1
    mov eax, 231
    syscall

distinct_text: .ascii "distinct "
total_text: .ascii " total "

# count: the token rsi, rcx bytes, counted in the table (r14, mask r15).
# Clobbers rax, rcx, rdx, rdi, r11 (rsi too when it grows the table).
count:
    cmp rcx, 8
    ja long
    lea eax, [rcx * 8]
    bzhi rdx, [rsi], rax         # the word, zero-padded
    mov eax, ecx
    crc32 rax, rdx
    mov r11d, eax
1:  and r11, r15
    mov rdi, r11
    shl rdi, 5
    add rdi, r14
    cmp [rdi], rdx
    jne 2f
    cmp [rdi + 8], ecx
    jne 2f
    inc qword ptr [rdi + 16]
    ret
2:  cmp dword ptr [rdi + 8], 0
    je insert
    inc r11
    jmp 1b

long:
    mov eax, ecx
    mov rdx, rsi
    mov rdi, rcx
3:  crc32 rax, qword ptr [rdx]
    add rdx, 8
    sub rdi, 8
    cmp rdi, 8
    ja 3b
    shl edi, 3
    bzhi rdi, [rdx], rdi
    crc32 rax, rdi
    mov rdx, [rsi]               # the first word
    mov r11d, eax
4:  and r11, r15
    mov rdi, r11
    shl rdi, 5
    add rdi, r14
    cmp [rdi], rdx
    jne 5f
    cmp [rdi + 8], ecx
    jne 5f
    push rsi
    push rdi
    push rcx
    mov rdi, [rdi + 24]
    add rsi, 8
    add rdi, 8
    sub rcx, 8
    repe cmpsb
    pop rcx
    pop rdi
    pop rsi
    jne 5f
    inc qword ptr [rdi + 16]
    ret
5:  cmp dword ptr [rdi + 8], 0
    je insert
    inc r11
    jmp 4b

# insert: a new entry at rdi (word rdx, hash eax, length ecx, token rsi).
insert:
    mov [rdi], rdx
    shl rax, 32
    or rax, rcx
    mov [rdi + 8], rax
    mov qword ptr [rdi + 16], 1
    mov [rdi + 24], rsi
    mov rax, [rsp + 8 + DISTINCT]
    inc rax
    mov [rsp + 8 + DISTINCT], rax
    add rax, rax
    cmp rax, r15
    ja grow
    ret

# grow: the table doubled, every entry placed again by its stored hash.
grow:
    push r8
    push r9
    push r10
    xor edi, edi
    lea rsi, [r15 + 1]
    shl rsi, 6
    mov edx, 3
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail
    lea r9, [r15 + r15 + 1]      # new mask
    mov rsi, r14
    lea rdx, [r15 + 1]
    shl rdx, 5
    add rdx, r14                 # old end
6:  cmp rsi, rdx
    jae 8f
    mov ecx, [rsi + 12]          # hash
    cmp dword ptr [rsi + 8], 0
    je 7f
9:  and rcx, r9
    mov rdi, rcx
    shl rdi, 5
    add rdi, rax
    inc rcx
    cmp dword ptr [rdi + 8], 0
    jne 9b
    vmovdqu ymm0, [rsi]
    vmovdqu [rdi], ymm0
7:  add rsi, 32
    jmp 6b
8:  mov r8, rax
    mov rdi, r14
    lea rsi, [r15 + 1]
    shl rsi, 5
    mov eax, 11                  # munmap
    syscall
    mov r14, r8
    mov r15, r9
    pop r10
    pop r9
    pop r8
    ret

# better: eax 1 when entry rdi ranks before entry rsi (count descending,
# then bytes ascending, a prefix first). Clobbers rax, rcx, rdx, r9-r11.
better:
    mov rax, [rdi + 16]
    cmp rax, [rsi + 16]
    ja 3f
    jb 2f
    mov r9, [rdi + 24]
    mov r10, [rsi + 24]
    mov ecx, [rdi + 8]
    mov edx, [rsi + 8]
    cmp ecx, edx
    cmova ecx, edx
1:  movzx eax, byte ptr [r9]
    movzx r11d, byte ptr [r10]
    cmp eax, r11d
    jb 3f
    ja 2f
    inc r9
    inc r10
    dec ecx
    jnz 1b
    mov eax, [rdi + 8]
    cmp eax, [rsi + 8]
    jb 3f
2:  xor eax, eax
    ret
3:  mov eax, 1
    ret

# number: rax in decimal at rbx (room made first).
number:
    push rax
    call room
    pop rax
    lea rdi, [rsp - 8]           # rendered backwards below the stack top
    mov r9, rdi
    mov r10d, 10
1:  xor edx, edx
    div r10
    add dl, '0'
    dec rdi
    mov [rdi], dl
    test rax, rax
    jnz 1b
    mov rcx, r9
    sub rcx, rdi
    mov rsi, rdi
    mov rdi, rbx
    rep movsb
    mov rbx, rdi
    ret

# The output buffer is r15 .. r15 + OUTSZ, filled up to rbx; each routine
# below keeps 64 bytes spare at its end.
# room: at least 64 bytes free in the buffer.
room:
    lea rax, [r15 + OUTSZ - 64]
    cmp rbx, rax
    jbe 1f
    jmp flush
1:  ret

# put: rsi, rcx bytes into the buffer, or straight out when they do not fit.
put:
    lea rax, [r15 + OUTSZ - 64]
    sub rax, rbx
    cmp rcx, rax
    jbe 2f
    push rsi
    push rcx
    call flush
    pop rdx
    pop rsi
    cmp rdx, OUTSZ - 64
    jbe 1f
    jmp write_all                # rsi, rdx
1:  mov rcx, rdx
2:  mov rdi, rbx
    rep movsb
    mov rbx, rdi
    ret

# flush: the buffer out; rbx back to its start.
flush:
    mov rsi, r15
    mov rdx, rbx
    sub rdx, rsi
    mov rbx, rsi
    jz 1f
    jmp write_all
1:  ret

# write_all: rsi, rdx bytes to stdout (EINTR retried, short writes advanced).
write_all:
    mov edi, 1
    mov eax, 1
    syscall
    cmp rax, -4
    je write_all
    test rax, rax
    jle fail
    add rsi, rax
    sub rdx, rax
    jnz write_all
    ret
