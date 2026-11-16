# datasummary (gauntlet numerics.script and numerics.notebook): read the CSV
# file argv[1] (header `group,value`), group the values by group, and print,
# per group in bytewise name order, "<group> n=<count> mean=<m>
# median=<lower middle> p95=<value at ceil(0.95 n) - 1> sd=<population sd>",
# each to two decimals, then "overall n=<count> mean=<m>".
#
# Technique:
# - the file streams through one 64 KiB buffer, a run of whole lines at a
#   time; each value is parsed straight to an integer count of thousandths
#   (the data's three decimals; a value with more fails), so that every sum
#   is exact;
# - a row finds its group by the last group hit, then a scan of the groups;
#   a new group's name is copied out of the buffer, and its values are
#   appended as int32s to its own run (a reserved mapping, touched only as
#   it fills): no row is kept beyond its value, and nothing is scattered;
# - transparent huge pages are off (prctl), so memory is the pages the
#   values fill, not 2 MiB at a time;
# - median and p95 are two quickselects (median-of-three Hoare partitions)
#   over the group's run, not a sort;
# - mean is the exact sum over the count, sd a second pass of squared
#   deviations; a median or p95 that sits exactly between two hundredths
#   rounds the way its double does (the sign of fma(x, 1000, -d)), as the
#   Jet program's Fixed(2) of that double does;
# - the output is built in one buffer and leaves in as few writes as fit.
# A file that cannot be read prints "read", a row that does not parse
# "csv", and a file with no rows "describe", each on stderr with exit 1, as
# the Jet program's panics do.
    .intel_syntax noprefix
    .text

    .set FD, 0
    .set NGROUPS, 8
    .set ORDER, 16
    .set OUT, 24
    .set TOTAL, 32
    .set NTOTAL, 40
    .set BUFP, 48
    .set EOF, 56
    .set NAMES, 64               # the names' start
    .set FRAME, 128

    # The area: the buffer, the names' cursor, the groups, the names.
    .set BUFSZ, 65536
    .set GROUPS, BUFSZ + 4096
    .set NAMEAREA, GROUPS + (1 << 26)
    .set AREASZ, NAMEAREA + (1 << 28)
    .set RUNSZ, 1 << 30          # a group's run: at most 2^28 values

    # A group: its name, row count, value sum, and its run of values.
    .set G_PTR, 0
    .set G_LEN, 8
    .set G_COUNT, 16
    .set G_SUM, 24
    .set G_START, 32
    .set GSZ, 40

_start:
    mov rdi, [rsp + 16]          # argv[1]
    sub rsp, FRAME
    test rdi, rdi
    jz fail_read
    xor esi, esi                 # O_RDONLY
    mov eax, 2                   # open
    syscall
    test rax, rax
    js fail_read
    mov [rsp + FD], rax
    mov edi, 41                  # prctl(PR_SET_THP_DISABLE, 1)
    mov esi, 1
    xor edx, edx
    xor r10d, r10d
    xor r8d, r8d
    mov eax, 157
    syscall
    xor edi, edi
    mov rsi, AREASZ
    mov edx, 3                   # PROT_READ | PROT_WRITE
    mov r10d, 0x4022             # MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE
    mov r8, -1
    xor r9d, r9d
    mov eax, 9                   # mmap
    syscall
    cmp rax, -4096
    ja fail_read
    mov [rsp + BUFP], rax
    lea r15, [rax + GROUPS]
    lea rcx, [rax + NAMEAREA]
    mov [rsp + NAMES], rcx
    mov [r15 - 8], rcx           # the names' cursor
    mov qword ptr [rsp + EOF], 0
    xor ebx, ebx                 # groups
    xor ebp, ebp                 # the last group hit
    mov r12, rax                 # the cursor, the lines' end, the data's end
    mov r13, rax
    mov r14, rax
    call refill

    # The header.
    mov rax, r13
    sub rax, r12
    cmp rax, 11
    jb fail_csv
    mov rsi, r12
    lea rdi, [rip + header]
    mov ecx, 11
    repe cmpsb
    jne fail_csv
    add r12, 11
    cmp r12, r13
    jae row
    cmp byte ptr [r12], 13
    jne 1f
    inc r12
1:  cmp r12, r13
    jae row
    cmp byte ptr [r12], 10
    jne fail_csv
    inc r12

row:
    cmp r12, r13
    jb 1f
    cmp qword ptr [rsp + EOF], 0
    jne parsed
    call refill
    jmp row
1:  cmp byte ptr [r12], 10       # a blank line
    jne 1f
    inc r12
    jmp row
1:  mov rsi, r12                 # the group
2:  cmp r12, r13
    jae fail_csv
    movzx eax, byte ptr [r12]
    cmp eax, ','
    je 3f
    cmp eax, 10
    je fail_csv
    inc r12
    jmp 2b
3:  mov rcx, r12
    sub rcx, rsi
    inc r12
    call find
    mov r9, rax
    # The value: [-]digits[.digits], in thousandths.
    xor r10d, r10d
    cmp r12, r13
    jae fail_csv
    cmp byte ptr [r12], '-'
    jne 4f
    mov r10d, 1
    inc r12
4:  xor eax, eax
    xor r11d, r11d               # digits
    mov r8d, -1                  # decimals; -1 before the point
5:  cmp r12, r13
    jae 8f
    movzx ecx, byte ptr [r12]
    cmp ecx, '.'
    jne 6f
    test r8d, r8d
    jns fail_csv
    xor r8d, r8d
    inc r12
    jmp 5b
6:  sub ecx, '0'
    cmp ecx, 9
    ja 8f
    test r8d, r8d
    js 7f
    cmp r8d, 3
    jae fail_csv
    inc r8d
7:  imul rax, rax, 10
    add rax, rcx
    cmp rax, 0x7fffffff
    ja fail_csv
    inc r11d
    inc r12
    jmp 5b
8:  test r11d, r11d
    jz fail_csv
    test r8d, r8d
    jns 9f
    xor r8d, r8d
9:  cmp r8d, 3
    jae 10f
    imul rax, rax, 10
    inc r8d
    jmp 9b
10: cmp rax, 0x7fffffff
    ja fail_csv
    test r10d, r10d
    jz 11f
    neg rax
11: cmp r12, r13                 # the line's end
    jae 13f
    cmp byte ptr [r12], 13
    jne 12f
    inc r12
    cmp r12, r13
    jae 13f
12: cmp byte ptr [r12], 10
    jne fail_csv
    inc r12
13: mov rdx, [r9 + G_COUNT]
    mov rcx, [r9 + G_START]
    mov [rcx + rdx * 4], eax
    inc rdx
    mov [r9 + G_COUNT], rdx
    add [r9 + G_SUM], rax
    cmp rdx, RUNSZ / 4
    jae fail_csv
    jmp row

# refill: the unparsed bytes [r12, r14) moved to the buffer's start and
# more read after them, until the data holds a newline (r13 just past the
# last one) or the file ends (r13 = r14, EOF set). A line longer than the
# buffer fails as "csv".
refill:
    mov rsi, r12
    mov rdi, [rsp + 8 + BUFP]
    mov rcx, r14
    sub rcx, r12
    mov r12, rdi
    rep movsb
    mov r14, rdi
1:  mov rdx, [rsp + 8 + BUFP]
    add rdx, BUFSZ
    mov rsi, r14
    sub rdx, r14
    jz fail_csv
    mov rdi, [rsp + 8 + FD]
    xor eax, eax                 # read
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    js fail_read
    jz 3f
    mov rdi, r14                 # the new bytes' first, for the scan's stop
    add r14, rax
    mov rcx, r14
2:  dec rcx                      # the last newline in them
    cmp byte ptr [rcx], 10
    je 4f
    cmp rcx, rdi
    ja 2b
    jmp 1b
3:  mov qword ptr [rsp + 8 + EOF], 1
    mov r13, r14
    ret
4:  lea r13, [rcx + 1]
    ret

parsed:
    test rbx, rbx
    jz fail_describe
    mov [rsp + NGROUPS], rbx
    # The totals.
    xor eax, eax
    xor edx, edx
    mov rdi, r15
    mov rcx, rbx
1:  add rax, [rdi + G_COUNT]
    add rdx, [rdi + G_SUM]
    add rdi, GSZ
    dec rcx
    jnz 1b
    mov [rsp + NTOTAL], rax
    mov [rsp + TOTAL], rdx
    # The name order (one pointer per group) and the output buffer.
    lea rsi, [rbx * 8 + 512]
    imul rax, rbx, 160
    add rsi, rax
    add rsi, [r15 - 8]
    sub rsi, [rsp + NAMES]
    xor edi, edi
    mov edx, 3
    mov r10d, 0x22               # MAP_PRIVATE | MAP_ANONYMOUS
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail_read
    mov [rsp + ORDER], rax
    lea rdi, [rax + rbx * 8]
    mov [rsp + OUT], rdi
    # Insertion sort of the groups by name.
    mov rdi, [rsp + ORDER]
    xor ecx, ecx
4:  imul rax, rcx, GSZ
    add rax, r15
    mov [rdi + rcx * 8], rax
    inc rcx
    cmp rcx, rbx
    jb 4b
    mov r12d, 1
5:  cmp r12, rbx
    jae 8f
    mov r8, [rdi + r12 * 8]      # the key
    mov r13, r12
6:  test r13, r13
    jz 7f
    mov r9, [rdi + r13 * 8 - 8]
    call namelt
    test eax, eax
    jz 7f
    mov [rdi + r13 * 8], r9
    dec r13
    jmp 6b
7:  mov [rdi + r13 * 8], r8
    inc r12
    jmp 5b
8:  # One line per group.
    mov rdi, [rsp + OUT]
    xor r12d, r12d
group:
    cmp r12, [rsp + NGROUPS]
    jae overall
    mov rax, [rsp + ORDER]
    mov r13, [rax + r12 * 8]
    mov rsi, [r13 + G_PTR]
    mov rcx, [r13 + G_LEN]
    rep movsb
    lea rsi, [rip + s_n]
    mov ecx, 3
    rep movsb
    mov rax, [r13 + G_COUNT]
    call put_uint
    lea rsi, [rip + s_mean]
    mov ecx, 6
    rep movsb
    mov rax, [r13 + G_SUM]
    mov rcx, [r13 + G_COUNT]
    call put_mean
    # median, then p95, over the group's run
    lea rsi, [rip + s_median]
    mov ecx, 8
    rep movsb
    mov r14, [r13 + G_START]
    mov r15, [r13 + G_COUNT]
    lea rdx, [r15 - 1]
    shr rdx, 1
    call select
    call put_thousandths
    lea rsi, [rip + s_p95]
    mov ecx, 5
    rep movsb
    imul rdx, r15, 19
    add rdx, 19
    mov rax, rdx
    xor edx, edx
    mov ecx, 20
    div rcx
    lea rdx, [rax - 1]
    call select
    call put_thousandths
    lea rsi, [rip + s_sd]
    mov ecx, 4
    rep movsb
    # sd: sqrt(sum((v - mean)^2) / n), in thousandths; printed in hundredths.
    cvtsi2sd xmm1, qword ptr [r13 + G_SUM]
    cvtsi2sd xmm2, r15
    divsd xmm1, xmm2             # the mean
    xorpd xmm0, xmm0
    xor ecx, ecx
9:  cvtsi2sd xmm3, dword ptr [r14 + rcx * 4]
    subsd xmm3, xmm1
    mulsd xmm3, xmm3
    addsd xmm0, xmm3
    inc rcx
    cmp rcx, r15
    jb 9b
    divsd xmm0, xmm2
    sqrtsd xmm0, xmm0
    mov rax, 0x4024000000000000  # 10.0
    movq xmm1, rax
    divsd xmm0, xmm1
    cvtsd2si rax, xmm0
    xor r10d, r10d
    call put_fixed2
    mov byte ptr [rdi], 10
    inc rdi
    inc r12
    jmp group

overall:
    lea rsi, [rip + s_overall]
    mov ecx, 10
    rep movsb
    mov rax, [rsp + NTOTAL]
    call put_uint
    lea rsi, [rip + s_mean]
    mov ecx, 6
    rep movsb
    mov rax, [rsp + TOTAL]
    mov rcx, [rsp + NTOTAL]
    call put_mean
    mov byte ptr [rdi], 10
    inc rdi
    mov rsi, [rsp + OUT]
    mov rdx, rdi
    sub rdx, rsi
1:  mov edi, 1
    mov eax, 1                   # write
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    js fail_read
    add rsi, rax
    sub rdx, rax
    jnz 1b
    xor edi, edi
    mov eax, 231                 # exit_group
    syscall

# find: the group named (rsi, rcx), added when new, in rax; the last hit
# (rbp) is tried first. Clobbers rdx, rdi, r8 through r11.
find:
    mov rax, rbp
    test rax, rax
    jz 1f
    call same
    je 4f
1:  mov rax, r15
    mov rdx, rbx
2:  test rdx, rdx
    jz 3f
    call same
    je 4f
    add rax, GSZ
    dec rdx
    jmp 2b
3:  push rsi                     # new: its name copied, its run reserved
    push rcx
    mov rdi, [r15 - 8]
    mov [rax + G_PTR], rdi
    mov [rax + G_LEN], rcx
    rep movsb
    mov [r15 - 8], rdi
    push rax
    xor edi, edi
    mov esi, RUNSZ
    mov edx, 3
    mov r10d, 0x4022             # MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE
    mov r8, -1
    xor r9d, r9d
    mov eax, 9                   # mmap
    syscall
    cmp rax, -4096
    ja fail_read
    mov rdx, rax
    pop rax
    mov [rax + G_START], rdx
    pop rcx
    pop rsi
    inc rbx
4:  mov rbp, rax
    ret

# same: ZF set when the group at rax is named (rsi, rcx). Clobbers rdi.
same:
    cmp [rax + G_LEN], rcx
    jne 1f
    push rsi
    push rcx
    mov rdi, [rax + G_PTR]
    repe cmpsb
    pop rcx
    pop rsi
1:  ret

# namelt: 1 in eax when the group at r8 sorts before the one at r9.
# Clobbers rdx.
namelt:
    push rsi
    push rdi
    push rcx
    mov rsi, [r8 + G_PTR]
    mov rdi, [r9 + G_PTR]
    mov rcx, [r8 + G_LEN]
    mov rdx, [r9 + G_LEN]
    cmp rdx, rcx
    cmovb rcx, rdx
    xor eax, eax
    test rcx, rcx
    jz 1f
    repe cmpsb
    je 1f
    setb al
    jmp 2f
1:  mov rcx, [r8 + G_LEN]
    cmp rcx, [r9 + G_LEN]
    setb al
2:  pop rcx
    pop rdi
    pop rsi
    ret

# select: the k-th smallest (rdx = k) of the r15 int32s at r14, in eax;
# reorders them. Clobbers rcx, rsi, r8 through r11.
select:
    xor r8d, r8d                 # lo
    lea r9, [r15 - 1]            # hi
1:  cmp r8, r9
    jge 9f
    lea rax, [r8 + r9]
    shr rax, 1
    mov ecx, [r14 + r8 * 4]
    mov r10d, [r14 + rax * 4]
    mov r11d, [r14 + r9 * 4]
    mov eax, ecx                 # the pivot: the median of the three
    cmp eax, r10d
    cmovg eax, r10d
    cmp ecx, r10d
    cmovl ecx, r10d
    cmp ecx, r11d
    cmovg ecx, r11d
    cmp ecx, eax
    cmovl ecx, eax
    mov r10, r8                  # i
    mov r11, r9                  # j
2:  cmp [r14 + r10 * 4], ecx
    jge 3f
    inc r10
    jmp 2b
3:  cmp [r14 + r11 * 4], ecx
    jle 4f
    dec r11
    jmp 3b
4:  cmp r10, r11
    jg 5f
    mov eax, [r14 + r10 * 4]
    mov esi, [r14 + r11 * 4]
    mov [r14 + r10 * 4], esi
    mov [r14 + r11 * 4], eax
    inc r10
    dec r11
    cmp r10, r11
    jle 2b
5:  cmp rdx, r11
    jle 6f
    cmp rdx, r10
    jl 9f
    mov r8, r10
    jmp 1b
6:  mov r9, r11
    jmp 1b
9:  mov eax, [r14 + rdx * 4]
    ret

# put_thousandths: the int32 eax of thousandths to two decimals. A value
# exactly between two hundredths rounds as its nearest double does: up when
# that double is above it, down when below, to even when it is exact.
put_thousandths:
    movsxd rax, eax
    xor r10d, r10d
    test rax, rax
    jns 1f
    neg rax
    mov r10d, 1
1:  mov r8, rax
    xor edx, edx
    mov ecx, 10
    div rcx
    cmp edx, 5
    jb put_fixed2
    ja 3f
    cvtsi2sd xmm0, r8
    mov rcx, 0x408f400000000000  # 1000.0
    movq xmm1, rcx
    divsd xmm0, xmm1             # x, the double nearest d / 1000
    mov rcx, r8
    neg rcx
    cvtsi2sd xmm2, rcx
    vfmadd231sd xmm2, xmm0, xmm1 # x * 1000 - d, exactly signed
    xorpd xmm3, xmm3
    ucomisd xmm2, xmm3
    ja 3f
    jb put_fixed2
    test al, 1
    jz put_fixed2
3:  inc rax
    jmp put_fixed2

# put_mean: the sum rax of thousandths over the count rcx, to two decimals.
put_mean:
    xor r10d, r10d
    test rax, rax
    jns 1f
    neg rax
    mov r10d, 1
1:  cvtsi2sd xmm0, rax
    imul rcx, rcx, 10
    cvtsi2sd xmm1, rcx
    divsd xmm0, xmm1
    cvtsd2si rax, xmm0           # round to nearest, ties to even
    # fall through

# put_fixed2: rax hundredths, with a minus sign when r10d is 1, as
# "<whole>.<two digits>". Clobbers rax, rcx, rdx, r8, r9.
put_fixed2:
    test r10d, r10d
    jz 1f
    mov byte ptr [rdi], '-'
    inc rdi
1:  xor edx, edx
    mov ecx, 100
    div rcx
    push rdx
    call put_uint
    pop rax
    mov byte ptr [rdi], '.'
    xor edx, edx
    mov ecx, 10
    div rcx
    add al, '0'
    add dl, '0'
    mov [rdi + 1], al
    mov [rdi + 2], dl
    add rdi, 3
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

fail_read:
    lea rsi, [rip + s_read]
    mov edx, 5
    jmp fail
fail_csv:
    lea rsi, [rip + s_csv]
    mov edx, 4
    jmp fail
fail_describe:
    lea rsi, [rip + s_describe]
    mov edx, 9
fail:
    mov edi, 2
    mov eax, 1                   # write
    syscall
    mov edi, 1
    mov eax, 231
    syscall

header:    .ascii "group,value"
s_n:       .ascii " n="
s_mean:    .ascii " mean="
s_median:  .ascii " median="
s_p95:     .ascii " p95="
s_sd:      .ascii " sd="
s_overall: .ascii "overall n="
s_read:    .ascii "read\n"
s_csv:     .ascii "csv\n"
s_describe: .ascii "describe\n"
