# The gauntlet taskfile-cli (Tools/gauntlet/entries/taskfile-cli/jet/run.jet)
# by hand: one step of the task store in ./tasks.json.
#   add TEXT  - append {id: count + 1, text, done: false}, save, "added ID TEXT"
#   done ID   - mark the first task with that id done and save ("done ID"),
#               or "no task ID"; ID is Int.parse (Unicode White_Space
#               trimmed, an optional sign, then ASCII digits; anything else
#               is -1), printed in canonical form
#   list      - "[x] ID TEXT" / "[ ] ID TEXT" per task, "open N done M"
#   other     - nothing (the store is still loaded)
# Loading follows json.decode<Store> of files.read:
# - no tasks.json is an empty store; otherwise the file must be valid UTF-8
#   (strict), at most 16777216 bytes, and one JSON value: an object with a
#   "tasks" array of objects with "id" (an integer), "text" (a string) and
#   "done" (true or false); other keys hold any JSON value; whitespace is
#   space, tab, LF, CR; containers nest at most 256 deep; a key repeated in
#   any object is an error;
# - a number is integral when its exact decimal value is (1.0, 1e2, 100e-2),
#   at most 1000000 digits with |exponent| <= 1000000; ids are kept as their
#   canonical decimal text, so they have no width limit, and done compares
#   texts (both canonical);
# - string escapes decode (\uXXXX with surrogate pairs to UTF-8; a lone
#   surrogate is an error).
# Saving writes Jet's compact encoding {"tasks":[{"id":1,"text":"..",
# "done":false},...]} (", \ and C0 controls escaped, \u00xx lowercase) over
# the file (O_TRUNC). An argument that is not UTF-8 is read lossily (each
# maximal invalid sequence -> U+FFFD), as process.args gives it.
# One lazy anonymous mapping holds the file, the 64-byte rows {id, id
# length, text, text length, done}, the decoded texts and ids, the keys of
# the open objects (for the repeat check) and the output. It is advised
# MADV_NOHUGEPAGE: the areas lie far apart, and under transparent huge
# pages (`always`) each first touch of one zero-filled a whole 2 MiB page,
# 6 of them per step (12,320 KiB peak RSS); with 4 KiB pages a step touches
# only the pages its small store and output use.
# Errors (unreadable or invalid store, failed save) -> exit 1 with no stdout.
# Scope: a store whose ids expand past the text area (~900 MB) is refused.
    .intel_syntax noprefix
    .text

.set ITEM, 16777216              # the largest store (max_item_bytes)
.set READCAP, ITEM + 4096
.set REC, 32 << 20               # rows
.set TEXT, 96 << 20              # decoded texts and ids
.set TEXT_END, 1 << 30
.set KEYS, 1 << 30               # {length, bytes padded to 8} per open key
.set SAVE, 0x60000000            # the encoded store, then stdout
.set KEYCUR, 24 << 20           # the key area's end
.set OBJKEYS, KEYCUR + 8         # the open object's first key
.set DEPTH, KEYCUR + 16          # open containers
.set MAXDEPTH, 256
.set LIMIT, 1000000              # digits and exponent

_start:
    mov rbp, rsp                 # argc, argv
    xor edi, edi
    mov rsi, 1 << 32             # reserved, touched lazily
    mov edx, 3                   # PROT_READ | PROT_WRITE
    mov r10d, 0x4022             # MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE
    mov r8, -1
    xor r9d, r9d
    mov eax, 9                   # mmap
    syscall
    cmp rax, -4096
    ja fail
    mov r15, rax
    mov rdi, rax
    mov rsi, 1 << 32
    mov edx, 15                  # MADV_NOHUGEPAGE
    mov eax, 28                  # madvise
    syscall
    lea rbx, [r15 + REC]         # rows
    xor r14d, r14d               # their count
    lea r13, [r15 + TEXT]        # the text area's end
    mov rax, KEYS
    add rax, r15
    mov [r15 + KEYCUR], rax
    mov [r15 + OBJKEYS], rax

    lea rdi, [rip + store]
    mov esi, 0x80000             # O_RDONLY | O_CLOEXEC
    xor edx, edx
    mov eax, 2                   # open
    syscall
    cmp rax, -2                  # ENOENT: no store
    je loaded
    test rax, rax
    js fail
    mov rdi, rax
    xor r12d, r12d               # bytes read
1:  lea rsi, [r15 + r12]
    mov edx, READCAP
    sub rdx, r12
    jz fail                      # past ITEM
    xor eax, eax                 # read
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    js fail
    jz 2f
    add r12, rax
    jmp 1b
2:  mov eax, 3                   # close
    syscall
    add r12, r15                 # the end; zero bytes follow it
    call utf8_check
    mov rax, r12
    sub rax, r15
    cmp rax, ITEM
    ja fail
    mov rsi, r15
    call parse
