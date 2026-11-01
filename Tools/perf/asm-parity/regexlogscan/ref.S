# regex-logscan (gauntlet text.regex-kernel): read the file argv[1]; count
# its lines (split on '\n', a last line without one included) that match
#   ^([0-9.]+) - - \[[^]]+\] "GET /api/[^ ]+ HTTP/1\.1" 5[0-9][0-9] 
# and the matches per client (the first group: the bytes before the line's
# first space), then print "matches <N>" and the top 5 clients by count
# descending, ties by bytes ascending, as "<count> <client>".
#
# Technique:
# - every run in the pattern stops at the first byte outside it ([0-9.]+ at
#   one that is not a digit or '.', [^]]+ at the first ']', [^ ]+ at the
#   first ' ') and what follows each run must start with that byte, so a
#   line has one way to match and is decided left to right with no
#   backtracking and no automaton: the cost is the bytes it reads;
# - the file is mapped (MAP_POPULATE) into a reservation one page longer,
#   so that vector loads past its end read zeros instead of faulting;
# - line ends come from an AVX2 '\n' mask of 64 bytes at a time, walked
#   with tzcnt/blsr;
# - [0-9.]+ is one AVX2 classification of the line's first 32 bytes and a
#   tzcnt; the ']' and ' ' runs are AVX2 compare/movemask/tzcnt searches
#   bounded by the line end (the only runs that can reach it: no literal
#   holds '\n' or a zero); the literals are 8, 4 and 2-byte compares;
# - a client of up to 16 bytes is its zero-padded 16 bytes (one masked
#   load), hashed by two crc32 seeded with its length and compared whole
#   with the stored entry; a longer one hashes all its words and compares
#   its bytes in the file;
# - the table is open addressing of 32-byte entries
#   {key | pointer, length | hash << 32, count}, doubled (rehashed from the
#   stored hash) when half full;
# - the top 5 is one pass of insertion into a sorted array of 5; output
#   goes out through one buffer.
# - the file must be UTF-8, as the Jet program reads its lines as Strings:
#   the '\n' pass ors every byte into one register, and only when one has
#   its high bit set does a scalar UTF-8 check run over the file.
# A client is shorter than 4 GiB. A file that is not UTF-8 stops as the Jet
# program does: its file line source reads every line before the loop, so
# with no output, status 70 and the read's E3001 report naming argv[1] on
# stderr, byte for byte. Exit 1 when the file cannot be opened, read or
# mapped (Jet returns the IOError: status 1) or stdout cannot be written.
    .intel_syntax noprefix
    .text

    .set STAT, 0                 # struct stat (144 bytes)
    .set MATCHES, 144
    .set DISTINCT, 152
    .set TOP, 160                # 5 entry pointers
    .set OUT, 256
    .set OUTSZ, 4096
    .set FRAME, OUT + OUTSZ
    .set INITCAP, 256

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
    mov ebp, INITCAP - 1         # mask
    xor eax, eax
    mov [rsp + MATCHES], rax
    mov [rsp + DISTINCT], rax
    lea r15, [r12 + r13]         # end

    mov eax, 0x0a0a0a0a
    vmovd xmm15, eax
    vpbroadcastd ymm15, xmm15    # '\n'
    mov eax, 0x5d5d5d5d
    vmovd xmm14, eax
    vpbroadcastd ymm14, xmm14    # ']'
    mov eax, 0x20202020
    vmovd xmm13, eax
    vpbroadcastd ymm13, xmm13    # ' '
    mov eax, 0x30303030
    vmovd xmm12, eax
    vpbroadcastd ymm12, xmm12    # '0'
    mov eax, 0x09090909
    vmovd xmm11, eax
    vpbroadcastd ymm11, xmm11    # 9
    mov eax, 0x2e2e2e2e
    vmovd xmm10, eax
    vpbroadcastd ymm10, xmm10    # '.'
    vmovdqu xmm9, [rip + iota]   # 0, 1, ..., 15
    vpxor xmm8, xmm8, xmm8       # every byte or-ed

    xor ebx, ebx                 # chunk offset
    xor r10d, r10d               # the chunk's '\n' not yet taken
    mov rsi, r12                 # line start
    test r13, r13
    jz scanned
    vmovdqu ymm0, [r12]
    vmovdqu ymm1, [r12 + 32]
    vpor ymm8, ymm8, ymm0
    vpor ymm8, ymm8, ymm1
    vpcmpeqb ymm0, ymm0, ymm15
    vpcmpeqb ymm1, ymm1, ymm15
    vpmovmskb eax, ymm0
    vpmovmskb ecx, ymm1
    shl rcx, 32
    or rax, rcx
    mov r10, rax

line:
    cmp rsi, r15
    jae scanned
    test r10, r10
    jnz 2f
1:  add rbx, 64
    cmp rbx, r13
    jae 3f
    vmovdqu ymm0, [r12 + rbx]
    vmovdqu ymm1, [r12 + rbx + 32]
    vpor ymm8, ymm8, ymm0
    vpor ymm8, ymm8, ymm1
    vpcmpeqb ymm0, ymm0, ymm15
    vpcmpeqb ymm1, ymm1, ymm15
    vpmovmskb eax, ymm0
    vpmovmskb ecx, ymm1
    shl rcx, 32
    or rax, rcx
    mov r10, rax
    test rax, rax
    jz 1b
2:  tzcnt rdx, r10
    blsr r10, r10
    add rdx, rbx
    add rdx, r12                 # line end: its '\n'
    jmp check
3:  mov rdx, r15                 # the last line has no '\n'

# The line [rsi, rdx). ^([0-9.]+): its length L to eax.
check:
    vmovdqu ymm0, [rsi]
    vpsubb ymm1, ymm0, ymm12
    vpminub ymm2, ymm1, ymm11
    vpcmpeqb ymm1, ymm1, ymm2    # digit
    vpcmpeqb ymm2, ymm0, ymm10   # '.'
    vpor ymm1, ymm1, ymm2
    vpmovmskb eax, ymm1
    not eax
    tzcnt eax, eax
    jc longip                    # 32 or more
    jz next                      # none
ipdone:
    lea rdi, [rsi + rax]
    mov rcx, [rdi]
    shl rcx, 16
    cmp rcx, [rip + lit_dash]    # " - - ["
    jne next
    # [^]]+\]
    lea r8, [rdi + 6]
    mov r9, r8
1:  vpcmpeqb ymm0, ymm14, [r9]
    vpmovmskb ecx, ymm0
    test ecx, ecx
    jnz 2f
    add r9, 32
    cmp r9, rdx
    jb 1b
    jmp next
2:  tzcnt ecx, ecx
    add r9, rcx                  # the ']'
    cmp r9, rdx
    jae next
    cmp r9, r8
    je next
    mov rcx, [r9 + 1]
    cmp rcx, [rip + lit_get]     # ' "GET /a'
    jne next
    cmp dword ptr [r9 + 8], 0x2f697061   # "api/"
    jne next
    # [^ ]+ then ' '
    lea r8, [r9 + 12]
    mov r9, r8
3:  vpcmpeqb ymm0, ymm13, [r9]
    vpmovmskb ecx, ymm0
    test ecx, ecx
    jnz 4f
    add r9, 32
    cmp r9, rdx
    jb 3b
    jmp next
4:  tzcnt ecx, ecx
    add r9, rcx                  # the ' '
    cmp r9, rdx
    jae next
    cmp r9, r8
    je next
    mov rcx, [r9 + 1]
    cmp rcx, [rip + lit_http]    # "HTTP/1.1"
    jne next
    cmp word ptr [r9 + 9], 0x2022        # '" '
    jne next
    mov ecx, [r9 + 11]           # '5', digit, digit, ' '
    mov edi, ecx
    and edi, 0xff0000ff
    cmp edi, 0x20000035
    jne next
    movzx edi, ch
    sub edi, '0'
    cmp edi, 9
    ja next
    shr ecx, 16
    movzx ecx, cl
    sub ecx, '0'
    cmp ecx, 9
    ja next

    # A match; the client is [rsi, rsi + L).
    inc qword ptr [rsp + MATCHES]
    cmp eax, 16
    ja longkey
    vmovd xmm1, eax
    vpbroadcastb xmm1, xmm1
    vpcmpgtb xmm1, xmm1, xmm9    # the bytes below L
    vpand xmm0, xmm1, [rsi]
    mov r8d, eax
    vmovq rax, xmm0
    vpextrq rcx, xmm0, 1
    mov edi, r8d
    crc32 rdi, rax
    crc32 rdi, rcx
    mov r9, rdi
    shl r9, 32
    or r8, r9                    # length | hash << 32
