# The gauntlet fft-4096 (Tools/gauntlet/entries/fft-4096/jet/run.jet) by
# hand: the 4096-point discrete Fourier transform of a unit impulse, every
# real part checked within 1e-9 of 1 and every imaginary part within 1e-9 of
# 0, then "fft4096 checked=8192 tolerance=1e-9".
# The transform is compute.fft's: in-order iterative radix-2 over separate
# real and imaginary arrays, twiddles e^(-2 pi i k / 4096). Here:
# - the twiddles are computed, not stored: cos and sin of k pi / 2048 for
#   k < 1024 by their Taylor series to x^21 / x^20 (error below 1e-16 on
#   [0, pi/2)), four angles to a vector, and the other half of the circle by
#   symmetry; each stage then gets its own contiguous table;
# - the input goes into place bit-reversed;
# - stages 1 and 2 run inside registers (unpck / vperm2f128 and a blend for
#   the -i twiddle), the other ten as AVX2 butterflies, four to a vector,
#   with FMA;
# - the check is four lanes at a time (a NaN fails it, as in run.jet).
# Errors: no memory, or a value out of tolerance -> exit 1.
    .intel_syntax noprefix
    .text

.set N, 4096
.set XRE, 0                      # the input, then the bit-reversed copy
.set XIM, XRE + N * 8
.set YRE, XIM + N * 8
.set YIM, YRE + N * 8
.set BRE, YIM + N * 8            # e^(-2 pi i k / N), k < N / 2
.set BIM, BRE + N * 4
.set TW, BIM + N * 4             # per stage of half h >= 4: h reals, h imaginaries
.set SIZE, (TW + 16 * N + 4095) & -4096

_start:
    xor edi, edi                 # mmap(NULL, SIZE, RW, PRIVATE|ANON)
    mov esi, SIZE
    mov edx, 3
    mov r10d, 0x22
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja 9f
    mov r15, rax
    # the signal: 1, then zeros (the mapping is zero)
    mov rax, 0x3ff0000000000000
    mov [r15 + XRE], rax
    # the twiddles: k and k + N/4 for k < N/4, four at a time
    vbroadcastsd ymm14, [rip + theta]
    vmovupd ymm13, [rip + k0123]
    vbroadcastsd ymm12, [rip + four]
    vbroadcastsd ymm11, [rip + sign]
    xor eax, eax
1:  vmulpd ymm0, ymm13, ymm14    # x
    vmulpd ymm1, ymm0, ymm0      # x^2
    vbroadcastsd ymm2, [rip + sinc + 80]
    vbroadcastsd ymm3, [rip + cosc + 80]
    .irp i, 72, 64, 56, 48, 40, 32, 24, 16, 8, 0
    vbroadcastsd ymm15, [rip + sinc + \i]
    vfmadd213pd ymm2, ymm1, ymm15
    vbroadcastsd ymm15, [rip + cosc + \i]
    vfmadd213pd ymm3, ymm1, ymm15
    .endr
    vmulpd ymm2, ymm2, ymm0      # sin x
    vxorpd ymm4, ymm2, ymm11     # -sin x
    vxorpd ymm5, ymm3, ymm11     # -cos x
    vmovapd [r15 + BRE + rax], ymm3
    vmovapd [r15 + BIM + rax], ymm4
    vmovapd [r15 + BRE + N * 2 + rax], ymm4   # e^(-i (pi/2 + x)) = -sin x - i cos x
    vmovapd [r15 + BIM + N * 2 + rax], ymm5
    vaddpd ymm13, ymm13, ymm12
    add eax, 32
    cmp eax, N * 2
    jb 1b
    # each stage's table: twiddle j * (N / 2h), j < h, for h = 4 .. N/2
    lea rdi, [r15 + TW]
    mov ecx, 4                   # h
    mov edx, N / 8 * 8           # the source stride in bytes: N / (2h) * 8
2:  xor esi, esi
    xor r8d, r8d
21: mov rax, [r15 + BRE + rsi]
    mov [rdi + r8 * 8], rax
    mov rax, [r15 + BIM + rsi]
    lea r9, [rdi + rcx * 8]
    mov [r9 + r8 * 8], rax
    add rsi, rdx
    inc r8
    cmp r8, rcx
    jb 21b
    lea rdi, [rdi + rcx * 8]
    lea rdi, [rdi + rcx * 8]
    shr edx, 1
    add ecx, ecx
    cmp ecx, N / 2
    jbe 2b
    # the input bit-reversed into Y
    xor ecx, ecx                 # i
    xor edx, edx                 # reversed i
3:  mov rax, [r15 + XRE + rdx * 8]
    mov [r15 + YRE + rcx * 8], rax
    mov rax, [r15 + XIM + rdx * 8]
    mov [r15 + YIM + rcx * 8], rax
    inc ecx
    cmp ecx, N
    jae 4f
    mov eax, N / 2
31: test edx, eax
    jz 32f
    xor edx, eax
    shr eax, 1
    jmp 31b
32: or edx, eax
    jmp 3b
    # stage 1 (h = 1): (e0, e1) -> (e0 + e1, e0 - e1), eight elements a turn
4:  xor eax, eax
41: .irp part, YRE, YIM
    vmovapd ymm0, [r15 + \part + rax]
    vmovapd ymm1, [r15 + \part + rax + 32]
    vunpcklpd ymm2, ymm0, ymm1   # the even elements
    vunpckhpd ymm3, ymm0, ymm1   # the odd
    vaddpd ymm4, ymm2, ymm3
    vsubpd ymm5, ymm2, ymm3
    vunpcklpd ymm0, ymm4, ymm5
    vunpckhpd ymm1, ymm4, ymm5
    vmovapd [r15 + \part + rax], ymm0
    vmovapd [r15 + \part + rax + 32], ymm1
    .endr
    add eax, 64
    cmp eax, N * 8
    jb 41b
    # stage 2 (h = 2): (e0, e2) with 1, (e1, e3) with -i, two groups a turn
    xor eax, eax
5:  vmovapd ymm0, [r15 + YRE + rax]
    vmovapd ymm1, [r15 + YRE + rax + 32]
    vmovapd ymm2, [r15 + YIM + rax]
    vmovapd ymm3, [r15 + YIM + rax + 32]
    vperm2f128 ymm4, ymm0, ymm1, 0x20    # top real: e0 e1 of both groups
    vperm2f128 ymm5, ymm0, ymm1, 0x31    # bottom real: e2 e3
    vperm2f128 ymm6, ymm2, ymm3, 0x20
    vperm2f128 ymm7, ymm2, ymm3, 0x31
    vxorpd ymm8, ymm5, ymm11             # -bottom real
    vblendpd ymm9, ymm5, ymm7, 0b1010    # t real: e2 re, e3 im
    vblendpd ymm10, ymm7, ymm8, 0b1010   # t imaginary: e2 im, -e3 re
    vaddpd ymm0, ymm4, ymm9
    vsubpd ymm1, ymm4, ymm9
    vaddpd ymm2, ymm6, ymm10
    vsubpd ymm3, ymm6, ymm10
    vperm2f128 ymm4, ymm0, ymm1, 0x20
    vperm2f128 ymm5, ymm0, ymm1, 0x31
    vperm2f128 ymm6, ymm2, ymm3, 0x20
    vperm2f128 ymm7, ymm2, ymm3, 0x31
    vmovapd [r15 + YRE + rax], ymm4
    vmovapd [r15 + YRE + rax + 32], ymm5
    vmovapd [r15 + YIM + rax], ymm6
    vmovapd [r15 + YIM + rax + 32], ymm7
    add eax, 64
    cmp eax, N * 8
    jb 5b
    # stages h = 4 .. N/2: a + w b, a - w b, four butterflies a vector
    lea r9, [r15 + TW]           # the stage's twiddle reals
    mov ecx, 4 * 8               # h, in bytes
6:  lea r10, [r9 + rcx]          # its imaginaries
    lea rsi, [r15 + YRE]         # the block's reals
    lea rdi, [r15 + YIM]
61: xor eax, eax
    lea rbx, [rsi + rcx]         # the block's second half
    lea r11, [rdi + rcx]
62: vmovapd ymm0, [rsi + rax]    # a
    vmovapd ymm1, [rbx + rax]    # b
    vmovapd ymm2, [rdi + rax]
    vmovapd ymm3, [r11 + rax]
    vmovapd ymm4, [r9 + rax]     # w
    vmovapd ymm5, [r10 + rax]
    vmulpd ymm6, ymm3, ymm5
    vfmsub231pd ymm6, ymm1, ymm4     # (w b) real
    vmulpd ymm7, ymm3, ymm4
    vfmadd231pd ymm7, ymm1, ymm5     # (w b) imaginary
    vaddpd ymm8, ymm0, ymm6
    vsubpd ymm9, ymm0, ymm6
    vaddpd ymm10, ymm2, ymm7
    vsubpd ymm12, ymm2, ymm7
    vmovapd [rsi + rax], ymm8
    vmovapd [rbx + rax], ymm9
    vmovapd [rdi + rax], ymm10
    vmovapd [r11 + rax], ymm12
    add eax, 32
    cmp eax, ecx
    jb 62b
    lea rsi, [rsi + rcx * 2]
    lea rdi, [rdi + rcx * 2]
    lea rax, [r15 + YIM]
    cmp rsi, rax
    jb 61b
    lea r9, [r9 + rcx * 2]
    add ecx, ecx
    cmp ecx, N / 2 * 8
    jbe 6b
    # the check: |re - 1| <= 1e-9 and |im| <= 1e-9 (ordered: NaN fails)
    vbroadcastsd ymm13, [rip + one]
    vbroadcastsd ymm14, [rip + tolerance]
    vbroadcastsd ymm12, [rip + abs]
    vpcmpeqd ymm15, ymm15, ymm15
    xor eax, eax
7:  vsubpd ymm0, ymm13, [r15 + YRE + rax]
    vandpd ymm0, ymm0, ymm12
    vcmplepd ymm0, ymm0, ymm14
    vandpd ymm1, ymm12, [r15 + YIM + rax]
    vcmplepd ymm1, ymm1, ymm14
    vandpd ymm15, ymm15, ymm0
    vandpd ymm15, ymm15, ymm1
    add eax, 32
    cmp eax, N * 8
    jb 7b
    vmovmskpd eax, ymm15
    vzeroupper
    cmp eax, 15
    jne 9f
    mov edi, 1
    lea rsi, [rip + line]
    mov edx, line_end - line
8:  mov eax, 1
    syscall
    cmp rax, -4
    je 8b
    test rax, rax
    jle 9f
    add rsi, rax
    sub rdx, rax
    jnz 8b
    xor edi, edi
    mov eax, 231
    syscall
9:  mov edi, 1
    mov eax, 231
    syscall

    .p2align 3
k0123: .double 0, 1, 2, 3
theta: .double 0.0015339807878856412     # pi / 2048
four: .double 4
one: .double 1
tolerance: .double 0.000000001
sign: .quad 0x8000000000000000
abs: .quad 0x7fffffffffffffff
sinc: .double 1, -0.16666666666666666, 0.008333333333333333, -0.0001984126984126984, 0.0000027557319223985893, -2.505210838544172e-8, 1.6059043836821613e-10, -7.647163731819816e-13, 2.8114572543455206e-15, -8.22063524662433e-18, 1.9572941063391263e-20
cosc: .double 1, -0.5, 0.041666666666666664, -0.001388888888888889, 0.0000248015873015873, -2.755731922398589e-7, 2.08767569878681e-9, -1.1470745597729725e-11, 4.779477332387385e-14, -1.5619206968586225e-16, 4.110317623312165e-19
line: .ascii "fft4096 checked=8192 tolerance=1e-9\n"
line_end:
