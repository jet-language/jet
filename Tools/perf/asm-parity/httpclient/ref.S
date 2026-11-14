# httpclient (gauntlet netserv.client): against the base URL argv[1]
# (http://<IPv4 or localhost>[:port][path]), GET <base>/items and print
# "items <count>"; then GET <base>/items/2, /5 and /99 and print "item <id>
# <name> qty=<qty>" for each, or "item <id> missing" for a 404; then
# "total-qty <sum of the listing's qty>".
#
# Technique: the four requests are independent, so all four connections
# are opened and their requests written before any response is read; each
# response is read to its end (the peer closes) into its own buffer when
# its line is next, and each line is written as soon as it is known, the
# listing's while the items are still in flight.
# A response is its status line's code, then the body after the blank line,
# decoded by a recursive-descent JSON reader that takes the fields it needs
# (keys in any order, unknown members skipped) without building values.
# A connection or request that fails prints "GET failed", and a body that
# is not the expected JSON (a string with escapes included) "decode
# failed", each on stderr with exit 1, as the Jet program's panics do.
    .intel_syntax noprefix
    .text

    .set SA, 0                   # struct sockaddr_in (16 bytes)
    .set HOST, 16                # the authority, for Host
    .set HOSTLEN, 24
    .set PREFIX, 32              # the base path
    .set PREFIXLEN, 40
    .set BUFS, 48
    .set FDS, 56                 # four descriptors (32 bytes)
    .set LENS, 88                # four response lengths (32 bytes)
    .set COUNT, 120
    .set TOTAL, 128
    .set ID, 136
    .set NAME, 144
    .set NAMELEN, 152
    .set QTY, 160
    .set SEEN, 168
    .set REQ, 256                # one request (1024 bytes)
    .set OUT, 1280               # the output (512 bytes)
    .set FRAME, 2048
    .set RBUF, 1 << 20           # one response's room

_start:
    mov rax, [rsp]
    cmp rax, 2
    jb fail_get
    mov rsi, [rsp + 16]          # argv[1]
    sub rsp, FRAME
    mov r15, rsp
    # The URL: "http://", the authority, then the base path.
    cmp dword ptr [rsi], 0x70747468  # "http"
    jne fail_get
    cmp dword ptr [rsi + 3], 0x2f2f3a70 # "p://"
    jne fail_get
    add rsi, 7
    mov [r15 + HOST], rsi
    mov rdi, rsi
1:  movzx eax, byte ptr [rdi]
    test eax, eax
    jz 2f
    cmp eax, '/'
    je 2f
    inc rdi
    jmp 1b
2:  mov rax, rdi
    sub rax, rsi
    mov [r15 + HOSTLEN], rax
    mov [r15 + PREFIX], rdi
    mov rcx, rdi
3:  cmp byte ptr [rcx], 0
    je 4f
    inc rcx
    jmp 3b
4:  sub rcx, rdi
    mov [r15 + PREFIXLEN], rcx
    # The host: "localhost" or a dotted IPv4; then ":port", 80 without.
    mov word ptr [r15 + SA], 2   # AF_INET
    mov qword ptr [r15 + SA + 8], 0
    cmp dword ptr [rsi], 0x61636f6c  # "loca"
    jne 5f
    cmp dword ptr [rsi + 4], 0x736f686c # "lhos"
    jne 5f
    cmp byte ptr [rsi + 8], 't'
    jne 5f
    mov dword ptr [r15 + SA + 4], 0x0100007f
    add rsi, 9
    jmp 7f
5:  xor ebx, ebx                 # the address, built a byte at a time
    xor r8d, r8d                 # bytes so far
6:  call decimal
    cmp eax, 255
    ja fail_get
    shl ebx, 8
    or ebx, eax
    inc r8d
    cmp r8d, 4
    je 61f
    cmp byte ptr [rsi], '.'
    jne fail_get
    inc rsi
    jmp 6b
61: bswap ebx
    mov [r15 + SA + 4], ebx
7:  mov eax, 80
    cmp byte ptr [rsi], ':'
    jne 8f
    inc rsi
    call decimal
    cmp eax, 65535
    ja fail_get
8:  cmp rsi, [r15 + PREFIX]
    jne fail_get
    xchg al, ah
    mov [r15 + SA + 2], ax
    # The response buffers, lazily touched; huge pages off so a buffer
    # costs the pages its response fills, not 2 MiB.
    mov edi, 41                  # prctl(PR_SET_THP_DISABLE, 1)
    mov esi, 1
    xor edx, edx
    xor r10d, r10d
    xor r8d, r8d
    mov eax, 157
    syscall
    xor edi, edi
    mov esi, RBUF * 4
    mov edx, 3
    mov r10d, 0x4022             # MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE
    mov r8, -1
    xor r9d, r9d
    mov eax, 9                   # mmap
    syscall
    cmp rax, -4096
    ja fail_get
    mov [r15 + BUFS], rax

    # Every request out first: /items, then /items/2, /5, /99.
    xor r12d, r12d
send:
    lea rax, [rip + ids]
    mov eax, [rax + r12 * 4]
    call request
    mov r13, rdi                 # the request's end
    mov edi, 2                   # AF_INET
    mov esi, 0x80001             # SOCK_STREAM | SOCK_CLOEXEC
    xor edx, edx
    mov eax, 41                  # socket
    syscall
    test eax, eax
    js fail_get
    mov [r15 + FDS + r12 * 8], rax
    mov edi, eax
    lea rsi, [r15 + SA]
    mov edx, 16
    mov eax, 42                  # connect
    syscall
    test rax, rax
    js fail_get
    mov rdx, r13
    lea rsi, [r15 + REQ]
    sub rdx, rsi
1:  mov rdi, [r15 + FDS + r12 * 8]
    mov eax, 1                   # write
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    js fail_get
    add rsi, rax
    sub rdx, rax
    jnz 1b
    inc r12
    cmp r12, 4
    jb send
    # Then each response, read when its line is next.
    xor r12d, r12d
    call recv

    # The listing.
    lea r14, [r15 + OUT]
    xor r12d, r12d
    call response
    call ws
    call open_object
    mov qword ptr [r15 + SEEN], 0
    mov qword ptr [r15 + COUNT], 0
    mov qword ptr [r15 + TOTAL], 0
1:  call member                  # rdi, rcx: the key
    jc 5f
    cmp rcx, 5
    jne 4f
    cmp dword ptr [rdi], 0x6d657469  # "item"
    jne 4f
    cmp byte ptr [rdi + 4], 's'
    jne 4f
    mov qword ptr [r15 + SEEN], 1
    call ws                      # the array
    cmp rsi, r9
    jae fail_decode
    cmp byte ptr [rsi], '['
    jne fail_decode
    inc rsi
    call ws
    cmp rsi, r9
    jae fail_decode
    cmp byte ptr [rsi], ']'
    je 3f
2:  call item
    inc qword ptr [r15 + COUNT]
    mov rax, [r15 + QTY]
    add [r15 + TOTAL], rax
    call ws
    cmp rsi, r9
    jae fail_decode
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp eax, ','
    je 21f
    cmp eax, ']'
    je 31f
    jmp fail_decode
21: call ws
    jmp 2b
3:  inc rsi
31: call after_member
    jmp 1b
4:  call skip_value
    call after_member
    jmp 1b
5:  call end_value
    cmp qword ptr [r15 + SEEN], 0
    je fail_decode
    mov rdi, r14
    mov dword ptr [rdi], 0x6d657469  # "item"
    mov word ptr [rdi + 4], 0x2073   # "s "
    add rdi, 6
    mov rax, [r15 + COUNT]
    call put_uint
    mov byte ptr [rdi], 10
    lea r14, [rdi + 1]
    call flush                   # the first line out before the items

    # The three items.
    mov r12d, 1
item_line:
    call recv
    call response
    mov rdi, r14
    mov dword ptr [rdi], 0x6d657469  # "item"
    mov byte ptr [rdi + 4], ' '
    add rdi, 5
    cmp eax, 404
    jne 1f
    lea rax, [rip + ids]
    mov eax, [rax + r12 * 4]
    call put_uint
    lea rsi, [rip + s_missing]
    mov ecx, 8
    rep movsb
    jmp 2f
1:  push rdi
    call ws
    call item
    call end_value
    pop rdi
    mov rax, [r15 + ID]
    call put_int
    mov byte ptr [rdi], ' '
    inc rdi
    mov rsi, [r15 + NAME]
    mov rcx, [r15 + NAMELEN]
    rep movsb
    mov dword ptr [rdi], 0x79747120  # " qty"
    mov byte ptr [rdi + 4], '='
    add rdi, 5
    mov rax, [r15 + QTY]
    call put_int
2:  mov byte ptr [rdi], 10
    lea r14, [rdi + 1]
    inc r12
    cmp r12, 4
    jb item_line

    mov rdi, r14
    lea rsi, [rip + s_total]
    mov ecx, 10
    rep movsb
    mov rax, [r15 + TOTAL]
    call put_int
    mov byte ptr [rdi], 10
    inc rdi
    mov r14, rdi
    call flush
    xor edi, edi
    mov eax, 231                 # exit_group
    syscall

# recv: response r12 read to its end into its buffer, its length to LENS,
# its socket closed. Clobbers rax, rcx, rdx, rsi, rdi, r11, rbx, r13.
recv:
    mov rbx, r12
    shl rbx, 20
    add rbx, [r15 + BUFS]
    xor r13d, r13d
1:  mov rdi, [r15 + FDS + r12 * 8]
    lea rsi, [rbx + r13]
    mov edx, RBUF
    sub rdx, r13
    jz fail_get
    xor eax, eax                 # read
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    js fail_get
    jz 2f
    add r13, rax
    jmp 1b
2:  mov [r15 + LENS + r12 * 8], r13
    mov rdi, [r15 + FDS + r12 * 8]
    mov eax, 3                   # close
    syscall
    ret

# flush: the output from OUT to r14 written; r14 back at OUT.
flush:
    lea rsi, [r15 + OUT]
    mov rdx, r14
    sub rdx, rsi
1:  mov edi, 1
    mov eax, 1                   # write
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    js fail_get
    add rsi, rax
    sub rdx, rax
    jnz 1b
    lea r14, [r15 + OUT]
    ret

# request: "GET <prefix>/items[/<eax>] HTTP/1.1" with Host and Connection
# close at REQ (eax 0: the listing); rdi its end.
request:
    lea rdi, [r15 + REQ]
    mov dword ptr [rdi], 0x20544547  # "GET "
    add rdi, 4
    mov rsi, [r15 + PREFIX]
    mov rcx, [r15 + PREFIXLEN]
    cmp rcx, 512
    ja fail_get
    rep movsb
    mov dword ptr [rdi], 0x6574692f  # "/ite"
    mov word ptr [rdi + 4], 0x736d   # "ms"
    add rdi, 6
    test eax, eax
    jz 1f
    mov byte ptr [rdi], '/'
    inc rdi
    call put_uint
1:  lea rsi, [rip + s_http]
    mov ecx, 17
    rep movsb
    mov rsi, [r15 + HOST]
    mov rcx, [r15 + HOSTLEN]
    cmp rcx, 256
    ja fail_get
    rep movsb
    lea rsi, [rip + s_close]
    mov ecx, 23
    rep movsb
    ret

# response: response r12's status in eax, rsi and r9 its body.
response:
    mov rsi, r12
    shl rsi, 20
    add rsi, [r15 + BUFS]
    mov r9, [r15 + LENS + r12 * 8]
    add r9, rsi
    lea rax, [rsi + 12]
    cmp rax, r9
    ja fail_get
    cmp dword ptr [rsi], 0x50545448  # "HTTP"
    jne fail_get
    xor eax, eax                 # the code
    mov ecx, 9
