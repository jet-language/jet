# parallelgrep (gauntlet concurrency.app): every regular .txt file under the
# directory argv[1], recursively, scanned for the needle argv[2]; print
# "<path>:<count>" per file with a count above 0 (non-overlapping
# occurrences, left to right, within lines), in bytewise path order, then
# "files <matched>/<scanned> total <occurrences>".
#
# Technique:
# - the walk is getdents64 on directory descriptors (64 KiB a call), each
#   subdirectory opened with openat on its parent (O_DIRECTORY), d_type
#   classifying the entries (fstatat only when it is unknown); a file is
#   kept as {its directory, its name}, so that no path is built for a file
#   that does not match;
# - the files are scanned by one thread per CPU the process may run on
#   (sched_getaffinity; raw clone threads with CLONE_CHILD_CLEARTID, joined
#   on futexes), each claiming 64 files at a time with lock xadd; a file is
#   openat on its directory's descriptor and read into the thread's 64 KiB
#   buffer, or mapped when it does not fit;
# - the search is the AVX2 first-and-last-byte filter: 32 candidate starts
#   per step, those whose first and last bytes both match compared in
#   full; the text is checked to be UTF-8 (an OR of all bytes, the scalar
#   check only when one is not ASCII), as the Jet program's read is;
# - the matched paths alone are built, merge-sorted, and printed from one
#   buffer.
# The open-files limit is raised to its hard limit for the directory
# descriptors. A directory that cannot be walked prints "walk failed", a
# file that cannot be read (or is not UTF-8) "read failed", an empty needle
# "empty needle", each on stderr with exit 1.
    .intel_syntax noprefix
    .text

    .set ROOT, 0
    .set NEEDLE, 8
    .set NLEN, 16
    .set DIRS, 24                # {fd u32, path length u32, path offset u64}
    .set NDIRS, 32
    .set FILES, 40               # {directory u32, name length u32, name offset u64}
    .set NFILES, 48
    .set ARENA, 56               # paths and names
    .set ARENAPOS, 64
    .set NEXT, 72                # the next file to claim
    .set MATCHES, 80             # {file u64, count u64}
    .set NMATCH, 88
    .set TOTAL, 96
    .set NTHREADS, 104
    .set BLOCKS, 112
    .set SORTA, 120
    .set SORTB, 128
    .set OUT, 136
    .set WIDTH, 144
    .set LIMIT, 152              # struct rlimit (16 bytes)
    .set STAT, 176               # struct stat (144 bytes)
    .set MASK, 320               # the affinity mask (128 bytes)
    .set DENTS, 512              # 64 KiB
    .set FRAME, DENTS + 65536

    .set DIRS_MAX, 1 << 20
    .set FILES_MAX, 1 << 24
    .set ARENA_MAX, 1 << 29
    .set RESERVE, DIRS_MAX * 16 + FILES_MAX * 16 + ARENA_MAX + FILES_MAX * 16
    .set CHUNK, 64
    .set THREADS_MAX, 64

    # A thread's block: its tid word, its stat buffer, its read buffer, then
    # its stack.
    .set TB_TID, 0
    .set TB_STAT, 16
    .set TB_BUF, 4096
    .set BUFSZ, 65536
    .set BLOCK, TB_BUF + BUFSZ + 32768

_start:
    mov rax, [rsp]               # argc
    cmp rax, 3
    jb fail_walk
    mov rbx, [rsp + 16]          # argv[1]
    mov rbp, [rsp + 24]          # argv[2]
    sub rsp, FRAME
    mov r15, rsp
    mov [r15 + ROOT], rbx
    mov [r15 + NEEDLE], rbp
    mov rdi, rbp
    call strlen
    test rax, rax
    jz fail_needle
    mov [r15 + NLEN], rax
    # Open files: the soft limit raised to the hard one.
    xor edi, edi
    mov esi, 7                   # RLIMIT_NOFILE
    xor edx, edx
    lea r10, [r15 + LIMIT]
    mov eax, 302                 # prlimit64
    syscall
    mov rax, [r15 + LIMIT + 8]
    mov [r15 + LIMIT], rax
    xor edi, edi
    mov esi, 7
    lea rdx, [r15 + LIMIT]
    xor r10d, r10d
    mov eax, 302
    syscall
    # Directories, files, the arena and the matches, reserved once.
    xor edi, edi
    mov rsi, RESERVE
    mov edx, 3                   # PROT_READ | PROT_WRITE
    mov r10d, 0x4022             # MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE
    mov r8, -1
    xor r9d, r9d
    mov eax, 9                   # mmap
    syscall
    cmp rax, -4096
    ja fail_walk
    mov [r15 + DIRS], rax
    add rax, DIRS_MAX * 16
    mov [r15 + FILES], rax
    add rax, FILES_MAX * 16
    mov [r15 + ARENA], rax
    mov rcx, ARENA_MAX
    add rax, rcx
    mov [r15 + MATCHES], rax
    xor eax, eax
    mov [r15 + NFILES], rax
    mov [r15 + NEXT], rax
    mov [r15 + NMATCH], rax
    mov [r15 + TOTAL], rax

    # The root: its path without trailing slashes, then its descriptor.
    mov rdi, rbx
    call strlen
    mov rcx, rax
1:  test rcx, rcx
    jz 2f
    cmp byte ptr [rbx + rcx - 1], '/'
    jne 2f
    dec rcx
    jmp 1b
2:  mov rdi, [r15 + ARENA]
    mov rsi, rbx
    mov rdx, rcx
    rep movsb
    sub rdi, [r15 + ARENA]
    mov [r15 + ARENAPOS], rdi
    mov rdi, rbx
    mov esi, 0x90000             # O_RDONLY | O_DIRECTORY | O_CLOEXEC
    mov eax, 2                   # open
    syscall
    test eax, eax
    js fail_walk
    mov rdi, [r15 + DIRS]
    mov [rdi], eax
    mov [rdi + 4], edx
    mov qword ptr [rdi + 8], 0
    mov qword ptr [r15 + NDIRS], 1

    # The walk, breadth first: r12 the directory, rbx its entry.
    xor r12d, r12d
walk:
    cmp r12, [r15 + NDIRS]
    jae walked
    mov rbx, r12
    shl rbx, 4
    add rbx, [r15 + DIRS]
dents:
    mov edi, [rbx]
    lea rsi, [r15 + DENTS]
    mov edx, 65536
    mov eax, 217                 # getdents64
    syscall
    test rax, rax
    js fail_walk
    jz 9f
    lea r13, [r15 + DENTS]       # the entry
    lea r14, [r13 + rax]         # their end
entry:
    cmp r13, r14
    jae dents
    lea rbp, [r13 + 19]          # the name
    movzx eax, byte ptr [r13 + 18] # d_type
    cmp word ptr [rbp], '.'      # "."
    je 8f
    cmp word ptr [rbp], '.' | '.' << 8
    jne 1f
    cmp byte ptr [rbp + 2], 0    # ".."
    je 8f
1:  test eax, eax
    jnz 2f
    mov edi, [rbx]               # unknown: fstatat
    mov rsi, rbp
    lea rdx, [r15 + STAT]
    mov r10d, 0x100              # AT_SYMLINK_NOFOLLOW
    mov eax, 262                 # newfstatat
    syscall
    test rax, rax
    js fail_walk
    mov eax, [r15 + STAT + 24]   # st_mode
    shr eax, 12                  # S_IFMT >> 12 is the d_type
2:  cmp eax, 4                   # DT_DIR
    je 4f
    cmp eax, 8                   # DT_REG
    jne 8f
    # A file: kept when its name ends in ".txt".
    mov rdi, rbp
    call strlen
    cmp rax, 4
    jb 8f
    cmp dword ptr [rbp + rax - 4], 0x7478742e # ".txt"
    jne 8f
    mov rcx, [r15 + NFILES]
    cmp rcx, FILES_MAX
    jae fail_walk
    shl rcx, 4
    add rcx, [r15 + FILES]
    mov [rcx], r12d
    mov [rcx + 4], eax
    mov rdx, [r15 + ARENAPOS]
    mov [rcx + 8], rdx
    inc qword ptr [r15 + NFILES]
    lea rcx, [rax + 1]           # the name and its NUL
    call to_arena
    jmp 8f
4:  # A directory: opened on its parent, its path its parent's "/" name.
    mov edi, [rbx]
    mov rsi, rbp
    mov edx, 0x90000             # O_RDONLY | O_DIRECTORY | O_CLOEXEC
    mov eax, 257                 # openat
    syscall
    test eax, eax
    js fail_walk
    mov rcx, [r15 + NDIRS]
    cmp rcx, DIRS_MAX
    jae fail_walk
    shl rcx, 4
    add rcx, [r15 + DIRS]
    mov [rcx], eax
    mov rdx, [r15 + ARENAPOS]
    mov [rcx + 8], rdx
    inc qword ptr [r15 + NDIRS]
    push rcx
    mov ecx, [rbx + 4]           # the parent's path
    mov rsi, [rbx + 8]
    add rsi, [r15 + ARENA]
    call to_arena_from
    lea rsi, [rip + s_slash]
    mov ecx, 1
    call to_arena_from
    mov rdi, rbp                 # the name
    call strlen
    mov rcx, rax
    mov rsi, rbp
    call to_arena_from
    pop rcx
    mov rax, [r15 + ARENAPOS]
    sub rax, [rcx + 8]
    mov [rcx + 4], eax
8:  movzx eax, word ptr [r13 + 16] # d_reclen
    add r13, rax
    jmp entry
9:  inc r12
    jmp walk

walked:
    # The search's vectors: the needle's first and last bytes.
    mov rsi, [r15 + NEEDLE]
    mov rcx, [r15 + NLEN]
    vpbroadcastb ymm1, byte ptr [rsi]
    vpbroadcastb ymm2, byte ptr [rsi + rcx - 1]
    # Threads: one per CPU this process may run on.
    xor edi, edi
    mov esi, 128
    lea rdx, [r15 + MASK]
    mov eax, 204                 # sched_getaffinity
    syscall
    xor ecx, ecx
    test rax, rax
    jle 2f
    xor edx, edx
1:  popcnt r8, [r15 + MASK + rdx]
    add rcx, r8
    add rdx, 8
    cmp rdx, rax
    jb 1b
2:  mov eax, 1
    test rcx, rcx
    cmovz rcx, rax
    mov eax, THREADS_MAX
    cmp rcx, rax
    cmova rcx, rax
    mov [r15 + NTHREADS], rcx
    imul rsi, rcx, BLOCK
    xor edi, edi
    mov edx, 3
    mov r10d, 0x22               # MAP_PRIVATE | MAP_ANONYMOUS
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail_walk
    mov [r15 + BLOCKS], rax
    mov r12d, 1
3:  cmp r12, [r15 + NTHREADS]
    jae 4f
    imul rbx, r12, BLOCK
    add rbx, [r15 + BLOCKS]
    mov dword ptr [rbx + TB_TID], 1
    mov edi, 0x250f00            # VM FS FILES SIGHAND THREAD SYSVSEM CHILD_CLEARTID
    lea rsi, [rbx + BLOCK]
    xor edx, edx
    lea r10, [rbx + TB_TID]
    xor r8d, r8d
    mov eax, 56                  # clone
    syscall
    test rax, rax
    jz thread
    js fail_walk
    inc r12
    jmp 3b
4:  mov rbx, [r15 + BLOCKS]
    call worker
    mov r12d, 1                  # join
5:  cmp r12, [r15 + NTHREADS]
    jae scanned
    imul rdi, r12, BLOCK
    add rdi, [r15 + BLOCKS]
6:  mov edx, [rdi + TB_TID]
    test edx, edx
    jz 7f
    xor esi, esi                 # FUTEX_WAIT
    xor r10d, r10d
    mov eax, 202                 # futex
    syscall
    jmp 6b
7:  inc r12
    jmp 5b

thread:
    call worker
    xor edi, edi
    mov eax, 60                  # exit (the thread)
    syscall

scanned:
    # The matched paths: {pointer, length, count} entries in SORTA.
    mov rcx, [r15 + NMATCH]
    shl rcx, 5
    lea rsi, [rcx * 2 + 4096]
    mov rax, [r15 + NMATCH]
    imul rax, rax, 24
    add rsi, rax
    add rsi, [r15 + ARENAPOS]    # the paths are at most the arena again
    xor edi, edi
    mov edx, 3
    mov r10d, 0x4022
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail_walk
    mov [r15 + SORTA], rax
    mov rcx, [r15 + NMATCH]
    shl rcx, 5
    add rax, rcx
    mov [r15 + SORTB], rax
    add rax, rcx
    mov [r15 + OUT], rax
    xor r12d, r12d
1:  cmp r12, [r15 + NMATCH]
    jae 2f
    mov rbx, r12
    shl rbx, 4
    add rbx, [r15 + MATCHES]
    mov rbp, [rbx]               # the file
    shl rbp, 4
    add rbp, [r15 + FILES]
    mov eax, [rbp]
    shl rax, 4
    add rax, [r15 + DIRS]
    mov r13, [r15 + ARENAPOS]    # the path's start
    mov ecx, [rax + 4]
    mov rsi, [rax + 8]
    add rsi, [r15 + ARENA]
    call to_arena_from
    lea rsi, [rip + s_slash]
    mov ecx, 1
    call to_arena_from
    mov ecx, [rbp + 4]
    mov rsi, [rbp + 8]
    add rsi, [r15 + ARENA]
    call to_arena_from
    mov rdi, r12
    shl rdi, 5
    add rdi, [r15 + SORTA]
    mov rax, r13
    add rax, [r15 + ARENA]
    mov [rdi], rax
    mov rax, [r15 + ARENAPOS]
    sub rax, r13
    mov [rdi + 8], rax
    mov rax, [rbx + 8]
    mov [rdi + 16], rax
    inc r12
    jmp 1b
2:  # Merge sort by path: runs of WIDTH from SORTA into SORTB, then swap.
    mov qword ptr [r15 + WIDTH], 1
pass:
    mov rax, [r15 + WIDTH]
    cmp rax, [r15 + NMATCH]
    jae sorted
    xor r12d, r12d
run:
    mov rbx, [r15 + NMATCH]
    cmp r12, rbx
    jae passed
    mov rax, [r15 + WIDTH]
    lea rcx, [r12 + rax]
    cmp rcx, rbx
    cmova rcx, rbx
    lea rdx, [rcx + rax]
    cmp rdx, rbx
    cmova rdx, rbx
    mov rax, [r15 + SORTA]
    mov r8, r12
    shl r8, 5
    mov r11, [r15 + SORTB]
    add r11, r8                  # out
    add r8, rax                  # left
    shl rcx, 5
    lea r9, [rax + rcx]          # left end, right
    mov r10, r9
    mov r12, rdx
    shl rdx, 5
    lea r13, [rax + rdx]         # right end
merge:
    cmp r8, r9
    jae rest
    cmp r10, r13
    jae rest
    call right_first
    jb 1f
    vmovdqu ymm0, [r8]
    vmovdqu [r11], ymm0
    add r8, 32
    add r11, 32
    jmp merge
1:  vmovdqu ymm0, [r10]
    vmovdqu [r11], ymm0
    add r10, 32
    add r11, 32
    jmp merge
rest:
    cmp r8, r9
    jae 2f
    vmovdqu ymm0, [r8]
    vmovdqu [r11], ymm0
    add r8, 32
    add r11, 32
    jmp rest
2:  cmp r10, r13
    jae run
    vmovdqu ymm0, [r10]
    vmovdqu [r11], ymm0
    add r10, 32
    add r11, 32
    jmp 2b
passed:
    mov rax, [r15 + SORTA]
    mov rcx, [r15 + SORTB]
    mov [r15 + SORTA], rcx
    mov [r15 + SORTB], rax
    shl qword ptr [r15 + WIDTH], 1
    jmp pass

sorted:
    mov rdi, [r15 + OUT]
    xor r12d, r12d
1:  cmp r12, [r15 + NMATCH]
    jae 2f
    mov rbx, r12
    shl rbx, 5
    add rbx, [r15 + SORTA]
    mov rsi, [rbx]
    mov rcx, [rbx + 8]
    rep movsb
    mov byte ptr [rdi], ':'
    inc rdi
    mov rax, [rbx + 16]
    call put_uint
    mov byte ptr [rdi], 10
    inc rdi
    inc r12
    jmp 1b
2:  mov dword ptr [rdi], 0x656c6966  # "file"
    mov word ptr [rdi + 4], 0x2073   # "s "
    add rdi, 6
    mov rax, [r15 + NMATCH]
    call put_uint
    mov byte ptr [rdi], '/'
    inc rdi
    mov rax, [r15 + NFILES]
    call put_uint
    mov dword ptr [rdi], 0x746f7420  # " tot"
    mov dword ptr [rdi + 3], 0x206c6174 # "tal "
    add rdi, 7
    mov rax, [r15 + TOTAL]
    call put_uint
    mov byte ptr [rdi], 10
    inc rdi
    mov rsi, [r15 + OUT]
    mov rdx, rdi
    sub rdx, rsi
3:  mov edi, 1
    mov eax, 1                   # write
    syscall
    cmp rax, -4
    je 3b
    test rax, rax
    js fail_walk
    add rsi, rax
    sub rdx, rax
    jnz 3b
    xor edi, edi
    mov eax, 231                 # exit_group
    syscall

# right_first: CF set when the entry at r10 sorts before the one at r8
# (bytewise, a prefix first), so that equal paths keep their order.
right_first:
    push rsi
    push rdi
    push rcx
    mov rsi, [r10]
    mov rdi, [r8]
    mov rcx, [r10 + 8]
    mov rdx, [r8 + 8]
    cmp rdx, rcx
    cmovb rcx, rdx
    test rcx, rcx
    jz 1f
    repe cmpsb
    jne 2f
1:  mov rcx, [r10 + 8]
    cmp rcx, [r8 + 8]
2:  pop rcx
    pop rdi
    pop rsi
    ret

# worker: claims CHUNK files at a time and scans them; rbx is the thread's
# block. Adds its occurrences to TOTAL.
worker:
    push r12
    push r13
    push r14
    push rbp
    xor r14d, r14d               # occurrences
1:  mov r12d, CHUNK
    lock xadd [r15 + NEXT], r12
    mov r13, [r15 + NFILES]
    cmp r12, r13
    jae 9f
    lea rax, [r12 + CHUNK]
    cmp rax, r13
    cmova rax, r13
    mov r13, rax
2:  cmp r12, r13
    jae 1b
    call scan
    inc r12
    jmp 2b
9:  lock add [r15 + TOTAL], r14
    pop rbp
    pop r14
    pop r13
    pop r12
    ret

# scan: the file r12 read and searched; a count above 0 recorded.
scan:
    mov rax, r12
    shl rax, 4
    add rax, [r15 + FILES]
    mov ecx, [rax]
    shl rcx, 4
    add rcx, [r15 + DIRS]
    mov edi, [rcx]
    mov rsi, [rax + 8]
    add rsi, [r15 + ARENA]
    mov edx, 0x80000             # O_RDONLY | O_CLOEXEC
    mov eax, 257                 # openat
    syscall
    test eax, eax
    js fail_read
    mov ebp, eax
1:  mov edi, ebp
    lea rsi, [rbx + TB_BUF]
    mov edx, BUFSZ
    xor eax, eax                 # read
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    js fail_read
    cmp rax, BUFSZ
    je 3f
    lea rsi, [rbx + TB_BUF]
    mov rdx, rax
    call count
2:  push rax
    mov edi, ebp
    mov eax, 3                   # close
    syscall
    pop rax
    test rax, rax
    jz 4f
    add r14, rax
    mov ecx, 1
    lock xadd [r15 + NMATCH], rcx
    shl rcx, 4
    add rcx, [r15 + MATCHES]
    mov [rcx], r12
    mov [rcx + 8], rax
4:  ret
3:  # Larger than the buffer: the whole file mapped.
    mov edi, ebp
    lea rsi, [rbx + TB_STAT]
    mov eax, 5                   # fstat
    syscall
    test rax, rax
    js fail_read
    xor edi, edi
    mov rsi, [rbx + TB_STAT + 48]
    mov edx, 1                   # PROT_READ
    mov r10d, 2                  # MAP_PRIVATE
    mov r8d, ebp
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail_read
    push rax
    mov rsi, rax
    mov rdx, [rbx + TB_STAT + 48]
    call count
    pop rdi
    push rax
    mov rsi, [rbx + TB_STAT + 48]
    mov eax, 11                  # munmap
    syscall
    pop rax
    jmp 2b

# count: the needle's non-overlapping occurrences in the rdx bytes at rsi,
# left to right, in rax; "read failed" when they are not UTF-8. A match
# never spans a newline, which the needle (an argument) does not hold, so
# that the whole text is one search. Clobbers rcx, rdx, rsi, rdi, r8
# through r11, ymm3 through ymm5.
count:
    mov rdi, rsi
    lea r9, [rsi + rdx]
    vpxor ymm3, ymm3, ymm3
1:  lea rax, [rdi + 32]
    cmp rax, r9
    ja 2f
    vpor ymm3, ymm3, [rdi]
    mov rdi, rax
    jmp 1b
2:  vpmovmskb r10d, ymm3
3:  cmp rdi, r9
    jae 4f
    movzx eax, byte ptr [rdi]
    and eax, 0x80
    or r10d, eax
    inc rdi
    jmp 3b
4:  test r10d, r10d
    jz 5f
    call utf8
5:  xor eax, eax
    mov r8, rsi                  # the first start a match may take
    mov r11, [r15 + NLEN]
    mov rcx, rdx
    sub rcx, r11
    jb 9f
    lea r10, [rsi + rcx]         # the last start
    mov rdi, rsi
6:  lea rcx, [rdi + 31]
    cmp rcx, r10
    ja 8f
    vpcmpeqb ymm4, ymm1, [rdi]
    vpcmpeqb ymm5, ymm2, [rdi + r11 - 1]
    vpand ymm4, ymm4, ymm5
    vpmovmskb r9d, ymm4
7:  test r9d, r9d
    jz 71f
    tzcnt ecx, r9d
    blsr r9d, r9d
    add rcx, rdi
    cmp rcx, r8
    jb 7b
    call same
    jne 7b
    inc rax
    lea r8, [rcx + r11]
    jmp 7b
71: add rdi, 32
    jmp 6b
8:  cmp rdi, r10
    ja 9f
    cmp rdi, r8
    jb 81f
    mov rcx, rdi
    call same
    jne 81f
    inc rax
    lea r8, [rdi + r11]
81: inc rdi
    jmp 8b
9:  ret

# same: ZF when the r11 bytes at rcx are the needle.
same:
    push rsi
    push rdi
    push rcx
    mov rsi, rcx
    mov rdi, [r15 + NEEDLE]
    mov rcx, r11
    repe cmpsb
    pop rcx
    pop rdi
    pop rsi
    ret

# utf8: the rdx bytes at rsi checked to be UTF-8, or "read failed".
# Clobbers rax, rcx, rdi, r8, r9, r10.
utf8:
    mov rdi, rsi
    lea r9, [rsi + rdx]
1:  cmp rdi, r9
    jae 9f
    movzx eax, byte ptr [rdi]
    inc rdi
    cmp eax, 0x80
    jb 1b
    mov r8d, 0x80                # the second byte's bounds
    mov r10d, 0xbf
    cmp eax, 0xc2
    jb fail_read
    mov ecx, 1
    cmp eax, 0xe0
    jb 3f
    mov ecx, 2
    cmp eax, 0xf0
    jae 2f
    cmp eax, 0xe0
    jne 11f
    mov r8d, 0xa0
11: cmp eax, 0xed
    jne 3f
    mov r10d, 0x9f
    jmp 3f
2:  mov ecx, 3
    cmp eax, 0xf4
    ja fail_read
    cmp eax, 0xf0
    jne 21f
    mov r8d, 0x90
21: cmp eax, 0xf4
    jne 3f
    mov r10d, 0x8f
3:  cmp rdi, r9
    jae fail_read
    movzx eax, byte ptr [rdi]
    inc rdi
    cmp eax, r8d
    jb fail_read
    cmp eax, r10d
    ja fail_read
    mov r8d, 0x80
    mov r10d, 0xbf
    dec ecx
    jnz 3b
    jmp 1b
9:  ret

# to_arena: rcx bytes at rbp appended to the arena. to_arena_from: from rsi.
# Clobbers rcx, rsi, rdi.
to_arena:
    mov rsi, rbp
to_arena_from:
    mov rdi, [r15 + ARENAPOS]
    lea rax, [rdi + rcx]
    cmp rax, ARENA_MAX
    ja fail_walk
    mov [r15 + ARENAPOS], rax
    add rdi, [r15 + ARENA]
    rep movsb
    ret

# strlen: the length of the NUL-terminated rdi, in rax.
strlen:
    mov rax, rdi
1:  cmp byte ptr [rax], 0
    je 2f
    inc rax
    jmp 1b
2:  sub rax, rdi
    ret

# put_uint: rax in decimal at rdi. Clobbers rax, rcx, rdx, r8, r9.
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

fail_walk:
    lea rsi, [rip + s_walk]
    mov edx, 12
    jmp fail
fail_read:
    lea rsi, [rip + s_read]
    mov edx, 12
    jmp fail
fail_needle:
    lea rsi, [rip + s_needle]
    mov edx, 13
fail:
    mov edi, 2
    mov eax, 1                   # write
    syscall
    mov edi, 1
    mov eax, 231
    syscall

s_slash:  .ascii "/"
s_walk:   .ascii "walk failed\n"
s_read:   .ascii "read failed\n"
s_needle: .ascii "empty needle\n"