loaded:
    cmp qword ptr [rbp], 2       # argc
    jb exit0
    mov rsi, [rbp + 16]          # the command
    mov eax, [rsi]
    cmp eax, 0x00646461          # "add"
    je add
    cmp eax, 0x656e6f64          # "done"
    jne 1f
    cmp byte ptr [rsi + 4], 0
    je done
1:  cmp eax, 0x7473696c          # "list"
    jne exit0
    cmp byte ptr [rsi + 4], 0
    je list
exit0:
    xor edi, edi
    mov eax, 231
    syscall
fail:
    mov edi, 1
    mov eax, 231
    syscall

# ---- commands ---------------------------------------------------------------

add:
    mov rdi, r13                 # the text: argv[2] (lossy UTF-8) or ""
    cmp qword ptr [rbp], 3
    jb 1f
    mov rsi, [rbp + 24]
    call lossy
1:  mov r12, r13                 # text
    mov r10, rdi
    sub r10, r13                 # its length
    mov r13, rdi
    lea rax, [r14 + 1]           # the id
    call decimal
    mov rdx, r14
    shl rdx, 6
    add rdx, rbx
    mov [rdx], r13
    mov rcx, rdi
    sub rcx, r13
    mov [rdx + 8], rcx
    mov [rdx + 16], r12
    mov [rdx + 24], r10
    mov qword ptr [rdx + 32], 0
    mov r13, rdi
    inc r14
    push rdx
    call save
    pop rdx
    lea rdi, [r15 + SAVE]
    mov dword ptr [rdi], 0x65646461   # "adde"
    mov word ptr [rdi + 4], 0x2064    # "d "
    add rdi, 6
    mov rsi, [rdx]
    mov rcx, [rdx + 8]
    rep movsb
    mov byte ptr [rdi], ' '
    inc rdi
    mov rsi, [rdx + 16]
    mov rcx, [rdx + 24]
    rep movsb
    jmp newline_out

done:
    mov rdi, r13                 # the argument (lossy UTF-8) or ""
    cmp qword ptr [rbp], 3
    jb 1f
    mov rsi, [rbp + 24]
    call lossy
1:  mov r12, rdi                 # its end
    mov byte ptr [rdi], 0        # (stops the trim)
    mov rsi, r13
    # Int.parse: Unicode white space trimmed, [+-] ASCII digits, else -1.
2:  call wslen                   # the front
    test eax, eax
    jz 3f
    add rsi, rax
    jmp 2b
3:  mov rdi, r12                 # the back
31: cmp rdi, rsi
    jbe 4f
    lea rdx, [rdi - 1]           # the last character's start
32: cmp rdx, rsi
    jbe 33f
    movzx eax, byte ptr [rdx]
    and eax, 0xc0
    cmp eax, 0x80
    jne 33f
    dec rdx
    jmp 32b
33: push rsi
    mov rsi, rdx
    call wslen
    pop rsi
    test eax, eax
    jz 4f
    mov rdi, rdx
    jmp 31b
4:  xor r9d, r9d                 # negative; rsi .. rdi the trimmed text
    cmp rsi, rdi
    jae 5f
    movzx eax, byte ptr [rsi]
    cmp eax, '-'
    jne 41f
    inc r9d
    inc rsi
    jmp 42f
41: cmp eax, '+'
    jne 42f
    inc rsi
42: cmp rsi, rdi
    jae 5f                       # no digits
    mov rax, rsi
43: cmp rax, rdi
    jae 6f
    movzx edx, byte ptr [rax]
    sub edx, '0'
    cmp edx, 9
    ja 5f
    inc rax
    jmp 43b
5:  lea rsi, [rip + minus_one + 1]   # not a number: -1
    lea rdi, [rsi + 1]
    mov r9d, 1
    # rsi .. rdi: the digits; leading zeros go
6:  cmp byte ptr [rsi], '0'
    jne 7f
    lea rax, [rsi + 1]
    cmp rax, rdi
    jae 7f                       # keep one zero
    inc rsi
    jmp 6b
7:  cmp byte ptr [rsi], '0'
    jne 8f
    xor r9d, r9d                 # -0 is 0
8:  mov r10, r12                 # the canonical text, after the argument
    mov rcx, rdi
    sub rcx, rsi
    mov rdi, r12
    test r9d, r9d
    jz 9f
    mov byte ptr [rdi], '-'
    inc rdi
9:  rep movsb
    mov r11, rdi
    sub r11, r10                 # its length
    xor ecx, ecx                 # the first task with that id
    mov rdx, rbx
10: cmp rcx, r14
    jae not_found
    cmp [rdx + 8], r11
    jne 11f
    mov rsi, [rdx]
    mov rdi, r10
    push rcx
    mov rcx, r11
    repe cmpsb
    pop rcx
    je 12f
