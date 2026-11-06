# regex-find-all-4mb (gauntlet text.regex-find-all-large): count the
# non-overlapping matches of the pattern `struct` (a literal) in the file
# argv[1] and print "matches <N>".
#
# Technique:
# - "struct" has no border (no proper prefix of it is also a suffix), so
#   two of its matches never overlap and the non-overlapping count is the
#   count of positions it starts at;
# - the file is mapped (MAP_POPULATE) into a reservation one page longer,
#   so that loads past its end read zeros, which match no byte of the
#   needle: no tail loop;
# - AVX2 decides 64 positions per iteration with no branch on the data:
#   each needle byte is compared at its offset (six unaligned loads per 32
#   positions), the compares are and-ed, and the movemask's popcount is
#   added to the count;
# - the file must be UTF-8, as the Jet program reads it as a String: the
#   loop ors every byte into one register, and only when one has its high
#   bit set does a scalar UTF-8 check run over the file.
# A file that is not UTF-8 stops as the Jet program does: files.read
# returns the IOError, which run returns, so status 1 and its report naming
# argv[1] on stderr, byte for byte. Exit 1, without the report, when the
# file cannot be opened, read or mapped (the Jet program's status too).
    .intel_syntax noprefix
    .text

    .set STAT, 0                 # struct stat (144 bytes)
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
1:  mov eax, 0x73737373
    vmovd xmm8, eax
    vpbroadcastd ymm8, xmm8      # 's'
    mov eax, 0x74747474
    vmovd xmm9, eax
    vpbroadcastd ymm9, xmm9      # 't'
    mov eax, 0x72727272
    vmovd xmm10, eax
    vpbroadcastd ymm10, xmm10    # 'r'
    mov eax, 0x75757575
    vmovd xmm11, eax
    vpbroadcastd ymm11, xmm11    # 'u'
    mov eax, 0x63636363
    vmovd xmm12, eax
    vpbroadcastd ymm12, xmm12    # 'c'
    vpxor xmm7, xmm7, xmm7       # every byte or-ed
    xor r14d, r14d               # matches
    mov rsi, r12
    lea r15, [r12 + r13]

    .p2align 5
scan:
    cmp rsi, r15
    jae scanned
    vmovdqu ymm5, [rsi]
    vmovdqu ymm6, [rsi + 32]
    vpor ymm7, ymm7, ymm5
    vpor ymm7, ymm7, ymm6
    vpcmpeqb ymm0, ymm8, ymm5
    vpcmpeqb ymm1, ymm9, [rsi + 1]
    vpand ymm0, ymm0, ymm1
    vpcmpeqb ymm1, ymm10, [rsi + 2]
    vpcmpeqb ymm2, ymm11, [rsi + 3]
    vpand ymm1, ymm1, ymm2
    vpcmpeqb ymm2, ymm12, [rsi + 4]
    vpcmpeqb ymm3, ymm9, [rsi + 5]
    vpand ymm2, ymm2, ymm3
    vpand ymm0, ymm0, ymm1
    vpand ymm0, ymm0, ymm2
    vpcmpeqb ymm4, ymm8, ymm6
    vpcmpeqb ymm1, ymm9, [rsi + 33]
    vpand ymm4, ymm4, ymm1
    vpcmpeqb ymm1, ymm10, [rsi + 34]
    vpcmpeqb ymm2, ymm11, [rsi + 35]
    vpand ymm1, ymm1, ymm2
    vpcmpeqb ymm2, ymm12, [rsi + 36]
    vpcmpeqb ymm3, ymm9, [rsi + 37]
    vpand ymm2, ymm2, ymm3
    vpand ymm4, ymm4, ymm1
    vpand ymm4, ymm4, ymm2
    vpmovmskb eax, ymm0
    vpmovmskb ecx, ymm4
    shl rcx, 32
    or rax, rcx
    popcnt rax, rax
    add r14, rax
    add rsi, 64
    jmp scan

scanned:
    vpmovmskb eax, ymm7
    test eax, eax
    jz print
    mov rsi, r12
    call utf8
    jc stop_utf8

print:
    lea rdi, [rsp + FRAME]
    dec rdi
    mov byte ptr [rdi], 10
    mov rax, r14
    mov ecx, 10
1:  xor edx, edx
    div rcx
    add dl, '0'
    dec rdi
    mov [rdi], dl
    test rax, rax
    jnz 1b
    sub rdi, 8
    mov rax, [rip + s_matches]
    mov [rdi], rax
    mov rsi, rdi
    lea rdx, [rsp + FRAME]
    sub rdx, rsi
2:  mov edi, 1
    mov eax, 1
    syscall
    cmp rax, -4
    je 2b
    test rax, rax
    jle fail
    add rsi, rax
    sub rdx, rax
    jnz 2b
    xor edi, edi
    mov eax, 231
    syscall

fail:
    mov edi, 1
    mov eax, 231
    syscall

# The Jet program's stop at a file that is not UTF-8: run's IOError report.
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
    jmp fail

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

s_matches:
    .ascii "matches "
e_u8:
    .ascii "Error: invalid input during read `"
e_u8b:
    .ascii "`: stream did not contain valid UTF-8 (type: IOError)\n"
    .ascii "  conversion: IOError -> Err\n"
    .ascii "  failed here: read (<corelib>/Core/Files/Files.jet:729)\n"
    .ascii "      Ok(String.from_bytes(bytes) ?? return Err(io_err(\"read\", source, \"stream did not contain valid UTF-8\")))\n"
    .ascii " Trail [E3002] (1 hop via ?, origin first):\n"
    .ascii "  1. run (run.jet:6)\n"
    .ascii "      text: files.read(args.path)\n"
e_end:
