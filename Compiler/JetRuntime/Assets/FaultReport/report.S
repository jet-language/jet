# The native fault report (Compiler/JetRuntime/Source/Fault.jet): the text
# of a fatal signal, as one position-independent run of x86-64 code with its
# texts. Tools/agent/RuntimeGenerators.jet assembles it into
# X64_FAULT_REPORT (Compiler/JetBackend/Source/X64/FaultReport.jet), which
# the static linker writes as the function the runtime's rt_fault_print
# intrinsic calls (JetIR BuildLibrary.jet; Image/StaticLink.jet
# link_fault_report) in every image but a split one (addresses.S):
#
#   jet_rt_fault_report(signal, info, context, backtrace, image, bias) -> I64
#
# signal, siginfo_t and ucontext_t as the kernel passes them to the handler;
# backtrace the image's `backtrace` option (0 full, 1 brief, 2 off); image a
# read-only mapping of this executable's ELF file, or 0; bias its load bias.
# It writes the report to fd 2 and returns 70 after an E3012 stop, else 0;
# the handler then exits or takes the signal's default action.
#
# The bytes are a whole C function: arguments in rdi, rsi, rdx, rcx, r8,
# r9, the result in rax, every callee-saved register kept. No byte is
# relocated: every reference is relative and inside the run.
#
# Frames come from the frame-pointer chain and the location table
# (.debug_jetline as the linker writes it, Image/DebugPrune.jet) of the
# image's section headers. Every byte goes through one 4 KiB buffer on the
# stack, from templates with seven field codes: no allocation, no unwinder.
    .intel_syntax noprefix

    # The frame: these words below rbp, the report buffer from rbp, then
    # the 8 KiB path buffer.
    .set V_SIG, -8
    .set V_ADDR, -16
    .set V_FP, -24
    .set V_SP, -32
    .set V_IP, -40
    .set V_NAME, -48
    .set V_FILE, -56
    .set V_DIR, -64
    .set V_DECL, -72
    .set V_ROWS, -80            # the function find matched: its rows and
    .set V_ROWEND, -88          # their end
    .set V_LINE, -96
    .set V_SRC, -104
    .set V_SRCLEN, -112
    .set V_HEX, -120
    .set V_LIMIT, -124
    .set V_STEPS, -128
    .set V_NUM, -144            # 16 bytes of decimal digits, backwards from V_NUM + 16
    .set V_MODE, -152           # backtrace: 0 full, 1 brief, 2 off
    .set V_BIAS, -160
    .set V_TAB, -168            # .debug_jetline: start (0: none) and end
    .set V_TABEND, -176
    .set V_LOW, -184            # the function find matched: its link-time
    .set V_HIGH, -192           # start and end
    .set WORDS, 192
    .set OUT_SIZE, 4096
    .set PATH, OUT_SIZE
    .set FRAME, WORDS + OUT_SIZE + 8192

    # Templates (texts below). Field codes: 1 name, 2 file, 3 declaration
    # line, 4 line, 5 [V_HEX] in hex, 6 the source line, 7 its gutter.
    .set T_STOP, 0
    .set T_NAMED, 1
    .set T_UNNAMED, 2
    .set T_NL, 3
    .set T_ARROW, 4
    .set T_SNIPPET, 5
    .set T_WHY, 6
    .set T_SEGV, 7
    .set T_BACKTRACE, 11
    .set T_ADDRESS, 12
    .set T_FRAME, 13
    .set T_QQ, 15

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
    mov [rbp + V_BIAS], r9
    mov rax, [rsi + 16]         # si_addr
    mov [rbp + V_ADDR], rax
    mov rax, [rdx + 120]        # the mcontext's rbp, rsp and rip
    mov [rbp + V_FP], rax
    mov rax, [rdx + 160]
    mov [rbp + V_SP], rax
    mov rax, [rdx + 168]
    mov [rbp + V_IP], rax
    mov [rbp + V_HEX], rax

    # The location table: the image's section named .debug_jetline.
    xor eax, eax
    mov [rbp + V_TAB], rax
    mov [rbp + V_TABEND], rax
    test r8, r8
    jz 3f
    mov r10, r8
    add r10, [r8 + 40]          # e_shoff
    movzx r9d, word ptr [r8 + 58]   # e_shentsize
    movzx edx, word ptr [r8 + 60]   # e_shnum
    movzx eax, word ptr [r8 + 62]   # e_shstrndx
    imul eax, r9d
    mov r11, [r10 + rax + 24]   # the section names' sh_offset
    add r11, r8
1:  test edx, edx
    jz 3f
    mov eax, [r10]              # sh_name
    lea rsi, [r11 + rax]
    lea rdi, [rip + jetline]
    push 15
    pop rcx
    repe cmpsb
    jne 2f
    mov rax, [r10 + 24]         # sh_offset, sh_size
    add rax, r8
    mov [rbp + V_TAB], rax
    add rax, [r10 + 32]
    mov [rbp + V_TABEND], rax
2:  add r10, r9
    dec edx
    jmp 1b
3:
    # A SIGSEGV within 1 MiB below the stack pointer is a stack overflow.
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

    # E3012: the innermost Jet frame, the faulting one or a caller.
    mov rax, [rbp + V_IP]
    call find
    test eax, eax
    jnz 3f
    lea r13, [rip + walk_find]
    push 1
    pop rdx
    call walk
3:  mov ebx, eax
    mov al, T_STOP
    call say
    mov al, T_NL
    cmp byte ptr [rbp + V_MODE], 2
    je 4f
    mov al, T_UNNAMED
    sub al, bl
    call say
    test ebx, ebx
    jz 5f
    mov al, T_ARROW
    call say
    call source
    test eax, eax
    jz 5f
    mov al, T_SNIPPET
4:  call say
5:  mov al, T_WHY
    call say
    call flush
    push 70
    pop rax
    jmp done

    # Any other fatal signal: its name, the address, the Jet frames.
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
    mov al, T_NL
    cmp byte ptr [rbp + V_MODE], 2
    je 4f
    mov al, T_BACKTRACE
    call say
    lea r13, [rip + walk_frame]
    mov rax, [rbp + V_IP]
    cmp qword ptr [rbp + V_TAB], 0
    jne 2f
    # No location table (`debug_info: false`): addresses.
    lea r13, [rip + walk_address]
    call walk_address
    jmp 3f
2:  call frame_at
    test eax, eax
    jnz 3f
    mov al, T_QQ
    call say
3:  cmp byte ptr [rbp + V_MODE], 1
    je 6f
    push 64
    pop rdx
    call walk
    jmp 6f
4:  call say
6:  call flush
    xor eax, eax
done:
    add rsp, FRAME
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    ret

# Walk the callers from the faulting frame, calling r13 with each return
# address in rax; stop after 4096 frames or when the calls have returned
# rdx in total. Returns that total.
walk:
    push rbx
    push r12
    mov [rbp + V_LIMIT], edx
    and dword ptr [rbp + V_STEPS], 0
    mov rax, [rbp + V_FP]
    mov rdx, [rbp + V_SP]
    test al, 7
    jnz 3f
    cmp rax, rdx
    jl 3f
    mov rcx, rax
    sub rcx, rdx
    cmp rcx, 16777216
    jge 3f
    xor ebx, ebx
1:  mov r12, rax
    cmp ebx, [rbp + V_LIMIT]
    jae 4f
    inc dword ptr [rbp + V_STEPS]
    cmp dword ptr [rbp + V_STEPS], 4096
    ja 4f
    mov rax, [r12 + 8]
    call r13
    add ebx, eax
    # The caller: the saved rbp when it ascends by under 16 MiB.
    mov rax, [r12]
    test al, 7
    jnz 4f
    cmp rax, r12
    jle 4f
    mov rcx, rax
    sub rcx, r12
    cmp rcx, 16777216
    jl 1b
    jmp 4f
3:  xor ebx, ebx
4:  mov eax, ebx
    pop r12
    pop rbx
    ret

walk_find:
    dec rax
    jmp find

walk_address:
    mov [rbp + V_HEX], rax
    mov al, T_ADDRESS
    call say
    push 1
    pop rax
    ret

walk_frame:
    dec rax
# `  name at file:line\n` (or `  name\n`) for the Jet function holding rax:
# eax 1, else 0 and nothing written.
frame_at:
    push rbx
    mov rbx, rax
    call find
    test eax, eax
    jz 1f
    mov rdi, rbx
    sub rdi, [rbp + V_BIAS]
    call line
    mov cl, al
    mov al, T_FRAME + 1
    sub al, cl
    call say
    push 1
    pop rax
1:  pop rbx
    ret

# The Jet function holding run-time pc rax: eax 1 and its name, file,
# directory, declaration line, bounds and rows in the frame words, else
# eax 0. Table blocks: u32 length (after this word), u32 count, u32
# strings (from the block's start), u64 base; then count records in
# address order: an sleb, the start's distance from the previous record's
# end (the first's from the base), then five ulebs, size, name, file,
# declaration line and the byte length of the rows that follow.
find:
    sub rax, [rbp + V_BIAS]
    mov rsi, [rbp + V_TAB]
1:  lea rdx, [rsi + 20]
    cmp rdx, [rbp + V_TABEND]
    ja 4f
    mov ecx, [rsi]
    lea rdi, [rsi + rcx + 4]
    cmp rdi, rdx
    jb 4f
    cmp rdi, [rbp + V_TABEND]
    ja 4f
    mov r9d, [rsi + 4]
    mov r11d, [rsi + 8]
    add r11, rsi
    mov r10, [rsi + 12]
    mov rsi, rdx
    push rax
2:  test r9d, r9d
    jz 3f
    cmp rsi, rdi
    jae 3f
    call sleb
    add r10, rax
    mov [rbp + V_LOW], r10
    call uleb
    add r10, rax
    mov [rbp + V_HIGH], r10
    call uleb
    mov [rbp + V_NAME], rax
    call uleb
    mov [rbp + V_FILE], rax
    call uleb
    mov [rbp + V_DECL], rax
    call uleb
    mov [rbp + V_ROWS], rsi
    add rsi, rax
    mov [rbp + V_ROWEND], rsi
    mov rax, [rsp]
    sub rax, [rbp + V_LOW]
    mov rcx, r10
    sub rcx, [rbp + V_LOW]
    cmp rax, rcx
    jb 5f
    dec r9d
    jmp 2b
3:  pop rax
    mov rsi, rdi
    jmp 1b
4:  xor eax, eax
    ret
5:  pop rax
    mov [rbp + V_DIR], r11
    add [rbp + V_NAME], r11
    add [rbp + V_FILE], r11
    push 1
    pop rax
    ret

# The line of link-time address rdi in the function find matched: the row
# before the first row past it, the last row's reaching the function's
# end. eax 1 and [V_LINE], else eax 0.
line:
    push rbx
    push r12
    push r13
    mov rbx, rdi
    mov rsi, [rbp + V_ROWS]
    mov r12, [rbp + V_ROWEND]
    mov r9, [rbp + V_LOW]
    mov r13, [rbp + V_HIGH]
    push 1
    pop r10
    or r11, -1
1:  cmp rsi, r12
    jae 3f
    call uleb
    add r9, rax
    call sleb
    add r10, rax
    test r11, r11
    js 2f
    cmp rbx, r11
    jl 2f
    cmp rbx, r9
    jl 4f
2:  mov r11, r9
    mov rdi, r10
    jmp 1b
3:  test r11, r11
    js 5f
    cmp rbx, r11
    jl 5f
    cmp rbx, r13
    jge 5f
4:  mov [rbp + V_LINE], rdi
    push 1
    pop rax
    jmp 6f
5:  xor eax, eax
6:  pop r13
    pop r12
    pop rbx
    ret

# Signed LEB128 at rsi (advanced) into rax. Clobbers rcx, rdx, r8.
sleb:
    call uleb
    cmp ecx, 64
    jae 1f
    test dl, 64
    jz 1f
    or r8, -1
    shl r8, cl
    or rax, r8
1:  ret

# LEB128 at rsi (advanced) into rax; cl ends as the shift, dl as the last
# byte. Clobbers rcx, rdx, r8.
uleb:
    xor eax, eax
    xor ecx, ecx
1:  movzx edx, byte ptr [rsi]
    inc rsi
    cmp ecx, 63
    jae 2f
    mov r8d, edx
    and r8d, 127
    shl r8, cl
    or rax, r8
2:  add ecx, 7
    test dl, dl
    js 1b
    ret

# Line [V_DECL] of the file find matched, without its line break or
# trailing blanks, into [V_SRC] and [V_SRCLEN]: eax 1, else 0. A relative
# path is read from the table's directory unless that is `.`; when no file
# is there, from the working directory itself, so a program run from its
# package root finds the source whichever directory built it.
source:
    lea rdi, [rbp + PATH]
    mov rsi, [rbp + V_FILE]
    cmp byte ptr [rsi], '/'
    je 1f
    mov rsi, [rbp + V_DIR]
    cmp word ptr [rsi], '.'
    je 1f
    call copy
    jc 9f
    lea rax, [rbp + PATH]
    cmp rdi, rax
    je 1f
    mov byte ptr [rdi], '/'
    inc rdi
1:  mov rsi, [rbp + V_FILE]
    call copy
    jc 9f
    mov byte ptr [rdi], 0
    mov edi, -100
    lea rsi, [rbp + PATH]
    mov edx, 524288
    mov eax, 257                # openat(AT_FDCWD, path, O_RDONLY | O_CLOEXEC)
    syscall
    test eax, eax
    jns 8f
    mov edi, -100
    mov rsi, [rbp + V_FILE]
    mov edx, 524288
    mov eax, 257                # openat(AT_FDCWD, file, O_RDONLY | O_CLOEXEC)
    syscall
    test eax, eax
    js 9f
8:  mov ebx, eax
    mov edi, eax
    xor esi, esi
    push 2
    pop rdx
    push 8                      # lseek(fd, 0, SEEK_END)
    pop rax
    syscall
    mov r12, rax
    or r13, -1
    test rax, rax
    jle 2f
    xor edi, edi
    mov rsi, rax
    push 1
    pop rdx
    push 2
    pop r10
    mov r8d, ebx
    xor r9d, r9d
    push 9                      # mmap(0, size, PROT_READ, MAP_PRIVATE, fd, 0)
    pop rax
    syscall
    mov r13, rax
2:  mov edi, ebx
    push 3                      # close
    pop rax
    syscall
    test r13, r13
    js 9f
    # Skip [V_DECL] - 1 line breaks; the line must start inside the file.
    mov rdi, r13
    mov rcx, r12
    mov rdx, [rbp + V_DECL]
    mov al, 10
3:  dec rdx
    jz 4f
    repne scasb
    je 3b
    jmp 9f
4:  jrcxz 9f
    mov rsi, rdi
    repne scasb
    jne 5f
    dec rdi
5:  cmp rdi, rsi
    jbe 7f
    mov al, [rdi - 1]
    cmp al, ' '
    je 6f
    cmp al, 9
    je 6f
    cmp al, 13
    jne 7f
6:  dec rdi
    jmp 5b
7:  mov [rbp + V_SRC], rsi
    sub rdi, rsi
    mov [rbp + V_SRCLEN], rdi
    push 1
    pop rax
    ret
9:  xor eax, eax
    ret

# Append the NUL-terminated rsi at rdi; CF when the path reaches 8191 bytes.
copy:
    lea rdx, [rbp + PATH + 8191]
1:  lodsb
    test al, al
    jz 2f
    stosb
    cmp rdi, rdx
    jb 1b
    stc
2:  ret

# Template number al (texts below): bytes 8 and up print, 1 to 7 print a
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
    movzx eax, al
    lea rdx, [rip + fields - 1]
    movzx ecx, byte ptr [rdx + rax]
    add rdx, rcx
    call rdx
    pop rbx
    pop rsi
    jmp 2b
4:  ret

fields:
    .byte f_name - fields + 1, f_file - fields + 1, f_decl - fields + 1, f_line - fields + 1
    .byte f_hex - fields + 1, f_source - fields + 1, f_gutter - fields + 1
f_name:
    mov rsi, [rbp + V_NAME]
    jmp putz
f_file:
    mov rsi, [rbp + V_FILE]
    jmp putz
f_line:
    mov rax, [rbp + V_LINE]
    jmp dec
f_source:
    mov rsi, [rbp + V_SRC]
    mov rcx, [rbp + V_SRCLEN]
    jmp put
# Spaces as wide as the declaration line's digits, plus 3.
f_gutter:
    mov rax, [rbp + V_DECL]
    push 3
    pop rbx
    push 10
    pop rcx
1:  inc ebx
    xor edx, edx
    div rcx
    test rax, rax
    jnz 1b
1:  mov al, ' '
    call putc
    dec ebx
    jnz 1b
    ret
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
    and al, 15
    add al, '0'
    cmp al, '9'
    jbe 3f
    add al, 39
3:  call putc
    sub cl, 4
    jns 2b
    ret
f_decl:
    mov rax, [rbp + V_DECL]
# rax in decimal.
dec:
    lea rsi, [rbp + V_NUM + 16]
    mov rdi, rsi
    push 10
    pop rcx
1:  xor edx, edx
    div rcx
    add dl, '0'
    dec rsi
    mov [rsi], dl
    test rax, rax
    jnz 1b
    mov rcx, rdi
    sub rcx, rsi
    jmp put

# The NUL-terminated rsi.
putz:
    mov rdi, rsi
    xor eax, eax
    or rcx, -1
    repne scasb
    not rcx
    dec rcx
# rcx bytes at rsi.
put:
    jrcxz 2f
1:  lodsb
    call putc
    loop 1b
2:  ret

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

jetline: .asciz ".debug_jetline"
texts:
    .asciz "Stop [E3012]: Stack overflow"
    .asciz " in `\1`\n"
    .asciz " in `??` at \5\n"
    .asciz "\n"
    .asciz "  --> \2:\3 in \1\n"
    .asciz "\7|\n\3 | \6\n\7| ^\n"
    .asciz " Why: The call stack kept growing without reaching a safe return \xe2\x80\x94 with Jet file, line, and source context\n Fix: End the recursion or make progress toward a base case, or give deep work more stack with `task.group deep(stack: bytes)`.\nMore: jet-lang.dev/e/E3012\n"
    .asciz "Fault [SIGSEGV]: invalid memory access at address \5"
    .asciz "Fault [SIGBUS]: bus error at address \5"
    .asciz "Fault [SIGILL]: illegal instruction"
    .asciz "Fault [SIGFPE]: arithmetic fault"
    .asciz "\nBacktrace:\n"
    .asciz "  \5\n"
    .asciz "  \1 at \2:\4\n"
    .asciz "  \1\n"
    .asciz "  ??\n"
