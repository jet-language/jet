# embedded-sensor-ring (gauntlet embedded.kernel): replay the sensor-ring
# firmware core for argv[1] ticks (a decimal) and print its six
# report lines, exactly as Tools/gauntlet/entries/embedded-sensor-ring
# defines them: per tick a 12-bit sample, a 16-bit frame and its crc16, a
# corrupted crc on every 127th tick (rejected), else the sample into a
# 16-slot ring and a clamped fixed-point controller; an MMIO checksum over
# status, control and crc mod 1000000007; a watchdog every 64 ticks.
#
# Technique:
# - the per-tick inputs depend only on the tick, and every one of them is
#   arithmetic mod 2^16 (raw mod 4096 divides it), so AVX2 computes 16
#   ticks at a time in 16-bit lanes: raw, frame, sample and the crc16 of
#   the frame, its 16 bit steps unrolled (a step is crc*2 plus 4129 when
#   crc's top bit differs from the frame's next bit: shift the frame's bit
#   to the top, xor, sign-spread, and);
# - the controller and the checksum are serial, so the scalar loop does
#   only them: ring, running sum, clamps (cmov), the truncating /8, and
#   checksum*33 + x;
# - the checksum is reduced once per 6 ticks (33^6 times a residue plus
#   the inputs stays below 2^61), by a multiply-high with
#   ceil(2^93 / 1000000007), exact below 2^61, and a multiply-subtract;
# - accepted, rejected, faults and watchdog resets have closed forms in
#   the tick count: rejected = faults = ceil(ticks / 127) and
#   resets = floor(ticks / 64) for ticks > 0.
# The input class: one argument, its digits only (leading zeros allowed),
# at most 10^17 + 9 ticks, so that no Int of the Jet program's
# overflows. Exit 2 outside it (the Jet program's CLI rejects most of those
# with status 2 and its usage report, but also takes "+5" and " 5"), 1 when
# stdout cannot be written. The Jet program has no stop inside the class.
    .intel_syntax noprefix
    .text

    .set RING, 0                 # 16 slots, 8 bytes each
    .set CRCS, 128               # one block's crc16s, 16 words
    .set SMPS, 160               # one block's samples, 16 words
    .set TICKS, 192
    .set OUT, 256
    .set FRAME, 512
    .set P, 1000000007

_start:
    cmp qword ptr [rsp], 2       # argc: the one argument
    jne class
    mov rsi, [rsp + 16]          # argv[1]
    sub rsp, FRAME
    mov r8, 10000000000000000    # 10^16
    xor eax, eax
    movzx edx, byte ptr [rsi]
    sub edx, '0'
    cmp edx, 9
    ja class                     # no digit
2:  cmp rax, r8
    ja class                     # past 10^17 + 9
    imul rax, rax, 10
    add rax, rdx
    inc rsi
    movzx edx, byte ptr [rsi]
    sub edx, '0'
    cmp edx, 9
    jbe 2b
    cmp byte ptr [rsi], 0
    jne class
    mov [rsp + TICKS], rax
    mov r12, rax                 # ticks left

    vpxor xmm0, xmm0, xmm0
    vmovdqu [rsp + RING], ymm0
    vmovdqu [rsp + RING + 32], ymm0
    vmovdqu [rsp + RING + 64], ymm0
    vmovdqu [rsp + RING + 96], ymm0
    vmovdqu ymm15, [rip + iota]  # the block's ticks mod 2^16
    vpbroadcastw ymm14, [rip + w16]
    vpbroadcastw ymm13, [rip + w73]
    vpbroadcastw ymm12, [rip + w19]
    vpbroadcastw ymm11, [rip + w4095]
    vpbroadcastw ymm10, [rip + w17]
    vpbroadcastw ymm9, [rip + w31]
    vpbroadcastw ymm8, [rip + w7]
    vpbroadcastw ymm7, [rip + w219]
    vpbroadcastw ymm6, [rip + w68]
    vpbroadcastw ymm5, [rip + w4129]

    mov r13d, 1                  # ticks to the next rejected one
    xor r14d, r14d               # running sum
    xor r15d, r15d               # integrator
    xor ebp, ebp                 # actuator
    xor esi, esi                 # checksum, reduced every 6 ticks
    xor edi, edi                 # ring position
    mov r11d, 6

block:
    test r12, r12
    jle done
    vpmullw ymm0, ymm15, ymm13
    vpaddw ymm0, ymm0, ymm12
    vpand ymm0, ymm0, ymm11      # raw
    vpmullw ymm0, ymm0, ymm10
    vpmullw ymm1, ymm15, ymm9
    vpaddw ymm0, ymm0, ymm1
    vpaddw ymm0, ymm0, ymm8      # frame
    vpmullw ymm1, ymm15, ymm7
    vpaddw ymm1, ymm1, ymm6
    vpand ymm1, ymm1, ymm11      # sample = (raw * 3 + 11) mod 4096
    vmovdqu [rsp + SMPS], ymm1
    vpcmpeqw ymm2, ymm2, ymm2    # crc = 65535
    .irp bit, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15
    vpsllw ymm3, ymm0, 15 - \bit
    vpxor ymm3, ymm3, ymm2
    vpsraw ymm3, ymm3, 15
    vpand ymm3, ymm3, ymm5
    vpaddw ymm2, ymm2, ymm2
    vpaddw ymm2, ymm2, ymm3
    .endr
    vmovdqu [rsp + CRCS], ymm2
    vpaddw ymm15, ymm15, ymm14
    xor ebx, ebx

tick:
    movzx eax, word ptr [rsp + CRCS + rbx * 2]
    movzx ecx, word ptr [rsp + SMPS + rbx * 2]
    dec r13
    jz reject
    mov rdx, [rsp + RING + rdi * 8]
    mov [rsp + RING + rdi * 8], rcx
    inc edi
    and edi, 15
    add r14, rcx
    sub r14, rdx
    mov rdx, r14
    shr rdx, 4                   # mean (the sum is never negative)
    mov r8d, 2048
    sub r8, rdx                  # error
    add r15, r8
    mov r9, -8192
    cmp r15, r9
    cmovl r15, r9
    mov r9d, 8192
    cmp r15, r9
    cmovg r15, r9
    mov r9, r15
    sar r9, 63
    and r9d, 7
    add r9, r15
    sar r9, 3                    # integrator / 8, toward zero
    lea rbp, [r9 + r8 * 4]
    mov r9, -4095
    cmp rbp, r9
    cmovl rbp, r9
    mov r9d, 4095
    cmp rbp, r9
    cmovg rbp, r9
    lea rax, [rax + rbp + 257 + 4095]    # status 1, control, crc
    jmp mix
reject:
    mov r13d, 127
    inc eax
    movzx eax, ax                # the corrupted crc
    lea rax, [rax + rbp + 514 + 4095]    # status 2, control, crc
mix:
    mov rdx, rsi
    shl rdx, 5
    add rsi, rdx
    add rsi, rax
    dec r11
    jnz 1f
    call reduce
    mov r11d, 6
1:  inc ebx
    dec r12
    jz done
    cmp ebx, 16
    jb tick
    jmp block

done:
    call reduce
    mov r15, rsi                 # mmio_checksum
    xor eax, eax                 # ring checksum
    xor ecx, ecx
    mov r8d, P
1:  imul rax, rax, 131
    add rax, [rsp + RING + rcx * 8]
    xor edx, edx
    div r8
    mov rax, rdx
    inc ecx
    cmp ecx, 16
    jb 1b
    mov r14, rax

    lea rdi, [rsp + OUT]
    lea rsi, [rip + s_ticks]
    mov rax, [rsp + TICKS]
    call line
    mov rcx, [rsp + TICKS]       # rejected = ceil(ticks / 127) when ticks > 0
    xor r8d, r8d
    xor r9d, r9d
    test rcx, rcx
    jle 2f
    lea rax, [rcx + 126]
    xor edx, edx
    mov r10d, 127
    div r10
    mov r8, rax
    mov r9, rcx
    shr r9, 6                    # watchdog resets
    sub rcx, r8                  # accepted
    jmp 3f
2:  xor ecx, ecx
3:  push r9
    push r8
    mov rax, rcx
    lea rsi, [rip + s_accepted]
    call word_
    mov rax, [rsp]
    lea rsi, [rip + s_rejected]
    call line
    mov rax, [rsp + 8]
    lea rsi, [rip + s_resets]
    call word_
    pop rax
    lea rsi, [rip + s_faults]
    call line
    pop rax
    mov rax, rbp
    lea rsi, [rip + s_actuator]
    call line
    mov rax, r15
    lea rsi, [rip + s_mmio]
    call line
    mov rax, r14
    lea rsi, [rip + s_ring]
    call line

    lea rsi, [rsp + OUT]
    mov rdx, rdi
    sub rdx, rsi
4:  mov edi, 1
    mov eax, 1
    syscall
    cmp rax, -4
    je 4b
    test rax, rax
    jle fail
    add rsi, rax
    sub rdx, rax
    jnz 4b
    xor edi, edi
    mov eax, 231
    syscall

fail:
    mov edi, 1
    mov eax, 231
    syscall

class:
    mov edi, 2
    mov eax, 231
    syscall

# rsi = rsi mod P, for rsi below 2^61. Clobbers rax, rdx.
reduce:
    mov rax, rsi
    movabs rdx, 0x89705f3112a28fe5   # ceil(2^93 / P)
    mul rdx
    shr rdx, 29
    imul rdx, rdx, P
    sub rsi, rdx
    ret

# At rdi: the counted string at rsi, a space, rax in signed decimal, then a
# newline (line) or a space (word_). Clobbers rax, rcx, rdx, rsi, r8, r10.
line:
    mov r8d, 10
    jmp 1f
word_:
    mov r8d, ' '
1:  movzx ecx, byte ptr [rsi]
    inc rsi
    rep movsb
    mov byte ptr [rdi], ' '
    inc rdi
    test rax, rax
    jns 2f
    mov byte ptr [rdi], '-'
    inc rdi
    neg rax
2:  mov rsi, rax                 # count the digits
    mov ecx, 1
    mov r10d, 10
3:  cmp rsi, 10
    jb 4f
    xor edx, edx
    push rax
    mov rax, rsi
    div r10
    mov rsi, rax
    pop rax
    inc ecx
    jmp 3b
4:  add rdi, rcx
    mov rsi, rdi
5:  xor edx, edx
    div r10
    add dl, '0'
    dec rsi
    mov [rsi], dl
    test rax, rax
    jnz 5b
    mov [rdi], r8b
    inc rdi
    ret

iota:
    .short 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15
w16:   .short 16
w73:   .short 73
w19:   .short 19
w4095: .short 4095
w17:   .short 17
w31:   .short 31
w7:    .short 7
w219:  .short 219
w68:   .short 68
w4129: .short 4129
s_ticks:    .byte 5
    .ascii "ticks"
s_accepted: .byte 8
    .ascii "accepted"
s_rejected: .byte 8
    .ascii "rejected"
s_resets:   .byte 15
    .ascii "watchdog_resets"
s_faults:   .byte 6
    .ascii "faults"
s_actuator: .byte 8
    .ascii "actuator"
s_mmio:     .byte 13
    .ascii "mmio_checksum"
s_ring:     .byte 13
    .ascii "ring_checksum"
