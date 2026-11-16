# concurrencyservice (gauntlet concurrency.service): a line service on
# 127.0.0.1:<argv[1]>, one connection at a time. A connection's text (up to
# 1024 bytes, to its first newline or its end), trimmed, is a command:
# "ready" answers "ready", "batch N" (1 <= N <= 32) queues 1..N to four
# workers, each value 1ms of work and then its square, and answers "batch N
# total <the squares' sum>", "shutdown" answers "bye" and then exits 0, and
# anything else answers "error"; each answer is a line, and the connection
# is closed after it.
#
# Technique: four raw clone threads share one page. A batch is published as
# one 64-bit word, the count above the next value, so a worker claims a
# value with a single lock xadd and no claim can mix two batches; a worker
# out of values sleeps on a futex over the batch generation. The worker that
# sums the last value writes the answer itself, so no wakeup stands between
# the last sleep and the reply; the server sleeps on a futex over a done
# word until then. The server loop is socket, bind, listen and accept4 with
# one read and one write per connection. The workers are made at the first
# batch, so readiness waits on nothing but the listen; their timer slack is
# 1ns, so a value's 1ms sleep ends at 1ms, not up to 50us later.
# A port it cannot listen on prints "listen failed" on stderr, exit 1.
    .intel_syntax noprefix
    .text

    # The shared page.
    .set CLAIM, 0                # low dword: the next value; high: the count
    .set GEN, 8                  # the batch generation
    .set LEFT, 12                # the batch's values not yet summed
    .set TOTAL, 16
    .set COUNT, 24               # the batch's count
    .set CONN, 28                # the batch's connection
    .set DONE, 32                # 1 once the batch is answered
    .set STACKS, 4096            # four 16 KiB worker stacks follow
    .set STACK, 16384
    .set WORKERS, 4

    # The server's frame.
    .set SA, 0
    .set LINE, 16                # the command (1024 bytes)
    .set REPLY, 1056             # the answer (64 bytes)
    .set FRAME, 1152

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

    xor ebx, ebx                 # no workers yet

    # The listener.
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
    mov esi, 64
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
    # The command: to the first newline, the end, or 1024 bytes.
    xor r12d, r12d
1:  mov edi, r14d
    lea rsi, [r15 + LINE + r12]
    mov edx, 1024
    sub edx, r12d
    jz 3f
    xor eax, eax                 # read
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    jle 3f
    lea rdi, [r15 + LINE + r12]  # a newline in what came?
    add r12, rax
    mov rcx, rax
    mov al, 10
    repne scasb
    jne 1b
3:  lea rsi, [r15 + LINE]        # trimmed: rsi up to rdx
    lea rdx, [rsi + r12]
4:  cmp rsi, rdx
    jae 5f
    movzx eax, byte ptr [rsi]
    call space
    jne 5f
    inc rsi
    jmp 4b
5:  cmp rdx, rsi
    jbe 6f
    movzx eax, byte ptr [rdx - 1]
    call space
    jne 6f
    dec rdx
    jmp 5b
6:  mov rcx, rdx
    sub rcx, rsi
    lea rdi, [r15 + REPLY]
    cmp rcx, 5
    jne 7f
    cmp dword ptr [rsi], 0x64616572  # "read"
    jne error
    cmp byte ptr [rsi + 4], 'y'
    jne error
    mov dword ptr [rdi], 0x64616572  # "ready\n"
    mov word ptr [rdi + 4], 0x0a79
    add rdi, 6
    jmp answer
7:  cmp rcx, 8
    jne 8f
    mov rax, 0x6e776f6474756873  # "shutdown"
    cmp [rsi], rax
    jne 8f
    mov dword ptr [rdi], 0x0a657962  # "bye\n"
    add rdi, 4
    call reply
    xor edi, edi
    mov eax, 231                 # exit_group
    syscall
8:  cmp rcx, 7
    jb error
    cmp dword ptr [rsi], 0x63746162  # "batc"
    jne error
    cmp word ptr [rsi + 4], 0x2068   # "h "
    jne error
    add rsi, 6                   # Int.parse of the rest: a sign, digits
    xor r8d, r8d
    cmp byte ptr [rsi], '+'
    je 81f
    cmp byte ptr [rsi], '-'
    jne 82f
    mov r8d, 1
81: inc rsi
82: cmp rsi, rdx
    jae error
    xor eax, eax
9:  movzx ecx, byte ptr [rsi]
    sub ecx, '0'
    cmp ecx, 9
    ja error
    cmp eax, 1000                # large enough to fail the bound
    jae 91f
    imul eax, eax, 10
    add eax, ecx
91: inc rsi
    cmp rsi, rdx
    jb 9b
    test r8d, r8d
    jnz error
    test eax, eax
    jz error
    cmp eax, 32
    ja error
    # The batch: its sums first, then its values published.
    test rbx, rbx                # the workers, made at the first batch
    jnz 12f
    push rax
    call spawn
    pop rax
12: mov [rbx + COUNT], eax
    mov [rbx + CONN], r14d
    mov dword ptr [rbx + DONE], 0
    mov qword ptr [rbx + TOTAL], 0
    mov [rbx + LEFT], eax
    mov rcx, rax
    shl rcx, 32
    inc rcx
    mov [rbx + CLAIM], rcx
    lock inc dword ptr [rbx + GEN]
    lea rdi, [rbx + GEN]
    mov esi, 1                   # FUTEX_WAKE
    mov edx, WORKERS
    mov eax, 202                 # futex
    syscall
10: mov edx, [rbx + DONE]        # the last value's worker answers
    test edx, edx
    jnz serve
    lea rdi, [rbx + DONE]
    xor esi, esi                 # FUTEX_WAIT
    xor r10d, r10d
    mov eax, 202
    syscall
    jmp 10b
error:
    lea rdi, [r15 + REPLY]
    mov dword ptr [rdi], 0x6f727265  # "error\n"
    mov word ptr [rdi + 4], 0x0a72
    add rdi, 6
answer:
    call reply
    jmp serve

# reply: REPLY up to rdi to the connection, then closed.
reply:
    lea rsi, [r15 + REPLY]
    mov rdx, rdi
    sub rdx, rsi
1:  mov edi, r14d
    mov eax, 1                   # write
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    jle 2f
    add rsi, rax
    sub rdx, rax
    jnz 1b
2:  mov edi, r14d
    mov eax, 3                   # close
    syscall
    ret

# space: ZF when al is whitespace (as trim takes it).
space:
    cmp eax, ' '
    je 1f
    sub eax, 9                   # \t \n \v \f \r
    cmp eax, 4
    ja 2f
    cmp eax, eax
1:  ret
2:  test esp, esp                # ZF clear (the stack is never 0)
    ret

# put_uint: rax in decimal at rdi.
put_uint:
    lea r8, [rsp - 8]
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

# spawn: the shared page in rbx and the four workers on it. The timer slack
# is set first, as a clone inherits it.
spawn:
    mov edi, 29                  # prctl(PR_SET_TIMERSLACK, 1)
    mov esi, 1
    xor edx, edx
    xor r10d, r10d
    xor r8d, r8d
    mov eax, 157
    syscall
    xor edi, edi
    mov esi, STACKS + STACK * WORKERS
    mov edx, 3
    mov r10d, 0x22               # MAP_PRIVATE | MAP_ANONYMOUS
    mov r8, -1
    xor r9d, r9d
    mov eax, 9                   # mmap
    syscall
    cmp rax, -4096
    ja fail_listen
    mov rbx, rax                 # the shared page, in every thread
    mov qword ptr [rbx + CLAIM], 1 # value 1 of no values: nothing to claim
    mov r12d, 1
1:  mov edi, 0x50f00             # VM FS FILES SIGHAND THREAD SYSVSEM
    imul rsi, r12, STACK
    lea rsi, [rbx + rsi + STACKS]
    xor edx, edx
    xor r10d, r10d
    xor r8d, r8d
    mov eax, 56                  # clone
    syscall
    test rax, rax
    jz worker
    js fail_listen
    inc r12
    cmp r12, WORKERS
    jbe 1b
    ret

# A worker: claim a value, 1ms of work, add its square; out of values,
# sleep until the next batch.
worker:
    push 1000000                 # the work: a 1ms nanosleep
    push 0
1:  mov r12d, [rbx + GEN]        # the generation before the claim
2:  mov eax, 1
    lock xadd [rbx + CLAIM], rax
    mov rcx, rax
    shr rcx, 32
    cmp eax, ecx
    ja 4f
    mov r13d, eax
3:  mov rdi, rsp
    xor esi, esi
    mov eax, 35                  # nanosleep
    syscall
    cmp rax, -4
    je 3b
    mov eax, r13d
    imul eax, eax
    lock add [rbx + TOTAL], rax
    lock dec dword ptr [rbx + LEFT]
    jnz 1b
    mov r14d, [rbx + CONN]       # the last value: the answer, from here
    lea rdi, [r15 + REPLY]
    mov dword ptr [rdi], 0x63746162  # "batch "
    mov word ptr [rdi + 4], 0x2068
    add rdi, 6
    mov eax, [rbx + COUNT]
    call put_uint
    mov dword ptr [rdi], 0x746f7420  # " total "
    mov dword ptr [rdi + 3], 0x206c6174
    add rdi, 7
    mov rax, [rbx + TOTAL]
    call put_uint
    mov byte ptr [rdi], 10
    inc rdi
    call reply
    mov dword ptr [rbx + DONE], 1
    lea rdi, [rbx + DONE]
    mov esi, 1                   # FUTEX_WAKE
    mov edx, 1
    mov eax, 202
    syscall
    jmp 1b
4:  lea rdi, [rbx + GEN]
    xor esi, esi                 # FUTEX_WAIT while the generation holds
    mov edx, r12d
    xor r10d, r10d
    mov eax, 202
    syscall
    jmp 1b

fail_listen:
    lea rsi, [rip + s_listen]
    mov edx, 14
    mov edi, 2
    mov eax, 1
    syscall
    mov edi, 1
    mov eax, 231
    syscall

s_listen: .ascii "listen failed\n"
