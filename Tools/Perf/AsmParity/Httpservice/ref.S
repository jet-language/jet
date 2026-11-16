# httpservice (gauntlet netserv.service): a JSON key-value HTTP/1.1 service
# on 127.0.0.1:<argv[1]>. GET /health answers {"status":"ok"}; PUT /kv/<key>
# stores the request body (its Content-Length bytes; one that is not UTF-8
# or is over 8 MiB stores "") and answers {"stored":"<key>"}; GET /kv/<key>
# answers {"key":"<key>","value":"<value>"}, or 404 {"error":"not found"};
# GET /shutdown answers {"bye":true} and then exits 0; anything else is 404
# {"error":"not found"}. Every answer is application/json with its
# Content-Length. Keys and values go into the JSON as they are, as the Jet
# program's interpolation puts them.
#
# Technique: one connection at a time, kept alive as HTTP/1.1 keeps it
# (closed on "Connection: close", HTTP/1.0, a body it does not read, or
# /shutdown), so a client's next request costs no connect. A kept
# connection that goes idle yields to a waiting one (poll over both). The
# head is read into one buffer until its blank line (what follows it is
# the next request's start), and each answer is one writev whose pieces
# point at the status line, the literals, the key in the request and the
# value in the store, so no body is copied. A body is read straight into
# the store's arena after its key; the store is an open-addressed table
# hashed by FNV-1a. Huge pages are off, so the table and arena cost the
# pages they fill.
# A port it cannot listen on prints "listen failed" on stderr, exit 1.
    .intel_syntax noprefix
    .text

    .set REQSZ, 65536            # the request head's buffer
    .set NSLOTS, 65536           # the table: {key, key length, value, value length}
    .set TABLE, REQSZ
    .set ARENA, REQSZ + NSLOTS * 32
    .set ARENASZ, 1 << 34
    .set BODYMAX, 8388608

    .set SA, 0
    .set IOV, 16                 # six struct iovec
    .set HEAD, 112               # the status line and headers (128 bytes)
    .set NREAD, 240
    .set HEND, 248               # the body's offset in the buffer
    .set CLEN, 256
    .set KEYLEN, 264
    .set PFD, 272                # two struct pollfd: the connection, the listener
    .set NEXT, 288               # the next request's offset in the buffer
    .set KEEP, 296               # 1 while the connection is kept
    .set KEPT, 304               # 1 once the connection has been answered
    .set FRAME, 320

_start:
    mov rax, [rsp]
    cmp rax, 2
    jb fail_listen
    mov rsi, [rsp + 16]
    sub rsp, FRAME
    mov r15, rsp
    xor eax, eax                 # the port
    xor ecx, ecx
1:  movzx edx, byte ptr [rsi + rcx]
    sub edx, '0'
    cmp edx, 9
    ja 2f
    imul eax, eax, 10
    add eax, edx
    inc ecx
    cmp ecx, 5
    jbe 1b
    jmp fail_listen
2:  test ecx, ecx
    jz fail_listen
    cmp byte ptr [rsi + rcx], 0
    jne fail_listen
    cmp eax, 65535
    ja fail_listen
    xchg al, ah
    mov word ptr [r15 + SA], 2   # AF_INET
    mov [r15 + SA + 2], ax
    mov dword ptr [r15 + SA + 4], 0x0100007f # 127.0.0.1
    mov qword ptr [r15 + SA + 8], 0
    mov edi, 41                  # prctl(PR_SET_THP_DISABLE, 1)
    mov esi, 1
    xor edx, edx
    xor r10d, r10d
    xor r8d, r8d
    mov eax, 157
    syscall
    xor edi, edi
    mov rsi, ARENA + ARENASZ
    mov edx, 3
    mov r10d, 0x4022             # MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE
    mov r8, -1
    xor r9d, r9d
    mov eax, 9                   # mmap
    syscall
    cmp rax, -4096
    ja fail_listen
    mov rbx, rax
    lea r12, [rax + ARENA]       # the arena's end
    mov edi, 2                   # AF_INET
    mov esi, 0x80001             # SOCK_STREAM | SOCK_CLOEXEC
    xor edx, edx
    mov eax, 41                  # socket
    syscall
    test eax, eax
    js fail_listen
    mov r13d, eax
    push 1
    mov edi, r13d
    mov esi, 1                   # SOL_SOCKET
    mov edx, 2                   # SO_REUSEADDR
    mov r10, rsp
    mov r8d, 4
    mov eax, 54                  # setsockopt
    syscall
    pop rax
    mov edi, r13d
    lea rsi, [r15 + SA]
    mov edx, 16
    mov eax, 49                  # bind
    syscall
    test rax, rax
    jnz fail_listen
    mov edi, r13d
    mov esi, 128
    mov eax, 50                  # listen
    syscall
    test rax, rax
    jnz fail_listen

serve:
    mov edi, r13d
    xor esi, esi
    xor edx, edx
    mov r10d, 0x80000            # SOCK_CLOEXEC
    mov eax, 288                 # accept4
    syscall
    test eax, eax
    js serve
    mov r14d, eax
    xor ebp, ebp                 # bytes in the buffer
    mov qword ptr [r15 + KEPT], 0
head:
    # The head: read until its blank line.
    xor edi, edi                 # where the blank line may start
2:  lea rcx, [rbp - 3]
21: cmp rdi, rcx
    jge 1f
    cmp dword ptr [rbx + rdi], 0x0a0d0a0d
    je 3f
    inc rdi
    jmp 21b
1:  test rbp, rbp                # an idle kept connection yields to a new one
    jnz 12f
    cmp qword ptr [r15 + KEPT], 0
    je 12f
    mov [r15 + PFD], r14d
    mov dword ptr [r15 + PFD + 4], 1     # POLLIN
    mov [r15 + PFD + 8], r13d
    mov dword ptr [r15 + PFD + 12], 1
    push rdi
11: lea rdi, [r15 + PFD]
    mov esi, 2
    mov edx, -1
    mov eax, 7                   # poll
    syscall
    cmp rax, -4
    je 11b
    pop rdi
    test word ptr [r15 + PFD + 6], 0x19  # POLLIN | POLLERR | POLLHUP
    jz drop
12: mov edx, REQSZ
    sub edx, ebp
    jz drop
    push rdi
    mov edi, r14d
    lea rsi, [rbx + rbp]
    xor eax, eax                 # read
    syscall
    pop rdi
    cmp rax, -4
    je 12b
    test rax, rax
    jle drop
    add rbp, rax
    jmp 2b
3:  mov [r15 + NREAD], rbp
    lea rax, [rdi + 4]
    mov [r15 + HEND], rax
    mov [r15 + NEXT], rax        # where the next request starts, bar a body
    lea r9, [rbx + rdi + 2]      # the head's end, past its last line's CRLF
    # HTTP/1.1 keeps the connection, unless it says close; 1.0 does not.
    mov qword ptr [r15 + KEEP], 1
    mov rsi, rbx
31: cmp byte ptr [rsi], 13
    je 32f
    inc rsi
    jmp 31b
32: cmp word ptr [rsi - 2], 0x312e     # ".1"
    je 33f
    mov qword ptr [r15 + KEEP], 0
33:
    # Content-Length and Connection: header lines, any case.
    mov qword ptr [r15 + CLEN], 0
    mov rsi, rbx
4:  cmp rsi, r9                  # the next line
    jae 6f
    cmp byte ptr [rsi], 10
    lea rsi, [rsi + 1]
    jne 4b
    lea rax, [rsi + 11]
    cmp rax, r9
    ja 6f
    mov rax, 0x2020202020202020
    mov rdx, [rsi]
    or rdx, rax
    mov rcx, 0x697463656e6e6f63  # "connecti"
    cmp rdx, rcx
    jne 41f
    mov edx, [rsi + 7]
    or edx, 0x20202020
    cmp edx, 0x3a6e6f69          # "ion:"
    jne 41f
    add rsi, 11
42: lea rax, [rsi + 5]           # "close" in its value
    cmp rax, r9
    ja 4b
    cmp byte ptr [rsi], 13
    je 4b
    mov edx, [rsi]
    or edx, 0x20202020
    inc rsi
    cmp edx, 0x736f6c63          # "clos"
    jne 42b
    movzx edx, byte ptr [rsi + 3]
    or edx, 0x20
    cmp edx, 'e'
    jne 42b
    mov qword ptr [r15 + KEEP], 0
    jmp 4b
41: lea rax, [rsi + 15]
    cmp rax, r9
    ja 4b
    mov rax, 0x2020202020202020
    mov rdx, [rsi]
    or rdx, rax
    mov rcx, 0x2d746e65746e6f63  # "content-"
    cmp rdx, rcx
    jne 4b
    mov rdx, [rsi + 7]
    or rdx, rax
    mov rcx, 0x3a6874676e656c2d  # "-length:"
    cmp rdx, rcx
    jne 4b
    add rsi, 15
5:  movzx eax, byte ptr [rsi]
    cmp eax, ' '
    je 51f
    cmp eax, 9
    jne 52f
51: inc rsi
    jmp 5b
52: xor eax, eax
53: movzx ecx, byte ptr [rsi]
    sub ecx, '0'
    cmp ecx, 9
    ja 54f
    imul rax, rax, 10
    add rax, rcx
    inc rsi
    bt rax, 40                   # far over any body taken
    jnc 53b
54: mov [r15 + CLEN], rax
    jmp 4b
6:  # The method and path (to a space; a query is not the path).
    mov eax, [rbx]
    xor r8d, r8d
    cmp eax, 0x20544547          # "GET "
    jne 61f
    mov r8d, 1
61: cmp eax, 0x20545550          # "PUT "
    jne 62f
    mov r8d, 2
62: cmp r8d, 2                   # a body it does not read ends the connection
    je 63f
    cmp qword ptr [r15 + CLEN], 0
    je 63f
    mov qword ptr [r15 + KEEP], 0
63: test r8d, r8d
    jz not_found
    lea rsi, [rbx + 4]
    mov rdi, rsi
7:  cmp rdi, r9
    jae not_found
    movzx eax, byte ptr [rdi]
    cmp eax, ' '
    je 8f
    cmp eax, '?'
    je 8f
    cmp eax, 13
    je 8f
    inc rdi
    jmp 7b
8:  mov rcx, rdi
    sub rcx, rsi                 # rsi, rcx: the path
    cmp rcx, 7
    jne 81f
    cmp dword ptr [rsi], 0x6165682f  # "/hea"
    jne 82f
    cmp dword ptr [rsi + 3], 0x68746c61 # "alth"
    jne 82f
    cmp r8d, 1
    jne not_found
    lea rax, [rip + s_health]
    mov [r15 + IOV + 16], rax
    mov qword ptr [r15 + IOV + 24], 15
    mov edx, 2
    call answer_ok
    jmp after
81: cmp rcx, 9
    jne 82f
    mov rax, 0x776f64747568732f  # "/shutdow"
    cmp [rsi], rax
    jne 82f
    cmp byte ptr [rsi + 8], 'n'
    jne 82f
    cmp r8d, 1
    jne not_found
    lea rax, [rip + s_bye]
    mov [r15 + IOV + 16], rax
    mov qword ptr [r15 + IOV + 24], 12
    mov qword ptr [r15 + KEEP], 0
    mov edx, 2
    call answer_ok
    xor edi, edi
    mov eax, 231                 # exit_group
    syscall
82: cmp rcx, 5                   # /kv/<key>: one segment, not empty
    jb not_found
    cmp dword ptr [rsi], 0x2f766b2f  # "/kv/"
    jne not_found
    add rsi, 4
    sub rcx, 4
    mov rdi, rsi
    mov rdx, rcx
    mov al, '/'
    xchg rdi, rsi
    push rcx
    repne scasb
    pop rcx
    xchg rdi, rsi
    je not_found
    mov rsi, rdi                 # rsi, rcx: the key
    mov [r15 + KEYLEN], rcx
    cmp r8d, 2
    je put

    # GET /kv/<key>.
    call find
    cmp qword ptr [rax], 0
    je not_found
    mov rsi, [r15 + IOV + 32]    # the key, as find left it
    lea rdx, [rip + s_key]
    mov [r15 + IOV + 16], rdx
    mov qword ptr [r15 + IOV + 24], 8
    lea rdx, [rip + s_value]
    mov [r15 + IOV + 48], rdx
    mov qword ptr [r15 + IOV + 56], 11
    mov rdx, [rax + 16]
    mov [r15 + IOV + 64], rdx
    mov rdx, [rax + 24]
    mov [r15 + IOV + 72], rdx
    lea rdx, [rip + s_end]
    mov [r15 + IOV + 80], rdx
    mov qword ptr [r15 + IOV + 88], 2
    mov edx, 6
    call answer_ok
    jmp after

    # PUT /kv/<key>: the key, then the body, into the arena.
put:
    mov rdi, r12
    rep movsb
    mov r12, rdi                 # the value's start
    mov rcx, [r15 + NREAD]       # the body bytes already read
    sub rcx, [r15 + HEND]
    mov rdx, [r15 + CLEN]
    cmp rdx, BODYMAX
    ja 5f
    cmp rcx, rdx
    cmova rcx, rdx
    mov rsi, [r15 + HEND]
    add rsi, rbx
    rep movsb
    mov rax, rsi                 # the next request after the body
    sub rax, rbx
    mov [r15 + NEXT], rax
    mov rbp, rdi                 # the value's end so far
1:  mov rdx, [r15 + CLEN]
    add rdx, r12
    sub rdx, rbp
    jz 2f
    mov edi, r14d
    mov rsi, rbp
    xor eax, eax                 # read
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    jle 60f                      # cut short: no text
    add rbp, rax
    jmp 1b
2:  mov rsi, r12
    mov rdx, rbp
    sub rdx, r12
    call utf8
    jc 6f
    jmp 7f
5:  # Over the limit: the rest of the body drained, no text.
    mov rax, [r15 + NREAD]
    mov [r15 + NEXT], rax
    mov rdx, [r15 + CLEN]
    sub rdx, rcx
51: test rdx, rdx
    jz 6f
    mov rbp, rdx
    cmp rdx, REQSZ
    jbe 52f
    mov edx, REQSZ
52: mov edi, r14d
    mov rsi, rbx
    xor eax, eax
    syscall
    cmp rax, -4
    je 52b
    test rax, rax
    jle 60f
    mov rdx, rbp
    sub rdx, rax
    jmp 51b
60: mov qword ptr [r15 + KEEP], 0   # the connection is done
6:  mov rbp, r12                 # ""
7:  # The table's slot for the key (just before the value in the arena).
    mov rsi, r12
    mov rcx, [r15 + KEYLEN]
    sub rsi, rcx
    call find
    mov rcx, [r15 + KEYLEN]
    cmp qword ptr [rax], 0
    jne 8f
    mov rsi, r12
    sub rsi, rcx
    mov [rax], rsi
    mov [rax + 8], rcx
8:  mov [rax + 16], r12
    mov rdx, rbp
    sub rdx, r12
    mov [rax + 24], rdx
    mov r12, rbp
    lea rdx, [rip + s_stored]
    mov [r15 + IOV + 16], rdx
    mov qword ptr [r15 + IOV + 24], 11
    mov rdx, [rax]               # the key
    mov [r15 + IOV + 32], rdx
    mov [r15 + IOV + 40], rcx
    lea rdx, [rip + s_end]
    mov [r15 + IOV + 48], rdx
    mov qword ptr [r15 + IOV + 56], 2
    mov edx, 4
    call answer_ok
    jmp after

not_found:
    cmp qword ptr [r15 + CLEN], 0    # a body not read ends the connection
    je 1f
    mov rax, [r15 + NEXT]
    cmp rax, [r15 + HEND]
    jne 1f
    mov qword ptr [r15 + KEEP], 0
1:
    lea rax, [rip + s_notfound]
    mov [r15 + IOV + 16], rax
    mov qword ptr [r15 + IOV + 24], 21
    lea rsi, [rip + s_404]
    mov ecx, 72
    mov edx, 2
    call answer
    jmp after

drop:
    mov edi, r14d
    mov eax, 3                   # close
    syscall
    jmp serve

# after: an answered request: the connection closed, or kept with what
# came after the request moved to the buffer's start and the next head read.
after:
    cmp qword ptr [r15 + KEEP], 0
    je drop
    mov rsi, [r15 + NEXT]
    mov rbp, [r15 + NREAD]
    sub rbp, rsi
    mov rcx, rbp
    add rsi, rbx
    mov rdi, rbx
    rep movsb
    mov qword ptr [r15 + KEPT], 1
    jmp head

# find: the slot of the key rsi, rcx in rax: its own, or the empty one it
# would take. Leaves the key as IOV[2] (for the answers that echo it).
find:
    mov [r15 + IOV + 32], rsi
    mov [r15 + IOV + 40], rcx
    mov rax, 0xcbf29ce484222325  # FNV-1a
    mov r8, 0x100000001b3
    xor edx, edx
1:  cmp rdx, rcx
    jae 2f
    movzx edi, byte ptr [rsi + rdx]
    xor rax, rdi
    imul rax, r8
    inc rdx
    jmp 1b
2:  mov r8, rax
    shr r8, 32
    xor r8, rax
    and r8d, NSLOTS - 1
3:  mov rax, r8
    shl rax, 5
    lea rax, [rbx + rax + TABLE]
    mov rdi, [rax]
    test rdi, rdi
    jz 5f
    cmp rcx, [rax + 8]
    jne 4f
    push rsi
    push rcx
    repe cmpsb
    pop rcx
    pop rsi
    je 5f
4:  inc r8d
    and r8d, NSLOTS - 1
    jmp 3b
5:  ret

# answer_ok, answer: the status line rsi, rcx (answer_ok: 200), the headers
# with the Content-Length of IOV[1 .. rdx), then one writev of them all;
# the connection closed.
answer_ok:
    lea rsi, [rip + s_200]
    mov ecx, 65
answer:
    lea rdi, [r15 + HEAD]
    rep movsb
    xor eax, eax                 # the body's length
    mov ecx, 1
1:  mov r8, rcx
    shl r8, 4
    add rax, [r15 + IOV + r8 + 8]
    inc ecx
    cmp ecx, edx
    jb 1b
    push rdx
    lea r8, [rsp - 8]            # its digits, in the red zone
    mov r9, r8
    mov ecx, 10
2:  xor edx, edx
    div rcx
    add dl, '0'
    dec r8
    mov [r8], dl
    test rax, rax
    jnz 2b
3:  mov dl, [r8]
    mov [rdi], dl
    inc rdi
    inc r8
    cmp r8, r9
    jb 3b
    lea rsi, [rip + s_tail]      # "Connection: close" only when it is
    mov ecx, 23
    cmp qword ptr [r15 + KEEP], 0
    je 31f
    add rsi, 19                  # the bare "\r\n\r\n"
    mov ecx, 4
31: rep movsb
    pop rdx
    lea rsi, [r15 + HEAD]
    mov [r15 + IOV], rsi
    sub rdi, rsi
    mov [r15 + IOV + 8], rdi
    lea rsi, [r15 + IOV]
4:  mov edi, r14d
    mov eax, 20                  # writev
    syscall
    cmp rax, -4
    je 4b
    test rax, rax
    jle 7f
5:  cmp rax, [rsi + 8]           # past the pieces it wrote whole
    jb 6f
    sub rax, [rsi + 8]
    add rsi, 16
    dec edx
    jnz 5b
    ret
6:  add [rsi], rax
    sub [rsi + 8], rax
    jmp 4b
7:  mov qword ptr [r15 + KEEP], 0   # failed: the connection is done
    ret

# utf8: CF clear when the rdx bytes at rsi are UTF-8.
utf8:
    mov rdi, rsi
    lea r9, [rsi + rdx]
1:  cmp rdi, r9
    jae 9f
    movzx eax, byte ptr [rdi]
    inc rdi
    cmp eax, 0x80
    jb 1b
    mov r8d, 0x80                # the second byte's bounds
    mov r10d, 0xbf
    cmp eax, 0xc2
    jb 8f
    mov ecx, 1
    cmp eax, 0xe0
    jb 3f
    mov ecx, 2
    cmp eax, 0xf0
    jae 2f
    cmp eax, 0xe0
    jne 11f
    mov r8d, 0xa0
11: cmp eax, 0xed
    jne 3f
    mov r10d, 0x9f
    jmp 3f
2:  mov ecx, 3
    cmp eax, 0xf4
    ja 8f
    cmp eax, 0xf0
    jne 21f
    mov r8d, 0x90
21: cmp eax, 0xf4
    jne 3f
    mov r10d, 0x8f
3:  cmp rdi, r9
    jae 8f
    movzx eax, byte ptr [rdi]
    inc rdi
    cmp eax, r8d
    jb 8f
    cmp eax, r10d
    ja 8f
    mov r8d, 0x80
    mov r10d, 0xbf
    dec ecx
    jnz 3b
    jmp 1b
8:  stc
    ret
9:  clc
    ret

fail_listen:
    lea rsi, [rip + s_listen]
    mov edx, 14
    mov edi, 2
    mov eax, 1
    syscall
    mov edi, 1
    mov eax, 231
    syscall

s_200:      .ascii "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: "
s_404:      .ascii "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: "
s_tail:     .ascii "\r\nConnection: close\r\n\r\n"
s_health:   .ascii "{\"status\":\"ok\"}"
s_bye:      .ascii "{\"bye\":true}"
s_notfound: .ascii "{\"error\":\"not found\"}"
s_stored:   .ascii "{\"stored\":\""
s_key:      .ascii "{\"key\":\""
s_value:    .ascii "\",\"value\":\""
s_end:      .ascii "\"}"
s_listen:   .ascii "listen failed\n"
