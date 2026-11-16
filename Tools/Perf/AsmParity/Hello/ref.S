# `run: fn() { print("hello") }` by hand, with every feature Jet's release
# hello keeps, each written as an expert would:
# - host startup: the start stack's argc, argv and envp kept (r12), and the
#   test-read recorder armed: JET_TEST_READS_PATH=<path> opens <path>
#   O_WRONLY|O_APPEND|O_CLOEXEC (descriptor in r13, negative when off);
# - checked allocation: the allocator's 12 KiB state mapped at start (r14);
#   a refused map is "memory allocation of N bytes failed" on fd 2, then
#   abort (SIGABRT unblocked and raised, SIG_DFL and raised again, exit 127);
# - fault report: a 256 KiB alternate signal stack (none if refused) and a
#   handler for SIGILL, SIGBUS, SIGFPE and SIGSEGV that writes the release
#   report (Compiler/JetRuntime/Assets/FaultReport/addresses.S, word for
#   word): the first line, the build id and the frames' addresses, then
#   exit 70 after a stack overflow or the signal's default action;
# - print's program-main contract: SIGPIPE ignored (a gone reader is
#   exit 0), EINTR retried, short writes advanced, a failed write ignored,
#   exit 0.
# A program that cannot stop links no crash receipt. The image's build id is
# 20 bytes the linker writes (build_id), so the report reads no ELF note and
# maps no image; the program runs one thread, so no report needs a claim.
# The harness assembles this one .text blob and writes it behind an ELF
# header, one R+X PT_LOAD and PT_GNU_STACK, so the blob starts at BLOB_BASE.
    .intel_syntax noprefix
    .set BLOB_BASE, 0x400000 + 64 + 2 * 56
    .set STATE_SIZE, 12288
    .set ALT_SIZE, 262144

    # The fault report's frame: these words below rbp, the buffer from rbp.
    .set V_ADDR, -16
    .set V_FP, -24
    .set V_SP, -32
    .set V_IP, -40
    .set V_HEX, -48
    .set WORDS, 64
    .set OUT_SIZE, 4096
    .set FRAME, WORDS + OUT_SIZE
    .set T_STOP, 0
    .set T_UNNAMED, 1
    .set T_SEGV, 3
    .set T_BUILD, 7
    .set T_BACKTRACE, 8
    .set T_ADDRESS, 9

    .text
_start:
    mov r12, rsp
    # The test-read recorder: the first JET_TEST_READS_PATH= in envp.
    mov eax, [r12]
    lea rbx, [r12 + rax * 8 + 16]
    or r13, -1
1:  mov rdi, [rbx]
    add rbx, 8
    test rdi, rdi
    jz 2f
    lea rsi, [rip + reads_key]
    push offset reads_key_len
    pop rcx
    repe cmpsb
    jne 1b
    mov esi, 0x80401
    push 2
    pop rax
    syscall
    mov r13, rax
2:
    # The allocator's state, checked.
    mov esi, STATE_SIZE
    call map
    jae oom
    mov r14, rax
    # The alternate signal stack: sigaltstack(&{sp, 0, ALT_SIZE}, NULL).
    mov esi, ALT_SIZE
    call map
    jae 3f
    push rsi
    push 0
    push rax
    mov rdi, rsp
    xor esi, esi
    mov eax, 131
    syscall
3:
    # rt_sigaction(sig, &fault_act, NULL, 8) for SIGILL, SIGBUS, SIGFPE,
    # SIGSEGV. Constant arguments, the action in the image: cannot fail.
    lea rsi, [rip + fault_act]
    xor edx, edx
    push 8
    pop r10
    mov ebx, 0x0b080704
4:  movzx edi, bl
    push 13
    pop rax
    syscall
    shr ebx, 8
    jnz 4b
    # rt_sigaction(SIGPIPE, &{SIG_IGN}, NULL, 8): print's program-main
    # contract. rdx and r10 still hold 0 and 8.
    push 0
    push 0
    push 0
    push 1
    mov rsi, rsp
    push 13
    pop rdi
    push 13
    pop rax
    syscall
    # Test builds only (as --defsym FAULT=n): fault on purpose.
    .ifdef FAULT
    .if FAULT == 1
    mov byte ptr [0], 0  # SIGSEGV
    .elseif FAULT == 2
    ud2                  # SIGILL
    .elseif FAULT == 3
    xor ecx, ecx         # SIGFPE
    div ecx
    .else
    call overflow        # SIGSEGV past the stack: E3012
    .endif
    .endif
    # print("hello"): EINTR retried, short writes advanced, failure ignored.
    lea rsi, [rip + msg]
    push 6
    pop rdx
    push 1
    pop rdi
    call write_all
    xor edi, edi
exit:
    mov eax, 231
    syscall

    .ifdef FAULT
overflow:
    push rbp
    mov rbp, rsp
    call overflow
    .endif

# mmap(NULL, rsi, RW, PRIVATE|ANONYMOUS, -1, 0): the map in rax, carry
# clear when refused. Keeps rsi.
map:
    xor edi, edi
    push 3
    pop rdx
    push 34
    pop r10
    or r8, -1
    xor r9d, r9d
    push 9
    pop rax
    syscall
    cmp rax, -4095
    ret

# write(rdi, rsi, rdx) until done: interrupted writes retried, short writes
# advanced, a failure ends it.
write_all:
    test rdx, rdx
    jle 2f
    push 1
    pop rax
    syscall
    cmp rax, -4
    je write_all
    test rax, rax
    jle 2f
    add rsi, rax
    sub rdx, rax
    jmp write_all
2:  ret

# Out of memory for rsi bytes: the report, then abort.
oom:
    mov rax, rsi
    sub rsp, 32
    lea rsi, [rsp + 32]
    push 10
    pop rcx
1:  xor edx, edx
    div rcx
    add dl, '0'
    dec rsi
    mov [rsi], dl
    test rax, rax
    jnz 1b
    mov rbx, rsi
    lea rsi, [rip + oom_pre]
    push offset oom_pre_len
    pop rdx
    call say_err
    mov rsi, rbx
    lea rdx, [rsp + 32]
    sub rdx, rbx
    call say_err
    lea rsi, [rip + oom_post]
    push offset oom_post_len
    pop rdx
    call say_err
    # rt_sigprocmask(SIG_UNBLOCK, &{SIGABRT}, NULL, 8), raise; SIG_DFL, raise.
    push 32
    mov rsi, rsp
    push 1
    pop rdi
    xor edx, edx
    push 8
    pop r10
    push 14
    pop rax
    syscall
    push 6
    pop rbx
    call raise
    push 0
    push 0
    push 0
    push 0
    mov rsi, rsp
    mov edi, ebx
    xor edx, edx
    push 13
    pop rax
    syscall
    call raise
    push 127
    pop rdi
    jmp exit

say_err:
    push 2
    pop rdi
    jmp write_all

# tgkill(getpid(), gettid(), ebx).
raise:
    push 39
    pop rax
    syscall
    xchg r15d, eax
    push 186
    pop rax
    syscall
    xchg esi, eax
    mov edi, r15d
    mov edx, ebx
    mov eax, 234
    syscall
    ret

# The fault handler (signal, siginfo, ucontext), on the alternate stack.
# Never returns: exit 70 after a stack overflow, else the signal's default
# action (SA_RESETHAND restored SIG_DFL, SA_NODEFER leaves it unblocked),
# exit_group(128 + signal) if that does not end the process.
fault_handler:
    mov r12d, edi
    sub rsp, FRAME
    lea rbp, [rsp + WORDS]
    mov r15, rbp
    lea r14, [rbp + OUT_SIZE]
    mov rax, [rsi + 16]         # si_addr
    mov [rbp + V_ADDR], rax
    mov rax, [rdx + 120]        # the mcontext's rbp, rsp and rip
    mov [rbp + V_FP], rax
    mov rax, [rdx + 160]
    mov [rbp + V_SP], rax
    mov rax, [rdx + 168]
    mov [rbp + V_IP], rax
    mov [rbp + V_HEX], rax
    # A SIGSEGV within 1 MiB below the stack pointer is a stack overflow.
    xor ebx, ebx
    cmp r12b, 11
    jne signal
    mov rax, [rbp + V_SP]
    mov rcx, [rbp + V_ADDR]
    lea rdx, [rax + 65536]
    cmp rcx, rdx
    jge signal
    add rcx, 1048576
    cmp rcx, rax
    jle signal
    mov bl, 70
    mov al, T_STOP
    call say
    mov al, T_UNNAMED
    jmp 1f
signal:
    mov rax, [rbp + V_ADDR]
    mov [rbp + V_HEX], rax
    mov al, T_SEGV
    cmp r12b, 11
    je 1f
    inc eax
    cmp r12b, 7
    je 1f
    inc eax
    cmp r12b, 4
    je 1f
    inc eax
1:  call say
    mov al, T_BUILD
    call say
    mov al, T_BACKTRACE
    call say
    mov rax, [rbp + V_IP]
    call address
    # The return addresses of up to 64 callers along the saved-rbp chain
    # from the faulting frame, while each frame is 8-aligned and ascends
    # (the first from the stack pointer) by under 16 MiB.
    push 64
    pop r13
    mov rax, [rbp + V_SP]
    dec rax
    mov rdi, [rbp + V_FP]
2:  test dil, 7
    jnz 3f
    mov rcx, rdi
    sub rcx, rax
    jle 3f
    cmp rcx, 16777216
    jge 3f
    push rdi
    mov rax, [rdi + 8]
    call address
    pop rax
    mov rdi, [rax]
    dec r13d
    jnz 2b
3:  call flush
    mov edi, ebx
    test ebx, ebx
    jnz exit
    mov ebx, r12d
    call raise
fault_restorer:          # never used: the handler does not return
    lea edi, [r12 + 128]
    jmp exit

# `  0x<rax>\n`.
address:
    mov [rbp + V_HEX], rax
    mov al, T_ADDRESS
# Template number al (texts below): bytes 8 and up print, 1 [V_HEX] in
# hex, 2 the build id, 0 ends it.
say:
    lea rsi, [rip + texts]
    movzx ecx, al
    jrcxz 2f
1:  lodsb
    test al, al
    jnz 1b
    loop 1b
2:  lodsb
    cmp al, 8
    jb 3f
    call putc
    jmp 2b
3:  test al, al
    jz 4f
    push rsi
    push rbx
    cmp al, 1
    jne 5f
    call f_hex
    jmp 6f
5:  call f_id
6:  pop rbx
    pop rsi
    jmp 2b
4:  ret

f_hex:
    mov rbx, [rbp + V_HEX]
    mov al, '0'
    call putc
    mov al, 'x'
    call putc
    mov cl, 60
1:  mov rax, rbx
    shr rax, cl
    test al, 15
    jnz 2f
    sub cl, 4
    jnz 1b
2:  mov rax, rbx
    shr rax, cl
    call nibble
    sub cl, 4
    jns 2b
    ret

# The build id's 20 bytes, two hex digits each.
f_id:
    lea rsi, [rip + build_id]
    push 20
    pop rcx
1:  lodsb
    push rax
    shr al, 4
    call nibble
    pop rax
    call nibble
    loop 1b
    ret

# The low four bits of al as a hex digit.
nibble:
    and al, 15
    add al, '0'
    cmp al, '9'
    jbe putc
    add al, 39
# The byte al; the buffer is written out when full. Keeps rcx and rsi.
putc:
    mov [r15], al
    inc r15
    cmp r15, r14
    jb 1f
    jmp flush
1:  ret

# Write the buffer to fd 2 and empty it. Keeps every register but rax.
flush:
    push rcx
    push rsi
    push rdi
    push rdx
    push r11
    mov rsi, rbp
    mov rdx, r15
    sub rdx, rsi
    call say_err
    mov r15, rbp
    pop r11
    pop rdx
    pop rdi
    pop rsi
    pop rcx
    ret

fault_act:
    .quad fault_handler - _start + BLOB_BASE
    .quad 0xCC000004     # SA_RESETHAND|SA_NODEFER|SA_ONSTACK|SA_RESTORER|SA_SIGINFO
    .quad fault_restorer - _start + BLOB_BASE
    .quad 0
build_id:
    .byte 0x9f, 0x2c, 0x41, 0xd0, 0x7a, 0x3b, 0xe9, 0x15, 0x0c, 0x6d
    .byte 0x1f, 0x2e, 0x3d, 0x4c, 0x5b, 0x6a, 0x79, 0x88, 0x97, 0xa6
msg:   .ascii "hello\n"
reads_key: .ascii "JET_TEST_READS_PATH="
    .set reads_key_len, . - reads_key
oom_pre: .ascii "memory allocation of "
    .set oom_pre_len, . - oom_pre
oom_post: .ascii " bytes failed\n"
    .set oom_post_len, . - oom_post
texts:
    .asciz "Stop [E3012]: Stack overflow"
    .asciz " in `??` at \1\n"
    .asciz "\n"
    .asciz "Fault [SIGSEGV]: invalid memory access at address \1\n"
    .asciz "Fault [SIGBUS]: bus error at address \1\n"
    .asciz "Fault [SIGILL]: illegal instruction\n"
    .asciz "Fault [SIGFPE]: arithmetic fault\n"
    .asciz "Build \2 (names and lines: jet crash)\n"
    .asciz "Backtrace:\n"
    .asciz "  \1\n"
