# binparse (gauntlet formats.binary-kernel): read the file argv[1] as "JGB1",
# a u32 count n, then n records {u32 id, f64 value, u16 length, length name
# bytes} (little-endian, unpadded); sum the values whose id is a multiple of
# 7 in record order, FNV-1a-64 every name byte in order, and print
# "records <n>", "sum7 <sum :Fixed(6)>", "fnv <hash as 16 hex digits>".
# As run.jet's `?? return`: a file that cannot be read, a wrong magic or a
# record cut short prints nothing and exits 0. Bytes after the n-th record
# are ignored.
#
# Technique:
# - the file is mapped with MAP_POPULATE and walked with one pointer, one
#   unsigned length compare per header and per name;
# - "id % 7 == 0" is one multiply by the inverse of 7 mod 2^32 and a
#   compare; the value or +0.0 is chosen by cmov and always added (the sum
#   starts at +0.0 and so is never -0.0: adding +0.0 changes nothing), so
#   the shuffled ids cost no branch;
# - FNV-1a is a serial xor/imul chain through every byte; the loop keeps
#   only that chain on the critical path;
# - `:Fixed(6)` is the runtime's jet_fmt_decimal: the value times 1e6,
#   rounded half away from zero; below 2^63 that integer's digits, at or
#   past it the exact binary expansion of the value rounded half away at
#   the sixth place (a multiword integer printed by divisions by 10^19);
#   NaN / inf / -inf for the non-finite.
    .intel_syntax noprefix
    .text

    .set STAT, 0                 # struct stat (144 bytes)
    .set COUNT, 144
    .set BIG, 160                # 17 words: a value up to 2^1024
    .set CHUNKS, 304             # 17 base-10^19 digits
    .set OUT, 448
    .set FRAME, OUT + 512

_start:
    mov rdi, [rsp + 16]          # argv[1]
    sub rsp, FRAME
    test rdi, rdi
    jz quiet
    xor esi, esi                 # O_RDONLY
    mov eax, 2                   # open
    syscall
    test rax, rax
    js quiet
    mov rbx, rax
    mov edi, eax
    lea rsi, [rsp + STAT]
    mov eax, 5                   # fstat
    syscall
    test rax, rax
    js quiet
    mov r13, [rsp + STAT + 48]   # st_size
    cmp r13, 8
    jb quiet
    xor edi, edi
    mov rsi, r13
    mov edx, 1                   # PROT_READ
    mov r10d, 0x8002             # MAP_PRIVATE | MAP_POPULATE
    mov r8, rbx
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja quiet
    lea r14, [rax + r13]         # end
    cmp dword ptr [rax], 0x3142474a      # "JGB1"
    jne quiet
    mov r15d, [rax + 4]          # n
    mov [rsp + COUNT], r15
    lea rsi, [rax + 8]           # cursor
    xorpd xmm0, xmm0             # sum7
    mov rbx, 0xcbf29ce484222325  # FNV offset basis
    mov r8, 0x100000001b3        # FNV prime
    xor r9d, r9d
    test r15, r15
    jz report
record:
    mov rax, r14
    sub rax, rsi
    cmp rax, 14
    jb quiet
    imul ecx, [rsi], 0xb6db6db7  # id * 7^-1 mod 2^32 <= (2^32 - 1) / 7 <=> 7 | id
    mov rax, [rsi + 4]
    movzx edx, word ptr [rsi + 12]
    add rsi, 14
    cmp ecx, 0x24924924
    cmova rax, r9
    movq xmm1, rax
    addsd xmm0, xmm1
    mov rax, r14
    sub rax, rsi
    cmp rax, rdx
    jb quiet
    lea rdi, [rsi + rdx]
    test edx, 1
    jz 2f
    movzx eax, byte ptr [rsi]
    inc rsi
    xor rbx, rax
    imul rbx, r8
2:  cmp rsi, rdi
    jae 3f
