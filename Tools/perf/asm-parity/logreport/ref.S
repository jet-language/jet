# logreport (gauntlet text.report-cli): read the log argv[1] line by line
# (split on '\n', a last line without one included); each line is
# "<stamp> <level> <component> ..." (three spaces at least: a line with
# fewer is an error), the stamp an RFC 3339 date-time. Count the levels
# DEBUG, INFO, WARN and ERROR, the ERROR lines per component, and print
#   DEBUG <n> / INFO <n> / WARN <n> / ERROR <n> / top-error-components:
#   "<count> <component>" for the top 3 (count descending, ties by bytes
#   ascending) / "span <first stamp> .. <last stamp> (<seconds>s)",
# the seconds the difference of the two stamps' Unix milliseconds divided
# by 1000, exactly: up to 3 decimals, trailing zeros dropped (-1.998, 0.01).
#
# Input class: a stamp has the canonical shape
#   YYYY-MM-DDTHH:MM:SS[.fraction of 1 to 9 digits](Z|+HH:MM|-HH:MM)
# with a year from 1700 to 2200 and no leap second (SS 60). In that shape
# the reference checks what Jet's time.parse_rfc3339 checks (month, day of
# the month, hour, minute, second and offset ranges) and fails where it
# fails. A stamp of another shape, a year outside that range, or a leap
# second is outside the class: exit 2.
#
# Technique:
# - the file is mapped into a reservation one page longer, so that vector
#   loads past its end read zeros; it is resident a window at a time (the
#   next two populated ahead, the one before the last dropped behind), so
#   memory stays three windows whatever the file's size;
# - AVX-512BW compares 64 bytes at a time into a '\n' mask and a ' ' mask,
#   walked together with tzcnt/blsr: three spaces give a line's stamp,
#   level and component, and then every space up to the line's '\n' goes
#   at once (blsmsk of the '\n' bit);
# - a stamp of the common shape (20 bytes, 'Z') whose date bytes equal the
#   last date checked has only its "HH:MM:SS" checked, as one 64-bit word:
#   digits and colons by an xor with the template and one add, ranges by
#   three byte compares; any other stamp takes the full parse; only the
#   first and last stamps are converted to milliseconds, at the end;
# - the level is its length and one 4-byte compare (5 bytes: and one more
#   byte);
# - an ERROR line's component of up to 16 bytes is its zero-padded 16
#   bytes (one masked load), hashed by two crc32 seeded with its length and
#   compared whole (a longer one hashes all its words), in open addressing
#   of 64-byte entries doubled at half full; the top 3 is one pass of
#   insertion over the entries;
# - the file must be UTF-8, as the Jet program reads its lines as Strings:
#   the classification ors every byte into one register and a scalar UTF-8
#   check runs only when one has its high bit set;
# - output goes out through one buffer.
# Stops are the Jet program's, in its order, with its status and its stderr
# report byte for byte. Its file line source reads every line before the
# loop's first, so a file that is not UTF-8 stops first wherever that is
# (status 70, the read's panic naming argv[1]). Then, at the first line
# that has one: fewer than three spaces (70, `bad line` at the split that
# failed, with the locals first_ms and last_ms of the lines before) or a
# stamp that is not a date-time (70, `bad timestamp`). Exit 1, without
# the report, when the file cannot be opened or mapped (Jet returns the
# IOError: status 1).
    .intel_syntax noprefix
    .text

    .set STAT, 0                 # struct stat (144 bytes)
    .set SP1, 144                # this line's first and second spaces
    .set SP2, 152
    .set FIRSTP, 160             # the first stamp (0 before any line)
    .set FIRSTN, 168
    .set LASTP, 176              # the last stamp
    .set LASTN, 184
    .set NDEBUG, 192
    .set NINFO, 200
    .set NWARN, 208
    .set NERROR, 216
    .set DISTINCT, 224
    .set DATE, 232               # the date bytes last checked (10)
    .set PATHP, 248              # argv[1]
    .set TOP, 256                # the top 3 entries
    .set OUT, 320
    .set OUTSZ, 4096
    .set FRAME, OUT + OUTSZ
    .set NEXTW, 304              # the chunk offset of the next window
    .set WINDOW, 1 << 17         # the file's resident window
    .set INITCAP, 64             # entries, 64 bytes each

_start:
    mov rdi, [rsp + 16]          # argv[1]
    sub rsp, FRAME
    mov [rsp + PATHP], rdi
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
    mov r10d, 0x12               # MAP_PRIVATE | MAP_FIXED
    mov r8, rbx
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail
1:  xor edi, edi                 # the components
    mov esi, INITCAP * 64
    mov edx, 3
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail
    mov r14, rax
    mov ebp, INITCAP - 1
    xor eax, eax
    mov [rsp + FIRSTP], rax
    mov [rsp + FIRSTN], rax
    mov [rsp + LASTP], rax
    mov [rsp + LASTN], rax
    mov [rsp + NDEBUG], rax
    mov [rsp + NINFO], rax
    mov [rsp + NWARN], rax
    mov [rsp + NERROR], rax
    mov [rsp + DISTINCT], rax
    mov [rsp + TOP], rax
    mov [rsp + TOP + 8], rax
    mov [rsp + TOP + 16], rax
    mov qword ptr [rsp + DATE], -1   # no date: never digits
    mov qword ptr [rsp + DATE + 8], -1
    lea r15, [r12 + r13]
    mov eax, 0x0a0a0a0a
    vpbroadcastd zmm20, eax      # '\n'
    mov eax, 0x20202020
    vpbroadcastd zmm21, eax      # ' '
    vpxord zmm25, zmm25, zmm25   # every byte or-ed
    vmovdqu xmm9, [rip + iota]

    xor ebx, ebx                 # chunk offset
    mov rsi, r12                 # line start
    xor edi, edi                 # spaces seen in the line (3: skip to '\n')
    mov qword ptr [rsp + NEXTW], -WINDOW
    call advance                 # the first two windows in
    call advance

chunk:
    cmp rbx, r13
    jae eof
    cmp rbx, [rsp + NEXTW]
    jb 1f
    call advance
1:
    vmovdqu64 zmm0, [r12 + rbx]
    vpord zmm25, zmm25, zmm0
    vpcmpeqb k1, zmm0, zmm20
    vpcmpeqb k2, zmm0, zmm21
    kmovq r11, k1                # '\n'
    kmovq r10, k2
    or r10, r11                  # '\n' and ' '
next:
    test r10, r10
    jz 9f
    cmp edi, 3
    je skip
    tzcnt rcx, r10
    blsr r10, r10
    bt r11, rcx
    jc stop                      # fewer than three spaces; edi: the split
    lea rdx, [r12 + rbx]
    add rdx, rcx
    inc edi
    cmp edi, 2
    jb 1f
    je 2f
    jmp line
1:  mov [rsp + SP1], rdx
    jmp next
2:  mov [rsp + SP2], rdx
    jmp next

skip:
    mov rax, r10
    and rax, r11
    jz 8f
    tzcnt rcx, rax
    blsmsk rax, rax
    andn r10, rax, r10           # the spaces up to the '\n' go
    lea rsi, [r12 + rbx + 1]
    add rsi, rcx
    xor edi, edi
    jmp next
8:  xor r10d, r10d
9:  add rbx, 64
    jmp chunk

# The third space of the line at rsi is rdx.
line:
    push r10
    push r11
    mov rcx, [rsp + 16 + SP1]
    sub rcx, rsi                 # the stamp [rsi, rsi + rcx)
    cmp qword ptr [rsp + 16 + FIRSTP], 0
    jne 1f
    mov [rsp + 16 + FIRSTP], rsi
    mov [rsp + 16 + FIRSTN], rcx