11: inc rcx
    add rdx, 64
    jmp 10b
12: mov qword ptr [rdx + 32], 1
    push r10
    push r11
    call save
    pop r11
    pop r10
    lea rdi, [r15 + SAVE]
    mov dword ptr [rdi], 0x656e6f64   # "done"
    mov byte ptr [rdi + 4], ' '
    add rdi, 5
    jmp 13f
not_found:
    lea rdi, [r15 + SAVE]
    mov rax, 0x6b736174206f6e    # "no task"
    mov [rdi], rax
    mov byte ptr [rdi + 7], ' '
    add rdi, 8
13: mov rsi, r10
    mov rcx, r11
    rep movsb
    jmp newline_out

list:
    lea rdi, [r15 + SAVE]
    xor r12d, r12d               # done
    xor r11d, r11d
    mov r10, rbx
1:  cmp r11, r14
    jae 3f
    mov eax, 0x205d205b          # "[ ] "
    cmp qword ptr [r10 + 32], 0
    je 2f
    mov eax, 0x205d785b          # "[x] "
    inc r12
2:  mov [rdi], eax
    add rdi, 4
    mov rsi, [r10]
    mov rcx, [r10 + 8]
    rep movsb
    mov byte ptr [rdi], ' '
    inc rdi
    mov rsi, [r10 + 16]
    mov rcx, [r10 + 24]
    rep movsb
    mov byte ptr [rdi], 10
    inc rdi
    inc r11
    add r10, 64
    jmp 1b
3:  mov dword ptr [rdi], 0x6e65706f      # "open"
    mov byte ptr [rdi + 4], ' '
    add rdi, 5
    mov rax, r14
    sub rax, r12
    call decimal
    mov rax, 0x20656e6f6420      # " done "
    mov [rdi], rax
    add rdi, 6
    mov rax, r12
    call decimal
newline_out:
    mov byte ptr [rdi], 10
    inc rdi
    lea rsi, [r15 + SAVE]
    mov rdx, rdi
    sub rdx, rsi
    mov r8d, 1
    call write_all
    jmp exit0

# ---- helpers ----------------------------------------------------------------

# write_all: rdx bytes at rsi to fd r8.
write_all:
1:  test rdx, rdx
    jz 2f
    mov edi, r8d
    mov eax, 1
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    jle fail
    add rsi, rax
    sub rdx, rax
    jmp 1b
2:  ret

# decimal: unsigned rax in decimal at rdi (advanced). Clobbers rax, rcx, rdx,
# rsi, r8.
decimal:
    lea rsi, [rsp - 8]
    mov r8d, 10
1:  xor edx, edx
    div r8
    add dl, '0'
    dec rsi
    mov [rsi], dl
    test rax, rax
    jnz 1b
    lea rcx, [rsp - 8]
    sub rcx, rsi
    rep movsb
    ret

# utf8_seq: the sequence led by the byte (>= 0x80) at rsi: eax = its length
# when valid, else eax = 0 and ecx = the maximal invalid prefix's length.
# Clobbers rdx, r8, r11.
utf8_seq:
    movzx edx, byte ptr [rsi]
    mov r8d, 0x80                # the second byte's range
    mov r11d, 0xbf
    cmp edx, 0xc2
    jb 1f
    mov eax, 2
    cmp edx, 0xe0
    jb 4f
    mov eax, 3
    cmp edx, 0xf0
    jb 2f
    mov eax, 4
    cmp edx, 0xf5
    jb 3f
1:  xor eax, eax
    mov ecx, 1
    ret
2:  cmp edx, 0xe0
    jne 21f
    mov r8d, 0xa0                # no overlong
21: cmp edx, 0xed
    jne 4f
    mov r11d, 0x9f               # no surrogates
    jmp 4f
3:  cmp edx, 0xf0
    jne 31f
    mov r8d, 0x90                # no overlong
31: cmp edx, 0xf4
    jne 4f
    mov r11d, 0x8f               # no past U+10FFFF
4:  movzx edx, byte ptr [rsi + 1]
    cmp edx, r8d
    jb 1b
    cmp edx, r11d
    ja 1b
    mov ecx, 2
5:  cmp ecx, eax
    jae 6f
    movzx edx, byte ptr [rsi + rcx]
    sub edx, 0x80
    cmp edx, 0x3f
    ja 7f
    inc ecx
    jmp 5b
6:  ret
7:  xor eax, eax
    ret

# utf8_check: the file (r15 .. r12) is valid UTF-8, else exit.
utf8_check:
    mov rsi, r15
    mov r9, 0x8080808080808080
1:  cmp rsi, r12
    jae 3f
    test [rsi], r9               # eight ASCII bytes (zeros follow the end)
    jnz 2f
    add rsi, 8
    jmp 1b
2:  cmp byte ptr [rsi], 0
    jl 4f
    inc rsi
    jmp 1b
4:  call utf8_seq
    test eax, eax
    jz fail
    add rsi, rax
    jmp 1b
3:  ret

# lossy: the C string at rsi as UTF-8 at rdi (advanced), each maximal invalid
# sequence as U+FFFD.
lossy:
1:  movzx eax, byte ptr [rsi]
    test eax, eax
    jz 4f
    cmp eax, 0x80
    jae 2f
    mov [rdi], al
    inc rdi
    inc rsi
    jmp 1b
2:  call utf8_seq
    test eax, eax
    jz 3f
    mov ecx, eax
    rep movsb
    jmp 1b
3:  mov word ptr [rdi], 0xbfef
    mov byte ptr [rdi + 2], 0xbd
    add rdi, 3
    add rsi, rcx
    jmp 1b
4:  ret

# wslen: the length of the Unicode White_Space character at rsi (valid
# UTF-8), else 0. Clobbers ecx.
wslen:
    movzx eax, byte ptr [rsi]
    cmp eax, ' '
    je 1f
    lea ecx, [rax - 9]           # TAB LF VT FF CR
    cmp ecx, 4
    jbe 1f
    cmp eax, 0xc2
    je 2f
    cmp eax, 0xe1
    je 3f
    cmp eax, 0xe2
    je 4f
    cmp eax, 0xe3
    je 5f
0:  xor eax, eax
    ret
1:  mov eax, 1
    ret
2:  movzx ecx, byte ptr [rsi + 1]
    cmp ecx, 0x85                # U+0085
    je 21f
    cmp ecx, 0xa0                # U+00A0
    jne 0b
21: mov eax, 2
    ret
3:  cmp word ptr [rsi + 1], 0x809a   # U+1680
    jne 0b
    jmp 6f
4:  movzx ecx, byte ptr [rsi + 1]
    cmp ecx, 0x81
    je 41f
    cmp ecx, 0x80
    jne 0b
    movzx ecx, byte ptr [rsi + 2]
    sub ecx, 0x80
    cmp ecx, 0x0a                # U+2000 .. U+200A
    jbe 6f
    cmp ecx, 0x28                # U+2028
    je 6f
    cmp ecx, 0x29                # U+2029
    je 6f
    cmp ecx, 0x2f                # U+202F
    je 6f
    jmp 0b
41: cmp byte ptr [rsi + 2], 0x9f # U+205F
    jne 0b
    jmp 6f
5:  cmp word ptr [rsi + 1], 0x8080   # U+3000
    jne 0b
6:  mov eax, 3
    ret

# save: the rows as Jet's compact JSON over tasks.json. Keeps rbx, r12-r15,
# rbp.
save:
    lea rdi, [r15 + SAVE]
    mov rax, 0x736b736174227b    # {"tasks
    mov [rdi], rax
    mov dword ptr [rdi + 7], 0x5b3a22  # ":[
    add rdi, 10
    xor r10d, r10d
    mov r11, rbx
1:  cmp r10, r14
    jae 6f
    test r10, r10
    jz 2f
    mov byte ptr [rdi], ','
    inc rdi
2:  mov rax, 0x3a226469227b      # {"id":
    mov [rdi], rax
    add rdi, 6
    mov rsi, [r11]
    mov rcx, [r11 + 8]
    rep movsb
    mov rax, 0x2274786574222c    # ,"text"
    mov [rdi], rax
    mov word ptr [rdi + 7], 0x223a     # :"
    add rdi, 9
    mov rsi, [r11 + 16]
    mov rcx, [r11 + 24]
    call escape
    mov rax, 0x656e6f64222c22    # ","done
    mov [rdi], rax
    mov word ptr [rdi + 7], 0x3a22     # ":
    add rdi, 9
    cmp qword ptr [r11 + 32], 0
    je 3f
    mov dword ptr [rdi], 0x65757274    # true
    add rdi, 4
    jmp 4f
3:  mov dword ptr [rdi], 0x736c6166    # fals
    mov byte ptr [rdi + 4], 'e'
    add rdi, 5
4:  mov byte ptr [rdi], '}'
    inc rdi
    inc r10
    add r11, 64
    jmp 1b
6:  mov word ptr [rdi], 0x7d5d       # ]}
    add rdi, 2
    push rdi
    lea rdi, [rip + store]
    mov esi, 0x80241             # O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC
    mov edx, 0666
    mov eax, 2
    syscall
    test rax, rax
    js fail
    mov r8, rax
    pop rdx
    lea rsi, [r15 + SAVE]
    sub rdx, rsi
    push r8
    call write_all
    pop rdi
    mov eax, 3                   # close
    syscall
    test rax, rax
    js fail
    ret