1:  movzx eax, byte ptr [rsi]
    movzx ecx, byte ptr [rsi + 1]
    add rsi, 2
    xor rbx, rax
    imul rbx, r8
    xor rbx, rcx
    imul rbx, r8
    cmp rsi, rdi
    jb 1b
3:  dec r15
    jnz record

report:
    lea rdi, [rsp + OUT]
    mov rax, 0x207364726f636572  # "records "
    mov [rdi], rax
    add rdi, 8
    mov rax, [rsp + COUNT]
    call put_dec
    mov dword ptr [rdi], 0x6d75730a      # "\nsum"
    mov word ptr [rdi + 4], 0x2037       # "7 "
    add rdi, 6
    call put_fixed6
    mov dword ptr [rdi], 0x766e660a      # "\nfnv"
    mov byte ptr [rdi + 4], 0x20
    add rdi, 5
    mov ecx, 16
    lea r10, [rip + hexdigits]
4:  rol rbx, 4
    mov eax, ebx
    and eax, 15
    movzx eax, byte ptr [r10 + rax]
    mov [rdi], al
    inc rdi
    dec ecx
    jnz 4b
    mov byte ptr [rdi], 10
    inc rdi
    lea rsi, [rsp + OUT]
    mov rdx, rdi
    sub rdx, rsi
5:  mov edi, 1
    mov eax, 1
    syscall
    cmp rax, -4                  # EINTR
    je 5b
    test rax, rax
    jle fail
    add rsi, rax
    sub rdx, rax
    jnz 5b
quiet:
    xor edi, edi
    mov eax, 231
    syscall
fail:
    mov edi, 1
    mov eax, 231
    syscall

# rax -> its decimal digits at rdi, rdi advanced. Clobbers rax, rcx, rdx, rsi, r10.
put_dec:
    mov r10d, 10
    lea rsi, [rsp - 8]           # the red zone: digits backwards from here
    mov rcx, rsi
1:  xor edx, edx
    div r10
    add dl, '0'
    dec rsi
    mov [rsi], dl
    test rax, rax
    jnz 1b
2:  mov dl, [rsi]
    mov [rdi], dl
    inc rsi
    inc rdi
    cmp rsi, rcx
    jb 2b
    ret

# rax -> exactly rcx decimal digits at rdi (zero-padded), rdi advanced.
# Clobbers rax, rdx, rsi, r10.
put_pad:
    mov r10d, 10
    lea rsi, [rdi + rcx]
1:  xor edx, edx
    div r10
    add dl, '0'
    dec rsi
    mov [rsi], dl
    cmp rsi, rdi
    ja 1b
    add rdi, rcx
    ret

# xmm0 -> its :Fixed(6) text at rdi, rdi advanced.
put_fixed6:
    movq r11, xmm0
    btr r11, 63                  # the magnitude's bits
    mov rax, 0x7ff0000000000000
    cmp r11, rax
    jae nonfinite
    xorpd xmm2, xmm2
    ucomisd xmm2, xmm0           # value < 0 (not -0.0)
    seta r9b
    movq xmm1, r11
    mulsd xmm1, [rip + million]  # |value * 10^6|
    roundsd xmm2, xmm1, 3        # truncated
    movapd xmm3, xmm1
    subsd xmm3, xmm2             # exact: the fraction
    ucomisd xmm3, [rip + half]
    jb 1f
    addsd xmm2, [rip + one]      # half away from zero
1:  ucomisd xmm2, [rip + two63]
    jae exact
    cvttsd2si rax, xmm2
    test rax, rax
    jz 2f
    test r9b, r9b
    jz 2f
    mov byte ptr [rdi], '-'
    inc rdi
2:  xor edx, edx
    mov ecx, 1000000
    div rcx
    push rdx
    call put_dec                 # the whole part
    mov byte ptr [rdi], '.'
    inc rdi
    pop rax
    mov ecx, 6
    jmp put_pad
