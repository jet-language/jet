# csvtransform (gauntlet formats.csv-cli): read the CSV file argv[1] by the
# rules of Jet's typed CSV decoder (core.encoding.csv: RFC 4180 quoting,
# LF or CRLF record ends, a bare CR, a quote inside an unquoted field or a
# byte other than quote, comma or line end after a closing quote is an
# error, a missing cell is empty, a blank line is a record of one empty
# cell), take the columns the header names `region` and `amount` (the
# first of each), keep the rows whose amount is above zero, and print per
# region in ascending byte order "<region> n=<count> sum=<sum, 2 places>",
# then "total n=<count> sum=<sum>".
#
# Input class: an amount is a Decimal spelling (ASCII-whitespace trimmed,
# optional sign, digits with at most one point) of at most two decimal
# places, the class the gauntlet's fixture writes, so every sum is an exact
# count of cents below 2^63 and the total prints with two places, as the
# Jet program's Decimal total (scale max(2, ...)) does. The Jet program sums
# a region in Float and prints it to two places, which is the exact cents
# while the Float error stays under half a cent (the fixture's: about
# 1e-5). An amount with an exponent or more places, or 2^63 cents or more
# in an amount or a sum, is outside the class: exit 2.
#
# Technique:
# - the file is mapped into a reservation one page longer, so that vector
#   loads past its end read zeros; it is resident a window at a time (the
#   next two populated ahead, the one before the last dropped behind), so
#   memory stays three windows whatever the file's size;
# - AVX-512BW classifies 64 bytes at a time straight into quote, comma, LF
#   and CR masks; which bytes are inside quotes is the prefix xor of the
#   quote mask (one carry-less multiply by all ones) with the carry of the
#   chunk before, and the structural separators are the commas and LFs
#   outside quotes;
# - the decoder's errors are mask identities over the chunk: an opening
#   quote follows a separator, a closing quote or the file start; a closing
#   quote is followed by a separator, a CR, an opening quote (an escaped
#   quote) or the file end; a CR outside quotes is followed by an LF; the
#   file does not end inside quotes;
# - the separators are walked with tzcnt/blsr: only the region and amount
#   cells are looked at, and a row is decided at its LF;
# - the amount parses to cents in one scalar pass; a region of up to 16
#   bytes is its zero-padded 16 bytes (one masked load), hashed by two
#   crc32 seeded with its length and compared whole; a longer or quoted one
#   hashes all its words (a quoted one unescaped first);
# - the groups are open addressing of 64-byte entries, doubled at half
#   full, then a bottom-up merge sort of their pointers by key;
# - the file must be UTF-8, as the Jet program reads it as a String: the
#   classification ors every byte into one register and a scalar UTF-8
#   check runs only when one has its high bit set;
# - output goes out through one buffer.
# Stops are the Jet program's, with its status and its stderr report byte
# for byte: a file that cannot be opened, read or mapped or is not UTF-8
# panics `read failed` (status 70); one that is UTF-8 but not CSV by those
# rules, lacks either column while it has a data row, or has an amount
# that is not a Decimal panics `csv failed` (70). The file is read whole
# before it is decoded, so a file that is not UTF-8 stops `read failed`
# wherever that is, and so does one outside the input class (exit 2
# otherwise). Exit 1 when argv[1] is missing or memory or stdout fails.
    .intel_syntax noprefix
    .text

    .set STAT, 0                 # struct stat (144 bytes)
    .set INQ, 144                # all ones while inside quotes
    .set CLOSEC, 152             # carry: a closing quote ended the chunk
    .set SEPC, 160               # carry: a separator or closing quote ended it
    .set CRC, 168                # carry: a CR outside quotes ended it
    .set RI, 176                 # region column (-1 until the header names it)
    .set AI, 184                 # amount column
    .set RS, 192                 # this row's region cell [RS, RE)
    .set RE, 200
    .set AS, 208                 # this row's amount cell [AS, AE)
    .set AE, 216
    .set HDR, 224                # 1 while in the header row
    .set COUNT, 232              # rows kept
    .set TOTAL, 240              # their cents
    .set DISTINCT, 248
    .set ARENA, 256              # unescaped quoted regions: next free byte
    .set LASTLF, 264             # 1 when the last separator was an LF
    .set OUT, 320
    .set OUTSZ, 4096
    .set FRAME, OUT + OUTSZ
    .set NEXTW, 304              # the chunk offset of the next window
    .set WINDOW, 1 << 17         # the file's resident window
    .set INITCAP, 64             # entries, 64 bytes each

_start:
    mov rdi, [rsp + 16]          # argv[1]
    sub rsp, FRAME
    test rdi, rdi
    jz fail
    xor esi, esi                 # O_RDONLY
    mov eax, 2                   # open
    syscall
    test rax, rax
    js readfail
    mov rbx, rax                 # fd
    mov edi, eax
    lea rsi, [rsp + STAT]
    mov eax, 5                   # fstat
    syscall
    test rax, rax
    js readfail
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
    ja readfail
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
    ja readfail
1:  # The arena for unescaped quoted regions: no larger than the file.
    xor edi, edi
    lea rsi, [r13 + 4096]
    mov edx, 3
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail
    mov [rsp + ARENA], rax
    # The groups.
    xor edi, edi
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
    mov [rsp + INQ], rax
    mov [rsp + CLOSEC], rax
    mov [rsp + CRC], rax
    mov [rsp + COUNT], rax
    mov [rsp + TOTAL], rax
    mov [rsp + DISTINCT], rax
    mov [rsp + LASTLF], rax
    mov qword ptr [rsp + SEPC], 1    # the file start opens a field
    mov qword ptr [rsp + RI], -1
    mov qword ptr [rsp + AI], -1
    mov qword ptr [rsp + HDR], 1
    lea r15, [r12 + r13]

    mov eax, 0x22222222
    vpbroadcastd zmm20, eax      # '"'
    mov eax, 0x2c2c2c2c
    vpbroadcastd zmm21, eax      # ','
    mov eax, 0x0a0a0a0a
    vpbroadcastd zmm22, eax      # '\n'
    mov eax, 0x0d0d0d0d
    vpbroadcastd zmm23, eax      # '\r'
    vpcmpeqd xmm7, xmm7, xmm7    # all ones
    vmovdqu xmm9, [rip + iota]
    vpxord zmm25, zmm25, zmm25   # every byte or-ed

    xor ebx, ebx                 # chunk offset
    mov rsi, r12                 # cell start
    xor edi, edi                 # cell index in the row
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
    vpcmpeqb k3, zmm0, zmm22
    vpcmpeqb k4, zmm0, zmm23
    kmovq rax, k1                # quotes
    kmovq rcx, k2                # commas
    kmovq rdx, k3                # LFs
    kmovq r8, k4                 # CRs
    vmovq xmm1, rax
    vpclmulqdq xmm1, xmm1, xmm7, 0
    vmovq r9, xmm1
    xor r9, [rsp + INQ]          # inside quotes (an opening quote included)
    mov r10, r9
    sar r10, 63
    mov [rsp + INQ], r10
    andn rcx, r9, rcx            # structural commas
    andn rdx, r9, rdx            # structural LFs
    andn r8, r9, r8              # structural CRs
    mov r10, rax
    and r10, r9                  # opening quotes
    andn rax, r9, rax            # closing quotes
    # A closing quote is followed by a comma, CR, LF, opening quote or the end.
    lea r11, [rax + rax]
    or r11, [rsp + CLOSEC]
    mov r9, rax
    shr r9, 63
    mov [rsp + CLOSEC], r9
    mov r9, rcx
    or r9, rdx
    or r9, r8
    or r9, r10
    andn r11, r9, r11
    jz 1f
    mov r9, r13                  # past the end is the end
    sub r9, rbx
    cmp r9, 64
    jae bad
    bzhi r11, r11, r9
    jnz bad
1:  # An opening quote follows a comma, LF, closing quote or the file start.
    mov r9, rcx
    or r9, rdx
    or r9, rax
    lea r11, [r9 + r9]
    or r11, [rsp + SEPC]
    shr r9, 63
    mov [rsp + SEPC], r9
    andn r11, r11, r10
    jnz bad
    # A structural CR is followed by an LF.
    lea r11, [r8 + r8]
    or r11, [rsp + CRC]
    shr r8, 63
    mov [rsp + CRC], r8
    andn r11, rdx, r11
    jnz bad
    or rcx, rdx
    mov r10, rcx                 # separators
    mov r11, rdx                 # of them, the LFs

sep:
    test r10, r10
    jz 9f
    tzcnt rcx, r10
    blsr r10, r10
    lea rdx, [r12 + rbx]
    add rdx, rcx                 # the separator
    mov rax, rdx                 # the cell's end
    bt r11, rcx
    setc cl
    movzx ecx, cl
    mov [rsp + LASTLF], rcx
    jnc 1f
    cmp rax, rsi
    je 1f
    cmp byte ptr [rax - 1], 13
    jne 1f
    dec rax                      # CRLF
1:  cmp qword ptr [rsp + HDR], 0
    jne hdrcell
    cmp rdi, [rsp + RI]
    jne 2f
    mov [rsp + RS], rsi
    mov [rsp + RE], rax
2:  cmp rdi, [rsp + AI]
    jne 3f
    mov [rsp + AS], rsi
    mov [rsp + AE], rax
3:  test ecx, ecx
    jnz rowend
    inc rdi
    lea rsi, [rdx + 1]
    jmp sep
9:  add rbx, 64
    jmp chunk

# The file's end: the last record when it has no LF.
eof:
    cmp qword ptr [rsp + INQ], 0
    jne bad                      # inside quotes
    cmp qword ptr [rsp + CRC], 0
    jne bad                      # a bare CR at the end
    cmp rsi, r15                 # bytes or cells after the last LF
    jb 2f
    test rdi, rdi
    jz 1f
2:  mov qword ptr [rsp + LASTLF], 1
    mov rdx, r15
    mov rax, r15
    xor r10d, r10d               # no separators left
    mov rbx, r13                 # and no chunks
    jmp hdrtest
1:  vpmovb2m k1, zmm25
    kortestq k1, k1
    jz report
    mov rsi, r12
    call utf8
    jc readfail
    jmp report
hdrtest:
    cmp qword ptr [rsp + HDR], 0
    jne hdrcell
    cmp rdi, [rsp + RI]
    jne 2f
    mov [rsp + RS], rsi
    mov [rsp + RE], rax
2:  cmp rdi, [rsp + AI]
    jne rowend
    mov [rsp + AS], rsi
    mov [rsp + AE], rax
    jmp rowend

# A header cell [rsi, rax): its column named region or amount (the first).
hdrcell:
    push rdx
    push r10
    push r11
    call cell                    # rsi, rcx = its text
    jc bad
    cmp rcx, 6
    jne 4f
    mov eax, [rsi]
    movzx r8d, word ptr [rsi + 4]
    cmp eax, 0x69676572          # "regi"
    jne 1f
    cmp r8d, 0x6e6f              # "on"
    jne 4f
    cmp qword ptr [rsp + 24 + RI], -1
    jne 4f
    mov [rsp + 24 + RI], rdi
    jmp 4f
1:  cmp eax, 0x756f6d61          # "amou"
    jne 4f
    cmp r8d, 0x746e              # "nt"
    jne 4f
    cmp qword ptr [rsp + 24 + AI], -1
    jne 4f
    mov [rsp + 24 + AI], rdi
4:  pop r11
    pop r10
    pop rdx
    cmp qword ptr [rsp + LASTLF], 0
    je 5f
    mov qword ptr [rsp + HDR], 0
    xor edi, edi
    lea rsi, [rdx + 1]
    jmp sep
5:  inc rdi
    lea rsi, [rdx + 1]
    jmp sep

# A data row ended at the separator rdx; rdi is its last cell's index.
rowend:
    push rdx
    push r10
    push r11
    mov rax, [rsp + 24 + AI]
    cmp rax, -1
    je bad
    cmp qword ptr [rsp + 24 + RI], -1
    je bad
    cmp rdi, rax
    jb bad                       # no amount cell: "" is not a Decimal
    mov rsi, [rsp + 24 + AS]
    mov rax, [rsp + 24 + AE]
    call cell
    jc bad
    call cents                   # rax
    test rax, rax
    jle 8f
    add [rsp + 24 + TOTAL], rax
    jo class
    inc qword ptr [rsp + 24 + COUNT]
    mov r9, rax                  # cents
    xor ecx, ecx                 # region "" when the row has no such cell
    mov rsi, r12
    cmp rdi, [rsp + 24 + RI]
    jb 1f
    mov rsi, [rsp + 24 + RS]
    mov rax, [rsp + 24 + RE]
    call cell
    jc bad
1:  call group                   # r8 = the entry of key [rsi, rsi + rcx)
    inc qword ptr [r8 + 24]
    add [r8 + 32], r9
    jo class
8:  pop r11
    pop r10
    pop rdx
    xor edi, edi
    lea rsi, [rdx + 1]
    jmp sep

# The text of the cell [rsi, rax): rsi, rcx = its bytes (a quoted cell's
# unescaped into the arena, which is not advanced). CF set when it is not a
# cell (only reachable for a quote, which the masks already checked).
# Clobbers rax, rcx, rdx, r8.
cell:
    mov rcx, rax
    sub rcx, rsi
    jz 9f
    cmp byte ptr [rsi], 34
    je 1f
9:  clc
    ret
1:  inc rsi                      # [rsi, rax - 1) less the doubled quotes
    dec rax
    mov rdx, [rsp + 32 + ARENA]
    mov r8, rdx
2:  cmp rsi, rax
    jae 4f
    mov cl, [rsi]
    mov [rdx], cl
    inc rdx
    inc rsi
    cmp cl, 34
    jne 2b
    inc rsi                      # the second of the pair
    jmp 2b
4:  mov rsi, r8
    mov rcx, rdx
    sub rcx, r8
    clc
    ret

# The amount text [rsi, rsi + rcx) to cents in rax; exit 1 when it is not
# a Decimal, 2 when it is one outside the class (an exponent, more than two
# places, 2^63 cents or more).
# Clobbers rcx, rdx, rsi, r8, r9, r10, r11.
cents:
    lea rdx, [rsi + rcx]
1:  cmp rsi, rdx                 # trim
    jae bad
    movzx eax, byte ptr [rsi]
    cmp al, ' '
    je 2f
    sub al, 9
    cmp al, 4
    ja 3f
2:  inc rsi
    jmp 1b
3:  movzx eax, byte ptr [rdx - 1]
    cmp al, ' '
    je 4f
    sub al, 9
    cmp al, 4
    ja 5f
4:  dec rdx
    jmp 3b
5:  xor r8d, r8d                 # 1 when negative
    movzx eax, byte ptr [rsi]
    cmp al, '-'
    sete r8b
    je 6f
    cmp al, '+'
    jne 7f
6:  inc rsi
7:  xor eax, eax                 # whole part
    xor r9d, r9d                 # 1 when outside the class
    xor r10d, r10d               # digits seen
    mov r11, -1                  # places, -1 without a point
10: cmp rsi, rdx
    jae 20f
    movzx ecx, byte ptr [rsi]
    inc rsi
    cmp cl, '.'
    je 15f
    sub ecx, '0'
    cmp ecx, 9
    ja 16f
    inc r10
    test r11, r11
    js 12f
    inc r11
    cmp r11, 2
    jbe 12f
    mov r9d, 1
    jmp 10b
12: imul rax, rax, 10
    jo 13f
    add rax, rcx
    jno 10b
13: mov r9d, 1                  # 2^63 cents or more
    jmp 10b
15: test r11, r11
    jns bad                      # a second point
    xor r11d, r11d
    jmp 10b
16: add ecx, '0'
    or cl, 0x20
    cmp cl, 'e'
    jne bad
    test r10, r10
    jz bad
    cmp rsi, rdx                 # [+-]digits to the end
    jae bad
    movzx ecx, byte ptr [rsi]
    cmp cl, '+'
    je 17f
    cmp cl, '-'
    jne 18f
17: inc rsi
    cmp rsi, rdx
    jae bad
18: movzx ecx, byte ptr [rsi]
    sub ecx, '0'
    cmp ecx, 9
    ja bad
    inc rsi
    cmp rsi, rdx
    jb 18b
    jmp class
20: test r10, r10
    jz bad
    test r9d, r9d
    jnz class
    test r11, r11
    jg 21f
    imul rax, rax, 100           # no places
    jo class
    jmp 23f
21: cmp r11, 1
    jne 23f
    imul rax, rax, 10
    jo class
23: test r8d, r8d
    jz 24f
    neg rax
24: ret
# The Jet program's stops (the frame need not be at rsp): an input outside
# the class exits 2, but a file that is not UTF-8 stops first, as does
# one that is not CSV by its rules.
class:
    mov rsi, r12
    call utf8
    jc readfail
    mov edi, 2
    mov eax, 231
    syscall
bad:
    mov rsi, r12
    call utf8
    jc readfail
    lea rsi, [rip + e_csv]
    mov edx, e_read - e_csv
    jmp 1f
readfail:
    lea rsi, [rip + e_read]
    mov edx, e_tail - e_read
1:  call ewrite
    lea rsi, [rip + e_tail]
    mov edx, e_end - e_tail
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

# The group of key [rsi, rsi + rcx) in r8, made (count and cents zero) when
# new. Entry: key | pointer (16), length | hash << 32, count, cents, key
# pointer. Clobbers rax, rcx, rdx, r10, r11, xmm0, xmm1.
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
    mov rax, [rsp + 40 + ARENA]
    cmp rsi, rax
    jne 1f
    mov ecx, r10d                # an unescaped key: keep it in the arena
    add rax, rcx
    mov [rsp + 40 + ARENA], rax
1:  mov [r8 + 40], rsi
8:  mov qword ptr [r8 + 24], 0
    mov qword ptr [r8 + 32], 0
    mov rax, [rsp + 40 + DISTINCT]
    inc rax
    mov [rsp + 40 + DISTINCT], rax
    add rax, rax
    cmp rax, rbp
    jbe 9f
    call grow
9:  pop rdi
    ret

# Double the groups; r8 (an entry) is followed to its new place.
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

# The groups' pointers, merge-sorted by key, then the report.
report:
    mov r13, [rsp + DISTINCT]
    lea rsi, [r13 * 8 + 4096]
    shl rsi, 1
    xor edi, edi
    mov edx, 3
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail
    mov r15, rax                 # the pointers
    lea rbx, [rax + r13 * 8]     # and the merge's other half
    mov rdi, r15
    mov rsi, r14
    lea rcx, [rbp + 1]
1:  cmp qword ptr [rsi + 40], 0
    je 2f
    mov [rdi], rsi
    add rdi, 8
2:  add rsi, 64
    dec rcx
    jnz 1b
    mov r12d, 1                  # run width
3:  cmp r12, r13
    jae 9f
    xor r11d, r11d               # run pair start
4:  cmp r11, r13
    jae 8f
    lea r9, [r11 + r12]          # middle
    cmp r9, r13
    cmova r9, r13
    lea r10, [r9 + r12]          # end
    cmp r10, r13
    cmova r10, r13
    mov rdx, r11                 # left
    mov rax, r9                  # right
    mov rbp, r11                 # out
5:  cmp rdx, r9
    jae 6f
    cmp rax, r10
    jae 7f
    push rax
    push rdx
    mov r8, [r15 + rdx * 8]
    mov rcx, [r15 + rax * 8]
    call before                  # CF: r8 before rcx
    pop rdx
    pop rax
    jc 7f
6:  cmp rax, r10
    jae 10f
    mov rcx, [r15 + rax * 8]
    mov [rbx + rbp * 8], rcx
    inc rax
    inc rbp
    jmp 5b
7:  mov rcx, [r15 + rdx * 8]
    mov [rbx + rbp * 8], rcx
    inc rdx
    inc rbp
    jmp 5b
10: add r11, r12
    add r11, r12
    jmp 4b
8:  xchg r15, rbx
    add r12, r12
    jmp 3b

9:  lea r12, [rsp + OUT]
    mov rbx, r12                 # output cursor
    xor r14d, r14d
1:  cmp r14, r13
    jae 2f
    mov r9, [r15 + r14 * 8]
    mov rsi, [r9 + 40]
    mov ecx, [r9 + 16]
    call put
    lea rsi, [rip + s_n]
    mov ecx, 3
    call put
    mov rax, [r9 + 24]
    call putnum
    lea rsi, [rip + s_sum]
    mov ecx, 5
    call put
    mov rax, [r9 + 32]
    call putcents
    call putnl
    inc r14
    jmp 1b
2:  lea rsi, [rip + s_total]
    mov ecx, 8
    call put
    mov rax, [rsp + COUNT]
    call putnum
    lea rsi, [rip + s_sum]
    mov ecx, 5
    call put
    mov rax, [rsp + TOTAL]
    call putcents
    call putnl
    call flush
    xor edi, edi
    mov eax, 231
    syscall

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

# Output through [r12, r12 + OUTSZ), cursor rbx. putcents writes rax as
# <whole>.<2 places>, putnum in decimal, put rcx bytes from rsi. They
# clobber rax, rcx, rdx, rsi, rdi, r10 and r11.
putcents:
    xor edx, edx
    mov ecx, 100
    div rcx
    push rdx
    call putnum
    lea rsi, [rip + s_dot]
    mov ecx, 1
    call put
    pop rax
    mov ecx, 10
    xor edx, edx
    div ecx
    add al, '0'
    add dl, '0'
    sub rsp, 8
    mov [rsp], al
    mov [rsp + 1], dl
    mov rsi, rsp
    mov ecx, 2
    call put
    add rsp, 8
    ret

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

# advance: the chunk offset (rbx) reached [NEXTW], a window boundary of the
# file at r12 (r13 bytes): the window two back dropped (MADV_DONTNEED; a
# later read of it, a kept cell, faults it back in from the page cache),
# the one after the next populated (MADV_POPULATE_READ), [NEXTW] a window
# on. Called from _start's frame; keeps every register.
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
s_n:
    .ascii " n="
s_sum:
    .ascii " sum="
s_total:
    .ascii "total n="
s_dot:
    .ascii "."
s_nl:
    .ascii "\n"
e_csv:
    .ascii "Stop [E3001]: `panic: csv failed`\n"
    .ascii "  --> run.jet:16 in run\n"
    .ascii "     |\n"
    .ascii "16 |     sales: data.csv<Sale>(text) ?? panic(\"csv failed\")\n"
    .ascii "     |                                    ^^^^^\n"
e_read:
    .ascii "Stop [E3001]: `panic: read failed`\n"
    .ascii "  --> run.jet:15 in run\n"
    .ascii "     |\n"
    .ascii "15 |     text: files.read(args.input) ?? panic(\"read failed\")\n"
    .ascii "     |                                     ^^^^^\n"
e_tail:
    .ascii " Why: The program hit a `panic`, `assert`, or `assert_eq` call that failed, a bounds/key check triggered at runtime, or another program-side stop. The shared report boundary owns the program-side stop; an unhandled entry error is a returned report, not E3001. Jet file and line are shown in Jet terms (I2).\n"
    .ascii " Fix: Fix the logic that led to the failure. Program-side stops exit 70 through the shared boundary; unhandled entry errors print their report and exit 1; exit 101 is reserved for Jet defects.\n"
    .ascii "More: jet-lang.dev/e/E3001\n"
e_end:
