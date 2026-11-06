# `run: fn() { print("hello") }` by hand, with everything Jet's hello keeps:
# SIGPIPE ignored (a gone reader is exit 0), EINTR retried, short writes
# advanced, a failed write ignored, exit 0, and the fixed-text fault
# handler of a program that cannot fault (D-RT-DEFECT-REPORT1 A). The
# harness assembles this one .text blob and writes it behind an ELF header,
# one R+X PT_LOAD and PT_GNU_STACK, so the blob starts at BLOB_BASE.
    .intel_syntax noprefix
    .set BLOB_BASE, 0x400000 + 64 + 2 * 56
    .text
_start:
    # rt_sigaction(sig, &fault_act, NULL, 8) for SIGILL, SIGBUS, SIGFPE,
    # SIGSEGV. Constant arguments, the action in the image: cannot fail.
    lea rsi, [rip + fault_act]
    xor edx, edx
    push 8
    pop r10
    mov ebx, 0x0b080704
1:  movzx edi, bl
    push 13
    pop rax
    syscall
    shr ebx, 8
    jnz 1b
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
    .else
    xor ecx, ecx         # SIGFPE
    div ecx
    .endif
    .endif
    # print("hello"): EINTR retried, short writes advanced, failure ignored.
    lea rsi, [rip + msg]
    push 6
    pop rdx
2:  push 1
    pop rdi
    push 1
    pop rax
    syscall
    cmp rax, -4
    je 2b
    test rax, rax
    jle 3f
    add rsi, rax
    sub rdx, rax
    jnz 2b
3:  xor edi, edi
    mov eax, 231
    syscall

# The fixed-text fault report: "Fault [SIG<name>]: Jet internal defect;
# rebuild with --backtrace=full", then the signal's default
# action, as Fault.jet fault_raise does: SA_RESETHAND has restored SIG_DFL,
# SA_NODEFER leaves the signal unblocked, so kill delivers it at once (a
# sent signal too, not only a fault); exit_group(128 + signal) if it is not.
fault_handler:
    mov ebx, edi
    lea rsi, [rip + pre]
    push offset pre_len
    pop rdx
    call fault_write
    lea rsi, [rip + names]
    lea rax, [rip + name_at - 4]
    movzx eax, byte ptr [rax + rbx]
    add rsi, rax
    push 3
    pop rdx
    cmp bl, 11
    jne 4f
    inc edx
4:  call fault_write
    lea rsi, [rip + post]
    push offset post_len
    pop rdx
    call fault_write
    push 39              # getpid: no task can be spawned, so one thread
    pop rax
    syscall
    xchg edi, eax
    mov esi, ebx
    push 62              # kill
    pop rax
    syscall
fault_restorer:          # never used: the handler does not return
    lea edi, [rbx + 128]
    mov eax, 231
    syscall
fault_write:
    push 2
    pop rdi
    push 1
    pop rax
    syscall
    ret

fault_act:
    .quad fault_handler - _start + BLOB_BASE
    .quad 0xC4000000     # SA_RESETHAND | SA_NODEFER | SA_RESTORER
    .quad fault_restorer - _start + BLOB_BASE
    .quad 0
msg:   .ascii "hello\n"
pre:   .ascii "Fault [SIG"
    .set pre_len, . - pre
names: .ascii "SEGVILLBUSFPE"
name_at: .byte 4, 0, 0, 7, 10, 0, 0, 0   # signals 4..11: ILL, -, -, BUS, FPE, -, -, SEGV
post:  .ascii "]: Jet internal defect; rebuild with --backtrace=full\n"
    .set post_len, . - post