exact:
    # |value| >= (2^63 - 1/2) / 10^6 > 2^43: a normal of exponent >= -9
    test r9b, r9b
    jz 1f
    mov byte ptr [rdi], '-'
    inc rdi
1:  mov rax, r11
    shr rax, 52
    lea rcx, [rax - 1075]        # value = m * 2^rcx
    mov rax, 0x000fffffffffffff
    and r11, rax
    bts r11, 52                  # m
    test rcx, rcx
    js 3f
    # m * 2^rcx: an integer of up to 1024 bits, then ".000000"
    lea r10, [rsp + BIG + 8]     # (a call deep)
    xor eax, eax
    mov edx, 17
2:  mov [r10 + rdx * 8 - 8], rax
    dec edx
    jnz 2b
    mov edx, ecx
    shr edx, 6                   # the word
    and ecx, 63
    mov rax, r11
    shl rax, cl
    mov [r10 + rdx * 8], rax
    mov rax, r11
    neg cl
    jz 21f
    shr rax, cl
    mov [r10 + rdx * 8 + 8], rax
21: push rbx
    push r12
    lea r12, [r10 + 136]         # past the top word
    lea r11, [rsp + CHUNKS + 24] # the chunks, least significant first (two pushes and a call deep)
    mov rbx, 10000000000000000000
    xor r8d, r8d                 # chunk count
22: xor edx, edx                 # one long division by 10^19, top word down
    mov rcx, r12
23: sub rcx, 8
    mov rax, [rcx]
    div rbx
    mov [rcx], rax
    cmp rcx, r10
    ja 23b
    mov [r11 + r8 * 8], rdx
    inc r8
    mov rcx, r12                 # drop zero top words; stop at zero
24: cmp rcx, r10
    je 25f
    cmp qword ptr [rcx - 8], 0
    jne 22b
    sub rcx, 8
    mov r12, rcx
    jmp 24b
25: pop r12
    pop rbx
    dec r8
    mov rax, [r11 + r8 * 8]      # the most significant chunk, unpadded
    push r8
    push r11
    call put_dec
    pop r11
    pop r8
26: test r8, r8
    jz 27f
    dec r8
    mov rax, [r11 + r8 * 8]
    mov ecx, 19
    push r8
    push r11
    call put_pad
    pop r11
    pop r8
    jmp 26b
27: mov byte ptr [rdi], '.'
    mov dword ptr [rdi + 1], 0x30303030
    mov word ptr [rdi + 5], 0x3030
    add rdi, 7
    ret
3:  # m / 2^-rcx with -rcx <= 9: whole part, six places rounded half away
    neg ecx
    mov rax, r11
    shr rax, cl                  # the whole part
    mov edx, 1
    shl rdx, cl
    dec rdx                      # the fraction mask
    and r11, rdx
    imul r11, r11, 1000000
    mov r8, r11
    shr r8, cl                   # six places
    and r11, rdx
    add r11, r11
    inc rdx                      # 2^-rcx
    cmp r11, rdx
    jb 31f
    inc r8
    cmp r8, 1000000
    jb 31f
    xor r8d, r8d
    inc rax
31: push r8
    call put_dec
    mov byte ptr [rdi], '.'
    inc rdi
    pop rax
    mov ecx, 6
    jmp put_pad
nonfinite:
    mov rax, 0x7ff0000000000000
    cmp r11, rax
    ja 1f
    movq rax, xmm0
    test rax, rax
    jns 2f
    mov byte ptr [rdi], '-'
    inc rdi
2:  mov dword ptr [rdi], 0x00666e69  # "inf"
    add rdi, 3
    ret
1:  mov dword ptr [rdi], 0x004e614e  # "NaN"
    add rdi, 3
    ret

    .p2align 3
million: .double 1000000
half: .double 0.5
one: .double 1
two63: .double 9223372036854775808
hexdigits: .ascii "0123456789abcdef"