1:  movzx edx, byte ptr [rsi + rcx]
    sub edx, '0'
    cmp edx, 9
    ja fail_get
    imul eax, eax, 10
    add eax, edx
    inc ecx
    cmp ecx, 12
    jb 1b
2:  lea rdx, [rsi + 3]           # the blank line
    cmp rdx, r9
    jae fail_get
    cmp dword ptr [rsi], 0x0a0d0a0d
    je 3f
    inc rsi
    jmp 2b
3:  add rsi, 4
    ret

# The JSON reader: rsi the cursor, r9 the end; fail_decode on anything else.

# ws: past whitespace.
ws:
    cmp rsi, r9
    jae 1f
    movzx eax, byte ptr [rsi]
    cmp eax, ' '
    je 2f
    cmp eax, 9
    je 2f
    cmp eax, 10
    je 2f
    cmp eax, 13
    jne 1f
2:  inc rsi
    jmp ws
1:  ret

# end_value: only whitespace left.
end_value:
    call ws
    cmp rsi, r9
    jne fail_decode
    ret

# open_object: past "{" and whitespace.
open_object:
    cmp rsi, r9
    jae fail_decode
    cmp byte ptr [rsi], '{'
    jne fail_decode
    inc rsi
    jmp ws

# member: the next key in rdi, rcx and the cursor past its ":", or CF when
# the object ends (cursor past its "}").
member:
    call ws
    cmp rsi, r9
    jae fail_decode
    cmp byte ptr [rsi], '}'
    jne 1f
    inc rsi
    stc
    ret
1:  call string
    call ws
    cmp rsi, r9
    jae fail_decode
    cmp byte ptr [rsi], ':'
    jne fail_decode
    inc rsi
    call ws
    clc
    ret

# after_member: past a "," (then whitespace) or up to the "}".
after_member:
    call ws
    cmp rsi, r9
    jae fail_decode
    cmp byte ptr [rsi], ','
    jne 1f
    inc rsi
    call ws
    cmp rsi, r9
    jae fail_decode
    cmp byte ptr [rsi], '}'
    je fail_decode
    ret
1:  cmp byte ptr [rsi], '}'
    jne fail_decode
    ret

# string: a string without escapes; rdi, rcx its bytes.
string:
    cmp rsi, r9
    jae fail_decode
    cmp byte ptr [rsi], '"'
    jne fail_decode
    inc rsi
    mov rdi, rsi
1:  cmp rsi, r9
    jae fail_decode
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp eax, '"'
    je 2f
    cmp eax, '\\'
    je fail_decode
    cmp eax, 0x20
    jb fail_decode
    jmp 1b
2:  lea rcx, [rsi - 1]
    sub rcx, rdi
    ret

# integer: an integer in rax.
integer:
    xor r8d, r8d
    cmp rsi, r9
    jae fail_decode
    cmp byte ptr [rsi], '-'
    jne 1f
    mov r8d, 1
    inc rsi
1:  xor eax, eax
    xor ecx, ecx                 # digits
2:  cmp rsi, r9
    jae 3f
    movzx edx, byte ptr [rsi]
    sub edx, '0'
    cmp edx, 9
    ja 3f
    imul rax, rax, 10
    jo fail_decode
    add rax, rdx
    jo fail_decode
    inc rsi
    inc ecx
    jmp 2b
3:  test ecx, ecx
    jz fail_decode
    cmp rsi, r9                  # a fraction or exponent is not an Int
    jae 4f
    movzx edx, byte ptr [rsi]
    cmp edx, '.'
    je fail_decode
    or edx, 0x20
    cmp edx, 'e'
    je fail_decode
4:  test r8d, r8d
    jz 5f
    neg rax
5:  ret

# item: an {id, name, qty} object into ID, NAME, QTY.
item:
    call open_object
    xor r10d, r10d               # fields seen, one bit each
1:  call member
    jc 9f
    cmp rcx, 2
    jne 2f
    cmp word ptr [rdi], 0x6469   # "id"
    jne 5f
    call integer
    mov [r15 + ID], rax
    or r10d, 1
    jmp 6f
2:  cmp rcx, 3
    jne 3f
    cmp word ptr [rdi], 0x7471   # "qt"
    jne 5f
    cmp byte ptr [rdi + 2], 'y'
    jne 5f
    call integer
    mov [r15 + QTY], rax
    or r10d, 4
    jmp 6f
3:  cmp rcx, 4
    jne 5f
    cmp dword ptr [rdi], 0x656d616e  # "name"
    jne 5f
    call string
    mov [r15 + NAME], rdi
    mov [r15 + NAMELEN], rcx
    or r10d, 2
    jmp 6f
5:  call skip_value
6:  call after_member
    jmp 1b
9:  cmp r10d, 7
    jne fail_decode
    ret

# skip_value: past one value of any kind.
skip_value:
    cmp rsi, r9
    jae fail_decode
    movzx eax, byte ptr [rsi]
    cmp eax, '"'
    je string
    cmp eax, '{'
    je 2f
    cmp eax, '['
    je 4f
    cmp eax, 't'
    je 7f
    cmp eax, 'f'
    je 7f
    cmp eax, 'n'
    je 7f
    # a number: its characters
1:  inc rsi
    cmp rsi, r9
    jae 8f
    movzx eax, byte ptr [rsi]
    cmp eax, '0'
    jb 11f
    cmp eax, '9'
    jbe 1b
11: cmp eax, '.'
    je 1b
    cmp eax, '-'
    je 1b
    cmp eax, '+'
    je 1b
    or eax, 0x20
    cmp eax, 'e'
    je 1b
    ret
2:  call open_object
3:  call member
    jc 8f
    call skip_value
    call after_member
    jmp 3b
4:  inc rsi
    call ws
    cmp rsi, r9
    jae fail_decode
    cmp byte ptr [rsi], ']'
    je 6f
5:  call skip_value
    call ws
    cmp rsi, r9
    jae fail_decode
    movzx eax, byte ptr [rsi]
    inc rsi
    cmp eax, ']'
    je 8f
    cmp eax, ','
    jne fail_decode
    call ws
    jmp 5b
6:  inc rsi
    ret
7:  inc rsi                      # true, false, null: their letters
    cmp rsi, r9
    jae 8f
    movzx eax, byte ptr [rsi]
    sub eax, 'a'
    cmp eax, 25
    jbe 7b
8:  ret

# decimal: the digits at rsi in eax (at least one, at most 9).
decimal:
    xor eax, eax
    xor ecx, ecx
1:  movzx edx, byte ptr [rsi]
    sub edx, '0'
    cmp edx, 9
    ja 2f
    imul eax, eax, 10
    add eax, edx
    inc rsi
    inc ecx
    cmp ecx, 9
    ja fail_get
    jmp 1b
2:  test ecx, ecx
    jz fail_get
    ret

# put_int: the signed rax in decimal at rdi. put_uint: unsigned eax/rax.
put_int:
    test rax, rax
    jns put_uint
    mov byte ptr [rdi], '-'
    inc rdi
    neg rax
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

fail_get:
    lea rsi, [rip + s_get]
    mov edx, 11
    jmp fail
fail_decode:
    lea rsi, [rip + s_decode]
    mov edx, 14
fail:
    mov edi, 2
    mov eax, 1                   # write
    syscall
    mov edi, 1
    mov eax, 231
    syscall

    .p2align 2
ids:       .long 0, 2, 5, 99
s_http:    .ascii " HTTP/1.1\r\nHost: "
s_close:   .ascii "\r\nConnection: close\r\n\r\n"
s_missing: .ascii " missing"
s_total:   .ascii "total-qty "
s_get:     .ascii "GET failed\n"
s_decode:  .ascii "decode failed\n"
