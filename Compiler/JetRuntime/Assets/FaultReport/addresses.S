# The release fault report: the report of an image built with `debug_info:
# split`, which keeps no location table. It writes
# report.S's first line where that names no function, the image's build id,
# and the frames as addresses, which `jet crash` (JetCli Cli/Crash.jet)
# names from the side debug file the id finds, in report.S's words:
#
#   Stop [E3012]: Stack overflow in `??` at 0x401a2c
#   Build 9f2c41d07a3be9150c6d1e2f3a4b5c6d7e8f9012 (names and lines: jet crash)
#   Backtrace:
#     0x401a2c
#     0x401b10
#
# `backtrace: off` ends a stop at `Stop [E3012]: Stack overflow` and writes
# no backtrace; brief only the faulting address; full, and every stack
# overflow (whose innermost Jet function may be a caller), the return
# addresses of up to 64 callers too. Tools/agent/gen-runtime-fault-report.mjs
# assembles it into X64_FAULT_REPORT_ADDRESSES (Compiler/JetBackend/Source/
# X64/FaultReport.jet), which the static linker writes as a split image's
# jet_rt_fault_report (Image/StaticLink.jet link_fault_report), with
# report.S's arguments, result and rules; the load bias goes unused: the
# addresses are the ones the side file names.
    .intel_syntax noprefix

    # The frame: these words below rbp, the report buffer from rbp.
    .set V_SIG, -8
    .set V_ADDR, -16
    .set V_FP, -24
    .set V_SP, -32
    .set V_IP, -40
    .set V_HEX, -48
    .set V_ID, -56              # the build id's 20 bytes (0: none)
    .set V_MODE, -64            # backtrace: 0 full, 1 brief, 2 off
    .set WORDS, 64
    .set OUT_SIZE, 4096
    .set FRAME, WORDS + OUT_SIZE

    # Templates (texts below). Field codes: 1 [V_HEX] in hex, 2 the build id.
    .set T_STOP, 0
    .set T_UNNAMED, 1
    .set T_NL, 2
    .set T_SEGV, 3
    .set T_BUILD, 7
    .set T_BACKTRACE, 8
    .set T_ADDRESS, 9

    .text
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    sub rsp, FRAME
    lea rbp, [rsp + WORDS]
    mov r15, rbp
    lea r14, [rbp + OUT_SIZE]
    mov [rbp + V_SIG], rdi
    mov [rbp + V_MODE], rcx
    mov rax, [rsi + 16]         # si_addr
    mov [rbp + V_ADDR], rax
    mov rax, [rdx + 120]        # the mcontext's rbp, rsp and rip
    mov [rbp + V_FP], rax
    mov rax, [rdx + 160]
    mov [rbp + V_SP], rax
    mov rax, [rdx + 168]
    mov [rbp + V_IP], rax
    mov [rbp + V_HEX], rax

    # The build id: the GNU build-id note a PT_NOTE program header holds.
    xor eax, eax
    mov [rbp + V_ID], rax
    test r8, r8
    jz 3f
    mov r10, r8
    add r10, [r8 + 32]          # e_phoff
    movzx r9d, word ptr [r8 + 54]   # e_phentsize
    movzx edx, word ptr [r8 + 56]   # e_phnum
1:  test edx, edx
    jz 3f
    cmp dword ptr [r10], 4      # PT_NOTE
    jne 2f
    mov rax, [r10 + 8]          # p_offset
    add rax, r8
    cmp dword ptr [rax + 4], 20 # descsz
    jne 2f
    cmp dword ptr [rax + 8], 3  # NT_GNU_BUILD_ID
    jne 2f
    add rax, 16
    mov [rbp + V_ID], rax
    jmp 3f
2:  add r10, r9
    dec edx
    jmp 1b
3:
    # A SIGSEGV within 1 MiB below the stack pointer is a stack overflow.
    xor ebx, ebx
    cmp byte ptr [rbp + V_SIG], 11
    jne signal
    mov rax, [rbp + V_SP]
    mov rcx, [rbp + V_ADDR]
    lea rdx, [rax + 65536]
    cmp rcx, rdx
    jge signal
    add rcx, 1048576
    cmp rcx, rax
    jle signal

    # E3012, its faulting address in place of the function.
    mov bl, 70
    mov al, T_STOP
    call say
    mov al, T_NL
    cmp byte ptr [rbp + V_MODE], 2
    je 1f
    mov al, T_UNNAMED
    jmp 1f

    # Any other fatal signal: its name and the address.
signal:
    mov rax, [rbp + V_ADDR]
    mov [rbp + V_HEX], rax
    mov ecx, [rbp + V_SIG]
    mov al, T_SEGV
    cmp cl, 11
    je 1f
    inc eax
    cmp cl, 7
    je 1f
    inc eax
    cmp cl, 4
    je 1f
    inc eax
1:  call say
    cmp qword ptr [rbp + V_ID], 0
    je 2f
    mov al, T_BUILD
    call say
2:  cmp byte ptr [rbp + V_MODE], 2
    je 4f
    mov al, T_BACKTRACE
    call say
    mov rax, [rbp + V_IP]
    call address
    test ebx, ebx
    jnz 3f
    cmp byte ptr [rbp + V_MODE], 1
    je 4f
3:  call walk
4:  call flush
    mov eax, ebx
    add rsp, FRAME
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret

# The return addresses of up to 64 callers along the saved-rbp chain from
# the faulting frame, while each frame is 8-aligned and ascends (the first
# from the stack pointer) by under 16 MiB.
walk:
    push 64
    pop r13
    mov r12, [rbp + V_FP]
    mov rax, [rbp + V_SP]
    dec rax
1:  test r12b, 7
    jnz 2f
    mov rcx, r12
    sub rcx, rax
    jle 2f
    cmp rcx, 16777216
    jge 2f
    mov rax, [r12 + 8]
    call address
    mov rax, r12
    mov r12, [r12]
    dec r13d
    jnz 1b
2:  ret

# `  0x<rax>\n`.
address:
    mov [rbp + V_HEX], rax
    mov al, T_ADDRESS
# Template number al (texts below): bytes 8 and up print, 1 and 2 print a
# field, 0 ends it.
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
    mov rsi, [rbp + V_ID]
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

# Write the buffer to fd 2 (interrupted writes retried, short writes
# advanced, a failure ends it) and empty it. Keeps every register but rax.
flush:
    push rcx
    push rsi
    push rdi
    push rdx
    push r11
    mov rsi, rbp
1:  mov rdx, r15
    sub rdx, rsi
    jle 2f
    push 2
    pop rdi
    push 1
    pop rax
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    jle 2f
    add rsi, rax
    jmp 1b
2:  mov r15, rbp
    pop r11
    pop rdx
    pop rdi
    pop rsi
    pop rcx
    ret

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
