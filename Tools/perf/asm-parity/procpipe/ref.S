# procpipe (gauntlet files.orchestration): four child-process steps, one
# transcript line each:
# - `python3 -c "print(6*7)"` with its stdout captured: "calc <trimmed>";
# - `python3 -c "print('b');print('a');print('c')"` piped into `sort`, the
#   sort's stdout captured: "sorted <trimmed lines joined by ,>";
# - `python3 -c "import time;time.sleep(5)"` given 300ms, killed when it
#   outlasts them: "slow timeout";
# - `python3 -c "import sys;sys.exit(3)"`: "exit <code>" when it fails.
#
# Technique: a child is vfork, dup2 onto its standard streams, then execve
# along PATH (the execvp search; "/usr/local/bin:/usr/bin:/bin" without
# one), so that nothing of the parent is copied; pipes are pipe2 with
# O_CLOEXEC, so that a child holds only its own ends; the timeout is a
# pidfd (pidfd_open) polled for 300ms, then SIGKILL and reap. The lines
# leave in one write at the end.
# A child that cannot be started (its execve fails along all of PATH, which
# the child reports through the memory vfork shares) stops the program with
# the Jet program's panic text, "calc failed", "sort failed", "slow failed"
# or "checked process failed", on stderr with exit 1.
    .intel_syntax noprefix
    .text

    .set ENVP, 0
    .set PATHV, 8
    .set FAILED, 16              # set by a child whose execve failed
    .set PIPE1, 24               # two descriptors
    .set PIPE2, 32
    .set POLLFD, 40
    .set STATUS, 48
    .set PIDA, 56
    .set PIDB, 64
    .set ARGV, 72                # three arguments and the null (32 bytes)
    .set OUTLEN, 104
    .set PATHBUF, 128            # 4096 bytes
    .set OUT, 4224               # 256 bytes
    .set BUF, 4480
    .set BUFSZ, 16384
    .set FRAME, BUF + BUFSZ

_start:
    mov rax, [rsp]               # argc
    lea rbx, [rsp + rax * 8 + 16] # envp
    sub rsp, FRAME
    mov r15, rsp
    mov [r15 + ENVP], rbx
    lea rax, [rip + s_default_path]
    mov [r15 + PATHV], rax
1:  mov rsi, [rbx]
    test rsi, rsi
    jz 2f
    add rbx, 8
    cmp dword ptr [rsi], 0x48544150  # "PATH"
    jne 1b
    cmp byte ptr [rsi + 4], '='
    jne 1b
    add rsi, 5
    mov [r15 + PATHV], rsi
2:  lea rax, [r15 + OUT]
    mov [r15 + OUTLEN], rax      # the output's end

    # calc: the arithmetic child's stdout, trimmed.
    lea rdi, [r15 + PIPE1]
    call open_pipe
    lea rdx, [rip + s_calc]
    call python
    mov r8d, -1
    mov r9d, [r15 + PIPE1 + 4]
    call spawn
    mov rbx, rax
    mov edi, [r15 + PIPE1 + 4]
    call close
    mov edi, [r15 + PIPE1]
    call read_all
    mov r12, rax
    mov edi, [r15 + PIPE1]
    call close
    mov rdi, rbx
    call reap
    cmp dword ptr [r15 + FAILED], 0
    jne fail_calc
    mov rdi, [r15 + OUTLEN]
    mov dword ptr [rdi], 0x636c6163  # "calc"
    mov byte ptr [rdi + 4], ' '
    add rdi, 5
    mov rcx, r12
    xor edx, edx                 # newlines kept
    call put_trimmed

    # sorted: the lines child piped into sort.
    lea rdi, [r15 + PIPE1]
    call open_pipe
    lea rdi, [r15 + PIPE2]
    call open_pipe
    lea rdx, [rip + s_lines]
    call python
    mov r8d, -1
    mov r9d, [r15 + PIPE1 + 4]
    call spawn
    mov [r15 + PIDA], rax
    mov ebx, [r15 + FAILED]
    lea rax, [rip + s_sort]
    mov [r15 + ARGV], rax
    mov qword ptr [r15 + ARGV + 8], 0
    mov r8d, [r15 + PIPE1]
    mov r9d, [r15 + PIPE2 + 4]
    call spawn
    mov [r15 + PIDB], rax
    or ebx, [r15 + FAILED]
    mov edi, [r15 + PIPE1]
    call close
    mov edi, [r15 + PIPE1 + 4]
    call close
    mov edi, [r15 + PIPE2 + 4]
    call close
    mov edi, [r15 + PIPE2]
    call read_all
    mov r12, rax
    mov edi, [r15 + PIPE2]
    call close
    mov rdi, [r15 + PIDA]
    call reap
    mov rdi, [r15 + PIDB]
    call reap
    test ebx, ebx
    jnz fail_sort
    mov rdi, [r15 + OUTLEN]
    lea rsi, [rip + s_sorted]
    mov ecx, 7
    rep movsb
    mov rcx, r12
    mov edx, 1                   # newlines become commas
    call put_trimmed

    # slow: 300ms, then SIGKILL.
    lea rdx, [rip + s_sleep]
    call python
    mov r8d, -1
    mov r9d, -1
    call spawn
    cmp dword ptr [r15 + FAILED], 0
    jne fail_slow
    mov rbx, rax
    mov edi, eax
    xor esi, esi
    mov eax, 434                 # pidfd_open
    syscall
    test eax, eax
    js fail_slow
    mov [r15 + POLLFD], eax
    mov dword ptr [r15 + POLLFD + 4], 1 # POLLIN
3:  lea rdi, [r15 + POLLFD]
    mov esi, 1
    mov edx, 300
    mov eax, 7                   # poll
    syscall
    cmp rax, -4
    je 3b
    mov r12, rax
    test rax, rax
    jnz 4f
    mov rdi, rbx
    mov esi, 9                   # SIGKILL
    mov eax, 62                  # kill
    syscall
4:  mov rdi, rbx
    call reap
    mov edi, [r15 + POLLFD]
    call close
    test r12, r12
    jnz 5f
    mov rdi, [r15 + OUTLEN]
    lea rsi, [rip + s_slow]
    mov ecx, 13
    rep movsb
    mov [r15 + OUTLEN], rdi

5:  # exit: the failing child's code.
    lea rdx, [rip + s_exit3]
    call python
    mov r8d, -1
    mov r9d, -1
    call spawn
    cmp dword ptr [r15 + FAILED], 0
    jne fail_checked
    mov rdi, rax
    call reap
    mov eax, [r15 + STATUS]
    test eax, 0x7f               # exited, not signalled
    jnz 6f
    shr eax, 8
    movzx eax, al
    test eax, eax
    jz 6f
    mov rdi, [r15 + OUTLEN]
    mov dword ptr [rdi], 0x74697865  # "exit"
    mov byte ptr [rdi + 4], ' '
    add rdi, 5
    call put_uint
    mov byte ptr [rdi], 10
    inc rdi
    mov [r15 + OUTLEN], rdi

6:  lea rsi, [r15 + OUT]
    mov rdx, [r15 + OUTLEN]
    sub rdx, rsi
7:  test rdx, rdx
    jz 8f
    mov edi, 1
    mov eax, 1                   # write
    syscall
    cmp rax, -4
    je 7b
    test rax, rax
    js 8f
    add rsi, rax
    sub rdx, rax
    jmp 7b
8:  xor edi, edi
    mov eax, 231                 # exit_group
    syscall

# python: ARGV = python3 -c <rdx>.
python:
    lea rax, [rip + s_python3]
    mov [r15 + ARGV], rax
    lea rax, [rip + s_dash_c]
    mov [r15 + ARGV + 8], rax
    mov [r15 + ARGV + 16], rdx
    mov qword ptr [r15 + ARGV + 24], 0
    ret

# open_pipe: pipe2(rdi, O_CLOEXEC).
open_pipe:
    mov esi, 0x80000
    mov eax, 293                 # pipe2
    syscall
    test rax, rax
    js fail_calc
    ret

close:
    mov eax, 3
    syscall
    ret

# spawn: ARGV as a child with stdin r8d and stdout r9d (-1: inherited); its
# pid in rax. FAILED is 1 when it could not be started. The child pushes
# nothing at or above the return address, which the parent still needs.
spawn:
    mov dword ptr [r15 + FAILED], 0
    mov eax, 58                  # vfork
    syscall
    test rax, rax
    js 9f
    jz 1f
    ret
9:  mov dword ptr [r15 + FAILED], 1
    ret
1:  cmp r8d, -1
    je 2f
    mov edi, r8d
    xor esi, esi
    mov eax, 33                  # dup2
    syscall
2:  cmp r9d, -1
    je 3f
    mov edi, r9d
    mov esi, 1
    mov eax, 33
    syscall
3:  mov rbx, [r15 + ARGV]        # the program
    mov rsi, rbx
4:  lodsb                        # a "/" in it: no search
    test al, al
    jz 5f
    cmp al, '/'
    jne 4b
    mov rdi, rbx
    lea rsi, [r15 + ARGV]
    mov rdx, [r15 + ENVP]
    mov eax, 59                  # execve
    syscall
    jmp 8f
5:  mov r12, [r15 + PATHV]       # one PATH entry per try
6:  lea rdi, [r15 + PATHBUF]
    lea r13, [rdi + 4000]
7:  movzx eax, byte ptr [r12]
    test al, al
    jz 71f
    inc r12
    cmp al, ':'
    je 72f
    cmp rdi, r13
    jae 7b
    stosb
    jmp 7b
71: xor r12d, r12d               # the last entry
72: lea rax, [r15 + PATHBUF]
    cmp rdi, rax
    jne 73f
    mov byte ptr [rdi], '.'      # an empty entry is the current directory
    inc rdi
73: mov byte ptr [rdi], '/'
    inc rdi
    mov rsi, rbx
74: lodsb
    stosb
    test al, al
    jnz 74b
    lea rdi, [r15 + PATHBUF]
    lea rsi, [r15 + ARGV]
    mov rdx, [r15 + ENVP]
    mov eax, 59                  # execve
    syscall
    test r12, r12
    jnz 6b
8:  mov dword ptr [r15 + FAILED], 1
    mov edi, 127
    mov eax, 60                  # exit
    syscall

# read_all: the descriptor edi to its end, the first BUFSZ bytes kept at
# BUF; their count in rax.
read_all:
    mov r8d, edi
    xor r9d, r9d
1:  mov edi, r8d
    lea rsi, [r15 + BUF + r9]
    mov edx, BUFSZ
    sub edx, r9d
    jnz 2f
    lea rsi, [r15 + PATHBUF]     # full: the rest is drained
    mov edx, 4096
2:  xor eax, eax                 # read
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    jle 3f
    cmp r9d, BUFSZ
    je 1b
    add r9, rax
    jmp 1b
3:  mov rax, r9
    ret

# reap: wait for the pid rdi; its status at STATUS.
reap:
    lea rsi, [r15 + STATUS]
    xor edx, edx
    xor r10d, r10d
    mov eax, 61                  # wait4
    syscall
    cmp rax, -4
    je reap
    ret

# put_trimmed: the rcx bytes at BUF without leading and trailing ASCII
# whitespace, then a newline, at rdi; with edx 1 a newline in them becomes
# ",". Advances OUTLEN.
put_trimmed:
    lea rsi, [r15 + BUF]
    lea r8, [rsi + rcx]
1:  cmp rsi, r8
    jae 3f
    movzx eax, byte ptr [rsi]
    call is_space
    jne 2f
    inc rsi
    jmp 1b
2:  movzx eax, byte ptr [r8 - 1]
    call is_space
    jne 3f
    dec r8
    jmp 2b
3:  cmp rsi, r8
    jae 5f
    lodsb
    test edx, edx
    jz 4f
    cmp al, 10
    jne 4f
    mov al, ','
4:  stosb
    jmp 3b
5:  mov byte ptr [rdi], 10
    inc rdi
    mov [r15 + OUTLEN], rdi
    ret

# is_space: ZF when eax is a space or \t through \r.
is_space:
    cmp eax, ' '
    je 1f
    sub eax, 9
    cmp eax, 4
    ja 2f
    cmp eax, eax
1:  ret
2:  test esp, esp                # ZF clear: rsp is never 0
    ret

# put_uint: eax in decimal at rdi. Clobbers rax, rcx, rdx, r8, r9.
put_uint:
    lea r8, [rsp - 8]            # the digits, in the red zone
    mov r9, r8
    mov ecx, 10
1:  xor edx, edx
    div ecx
    add dl, '0'
    dec r8
    mov [r8], dl
    test eax, eax
    jnz 1b
2:  mov dl, [r8]
    mov [rdi], dl
    inc rdi
    inc r8
    cmp r8, r9
    jb 2b
    ret

fail_calc:
    lea rsi, [rip + s_fail_calc]
    mov edx, 12
    jmp fail
fail_sort:
    lea rsi, [rip + s_fail_sort]
    mov edx, 12
    jmp fail
fail_slow:
    lea rsi, [rip + s_fail_slow]
    mov edx, 12
    jmp fail
fail_checked:
    lea rsi, [rip + s_fail_checked]
    mov edx, 23
fail:
    mov edi, 2
    mov eax, 1                   # write
    syscall
    mov edi, 1
    mov eax, 231
    syscall

s_default_path: .asciz "/usr/local/bin:/usr/bin:/bin"
s_python3:      .asciz "python3"
s_dash_c:       .asciz "-c"
s_calc:         .asciz "print(6*7)"
s_lines:        .asciz "print('b');print('a');print('c')"
s_sort:         .asciz "sort"
s_sleep:        .asciz "import time;time.sleep(5)"
s_exit3:        .asciz "import sys;sys.exit(3)"
s_sorted:       .ascii "sorted "
s_slow:         .ascii "slow timeout\n"
s_fail_calc:    .ascii "calc failed\n"
s_fail_sort:    .ascii "sort failed\n"
s_fail_slow:    .ascii "slow failed\n"
s_fail_checked: .ascii "checked process failed\n"