# escape: rcx bytes at rsi as a JSON string body at rdi.
escape:
    test rcx, rcx
    jz 9f
1:  movzx eax, byte ptr [rsi]
    inc rsi
    cmp eax, '"'
    je 3f
    cmp eax, '\\'
    je 3f
    cmp eax, 32
    jb 4f
2:  mov [rdi], al
    inc rdi
    dec rcx
    jnz 1b
9:  ret
3:  mov byte ptr [rdi], '\\'
    inc rdi
    jmp 2b
4:  mov byte ptr [rdi], '\\'
    lea rdx, [rip + short_escape]
    movzx edx, byte ptr [rdx + rax]
    test edx, edx
    jz 5f
    mov [rdi + 1], dl
    add rdi, 2
    dec rcx
    jnz 1b
    ret
5:  mov dword ptr [rdi + 1], 0x30303075  # u00
    mov edx, eax
    shr edx, 4
    add edx, '0'
    mov [rdi + 4], dl
    and eax, 15
    lea edx, [rax + '0']
    lea eax, [rax + 87]
    cmp edx, '9'
    cmovbe eax, edx
    mov [rdi + 5], al
    add rdi, 6
    dec rcx
    jnz 1b
    ret

# ---- the store's JSON -------------------------------------------------------
# rsi: the cursor (zero bytes follow the text, so no read passes the end
# unchecked); r12: the end. Any error exits.

parse:
    call ws
    cmp byte ptr [rsi], '{'
    jne fail
    inc rsi
    call enter
    push qword ptr [r15 + OBJKEYS]
    mov rax, [r15 + KEYCUR]
    mov [r15 + OBJKEYS], rax
    xor r9d, r9d                 # tasks seen
    call ws
    cmp byte ptr [rsi], '}'
    je fail
1:  call key
    xor r8d, r8d
    cmp rcx, 5
    jne 2f
    cmp dword ptr [rax], 0x6b736174    # task
    jne 2f
    cmp byte ptr [rax + 4], 's'
    sete r8b
2:  push r8
    call colon
    pop r8
    test r8d, r8d
    jz 3f
    call tasks
    mov r9d, 1
    jmp 4f
3:  call skip_value
4:  call ws
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp eax, ','
    jne 5f
    call ws
    jmp 1b
5:  cmp eax, '}'
    jne fail
    call leave_object
    call ws
    cmp rsi, r12
    jne fail
    test r9d, r9d
    jz fail
    ret

# enter: one container deeper (at most MAXDEPTH).
enter:
    inc dword ptr [r15 + DEPTH]
    cmp dword ptr [r15 + DEPTH], MAXDEPTH
    ja fail
    ret

# leave_object: drop the closing object's keys, restore the outer object's
# (the caller's last push) and go one level up.
leave_object:
    mov rax, [r15 + OBJKEYS]
    mov [r15 + KEYCUR], rax
    pop rdx                      # the return address
    pop qword ptr [r15 + OBJKEYS]
    dec dword ptr [r15 + DEPTH]
    jmp rdx

# key: an object key onto the key area, refusing a repeat within the object;
# rax its bytes, rcx its length. Clobbers rdx, rdi, r8, r11.
key:
    push r13
    mov r13, [r15 + KEYCUR]
    add r13, 8
    call string
    pop r13
    lea rdx, [rax - 8]           # its slot
    mov [rdx], rcx
    mov rdi, [r15 + OBJKEYS]
1:  cmp rdi, rdx
    jae 3f
    mov r8, [rdi]
    cmp r8, rcx
    jne 2f
    push rsi
    push rdi
    push rcx
    lea rsi, [rdi + 8]
    mov rdi, rax
    repe cmpsb                   # (no bytes: ZF stays from the length cmp)
    pop rcx
    pop rdi
    pop rsi
    je fail                      # a repeated key
2:  lea rdi, [rdi + r8 + 15]
    and rdi, -8
    jmp 1b
3:  lea rdi, [rax + rcx + 7]
    and rdi, -8
    mov [r15 + KEYCUR], rdi
    ret

# colon: whitespace, ':', whitespace.
colon:
    call ws
    cmp byte ptr [rsi], ':'
    jne fail
    inc rsi
# ws: past JSON whitespace.
ws:
1:  movzx eax, byte ptr [rsi]
    cmp eax, ' '
    je 2f
    cmp eax, 10
    je 2f
    cmp eax, 13
    je 2f
    cmp eax, 9
    jne 3f
2:  inc rsi
    jmp 1b
3:  ret

# tasks: the array into the rows.
tasks:
    cmp byte ptr [rsi], '['
    jne fail
    inc rsi
    call enter
    call ws
    cmp byte ptr [rsi], ']'
    je 3f
1:  call task
    call ws
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp eax, ','
    jne 2f
    call ws
    jmp 1b
2:  cmp eax, ']'
    jne fail
    dec dword ptr [r15 + DEPTH]
    ret
3:  inc rsi
    dec dword ptr [r15 + DEPTH]
    ret

# task: one object into row r14 (then counted).
task:
    cmp byte ptr [rsi], '{'
    jne fail
    inc rsi
    call enter
    push r9
    push qword ptr [r15 + OBJKEYS]
    mov rax, [r15 + KEYCUR]
    mov [r15 + OBJKEYS], rax
    xor r9d, r9d                 # fields seen: 1 id, 2 text, 4 done
    mov r10, r14
    shl r10, 6
    add r10, rbx                 # the row
    call ws
    cmp byte ptr [rsi], '}'
    je fail
1:  call key
    xor r8d, r8d                 # 1 id, 2 text, 4 done, 0 other
    cmp rcx, 2
    jne 2f
    cmp word ptr [rax], 0x6469   # id
    jne 3f
    mov r8d, 1
    jmp 3f
2:  cmp rcx, 4
    jne 3f
    mov edx, [rax]
    cmp edx, 0x74786574          # text
    jne 21f
    mov r8d, 2
    jmp 3f
21: cmp edx, 0x656e6f64          # done
    jne 3f
    mov r8d, 4
3:  push r8
    call colon
    pop r8
    or r9d, r8d
    cmp r8d, 1
    jne 4f
    call id_number
    mov [r10], rax
    mov [r10 + 8], rcx
    jmp 7f
4:  cmp r8d, 2
    jne 5f
    cmp byte ptr [rsi], '"'
    jne fail
    call string
    mov [r10 + 16], rax
    mov [r10 + 24], rcx
    jmp 7f
5:  cmp r8d, 4
    jne 6f
    call boolean
    mov [r10 + 32], rax
    jmp 7f
6:  call skip_value
7:  call ws
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp eax, ','
    jne 8f
    call ws
    jmp 1b
8:  cmp eax, '}'
    jne fail
    cmp r9d, 7
    jne fail
    call leave_object
    pop r9
    inc r14
    ret

# boolean: rax = 1 for true, 0 for false.
boolean:
    cmp dword ptr [rsi], 0x65757274    # true
    jne 1f
    add rsi, 4
    mov eax, 1
    ret
1:  cmp dword ptr [rsi], 0x736c6166    # fals
    jne fail
    cmp byte ptr [rsi + 4], 'e'
    jne fail
    add rsi, 5
    xor eax, eax
    ret

# id_number: a JSON number whose exact value is an integer, as its canonical
# decimal text on the text area: rax, rcx. Keeps r9, r10.
id_number:
    xor r8d, r8d                 # negative
    cmp byte ptr [rsi], '-'
    jne 1f
    inc r8d
    inc rsi
1:  lea rdi, [r13 + 1]           # the digits D (int then fraction), after
    movzx eax, byte ptr [rsi]    # room for a '-'
    sub eax, '0'
    cmp eax, 9
    ja fail
    test eax, eax
    jnz 2f
    mov byte ptr [rdi], '0'      # a lone zero
    inc rdi
    inc rsi
    movzx eax, byte ptr [rsi]
    sub eax, '0'
    cmp eax, 9
    jbe fail                     # a leading zero
    jmp 3f
2:  movzx eax, byte ptr [rsi]
    lea edx, [rax - '0']
    cmp edx, 9
    ja 3f
    mov [rdi], al
    inc rdi
    inc rsi
    jmp 2b
3:  xor ecx, ecx                 # fraction digits
    cmp byte ptr [rsi], '.'
    jne 5f
    inc rsi
    movzx eax, byte ptr [rsi]
    sub eax, '0'
    cmp eax, 9
    ja fail
4:  movzx eax, byte ptr [rsi]
    lea edx, [rax - '0']
    cmp edx, 9
    ja 5f
    mov [rdi], al
    inc rdi
    inc rsi
    inc rcx
    jmp 4b
5:  xor edx, edx                 # the exponent
    movzx eax, byte ptr [rsi]
    or eax, 0x20
    cmp eax, 'e'
    jne 8f
    inc rsi
    xor r11d, r11d               # negative exponent
    movzx eax, byte ptr [rsi]
    cmp eax, '+'
    je 6f
    cmp eax, '-'
    jne 61f
    inc r11d
6:  inc rsi
61: movzx eax, byte ptr [rsi]
    sub eax, '0'
    cmp eax, 9
    ja fail