5:  and rdi, rbp
    mov r9, rdi
    shl r9, 5
    add r9, r14
    cmp qword ptr [r9 + 24], 0
    je 7f
    cmp [r9 + 16], r8
    jne 6f
    cmp [r9], rax
    jne 6f
    cmp [r9 + 8], rcx
    je 8f
6:  inc rdi
    jmp 5b
7:  mov [r9], rax
    mov [r9 + 8], rcx
    mov [r9 + 16], r8
    mov qword ptr [r9 + 24], 1
    jmp added
8:  inc qword ptr [r9 + 24]
next:
    lea rsi, [rdx + 1]
    jmp line

# [0-9.]+ ran past the first 32 bytes: the rest of it.
longip:
    mov r8, rsi
1:  add r8, 32
    vmovdqu ymm0, [r8]
    vpsubb ymm1, ymm0, ymm12
    vpminub ymm2, ymm1, ymm11
    vpcmpeqb ymm1, ymm1, ymm2
    vpcmpeqb ymm2, ymm0, ymm10
    vpor ymm1, ymm1, ymm2
    vpmovmskb eax, ymm1
    not eax
    tzcnt eax, eax
    jc 1b
    add rax, r8
    sub rax, rsi
    jmp ipdone

# A client longer than 16 bytes: rsi its start, rax its length.
longkey:
    push rdx
    mov r8, rax
    mov edi, eax
    xor ecx, ecx
1:  lea rdx, [rcx + 8]
    cmp rdx, r8
    ja 2f
    crc32 rdi, qword ptr [rsi + rcx]
    mov rcx, rdx
    jmp 1b
2:  mov rdx, r8
    sub rdx, rcx                 # 0 to 7 bytes left
    jz 3f
    shl edx, 3
    mov r9, [rsi + rcx]
    bzhi r9, r9, rdx
    crc32 rdi, r9
3:  mov r9, rdi
    shl r9, 32
    or r8, r9                    # length | hash << 32
4:  and rdi, rbp
    mov r9, rdi
    shl r9, 5
    add r9, r14
    cmp qword ptr [r9 + 24], 0
    je 6f
    cmp [r9 + 16], r8
    jne 5f
    push rsi
    push rdi
    mov rdi, [r9]
    mov ecx, r8d
    repe cmpsb
    pop rdi
    pop rsi
    je 7f
5:  inc rdi
    jmp 4b
6:  mov [r9], rsi
    mov qword ptr [r9 + 8], 0
    mov [r9 + 16], r8
    mov qword ptr [r9 + 24], 1
    pop rdx
    jmp added
7:  inc qword ptr [r9 + 24]
    pop rdx
    jmp next

added:
    mov rax, [rsp + DISTINCT]
    inc rax
    mov [rsp + DISTINCT], rax
    add rax, rax
    cmp rax, rbp
    jbe next
    call grow
    jmp next

# Double the table: every entry reinserted at its stored hash.
grow:
    push rsi
    push rdx
    push r10
    lea rsi, [rbp + 1]
    shl rsi, 6                   # twice the entries, 32 bytes each
    xor edi, edi
    mov edx, 3
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail
    lea rdx, [rbp + rbp + 1]     # the new mask
    mov rsi, r14
    lea rcx, [rbp + 1]
1:  cmp qword ptr [rsi + 24], 0
    je 4f
    mov rdi, [rsi + 16]
    shr rdi, 32
2:  and rdi, rdx
    mov r8, rdi
    shl r8, 5
    add r8, rax
    cmp qword ptr [r8 + 24], 0
    je 3f
    inc rdi
    jmp 2b
3:  vmovdqu ymm0, [rsi]
    vmovdqu [r8], ymm0
4:  add rsi, 32
    dec rcx
    jnz 1b
    mov r8, rax
    mov rdi, r14
    lea rsi, [rbp + 1]
    shl rsi, 5
    mov rbp, rdx
    mov eax, 11                  # munmap
    syscall
    mov r14, r8
    pop r10
    pop rdx
    pop rsi
    ret

# The top 5: TOP[0..n) best first, n in ebx; first the UTF-8 check.
scanned:
    vpmovmskb eax, ymm8
    test eax, eax
    jz 1f
    mov rsi, r12
    call utf8
    jc stop_utf8
1:  xor ebx, ebx
    mov r9, r14
    lea r13, [rbp + 1]
1:  cmp qword ptr [r9 + 24], 0
    je 5f
    cmp ebx, 5
    jb 2f
    mov r8, [rsp + TOP + 32]
    call better
    test al, al
    jz 5f
    mov ecx, 4
    jmp 3f
2:  mov ecx, ebx
    inc ebx
3:  test ecx, ecx
    jz 4f
    mov r8, [rsp + TOP + rcx * 8 - 8]
    push rcx
    call better
    pop rcx
    test al, al
    jz 4f
    mov [rsp + TOP + rcx * 8], r8
    dec ecx
    jmp 3b
4:  mov [rsp + TOP + rcx * 8], r9
5:  add r9, 32
    dec r13
    jnz 1b

    mov r15d, ebx
    lea r12, [rsp + OUT]
    mov rbx, r12                 # output cursor
    lea rsi, [rip + s_matches]
    mov ecx, 8
    call put
    mov rax, [rsp + MATCHES]
    call putnum
    call putnl
    xor r13d, r13d
6:  cmp r13, r15
    jae 8f
    mov r9, [rsp + TOP + r13 * 8]
    mov rax, [r9 + 24]
    call putnum
    lea rsi, [rip + s_space]
    mov ecx, 1
    call put
    mov ecx, [r9 + 16]
    mov rsi, r9
    cmp rcx, 16
    jbe 7f
    mov rsi, [r9]
7:  call put
    call putnl
    inc r13
    jmp 6b
8:  call flush
    xor edi, edi
    mov eax, 231
    syscall

# al = 1 when entry r9 ranks before entry r8: more matches, or as many and
# its client's bytes first.
better:
    mov rax, [r9 + 24]
    cmp rax, [r8 + 24]
    ja 4f
    jb 3f
    mov r10d, [r9 + 16]
    mov r11d, [r8 + 16]
    mov rsi, r9
    cmp r10, 16
    jbe 1f
    mov rsi, [r9]
1:  mov rdi, r8
    cmp r11, 16
    jbe 2f
    mov rdi, [r8]
2:  mov rcx, r10
    cmp rcx, r11
    cmova rcx, r11
    repe cmpsb
    jne 5f
    cmp r10, r11
    jb 4f
3:  xor eax, eax
    ret
4:  mov eax, 1
    ret
5:  movzx eax, byte ptr [rsi - 1]
    cmp al, [rdi - 1]
    jb 4b
    xor eax, eax
    ret

# Output through [r12, r12 + OUTSZ), cursor rbx. putnum writes rax in
# decimal, put rcx bytes from rsi. They clobber rax, rcx, rdx, rsi, rdi,
# r10 and r11.
putnum:
    sub rsp, 32
    lea rsi, [rsp + 32]
    mov r10d, 10
1:  xor edx, edx
    div r10
    add dl, '0'
    dec rsi
    mov [rsi], dl
    test rax, rax
    jnz 1b
    lea rcx, [rsp + 32]
    sub rcx, rsi
    call put
    add rsp, 32
    ret

putnl:
    lea rsi, [rip + s_nl]
    mov ecx, 1
put:
    lea rax, [rbx + rcx]
    lea rdx, [r12 + OUTSZ]
    cmp rax, rdx
    jbe 1f
    push rsi
    push rcx
    call flush
    pop rcx
    pop rsi
    cmp rcx, OUTSZ
    jbe 1f
    mov rdx, rcx
    jmp write_all
1:  mov rdi, rbx
    rep movsb
    mov rbx, rdi
    ret

flush:
    mov rsi, r12
    mov rdx, rbx
    sub rdx, r12
    mov rbx, r12
# write(1, rsi, rdx) until done, EINTR retried, short writes advanced.
write_all:
    test rdx, rdx
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

fail:
    mov edi, 1
    mov eax, 231
    syscall

# The Jet program's stop at a file that is not UTF-8: its E3001 report.
# The frame is at rsp.
stop_utf8:
    lea rsi, [rip + e_u8]
    mov edx, e_u8b - e_u8
    call ewrite
    mov rsi, [rsp + FRAME + 16]  # argv[1]
    xor edx, edx
1:  cmp byte ptr [rsi + rdx], 0
    je 2f
    inc rdx
    jmp 1b
2:  call ewrite
    lea rsi, [rip + e_u8b]
    mov edx, e_end - e_u8b
    call ewrite
    mov edi, 70
    mov eax, 231
    syscall

# write(2, rsi, rdx) until done, EINTR retried, short writes advanced; a
# failed write ends it. Clobbers rax, rcx, rdx, rsi, rdi and r11.
ewrite:
    test rdx, rdx
    jz 2f
    mov edi, 2
    mov eax, 1
    syscall
    cmp rax, -4
    je ewrite
    test rax, rax
    jle 2f
    add rsi, rax
    sub rdx, rax
    jmp ewrite
2:  ret

# CF set when [rsi, r15) is not UTF-8 (overlong forms, surrogates and code
# points past U+10FFFF rejected). Clobbers rax, rcx, rsi.
utf8:
    cmp rsi, r15
    jae 9f
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp al, 0x80
    jb utf8
    cmp al, 0xc2
    jb 8f
    cmp al, 0xdf
    jbe 5f                       # one continuation
    cmp al, 0xe0
    je 1f
    cmp al, 0xed
    je 2f
    cmp al, 0xef
    jbe 4f                       # two continuations
    cmp al, 0xf0
    je 3f
    cmp al, 0xf4
    jb 6f                        # three continuations
    ja 8f
    mov cl, 0x8f                 # F4: 80..8F, then two
    jmp 7f
1:  mov cl, 0xa0                 # E0: A0..BF, then one
    cmp rsi, r15
    jae 8f
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp al, cl
    jb 8f
    cmp al, 0xbf
    ja 8f
    jmp 5f
2:  cmp rsi, r15                 # ED: 80..9F, then one
    jae 8f
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp al, 0x80
    jb 8f
    cmp al, 0x9f
    ja 8f
    jmp 5f
3:  cmp rsi, r15                 # F0: 90..BF, then two
    jae 8f
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp al, 0x90
    jb 8f
    cmp al, 0xbf
    ja 8f
    jmp 4f
6:  call cont
    jc 8f
4:  call cont
    jc 8f
5:  call cont
    jc 8f
    jmp utf8
7:  cmp rsi, r15
    jae 8f
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp al, 0x80
    jb 8f
    cmp al, cl
    ja 8f
    jmp 4b
8:  stc
    ret
9:  clc
    ret

# One continuation byte (80..BF) at rsi, before r15: CF clear and rsi past
# it, or CF set.
cont:
    cmp rsi, r15
    jae 1f
    movzx eax, byte ptr [rsi]
    inc rsi
    and al, 0xc0
    cmp al, 0x80
    jne 1f
    clc
    ret
1:  stc
    ret

iota:
    .byte 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15
lit_dash:
    .quad 0x5b202d202d200000
lit_get:
    .quad 0x612f205445472220
lit_http:
    .quad 0x312e312f50545448
s_matches:
    .ascii "matches "
s_space:
    .ascii " "
s_nl:
    .ascii "\n"
e_u8:
    .ascii "Stop [E3001]: `panic: invalid input during read `"
e_u8b:
    .ascii "`: stream did not contain valid UTF-8`\n"
    .ascii "  --> <core.prelude>:0\n"
    .ascii " Why: The program hit a `panic`, `assert`, or `assert_eq` call that failed, a bounds/key check triggered at runtime, or another program-side stop. The shared report boundary owns the program-side stop; an unhandled entry error is a returned report, not E3001. Jet file and line are shown in Jet terms (I2).\n"
    .ascii " Fix: Fix the logic that led to the failure. Program-side stops exit 70 through the shared boundary; unhandled entry errors print their report and exit 1; exit 101 is reserved for Jet defects.\n"
    .ascii "More: jet-lang.dev/e/E3001\n"
e_end:
