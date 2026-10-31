# Shared start for the kernel references (included first by each ref.S, so
# _start is the first byte of the blob): parse argv[1] as an unsigned
# decimal, call `kernel(n)`, which returns its first value in rax and its
# second in rdx, then print "<prefix> <rax>\n" and, when prefix2_len is
# nonzero, "<prefix2> <rdx>\n", in one write loop (EINTR retried, short
# writes advanced), and exit 0. Each ref.S defines kernel, prefix,
# prefix_len, prefix2 and prefix2_len.
    .intel_syntax noprefix
    .text
_start:
    mov rsi, [rsp + 16]          # argv[1]
    xor edi, edi
    test rsi, rsi
    jz 2f
1:  movzx eax, byte ptr [rsi]
    sub eax, '0'
    cmp eax, 9
    ja 2f
    imul rdi, rdi, 10
    add rdi, rax
    inc rsi
    jmp 1b
2:  call kernel
    mov r9, rdx
    sub rsp, 128
    lea r8, [rsp + 128]          # lines are rendered backwards from here
    cmp qword ptr [rip + prefix2_len], 0
    je 3f
    push rax
    mov rax, r9
    lea rsi, [rip + prefix2]
    mov rcx, [rip + prefix2_len]
    call render
    pop rax
3:  lea rsi, [rip + prefix]
    mov rcx, [rip + prefix_len]
    call render
    mov rsi, r8
    lea rdx, [rsp + 128]
    sub rdx, rsi
4:  mov edi, 1
    mov eax, 1
    syscall
    cmp rax, -4
    je 4b
    test rax, rax
    jle 5f
    add rsi, rax
    sub rdx, rax
    jnz 4b
5:  xor edi, edi
    mov eax, 231
    syscall

# render: writes "<prefix> <rax>\n" ending just before r8; r8 = its start.
# rsi = prefix, rcx = its length. Clobbers rax, rcx, rdx, rdi, rsi, r10.
render:
    dec r8
    mov byte ptr [r8], 10
    mov r10d, 10
6:  xor edx, edx
    div r10
    add dl, '0'
    dec r8
    mov [r8], dl
    test rax, rax
    jnz 6b
    dec r8
    mov byte ptr [r8], ' '
    sub r8, rcx
    mov rdi, r8
    rep movsb
    ret
