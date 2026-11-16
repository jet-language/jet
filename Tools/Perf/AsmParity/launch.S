# The gains mode's launcher: `launch EXE ARGS...` runs EXE with ARGS and the
# launcher's environment, waits for it, and writes its wait status (8 bytes)
# and struct rusage (144 bytes) to fd 3. Linux carries a process's peak RSS
# across exec (the old memory's high-water mark goes into ru_maxrss), so a
# child forked from a large parent (node, python) reports at least the
# parent's size; forked from this headers-only static blob it starts at a
# few pages. Assembled and wrapped like a cell's ref.S.
    .intel_syntax noprefix
    .text
_start:
    mov rbx, rsp
    # fork()
    mov eax, 57
    syscall
    test rax, rax
    jnz 1f
    # The child: execve(argv[1], &argv[1], envp); envp follows argv's NULL.
    mov rdi, [rbx + 16]
    lea rsi, [rbx + 16]
    mov rcx, [rbx]
    lea rdx, [rbx + rcx * 8 + 16]
    mov eax, 59
    syscall
    mov edi, 127
    mov eax, 60
    syscall
1:  # The parent: wait4(pid, &status, 0, &rusage) into 152 bytes of stack.
    sub rsp, 152
    mov rdi, rax
    mov rsi, rsp
    xor edx, edx
    lea r10, [rsp + 8]
    mov eax, 61
    syscall
    # write(3, status and rusage, 152), then exit(0).
    mov edi, 3
    mov rsi, rsp
    mov edx, 152
    mov eax, 1
    syscall
    xor edi, edi
    mov eax, 60
    syscall
