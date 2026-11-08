# ScriptBench w11 (Tools/perf/scriptbench/w11-fetch/run.jet) by hand: fetch
# http://127.0.0.1:18411/item/<i> for i in 0..<100, at most 32 at a time,
# from the fixture server (server.js, 20 ms per answer), and print
# "ok=<n>\n", n counting the answers with status 200 and a nonempty body.
# One thread, one epoll set, 32 nonblocking TCP_NODELAY sockets connected
# up front, edge-triggered. Each socket carries one HTTP/1.1 GET at a time
# over a kept-alive connection: an answer is whole once its content-length
# bytes follow the header, and the next id's GET goes out on the same socket
# in the same pass that read the answer. SIGPIPE is ignored, EINTR retried;
# any other failure exits 1. The harness assembles this one .text blob
# behind an ELF header, one R+X PT_LOAD and PT_GNU_STACK; every buffer is on
# the stack.
#
# Timing builds only (as --defsym TRACE=1): each GET sent, each answer's
# first byte and each answer completed is logged with its CLOCK_MONOTONIC
# time and id, and the log goes to stderr at exit as 16-byte records
# {ns u64, kind u32, id u32}: kind 0 started, 1 sent, 2 first byte,
# 3 complete, 4 finished.
    .intel_syntax noprefix
    .set BLOB_BASE, 0x400000 + 64 + 2 * 56
    .set SLOTS, 32
    .set TOTAL, 100
    .set SLOT_BYTES, 4096        # a slot: fd0 length8 id16 index24 phase32 scratch40..63 buffer64
    .set BUF_OFF, 64
    .set BUF_CAP, SLOT_BYTES - BUF_OFF
    .set EVENTS, SLOTS * SLOT_BYTES   # 32 12-byte epoll_events, then end400 cursor408 saved416, output424, log cursor464
    .ifdef TRACE
    .set LOG_BYTES, 16 * 512
    .else
    .set LOG_BYTES, 0
    .endif

    # One log record; r8 names the slot when `slot` is 1.
    .macro LOG kind, slot
    .ifdef TRACE
    push rax
    push rcx
    push rdx
    push rsi
    push rdi
    push r11
    sub rsp, 16
    mov edi, 1                   # clock_gettime(CLOCK_MONOTONIC, &ts)
    mov rsi, rsp
    mov eax, 228
    syscall
    mov rax, [rsp]
    imul rax, rax, 1000000000
    add rax, [rsp + 8]
    add rsp, 16
    mov rdi, [rbp + 464]
    mov [rdi], rax
    mov dword ptr [rdi + 8], \kind
    .if \slot
    mov rax, [r8 + 16]
    .else
    mov rax, -1
    .endif
    mov [rdi + 12], eax
    add qword ptr [rbp + 464], 16
    pop r11
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rax
    .endif
    .endm

    .text
# Registers for the whole run: rbx the slots, rbp the event area, r12 the
# epoll fd, r13 the next id, r14 the finished requests, r15 the good ones.
_start:
    # rt_sigaction(SIGPIPE, &{SIG_IGN}, NULL, 8)
    push 0
    push 0
    push 0
    push 1
    mov rsi, rsp
    xor edx, edx
    mov r10d, 8
    mov edi, 13
    mov eax, 13
    syscall
    sub rsp, EVENTS + 512 + LOG_BYTES
    mov rbx, rsp
    lea rbp, [rsp + EVENTS]
    .ifdef TRACE
    lea rax, [rbp + 512]
    mov [rbp + 464], rax
    .endif
    LOG 0, 0
    xor edi, edi                 # epoll_create1(0)
    mov eax, 291
    syscall
    test rax, rax
    js fail
    mov r12, rax
    xor r13d, r13d
    xor r14d, r14d
    xor r15d, r15d
    xor edi, edi
1:  push rdi
    call start
    pop rdi
    inc rdi
    cmp rdi, SLOTS
    jb 1b

event_loop:
    cmp r14, TOTAL
    jae finish
    mov rdi, r12                 # epoll_wait(epfd, events, SLOTS, -1)
    mov rsi, rbp
    mov edx, SLOTS
    mov r10, -1
    mov eax, 232
    syscall
    cmp rax, -4
    je event_loop
    test rax, rax
    js fail
    imul rax, rax, 12
    lea rcx, [rbp + rax]
    mov [rbp + 400], rcx
    mov [rbp + 408], rbp
next_event:
    mov rcx, [rbp + 408]
    cmp rcx, [rbp + 400]
    jae event_loop
    mov eax, [rcx]               # events
    mov r8, [rcx + 4]            # the slot
    add rcx, 12
    mov [rbp + 408], rcx
    test eax, 8                  # EPOLLERR: the connect failed
    jnz fail
    cmp qword ptr [r8 + 32], 0
    jne 2f
    test eax, 4                  # EPOLLOUT: connected
    jz next_event
    mov [rbp + 416], eax
    call send_request
    mov eax, [rbp + 416]
    test eax, 17                 # EPOLLIN | EPOLLHUP in the same edge
    jz next_event
2:  call drain
    jmp next_event

finish:
    LOG 4, 0
    lea rdi, [rbp + 424]
    mov byte ptr [rdi], 'o'
    mov byte ptr [rdi + 1], 'k'
    mov byte ptr [rdi + 2], '='
    add rdi, 3
    mov rax, r15
    lea r8, [rbp + 448]          # put_dec's scratch at rbp+488..511
    call put_dec
    mov byte ptr [rdi], 10
    inc rdi
    lea rsi, [rbp + 424]
    mov rdx, rdi
    sub rdx, rsi
    mov edi, 1
    call write_all
    .ifdef TRACE
    lea rsi, [rbp + 512]
    mov rdx, [rbp + 464]
    sub rdx, rsi
    mov edi, 2
    call write_all
    .endif
    xor edi, edi                 # exit_group(0); the sockets close with it
    mov eax, 231
    syscall

fail:
    mov edi, 1
    mov eax, 231
    syscall

# write_all: writes rdx bytes at rsi to fd edi, EINTR retried, short writes
# advanced, a failure ignored.
write_all:
    test rdx, rdx
    jz 4f
3:  mov eax, 1
    syscall
    cmp rax, -4
    je 3b
    test rax, rax
    jle 4f
    add rsi, rax
    sub rdx, rax
    jnz 3b
4:  ret

# start: rdi = a slot index. Opens a nonblocking TCP_NODELAY socket, starts
# its connect, adds it to the epoll set and sends the next id's GET at once
# when the connect already finished.
start:
    mov r8, rdi
    shl r8, 12
    add r8, rbx
    mov [r8 + 24], rdi
    mov [r8 + 16], r13
    inc r13
    mov qword ptr [r8 + 8], 0
    mov qword ptr [r8 + 32], 0
    mov edi, 2                   # socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0)
    mov esi, 0x80801
    xor edx, edx
    mov eax, 41
    syscall
    test rax, rax
    js fail
    mov [r8], rax
    mov dword ptr [r8 + 40], 1   # setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &1, 4)
    mov rdi, rax
    mov esi, 6
    mov edx, 1
    lea r10, [r8 + 40]
    mov r9d, 4
    xchg r8, r9
    mov eax, 54
    syscall
    xchg r8, r9
    test rax, rax
    jnz fail
    mov rdi, [r8]                # connect(fd, &addr, 16)
    lea rsi, [rip + addr]
    mov edx, 16
    mov eax, 42
    syscall
    cmp rax, -115                # EINPROGRESS
    je 5f
    test rax, rax
    jnz fail
5:  mov dword ptr [r8 + 40], 0x80000005   # EPOLLIN | EPOLLOUT | EPOLLET
    mov [r8 + 44], r8
    mov rdi, r12                 # epoll_ctl(epfd, EPOLL_CTL_ADD, fd, &event)
    mov esi, 1
    mov rdx, [r8]
    lea r10, [r8 + 40]
    mov eax, 233
    syscall
    test rax, rax
    jnz fail
    jmp send_request             # loopback often connects at once: try now

# send_request: r8 = a slot. Writes its id's GET in one write; a socket
# still connecting (ENOTCONN, EAGAIN) is left for its EPOLLOUT edge.
send_request:
    lea rdi, [r8 + BUF_OFF]
    lea rsi, [rip + req_head]
    mov ecx, offset req_head_len
    rep movsb
    mov rax, [r8 + 16]
    call put_dec
    lea rsi, [rip + req_tail]
    mov ecx, offset req_tail_len
    rep movsb
    lea rsi, [r8 + BUF_OFF]
    mov rdx, rdi
    sub rdx, rsi
6:  mov rdi, [r8]
    mov eax, 1
    syscall
    cmp rax, -4
    je 6b
    cmp rax, -107
    je 19f
    cmp rax, -11
    je 19f
    cmp rax, rdx
    jne fail
    mov qword ptr [r8 + 32], 1
    mov qword ptr [r8 + 8], 0
    LOG 1, 1
19: ret

# drain: r8 = a reading slot. Reads until the socket would block. Each whole
# answer is counted, and the slot's next id, if any, is sent at once.
drain:
    mov rdi, [r8]
    mov rdx, [r8 + 8]
    lea rsi, [r8 + rdx + BUF_OFF]
    neg rdx
    add rdx, BUF_CAP
    jz fail
    xor eax, eax                 # read(fd, buffer + length, room)
    syscall
    cmp rax, -4
    je drain
    cmp rax, -11                 # EAGAIN: the next edge resumes it
    je 7f
    test rax, rax
    jle fail                     # an error, or the server closed mid-answer
    .ifdef TRACE
    cmp qword ptr [r8 + 8], 0
    jne 8f
    LOG 2, 1
8:
    .endif
    add [r8 + 8], rax
    call answer_length
    test rax, rax
    jz drain
    cmp [r8 + 8], rax
    jb drain
    jne fail                     # bytes past the answer: none were asked for
    LOG 3, 1
    add r15, r9
    inc r14
    cmp r13, TOTAL
    jae drain
    mov [r8 + 16], r13
    inc r13
    call send_request
    jmp drain
7:  ret

# answer_length: r8 = a slot. rax = the whole answer's length once its
# header is in (0 before), r9 = 1 when it is a 200 with a nonempty body.
# Clobbers rcx, rdx, rsi, rdi, r10, r11.
answer_length:
    lea rsi, [r8 + BUF_OFF]
    mov rcx, [r8 + 8]
    xor eax, eax
    xor edx, edx                 # the header's end, at "\r\n\r\n"
    lea r10, [rcx - 4]
    cmp rcx, 4
    jb 13f
9:  cmp rdx, r10
    ja 13f
    cmp dword ptr [rsi + rdx], 0x0a0d0a0d
    je 10f
    inc rdx
    jmp 9b
10: mov r10, 0x2d746e65746e6f63  # "content-" ... "length: " within the header
    xor edi, edi
11: lea r11, [rdi + 16]
    cmp r11, rdx
    ja fail                      # no content-length
    cmp [rsi + rdi], r10
    jne 12f
    mov r11, 0x203a6874676e656c
    cmp [rsi + rdi + 8], r11
    je 14f
12: inc rdi
    jmp 11b
14: add rdi, 16
    xor ecx, ecx
15: movzx r11d, byte ptr [rsi + rdi]
    sub r11d, '0'
    cmp r11d, 9
    ja 16f
    imul rcx, rcx, 10
    add rcx, r11
    inc rdi
    jmp 15b
16: xor r9d, r9d
    cmp word ptr [rsi + 9], 0x3032   # "HTTP/1.1 200"
    jne 17f
    cmp byte ptr [rsi + 11], '0'
    jne 17f
    test rcx, rcx
    setnz r9b
17: lea rax, [rdx + rcx + 4]
13: ret

# put_dec: writes rax in decimal at rdi and advances it, rendering through
# the scratch at r8+40..63. Clobbers rax, rcx, rdx, rsi, r10.
put_dec:
    lea rsi, [r8 + 64]
    mov r10d, 10
18: xor edx, edx
    div r10
    add dl, '0'
    dec rsi
    mov [rsi], dl
    test rax, rax
    jnz 18b
    lea rcx, [r8 + 64]
    sub rcx, rsi
    rep movsb
    ret

addr:     .byte 2, 0, 0x47, 0xeb, 127, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0   # AF_INET 127.0.0.1:18411
req_head: .ascii "GET /item/"
    .set req_head_len, . - req_head
req_tail: .ascii " HTTP/1.1\r\nHost: 127.0.0.1:18411\r\n\r\n"
    .set req_tail_len, . - req_tail
