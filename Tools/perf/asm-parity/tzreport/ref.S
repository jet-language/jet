# tzreport (gauntlet time.script and time.notebook): read the meetings file
# argv[1], one "<name>|<RFC 3339 timestamp>" per line (blank lines
# skipped), order the meetings by instant (stable), and print per meeting
# "<name> utc=<YYYY-MM-DDTHH:MM:SSZ> day=<Www> gap=<minutes since the one
# before, or ->", then "span <minutes first to last> minutes <count>".
#
# Technique:
# - the file is mapped (MAP_POPULATE); each line becomes one 32-byte entry
#   {instant in seconds, name pointer, name length} that points into it;
# - a timestamp is fixed-position digits checked in place, and its day is
#   the closed-form civil-date count (400-year eras), so that no table or
#   calendar loop runs; the UTC date printed is the inverse closed form, the
#   weekday the day count mod 7;
# - the entries sort by a stable bottom-up merge between two arrays;
# - the output is built in one buffer and leaves in as few writes as fit
#   (EINTR retried, short writes advanced).
# Fractional seconds are refused with the malformed timestamps. A file that
# cannot be read prints "read failed", a line without "|" "bad meeting", a
# malformed timestamp "bad timestamp", and a file with no meetings "no
# meetings" (the Jet program's bounds stop on the last meeting), each on
# stderr with exit 1.
    .intel_syntax noprefix
    .text

    .set STAT, 0                 # struct stat (144 bytes)
    .set SRC, 144
    .set DST, 152
    .set OUT, 160
    .set COUNT, 168
    .set WIDTH, 176
    .set FRAME, 256
    .set SHIFT, 400              # years added so that every division is unsigned
    .set EPOCH, 719468 + 146097  # day 0 of 1970-01-01 in the shifted count

    # Two decimal digits at [r10 + off] into reg, or bad_stamp.
    .macro DIG2 off, reg
    movzx eax, byte ptr [r10 + \off]
    sub eax, '0'
    cmp eax, 9
    ja bad_stamp
    movzx ecx, byte ptr [r10 + \off + 1]
    sub ecx, '0'
    cmp ecx, 9
    ja bad_stamp
    imul eax, eax, 10
    add eax, ecx
    mov \reg, eax
    .endm

_start:
    mov rdi, [rsp + 16]          # argv[1]
    sub rsp, FRAME
    test rdi, rdi
    jz bad_read
    xor esi, esi                 # O_RDONLY
    mov eax, 2                   # open
    syscall
    test rax, rax
    js bad_read
    mov rbx, rax
    mov edi, eax
    lea rsi, [rsp + STAT]
    mov eax, 5                   # fstat
    syscall
    test rax, rax
    js bad_read
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
    ja bad_read
    mov r12, rax
1:  # Two entry arrays (a meeting line is at least 21 bytes), then the
    # output buffer.
    mov rbp, r13
    shr rbp, 4
    add rbp, 2
    shl rbp, 5                   # one array's bytes
    lea rsi, [rbp * 2 + 512]
    imul rax, r13, 6
    add rsi, rax
    xor edi, edi
    mov edx, 3                   # PROT_READ | PROT_WRITE
    mov r10d, 0x22               # MAP_PRIVATE | MAP_ANONYMOUS
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja bad_read
    mov [rsp + SRC], rax
    lea rdi, [rax + rbp]
    mov [rsp + DST], rdi
    lea rdi, [rdi + rbp]
    mov [rsp + OUT], rdi

    # Scan: one entry per line that is not empty.
    mov rdi, [rsp + SRC]
    mov rsi, r12
    lea r8, [r12 + r13]
line:
    cmp rsi, r8
    jae scanned
    mov r12, rsi                 # the line's start
    xor r14d, r14d               # its "|"
2:  cmp rsi, r8
    jae 3f
    movzx eax, byte ptr [rsi]
    cmp eax, 10
    je 3f
    test r14, r14
    jnz 21f
    cmp eax, '|'
    jne 21f
    mov r14, rsi
21: inc rsi
    jmp 2b
3:  mov r13, rsi                 # its end
    inc rsi                      # past the newline
    cmp r13, r12
    je line
    test r14, r14
    jz bad_meeting
    # The timestamp: YYYY-MM-DDTHH:MM:SS then Z or +HH:MM / -HH:MM.
    lea r10, [r14 + 1]
    mov r11, r13
    sub r11, r10
    cmp r11, 20
    jb bad_stamp
    DIG2 0, ebx
    DIG2 2, edx
    imul ebx, ebx, 100
    add ebx, edx                 # year
    cmp byte ptr [r10 + 4], '-'
    jne bad_stamp
    cmp byte ptr [r10 + 7], '-'
    jne bad_stamp
    movzx eax, byte ptr [r10 + 10]
    or eax, 0x20
    cmp eax, 't'
    jne bad_stamp
    cmp byte ptr [r10 + 13], ':'
    jne bad_stamp
    cmp byte ptr [r10 + 16], ':'
    jne bad_stamp
    DIG2 5, r9d                  # month
    DIG2 8, r15d                 # day
    lea eax, [r9 - 1]
    cmp eax, 11
    ja bad_stamp
    lea eax, [r15 - 1]
    cmp eax, 30
    ja bad_stamp
    DIG2 11, edx                 # hour
    cmp edx, 23
    ja bad_stamp
    imul edx, edx, 3600
    push rdx
    DIG2 14, edx                 # minute
    cmp edx, 59
    ja bad_stamp
    imul edx, edx, 60
    add [rsp], rdx
    DIG2 17, edx                 # second
    cmp edx, 59
    ja bad_stamp
    add [rsp], rdx
    movzx eax, byte ptr [r10 + 19]
    or eax, 0x20
    cmp eax, 'z'
    jne 4f
    cmp r11, 20
    jne bad_stamp
    xor edx, edx
    jmp 6f
4:  cmp r11, 25
    jne bad_stamp
    cmp byte ptr [r10 + 22], ':'
    jne bad_stamp
    DIG2 20, edx
    cmp edx, 23
    ja bad_stamp
    imul edx, edx, 60
    push rdx
    DIG2 23, edx
    cmp edx, 59
    ja bad_stamp
    pop rax
    add edx, eax
    imul edx, edx, 60            # the offset in seconds
    movzx eax, byte ptr [r10 + 19]
    cmp eax, '+'
    je 6f
    cmp eax, '-'
    jne bad_stamp
    neg rdx
6:  pop r11
    sub r11, rdx                 # seconds of the day, less the offset
    # The day: era, year of era, day of year, day of era.
    lea ecx, [r9 - 3]
    lea edx, [r9 + 9]
    cmp r9d, 2
    cmovle ecx, edx              # the month counted from March
    imul eax, ecx, 153
    add eax, 2
    xor edx, edx
    mov ecx, 5
    div ecx
    lea r10d, [rax + r15 - 1]    # day of year
    lea eax, [rbx + SHIFT]
    cmp r9d, 2
    jg 7f
    dec eax
7:  xor edx, edx
    mov ecx, 400
    div ecx
    mov r15d, eax                # era
    mov r9d, edx                 # year of era
    imul ecx, edx, 365
    shr edx, 2
    add ecx, edx
    mov eax, r9d
    xor edx, edx
    mov ebx, 100
    div ebx
    sub ecx, eax
    add ecx, r10d                # day of era
    imul rax, r15, 146097
    add rax, rcx
    sub rax, EPOCH
    imul rax, rax, 86400
    add rax, r11
    mov [rdi], rax
    mov [rdi + 8], r12
    mov rcx, r14
    sub rcx, r12
    mov [rdi + 16], rcx
    add rdi, 32
    jmp line
scanned:
    sub rdi, [rsp + SRC]
    shr rdi, 5
    mov [rsp + COUNT], rdi
    test rdi, rdi
    jz bad_none

    # Sort: merge runs of WIDTH entries from SRC into DST, then swap.
    mov qword ptr [rsp + WIDTH], 1
pass:
    mov rax, [rsp + WIDTH]
    cmp rax, [rsp + COUNT]
    jae sorted
    xor r15d, r15d
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
    shl rsi, 5
    mov rdi, [rsp + DST]
    add rdi, rsi
    add rsi, rax
    shl rcx, 5
    lea r12, [rax + rcx]
    mov r13, r12
    mov r15, rdx
    shl rdx, 5
    lea r14, [rax + rdx]
merge:
    cmp rsi, r12
    jae rest
    cmp r13, r14
    jae rest
    mov rax, [r13]
    cmp rax, [rsi]
    jl 1f
    movups xmm0, [rsi]
    movups xmm1, [rsi + 16]
    movups [rdi], xmm0
    movups [rdi + 16], xmm1
    add rsi, 32
    add rdi, 32
    jmp merge
1:  movups xmm0, [r13]
    movups xmm1, [r13 + 16]
    movups [rdi], xmm0
    movups [rdi + 16], xmm1
    add r13, 32
    add rdi, 32
    jmp merge
rest:
    cmp rsi, r12
    jae 2f
    movups xmm0, [rsi]
    movups xmm1, [rsi + 16]
    movups [rdi], xmm0
    movups [rdi + 16], xmm1
    add rsi, 32
    add rdi, 32
    jmp rest
2:  cmp r13, r14
    jae run
    movups xmm0, [r13]
    movups xmm1, [r13 + 16]
    movups [rdi], xmm0
    movups [rdi + 16], xmm1
    add r13, 32
    add rdi, 32
    jmp 2b
passed:
    mov rax, [rsp + SRC]
    mov rcx, [rsp + DST]
    mov [rsp + SRC], rcx
    mov [rsp + DST], rax
    shl qword ptr [rsp + WIDTH], 1
    jmp pass

sorted:
    mov rdi, [rsp + OUT]
    mov rbx, [rsp + SRC]
    xor r12d, r12d               # position
meeting:
    cmp r12, [rsp + COUNT]
    jae span
    mov rsi, [rbx + 8]
    mov rcx, [rbx + 16]
    rep movsb
    mov dword ptr [rdi], 0x63747520  # " utc"
    mov byte ptr [rdi + 4], '='
    add rdi, 5
    # The UTC date and time.
    mov rax, [rbx]
    mov rcx, 86400
    cqo
    idiv rcx
    test rdx, rdx
    jns 1f
    add rdx, rcx
    dec rax
1:  mov r13, rdx                 # seconds of the day
    lea r14, [rax + EPOCH]       # the shifted day count
    mov rax, r14
    xor edx, edx
    mov ecx, 146097
    div rcx
    mov r15, rax                 # era
    mov r8, rdx                  # day of era
    mov rax, rdx
    xor edx, edx
    mov ecx, 1460
    div rcx
    mov r9, r8
    sub r9, rax
    mov rax, r8
    xor edx, edx
    mov ecx, 36524
    div rcx
    add r9, rax
    mov rax, r8
    xor edx, edx
    mov ecx, 146096
    div rcx
    sub r9, rax
    mov rax, r9
    xor edx, edx
    mov ecx, 365
    div rcx
    mov r9, rax                  # year of era
    imul r10, r9, 365
    mov rax, r9
    shr rax, 2
    add r10, rax
    mov rax, r9
    xor edx, edx
    mov ecx, 100
    div rcx
    sub r10, rax
    mov r11, r8
    sub r11, r10                 # day of year
    imul rax, r11, 5
    add rax, 2
    xor edx, edx
    mov ecx, 153
    div rcx
    mov r10, rax                 # the month counted from March
    imul rax, rax, 153
    add rax, 2
    xor edx, edx
    mov ecx, 5
    div rcx
    sub r11, rax
    inc r11                      # day
    lea rax, [r10 + 3]
    lea rcx, [r10 - 9]
    cmp r10, 10
    cmovae rax, rcx
    mov r10, rax                 # month
    imul rax, r15, 400
    add rax, r9
    sub rax, SHIFT
    cmp r10, 2
    ja 2f
    inc rax
2:  xor edx, edx                 # YYYY
    mov ecx, 100
    div rcx
    push rdx
    call put2
    pop rax
    call put2
    mov byte ptr [rdi], '-'
    inc rdi
    mov rax, r10
    call put2
    mov byte ptr [rdi], '-'
    inc rdi
    mov rax, r11
    call put2
    mov byte ptr [rdi], 'T'
    inc rdi
    mov rax, r13
    xor edx, edx
    mov ecx, 3600
    div rcx
    push rdx
    call put2
    mov byte ptr [rdi], ':'
    inc rdi
    pop rax
    xor edx, edx
    mov ecx, 60
    div rcx
    push rdx
    call put2
    mov byte ptr [rdi], ':'
    inc rdi
    pop rax
    call put2
    mov byte ptr [rdi], 'Z'
    inc rdi
    # " day=Www"
    mov dword ptr [rdi], 0x79616420  # " day"
    mov byte ptr [rdi + 4], '='
    add rdi, 5
    lea rax, [r14 + 3]
    xor edx, edx
    mov ecx, 7
    div rcx
    lea rsi, [rip + days]
    lea rsi, [rsi + rdx * 2]
    add rsi, rdx
    mov ecx, 3
    rep movsb
    # " gap=<minutes>" or " gap=-"
    mov dword ptr [rdi], 0x70616720  # " gap"
    mov byte ptr [rdi + 4], '='
    add rdi, 5
    test r12, r12
    jnz 3f
    mov byte ptr [rdi], '-'
    inc rdi
    jmp 4f
3:  mov rax, [rbx]
    sub rax, [rbx - 32]
    xor edx, edx
    mov ecx, 60
    div rcx
    call put_uint
4:  mov byte ptr [rdi], 10
    inc rdi
    add rbx, 32
    inc r12
    jmp meeting

span:
    mov dword ptr [rdi], 0x6e617073  # "span"
    mov byte ptr [rdi + 4], ' '
    add rdi, 5
    mov rax, [rbx - 32]
    mov rcx, [rsp + SRC]
    sub rax, [rcx]
    xor edx, edx
    mov ecx, 60
    div rcx
    call put_uint
    lea rsi, [rip + s_minutes]
    mov ecx, 9
    rep movsb
    mov rax, [rsp + COUNT]
    call put_uint
    mov byte ptr [rdi], 10
    inc rdi
    mov rsi, [rsp + OUT]
    mov rdx, rdi
    sub rdx, rsi
5:  mov edi, 1
    mov eax, 1                   # write
    syscall
    cmp rax, -4
    je 5b
    test rax, rax
    js bad_read
    add rsi, rax
    sub rdx, rax
    jnz 5b
    xor edi, edi
    mov eax, 231                 # exit_group
    syscall

# put2: rax (0 to 99) as two digits at rdi. Clobbers rax, rcx, rdx.
put2:
    xor edx, edx
    mov ecx, 10
    div rcx
    add al, '0'
    add dl, '0'
    mov [rdi], al
    mov [rdi + 1], dl
    add rdi, 2
    ret

# put_uint: rax in decimal at rdi. Clobbers rax, rcx, rdx, r8, r9.
put_uint:
    lea r8, [rsp - 8]            # the digits, in the red zone
    mov r9, r8
    mov ecx, 10
1:  xor edx, edx
    div rcx
    add dl, '0'
    dec r8
    mov [r8], dl
    test rax, rax
    jnz 1b
2:  mov dl, [r8]
    mov [rdi], dl
    inc rdi
    inc r8
    cmp r8, r9
    jb 2b
    ret

bad_read:
    lea rsi, [rip + s_read]
    mov edx, 12
    jmp fail
bad_meeting:
    lea rsi, [rip + s_meeting]
    mov edx, 12
    jmp fail
bad_stamp:
    lea rsi, [rip + s_stamp]
    mov edx, 14
    jmp fail
bad_none:
    lea rsi, [rip + s_none]
    mov edx, 12
fail:
    mov edi, 2
    mov eax, 1                   # write
    syscall
    mov edi, 1
    mov eax, 231
    syscall

days:      .ascii "SunMonTueWedThuFriSat"
s_minutes: .ascii " minutes "
s_read:    .ascii "read failed\n"
s_meeting: .ascii "bad meeting\n"
s_stamp:   .ascii "bad timestamp\n"
s_none:    .ascii "no meetings\n"