7:  movzx eax, byte ptr [rsi]
    sub eax, '0'
    cmp eax, 9
    ja 71f
    imul edx, edx, 10
    add edx, eax
    cmp edx, LIMIT
    ja fail                      # |exponent| past the limit
    inc rsi
    jmp 7b
71: test r11d, r11d
    jz 8f
    neg rdx
8:  movsxd rdx, edx
    lea rax, [r13 + 1]           # D
    mov r11, rdi
    sub r11, rax                 # raw: its length
    cmp r11, LIMIT
    ja fail
    sub rcx, rdx                 # scale: fraction digits - exponent
    jns 9f
    mov rdx, r11
    sub rdx, rcx
    cmp rdx, LIMIT
    ja fail
9:  cmp rax, rdi                 # the first nonzero digit
    jae 14f
    cmp byte ptr [rax], '0'
    jne 10f
    inc rax
    jmp 9b
10: lea rdx, [r13 + 1]
    sub rax, rdx                 # first
    mov rdx, r11                 # keep
    test rcx, rcx
    jle 12f
    sub rdx, rcx                 # raw - scale
    mov rdi, rdx
    cmp rdi, rax
    cmovl rdi, rax               # check from max(first, raw - scale)
11: cmp rdi, r11
    jae 111f
    cmp byte ptr [r13 + 1 + rdi], '0'
    jne fail                     # not an exact integer
    inc rdi
    jmp 11b
111: cmp rdx, rax
    jle 14f                      # nothing left: 0
12: mov rdi, r13                 # the text: [-] D[first..keep] zeros
    test r8d, r8d
    jz 13f
    mov byte ptr [rdi], '-'
    inc rdi
13: push rsi
    push rcx
    lea rsi, [r13 + 1 + rax]
    mov rcx, rdx
    sub rcx, rax
    rep movsb
    pop rcx
    pop rsi
    test rcx, rcx
    jns 15f
    neg rcx                      # zeros
    mov rax, TEXT_END
    add rax, r15
    sub rax, rdi
    cmp rcx, rax
    ja fail                      # (past the text area)
    mov al, '0'
    rep stosb
    jmp 15f
14: mov byte ptr [r13], '0'
    lea rdi, [r13 + 1]
15: mov rax, r13
    mov rcx, rdi
    sub rcx, rax
    mov r13, rdi
    ret

# string: a JSON string decoded onto the text area at r13; rax its start,
# rcx its length; r13 past it.
string:
    cmp byte ptr [rsi], '"'
    jne fail
    inc rsi
    mov rdi, r13
1:  movzx eax, byte ptr [rsi]
    inc rsi
    cmp eax, '"'
    je 9f
    cmp eax, '\\'
    je 2f
    cmp eax, 32
    jb fail                      # a control byte (or the end)
    mov [rdi], al
    inc rdi
    jmp 1b
2:  movzx eax, byte ptr [rsi]
    inc rsi
    lea rdx, [rip + unescape]
    movzx edx, byte ptr [rdx + rax]
    cmp edx, 1
    jb fail
    je 3f                        # \u
    mov [rdi], dl
    inc rdi
    jmp 1b
3:  call hex4
    mov edx, eax
    and edx, 0xfc00
    cmp edx, 0xdc00
    je fail                      # a lone low surrogate
    cmp edx, 0xd800
    jne 4f
    cmp word ptr [rsi], 0x755c   # \u
    jne fail
    add rsi, 2
    mov r11d, eax
    call hex4
    mov edx, eax
    and edx, 0xfc00
    cmp edx, 0xdc00
    jne fail
    sub r11d, 0xd800
    shl r11d, 10
    sub eax, 0xdc00
    add eax, r11d
    add eax, 0x10000
4:  cmp eax, 0x80                # UTF-8
    jae 5f
    mov [rdi], al
    inc rdi
    jmp 1b
5:  cmp eax, 0x800
    jae 6f
    mov edx, eax
    shr edx, 6
    or edx, 0xc0
    mov [rdi], dl
    and eax, 0x3f
    or eax, 0x80
    mov [rdi + 1], al
    add rdi, 2
    jmp 1b
6:  cmp eax, 0x10000
    jae 7f
    mov edx, eax
    shr edx, 12
    or edx, 0xe0
    mov [rdi], dl
    mov edx, eax
    shr edx, 6
    and edx, 0x3f
    or edx, 0x80
    mov [rdi + 1], dl
    and eax, 0x3f
    or eax, 0x80
    mov [rdi + 2], al
    add rdi, 3
    jmp 1b
7:  mov edx, eax
    shr edx, 18
    or edx, 0xf0
    mov [rdi], dl
    mov edx, eax
    shr edx, 12
    and edx, 0x3f
    or edx, 0x80
    mov [rdi + 1], dl
    mov edx, eax
    shr edx, 6
    and edx, 0x3f
    or edx, 0x80
    mov [rdi + 2], dl
    and eax, 0x3f
    or eax, 0x80
    mov [rdi + 3], al
    add rdi, 4
    jmp 1b
9:  mov rax, r13
    mov rcx, rdi
    sub rcx, rax
    mov r13, rdi
    ret

# hex4: four hex digits at rsi into eax.
hex4:
    xor eax, eax
    mov ecx, 4
1:  movzx edx, byte ptr [rsi]
    inc rsi
    shl eax, 4
    sub edx, '0'
    cmp edx, 9
    jbe 2f
    movzx edx, byte ptr [rsi - 1]
    or edx, 0x20                 # letters, either case
    sub edx, 'a'
    cmp edx, 5
    ja fail
    add edx, 10
2:  or eax, edx
    dec ecx
    jnz 1b
    ret

# skip_value: past any JSON value (its keys checked, its depth counted).
skip_value:
    movzx eax, byte ptr [rsi]
    cmp eax, '"'
    jne 1f
    push r13
    call string
    pop r13
    ret
1:  cmp eax, '{'
    jne 4f
    inc rsi
    call enter
    push qword ptr [r15 + OBJKEYS]
    mov rax, [r15 + KEYCUR]
    mov [r15 + OBJKEYS], rax
    call ws
    cmp byte ptr [rsi], '}'
    je 3f
2:  call key
    call colon
    call skip_value
    call ws
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp eax, ','
    jne 21f
    call ws
    jmp 2b
21: cmp eax, '}'
    jne fail
    call leave_object
    ret
3:  inc rsi
    call leave_object
    ret
4:  cmp eax, '['
    jne 6f
    inc rsi
    call enter
    call ws
    cmp byte ptr [rsi], ']'
    je 52f
5:  call skip_value
    call ws
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp eax, ','
    jne 51f
    call ws
    jmp 5b
51: cmp eax, ']'
    jne fail
    dec dword ptr [r15 + DEPTH]
    ret
52: inc rsi
    dec dword ptr [r15 + DEPTH]
    ret
6:  cmp dword ptr [rsi], 0x6c6c756e      # null
    jne 7f
    add rsi, 4
    ret
7:  cmp eax, 't'
    je boolean
    cmp eax, 'f'
    je boolean
    # a number: -? (0 | [1-9][0-9]*) (. [0-9]+)? ([eE] [+-]? [0-9]+)?
    cmp eax, '-'
    jne 8f
    inc rsi
8:  movzx eax, byte ptr [rsi]
    sub eax, '0'
    cmp eax, 9
    ja fail
    inc rsi
    test eax, eax
    jnz 9f
    movzx eax, byte ptr [rsi]
    sub eax, '0'
    cmp eax, 9
    jbe fail                     # a leading zero
    jmp 10f
9:  movzx eax, byte ptr [rsi]
    sub eax, '0'
    cmp eax, 9
    ja 10f
    inc rsi
    jmp 9b
10: cmp byte ptr [rsi], '.'
    jne 12f
    inc rsi
    call digits
12: movzx eax, byte ptr [rsi]
    or eax, 0x20
    cmp eax, 'e'
    jne 13f
    inc rsi
    movzx eax, byte ptr [rsi]
    cmp eax, '+'
    je 14f
    cmp eax, '-'
    jne 15f
14: inc rsi
15: call digits
13: ret

# digits: one or more decimal digits.
digits:
    movzx eax, byte ptr [rsi]
    sub eax, '0'
    cmp eax, 9
    ja fail
1:  inc rsi
    movzx eax, byte ptr [rsi]
    sub eax, '0'
    cmp eax, 9
    jbe 1b
    ret

    # (read-only data stays in .text: the harness keeps one R+X segment)
store:      .asciz "tasks.json"
empty:      .byte 0
minus_one:  .asciz "-1"
# JSON string escapes: the byte after '\' -> the byte it means, 1 for \u,
# 0 for none.
unescape:
    .fill 34, 1, 0
    .byte '"'                    # 34
    .fill 47 - 35, 1, 0
    .byte '/'                    # 47
    .fill 92 - 48, 1, 0
    .byte '\\'                   # 92
    .fill 98 - 93, 1, 0
    .byte 8                      # b
    .fill 102 - 99, 1, 0
    .byte 12                     # f
    .fill 110 - 103, 1, 0
    .byte 10                     # n
    .fill 114 - 111, 1, 0
    .byte 13                     # r
    .byte 0                      # s
    .byte 9                      # t
    .byte 1                      # u
    .fill 256 - 118, 1, 0
# Jet's short escapes for C0 bytes (0: \u00xx).
short_escape:
    .fill 8, 1, 0
    .byte 'b', 't', 'n', 0, 'f', 'r'
    .fill 32 - 14, 1, 0