1:  mov [rsp + 16 + LASTP], rsi
    mov [rsp + 16 + LASTN], rcx
    cmp rcx, 20
    jne slow
    cmp byte ptr [rsi + 19], 'Z'
    jne slow
    mov rax, [rsi]
    cmp rax, [rsp + 16 + DATE]
    jne slow
    movzx eax, word ptr [rsi + 8]
    cmp ax, [rsp + 16 + DATE + 8]
    jne slow
    cmp byte ptr [rsi + 10], 'T'
    jne slow
    mov rax, [rsi + 11]          # "HH:MM:SS"
    mov r8, 0x30303a30303a3030
    xor rax, r8                  # digits 0..9, colons 0
    mov r8, 0x7676767676767676
    add r8, rax
    or r8, rax
    mov r9, 0x8080808080808080
    test r8, r9
    jnz slow
    mov r9, 0x0000ff0000ff0000
    test rax, r9
    jnz slow
    cmp al, 2
    jb 2f
    ja slow
    cmp ah, 3
    ja slow
2:  shr rax, 24
    cmp al, 5
    ja slow
    shr rax, 24
    cmp al, 5
    ja slow
    jmp level
slow:
    push rdx
    lea rdi, [rsp + 24 + DATE]
    call stamp
    pop rax
    test edx, edx
    jnz 3f
    mov rdx, rax
    jmp level
3:  add rsp, 16                  # not a date-time (1) or outside the class (2)
    lea edi, [rdx + 2]
    jmp stop
level:
    mov rsi, [rsp + 16 + SP1]
    inc rsi                      # the level [rsi, SP2)
    mov rcx, [rsp + 16 + SP2]
    sub rcx, rsi
    mov eax, [rsi]
    cmp rcx, 4
    je 4f
    cmp rcx, 5
    jne 8f
    movzx ecx, byte ptr [rsi + 4]
    cmp eax, 0x55424544          # "DEBU"
    jne 5f
    cmp cl, 'G'
    jne 8f
    inc qword ptr [rsp + 16 + NDEBUG]
    jmp 8f
5:  cmp eax, 0x4f525245          # "ERRO"
    jne 8f
    cmp cl, 'R'
    jne 8f
    inc qword ptr [rsp + 16 + NERROR]
    mov rsi, [rsp + 16 + SP2]
    inc rsi                      # the component [rsi, rdx)
    mov rcx, rdx
    sub rcx, rsi
    call group
    inc qword ptr [r8 + 24]
    jmp 8f
4:  cmp eax, 0x4f464e49          # "INFO"
    jne 6f
    inc qword ptr [rsp + 16 + NINFO]
    jmp 8f
6:  cmp eax, 0x4e524157          # "WARN"
    jne 8f
    inc qword ptr [rsp + 16 + NWARN]
8:  pop r11
    pop r10
    mov edi, 3
    jmp next

eof:
    test edi, edi
    jnz 1f
    cmp rsi, r15
    jb stop                      # a last line with no space
    jmp 2f
1:  cmp edi, 3
    jne stop
2:  vpmovb2m k1, zmm25
    kortestq k1, k1
    jz report
    mov rsi, r12
    call utf8
    jc stop_utf8

report:
    # The top 3: insertion of every entry.
    mov rsi, r14
    lea r13, [rbp + 1]
1:  cmp qword ptr [rsi + 40], 0
    je 6f
    mov r8, rsi                  # carried down the slots
    lea r12, [rsp + TOP]
    mov r15d, 3
2:  mov rcx, [r12]
    test rcx, rcx
    jz 4f
    mov rax, [r8 + 24]
    cmp rax, [rcx + 24]
    ja 3f
    jb 5f
    push rsi
    push r8
    push rcx
    call before                  # CF: r8's key sorts first
    pop rcx
    pop r8
    pop rsi
    jnc 5f
3:  mov [r12], r8                # r8 takes the slot, rcx moves down
    mov r8, rcx
    jmp 5f
4:  mov [r12], r8
    jmp 6f
5:  add r12, 8
    dec r15d
    jnz 2b
6:  add rsi, 64
    dec r13
    jnz 1b

    lea r12, [rsp + OUT]
    mov rbx, r12                 # output cursor
    mov r14d, 1                  # to stdout
    lea rsi, [rip + s_debug]
    mov ecx, 6
    call put
    mov rax, [rsp + NDEBUG]
    call putnum
    lea rsi, [rip + s_info]
    mov ecx, 6
    call put
    mov rax, [rsp + NINFO]
    call putnum
    lea rsi, [rip + s_warn]
    mov ecx, 6
    call put
    mov rax, [rsp + NWARN]
    call putnum
    lea rsi, [rip + s_error]
    mov ecx, 7
    call put
    mov rax, [rsp + NERROR]
    call putnum
    lea rsi, [rip + s_top]
    mov ecx, 23
    call put
    xor r13d, r13d
1:  mov r9, [rsp + TOP + r13 * 8]
    test r9, r9
    jz 2f
    mov rax, [r9 + 24]
    call putnum
    lea rsi, [rip + s_sp]
    mov ecx, 1
    call put
    mov rsi, [r9 + 40]
    mov ecx, [r9 + 16]
    call put
    call putnl
    inc r13
    cmp r13, 3
    jb 1b
2:  lea rsi, [rip + s_span]
    mov ecx, 5
    call put
    mov rsi, [rsp + FIRSTP]
    mov rcx, [rsp + FIRSTN]
    call put
    lea rsi, [rip + s_dots]
    mov ecx, 4
    call put
    mov rsi, [rsp + LASTP]
    mov rcx, [rsp + LASTN]
    call put
    lea rsi, [rip + s_open]
    mov ecx, 2
    call put
    xor r13d, r13d               # the milliseconds between them
    cmp qword ptr [rsp + FIRSTP], 0
    je 3f
    mov rsi, [rsp + LASTP]
    mov rcx, [rsp + LASTN]
    lea rdi, [rsp + 8 + DATE]
    call stamp
    mov r13, rax
    mov rsi, [rsp + FIRSTP]
    mov rcx, [rsp + FIRSTN]
    lea rdi, [rsp + 8 + DATE]
    call stamp
    sub r13, rax
    jns 3f
    lea rsi, [rip + s_minus]
    mov ecx, 1
    call put
    neg r13
3:  mov rax, r13
    xor edx, edx
    mov ecx, 1000
    div rcx
    mov r13, rdx                 # the fraction, in thousandths
    call putnum
    test r13, r13
    jz 5f
    mov eax, r13d                # ".ddd", trailing zeros dropped
    xor edx, edx
    mov ecx, 100
    div ecx
    add al, '0'
    mov byte ptr [rsp + SP1], '.'
    mov [rsp + SP1 + 1], al
    mov eax, edx
    mov cl, 10
    div cl
    add ax, 0x3030
    mov [rsp + SP1 + 2], ax
    mov ecx, 4
4:  cmp byte ptr [rsp + SP1 + rcx - 1], '0'
    jne 6f
    dec ecx
    jmp 4b
6:  lea rsi, [rsp + SP1]
    call put
5:  lea rsi, [rip + s_close]
    mov ecx, 3
    call put
    call flush
    xor edi, edi
    mov eax, 231
    syscall

# The Jet program's stop at a line, edi saying which: 0, 1 or 2 its split
# that failed, 3 its stamp not a date-time, 4 its stamp outside the class.
# A file that is not UTF-8 stops first. The frame is at rsp.
stop:
    mov ebp, edi
    mov rsi, r12
    call utf8
    jc stop_utf8
    cmp ebp, 4
    je class
    lea r12, [rsp + OUT]
    mov rbx, r12
    mov r14d, 2                  # to stderr
    cmp ebp, 3
    jne 1f
    lea rsi, [rip + e_ts]
    mov ecx, e_bl - e_ts
    call put
    jmp stop_tail
1:  lea rsi, [rip + e_bl]
    mov ecx, e_bl0 - e_bl
    call put
    lea rsi, [rip + e_bl0]
    mov ecx, e_bl1 - e_bl0
    cmp ebp, 1
    jb 2f
    lea rsi, [rip + e_bl1]
    mov ecx, e_bl2 - e_bl1
    je 2f
    lea rsi, [rip + e_bl2]
    mov ecx, e_locals - e_bl2
2:  call put
    lea rsi, [rip + e_locals]
    mov ecx, e_last - e_locals
    call put
    xor r13d, r13d               # first_ms and last_ms: 0 before any line
    xor eax, eax
    cmp qword ptr [rsp + FIRSTP], 0
    je 3f
    mov rsi, [rsp + LASTP]
    mov rcx, [rsp + LASTN]
    lea rdi, [rsp + 8 + DATE]
    call stamp
    mov r13, rax
    mov rsi, [rsp + FIRSTP]
    mov rcx, [rsp + FIRSTN]
    lea rdi, [rsp + 8 + DATE]
    call stamp
3:  call putsnum
    lea rsi, [rip + e_last]
    mov ecx, e_u8 - e_last
    call put
    mov rax, r13
    call putsnum
    call putnl
    jmp stop_tail

# The read's stop: a line that is not UTF-8.
stop_utf8:
    lea r12, [rsp + OUT]
    mov rbx, r12
    mov r14d, 2
    lea rsi, [rip + e_u8]
    mov ecx, e_u8b - e_u8
    call put
    mov rsi, [rsp + PATHP]
    xor ecx, ecx
1:  cmp byte ptr [rsi + rcx], 0
    je 2f
    inc rcx
    jmp 1b
2:  call put
    lea rsi, [rip + e_u8b]
    mov ecx, e_tail - e_u8b
    call put
stop_tail:
    lea rsi, [rip + e_tail]
    mov ecx, e_end - e_tail
    call put
    call flush
    mov edi, 70
    mov eax, 231
    syscall

# The stamp [rsi, rsi + rcx) in Unix milliseconds in rax and edx 0; edx 1
# when it is not a date-time, 2 when it is outside the class. rdi points
# at the 10 date bytes last checked, which a valid date replaces. Clobbers
# rcx, rdi, r8, r9, r10, r11.
stamp:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    cmp rcx, 20
    jb stamp_class
    cmp byte ptr [rsi + 4], '-'
    jne stamp_class
    cmp byte ptr [rsi + 7], '-'
    jne stamp_class
    cmp byte ptr [rsi + 10], 'T'
    jne stamp_class
    cmp byte ptr [rsi + 13], ':'
    jne stamp_class
    cmp byte ptr [rsi + 16], ':'
    jne stamp_class
    .macro D2 at, reg
    movzx eax, byte ptr [\at]
    sub eax, '0'
    cmp eax, 9
    ja stamp_class
    movzx edx, byte ptr [\at + 1]
    sub edx, '0'
    cmp edx, 9
    ja stamp_class
    imul eax, eax, 10
    add eax, edx
    mov \reg, eax
    .endm
    D2 rsi, r8d
    D2 rsi+2, r9d
    imul r8d, r8d, 100
    add r8d, r9d                 # year
    D2 rsi+5, r9d                # month
    D2 rsi+8, r10d               # day
    D2 rsi+11, r11d              # hour
    D2 rsi+14, ebx               # minute
    D2 rsi+17, ebp               # second
    xor r12d, r12d               # milliseconds of the fraction
    mov r13d, 19                 # the next byte
    cmp byte ptr [rsi + 19], '.'
    jne 3f
    mov r13d, 20
    mov r14d, 100                # the weight of the next digit
1:  cmp r13, rcx
    jae stamp_class
    movzx eax, byte ptr [rsi + r13]
    sub eax, '0'
    cmp eax, 9
    ja 2f
    imul eax, r14d
    add r12d, eax
    mov eax, r14d                # 100, 10, 1, then 0
    xor edx, edx
    mov r15d, 10
    div r15d
    mov r14d, eax
    inc r13
    jmp 1b
2:  lea rax, [r13 - 20]          # 1 to 9 digits
    test rax, rax
    jz stamp_class
    cmp rax, 9
    ja stamp_class
3:  xor r14d, r14d               # the offset in seconds
    mov rax, rcx
    sub rax, r13
    cmp rax, 1
    jne 4f
    cmp byte ptr [rsi + r13], 'Z'
    jne stamp_class
    jmp 6f
4:  cmp rax, 6
    jne stamp_class
    cmp byte ptr [rsi + r13 + 3], ':'
    jne stamp_class
    movzx r15d, byte ptr [rsi + r13]
    cmp r15d, '+'
    je 5f
    cmp r15d, '-'
    jne stamp_class
5:  D2 rsi+r13+1, r14d           # offset hour
    D2 rsi+r13+4, edx            # offset minute
    cmp r14d, 23
    ja stamp_bad
    cmp edx, 59
    ja stamp_bad
    imul r14d, r14d, 3600
    imul edx, edx, 60
    add r14d, edx
    cmp r15d, '-'
    jne 6f
    neg r14
6:  # Ranges.
    cmp r8d, 1700
    jb stamp_class
    cmp r8d, 2200
    ja stamp_class
    lea eax, [r9 - 1]
    cmp eax, 11
    ja stamp_bad
    test r10d, r10d
    jz stamp_bad
    lea rdx, [rip + mdays - 1]
    movzx eax, byte ptr [rdx + r9]
    cmp r9d, 2
    jne 7f
    test r8d, 3                  # a leap year: 29
    jnz 7f
    mov edx, r8d
    imul edx, edx, 0xc28f5c29    # divisible by 100 (rotated reciprocal)
    ror edx, 2
    cmp edx, 0x028f5c28
    ja 8f
    test r8d, 15                 # by 400 too (a multiple of 100 and of 16)
    jnz 7f
8:  inc eax
7:  cmp r10d, eax
    ja stamp_bad
    cmp r11d, 23
    ja stamp_bad
    cmp ebx, 59
    ja stamp_bad
    cmp ebp, 60
    ja stamp_bad
    je stamp_class               # a leap second
    mov rax, [rsi]               # the date is valid
    mov [rdi], rax
    movzx eax, word ptr [rsi + 8]
    mov [rdi + 8], ax
    # Days from the civil date (year > 0): March-based years.
    mov eax, r8d
    cmp r9d, 2
    ja 9f
    dec eax
    add r9d, 12
9:  sub r9d, 3                   # months since March
    imul r9d, r9d, 153
    add r9d, 2
    mov r15, rax                 # year
    mov eax, r9d
    xor edx, edx
    mov ecx, 5
    div ecx
    lea r9d, [eax + r10d - 1]    # day of the year
    mov eax, r15d
    xor edx, edx
    mov ecx, 400
    div ecx                      # eax era, edx year of era
    imul rax, rax, 146097
    mov ecx, edx
    imul edx, edx, 365
    add rax, rdx
    mov edx, ecx
    shr edx, 2
    add rax, rdx
    mov eax, eax                 # (under 2^32: era at most 5)
    mov edx, ecx                 # year of era / 100
    imul rdx, rdx, 1311
    shr rdx, 17
    sub rax, rdx
    add rax, r9
    sub rax, 719468              # days since 1970-01-01
    imul rax, rax, 86400
    imul r11d, r11d, 3600
    add rax, r11
    imul ebx, ebx, 60
    add rax, rbx
    add rax, rbp
    sub rax, r14
    imul rax, rax, 1000
    add rax, r12
    xor edx, edx
    jmp 1f
stamp_bad:
    mov edx, 1
    jmp 1f
stamp_class:
    mov edx, 2
1:  pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret
class:
    mov edi, 2
    mov eax, 231
    syscall

# The entry of key [rsi, rsi + rcx) in r8, made (count zero) when new.
# Entry: key | pointer (16), length | hash << 32, count, -, key pointer.
# Clobbers rax, rcx, rdx, r10, r11, xmm0, xmm1. Called from line only.
group:
    push rdi
    cmp rcx, 16
    ja 6f
    vmovd xmm1, ecx
    vpbroadcastb xmm1, xmm1
    vpcmpgtb xmm1, xmm1, xmm9
    vpand xmm0, xmm1, [rsi]
    mov r10d, ecx
    vmovq rax, xmm0
    vpextrq rcx, xmm0, 1
    mov edi, r10d
    crc32 rdi, rax
    crc32 rdi, rcx
    mov rdx, rdi
    shl rdx, 32
    or r10, rdx                  # length | hash << 32
1:  and rdi, rbp
    mov r8, rdi
    shl r8, 6
    add r8, r14
    cmp qword ptr [r8 + 40], 0
    je 3f
    cmp [r8 + 16], r10
    jne 2f
    cmp [r8], rax
    jne 2f
    cmp [r8 + 8], rcx
    je 9f
2:  inc rdi
    jmp 1b
3:  mov [r8], rax
    mov [r8 + 8], rcx
    mov [r8 + 16], r10
    mov [r8 + 40], r8            # the key is inline
    jmp 8f
6:  # Longer than 16 bytes.
    mov r10, rcx
    mov edi, ecx
    xor edx, edx
1:  lea rax, [rdx + 8]
    cmp rax, r10
    ja 2f
    crc32 rdi, qword ptr [rsi + rdx]
    mov rdx, rax
    jmp 1b
2:  mov rcx, r10
    sub rcx, rdx
    jz 3f
    shl ecx, 3
    mov rax, [rsi + rdx]
    bzhi rax, rax, rcx
    crc32 rdi, rax
3:  mov rdx, rdi
    shl rdx, 32
    or r10, rdx
4:  and rdi, rbp
    mov r8, rdi
    shl r8, 6
    add r8, r14
    cmp qword ptr [r8 + 40], 0
    je 7f
    cmp [r8 + 16], r10
    jne 5f
    push rsi
    push rdi
    mov rdi, [r8 + 40]
    mov ecx, r10d
    repe cmpsb
    pop rdi
    pop rsi
    je 9f
5:  inc rdi
    jmp 4b
7:  mov [r8 + 16], r10
    mov [r8 + 40], rsi
8:  mov qword ptr [r8 + 24], 0
    mov rax, [rsp + 32 + DISTINCT]
    inc rax
    mov [rsp + 32 + DISTINCT], rax
    add rax, rax
    cmp rax, rbp
    jbe 9f
    call grow
9:  pop rdi
    ret

# Double the entries; r8 (an entry) is followed to its new place.
grow:
    push rsi
    push rdi
    push r9
    push r8
    lea rsi, [rbp + 1]
    shl rsi, 7
    xor edi, edi
    mov edx, 3
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail
    lea rdx, [rbp + rbp + 1]
    mov rsi, r14
    lea rcx, [rbp + 1]
1:  cmp qword ptr [rsi + 40], 0
    je 4f
    mov rdi, [rsi + 16]
    shr rdi, 32
2:  and rdi, rdx
    mov r8, rdi
    shl r8, 6
    add r8, rax
    cmp qword ptr [r8 + 40], 0
    je 3f
    inc rdi
    jmp 2b
3:  vmovdqu64 zmm0, [rsi]
    vmovdqu64 [r8], zmm0
    cmp [rsi + 40], rsi
    jne 5f
    mov [r8 + 40], r8            # an inline key moves with its entry
5:  cmp [rsp], rsi
    jne 4f
    mov [rsp], r8
4:  add rsi, 64
    dec rcx
    jnz 1b
    mov r9, rax
    mov rdi, r14
    lea rsi, [rbp + 1]
    shl rsi, 6
    mov rbp, rdx
    mov eax, 11                  # munmap
    syscall
    mov r14, r9
    pop r8
    pop r9
    pop rdi
    pop rsi
    ret

# CF set when the key of entry r8 sorts before the key of entry rcx (bytes,
# then length). Clobbers rax, rcx, rsi, rdi, r8.
before:
    mov rsi, [r8 + 40]
    mov eax, [r8 + 16]
    mov rdi, [rcx + 40]
    mov r8d, [rcx + 16]
    mov rcx, rax
    cmp rcx, r8
    cmova rcx, r8
    test rcx, rcx
    jz 1f
    repe cmpsb
    jne 2f
1:  cmp rax, r8
    ret
2:  mov al, [rsi - 1]
    cmp al, [rdi - 1]
    ret

# Output through [r12, r12 + OUTSZ) to the descriptor r14, cursor rbx.
# putsnum writes rax in signed decimal, putnum unsigned, put rcx bytes from
# rsi. They clobber rax, rcx, rdx, rsi, rdi, r10 and r11.
putsnum:
    test rax, rax
    jns putnum
    push rax
    lea rsi, [rip + s_minus]
    mov ecx, 1
    call put
    pop rax
    neg rax
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
# write(r14, rsi, rdx) until done, EINTR retried, short writes advanced.
write_all:
    test rdx, rdx
    jz 2f
1:  mov edi, r14d
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

# advance: the chunk offset (rbx) reached [NEXTW], a window boundary of the
# file at r12 (r13 bytes): the window two back dropped (MADV_DONTNEED; a
# later read of it, a kept stamp or component, faults it back in from the
# page cache), the one after the next populated (MADV_POPULATE_READ),
# [NEXTW] a window on. Called from _start's frame; keeps every register.
advance:
    push rax
    push rcx
    push rdx
    push rsi
    push rdi
    push r11
    mov rax, [rsp + 56 + NEXTW]
    lea rdi, [r12 + rax - 2 * WINDOW]
    cmp rdi, r12
    jb 1f
    mov esi, WINDOW
    mov edx, 4                   # MADV_DONTNEED
    mov eax, 28                  # madvise
    syscall
1:  mov rax, [rsp + 56 + NEXTW]
    lea rdi, [r12 + rax + WINDOW]
    mov rsi, r13
    sub rsi, rax
    sub rsi, WINDOW
    jle 2f
    mov eax, WINDOW
    cmp rsi, rax
    cmova rsi, rax
    mov edx, 22                  # MADV_POPULATE_READ
    mov eax, 28
    syscall
2:  add qword ptr [rsp + 56 + NEXTW], WINDOW
    pop r11
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rax
    ret

fail:
    mov edi, 1
    mov eax, 231
    syscall

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
mdays:
    .byte 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31
s_debug:
    .ascii "DEBUG "
s_info:
    .ascii "\nINFO "
s_warn:
    .ascii "\nWARN "
s_error:
    .ascii "\nERROR "
s_top:
    .ascii "\ntop-error-components:\n"
s_span:
    .ascii "span "
s_dots:
    .ascii " .. "
s_open:
    .ascii " ("
s_close:
    .ascii "s)\n"
s_minus:
    .ascii "-"
s_sp:
    .ascii " "
s_nl:
    .ascii "\n"
e_ts:
    .ascii "Stop [E3001]: `panic: bad timestamp`\n"
    .ascii "  --> run.jet:9 in parse_millis\n"
    .ascii "    |\n"
    .ascii "9 |     (time.parse_rfc3339(stamp) ?? panic(\"bad timestamp\")).to_unix_ms()\n"
    .ascii "    |                                   ^^^^^\n"
e_bl:
    .ascii "Stop [E3001]: `panic: bad line`\n"
e_bl0:
    .ascii "  --> run.jet:25 in run\n"
    .ascii "     |\n"
    .ascii "25 |         first_fields: line.split_once(\" \") ?? panic(\"bad line\")\n"
    .ascii "     |                                               ^^^^^\n"
e_bl1:
    .ascii "  --> run.jet:27 in run\n"
    .ascii "     |\n"
    .ascii "27 |         second_fields: rest.split_once(\" \") ?? panic(\"bad line\")\n"
    .ascii "     |                                                ^^^^^\n"
e_bl2:
    .ascii "  --> run.jet:28 in run\n"
    .ascii "     |\n"
    .ascii "28 |         third_fields: second_fields.after.split_once(\" \") ?? panic(\"bad line\")\n"
    .ascii "     |                                                              ^^^^^\n"
e_locals:
    .ascii "locals: first_ms = "
e_last:
    .ascii ", last_ms = "
e_u8:
    .ascii "Stop [E3001]: `panic: invalid input during read `"
e_u8b:
    .ascii "`: stream did not contain valid UTF-8`\n"
    .ascii "  --> <core.prelude>:0\n"
e_tail:
    .ascii " Why: The program hit a `panic`, `assert`, or `assert_eq` call that failed, a bounds/key check triggered at runtime, or another program-side stop. The shared report boundary owns the program-side stop; an unhandled entry error is a returned report, not E3001. Jet file and line are shown in Jet terms (I2).\n"
    .ascii " Fix: Fix the logic that led to the failure. Program-side stops exit 70 through the shared boundary; unhandled entry errors print their report and exit 1; exit 101 is reserved for Jet defects.\n"
    .ascii "More: jet-lang.dev/e/E3001\n"
e_end:
