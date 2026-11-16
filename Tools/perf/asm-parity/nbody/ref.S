# The gauntlet nbody (Tools/gauntlet/entries/nbody/jet/run.jet) by hand:
# five bodies, N symplectic-Euler steps of dt 0.01, and the system energy
# before and after with nine decimals. Every double operation is the Jet
# program's, in its order and with its rounding (no fused multiply-add, no
# reciprocal estimate), so the states are bit for bit those of run.jet;
# what changes is the schedule:
# - the bodies live in registers, one ymm per position and per velocity
#   (x, y, z, 0), ymm0-ymm4 and ymm5-ymm9, for the whole run;
# - a step's ten pair differences are computed together, their squared
#   lengths ((x*x + y*y) + z*z, run.jet's dot) gathered four to a register by
#   hadd/perm/blend, so the ten square roots and divides g = dt / (d *
#   sqrt(d)) are three vsqrtpd and three vdivpd;
# - each pair's g is broadcast from its lane (vpermpd, no store and reload)
#   and scaled by the two masses (run.jet's m * g); the velocity updates run
#   in run.jet's pair order (each body's sum keeps its order), and the
#   positions move by v * dt.
# The start state (masses, velocities, the Sun's offset momentum) is the
# program's constant expressions, evaluated in its order in IEEE doubles.
# The energies print as correctly rounded nine-decimal fixed point, from the
# double's exact value (mantissa times 1e9, shifted, ties to even); each
# line is written once known, the first before any step.
# Errors: no step count -> exit 1.
    .intel_syntax noprefix
    .text

.set R, 0                        # ten pair differences, 32 bytes each
.set P, 512                      # positions, for the energy
.set V, 672                      # velocities, for the energy
.set TMP, 832                    # integer digits
.set OUT, 864                    # the two output lines
.set FRAME, 1024

_start:
    mov rsi, [rsp + 16]          # argv[1]
    test rsi, rsi
    jz fail
    xor ebx, ebx                 # steps
1:  movzx eax, byte ptr [rsi]
    sub eax, '0'
    cmp eax, 9
    ja 2f
    imul rbx, rbx, 10
    add rbx, rax
    inc rsi
    jmp 1b
2:  and rsp, -32
    sub rsp, FRAME
    vmovupd ymm0, [rip + start_p]
    vmovupd ymm1, [rip + start_p + 32]
    vmovupd ymm2, [rip + start_p + 64]
    vmovupd ymm3, [rip + start_p + 96]
    vmovupd ymm4, [rip + start_p + 128]
    vmovupd ymm5, [rip + start_v]
    vmovupd ymm6, [rip + start_v + 32]
    vmovupd ymm7, [rip + start_v + 64]
    vmovupd ymm8, [rip + start_v + 96]
    vmovupd ymm9, [rip + start_v + 128]
    vbroadcastsd ymm15, [rip + dt]
    lea r12, [rsp + OUT]         # output position
    call energy
    call flush                   # the first line out before the steps
    test rbx, rbx
    jle 9f

step:
    # Pairs 0-3: (0,1) (0,2) (0,3) (0,4).
    vsubpd ymm10, ymm0, ymm1
    vsubpd ymm11, ymm0, ymm2
    vsubpd ymm12, ymm0, ymm3
    vsubpd ymm13, ymm0, ymm4
    vmovapd [rsp + R], ymm10
    vmovapd [rsp + R + 32], ymm11
    vmovapd [rsp + R + 64], ymm12
    vmovapd [rsp + R + 96], ymm13
    vmulpd ymm10, ymm10, ymm10
    vmulpd ymm11, ymm11, ymm11
    vmulpd ymm12, ymm12, ymm12
    vmulpd ymm13, ymm13, ymm13
    vhaddpd ymm10, ymm10, ymm11       # xx+yy of 0, 1; zz of 0, 1
    vhaddpd ymm12, ymm12, ymm13       # the same of 2, 3
    vperm2f128 ymm11, ymm10, ymm12, 0x21
    vblendpd ymm10, ymm10, ymm12, 0b1100
    vaddpd ymm10, ymm10, ymm11        # d = (xx + yy) + zz
    vsqrtpd ymm11, ymm10
    vmulpd ymm10, ymm10, ymm11
    vdivpd ymm14, ymm15, ymm10        # g = dt / (d * sqrt(d)), pairs 0-3
    # Pairs 4-7: (1,2) (1,3) (1,4) (2,3).
    vsubpd ymm10, ymm1, ymm2
    vsubpd ymm11, ymm1, ymm3
    vsubpd ymm12, ymm1, ymm4
    vsubpd ymm13, ymm2, ymm3
    vmovapd [rsp + R + 128], ymm10
    vmovapd [rsp + R + 160], ymm11
    vmovapd [rsp + R + 192], ymm12
    vmovapd [rsp + R + 224], ymm13
    vmulpd ymm10, ymm10, ymm10
    vmulpd ymm11, ymm11, ymm11
    vmulpd ymm12, ymm12, ymm12
    vmulpd ymm13, ymm13, ymm13
    vhaddpd ymm10, ymm10, ymm11
    vhaddpd ymm12, ymm12, ymm13
    vperm2f128 ymm11, ymm10, ymm12, 0x21
    vblendpd ymm10, ymm10, ymm12, 0b1100
    vaddpd ymm10, ymm10, ymm11
    vsqrtpd ymm11, ymm10
    vmulpd ymm10, ymm10, ymm11
    vdivpd ymm13, ymm15, ymm10        # g, pairs 4-7
    # Pairs 8-9: (2,4) (3,4), padded with d = 2.
    vsubpd ymm10, ymm2, ymm4
    vsubpd ymm11, ymm3, ymm4
    vmovapd [rsp + R + 256], ymm10
    vmovapd [rsp + R + 288], ymm11
    vmulpd ymm10, ymm10, ymm10
    vmulpd ymm11, ymm11, ymm11
    vhaddpd ymm10, ymm10, ymm11
    vperm2f128 ymm11, ymm10, [rip + two], 0x21
    vblendpd ymm10, ymm10, [rip + two], 0b1100
    vaddpd ymm10, ymm10, ymm11
    vsqrtpd ymm11, ymm10
    vmulpd ymm10, ymm10, ymm11
    vdivpd ymm12, ymm15, ymm10        # g, pairs 8-9
    # The velocities, in pair order: v_i -= r * (m_j * g), v_j += r * (m_i * g),
    # g broadcast from its lane.
    .macro pair k, g, lane, i, j, vi, vj
    vpermpd ymm10, \g, 0x55 * \lane
    vmulpd ymm11, ymm10, [rip + mass + 32 * \j]
    vmulpd ymm10, ymm10, [rip + mass + 32 * \i]
    vmulpd ymm11, ymm11, [rsp + R + 32 * \k]
    vmulpd ymm10, ymm10, [rsp + R + 32 * \k]
    vsubpd \vi, \vi, ymm11
    vaddpd \vj, \vj, ymm10
    .endm
    pair 0, ymm14, 0, 0, 1, ymm5, ymm6
    pair 1, ymm14, 1, 0, 2, ymm5, ymm7
    pair 2, ymm14, 2, 0, 3, ymm5, ymm8
    pair 3, ymm14, 3, 0, 4, ymm5, ymm9
    pair 4, ymm13, 0, 1, 2, ymm6, ymm7
    pair 5, ymm13, 1, 1, 3, ymm6, ymm8
    pair 6, ymm13, 2, 1, 4, ymm6, ymm9
    pair 7, ymm13, 3, 2, 3, ymm7, ymm8
    pair 8, ymm12, 0, 2, 4, ymm7, ymm9
    pair 9, ymm12, 1, 3, 4, ymm8, ymm9
    # The positions: p += v * dt.
    vmulpd ymm10, ymm5, ymm15
    vmulpd ymm11, ymm6, ymm15
    vmulpd ymm12, ymm7, ymm15
    vmulpd ymm13, ymm8, ymm15
    vaddpd ymm0, ymm0, ymm10
    vaddpd ymm1, ymm1, ymm11
    vmulpd ymm10, ymm9, ymm15
    vaddpd ymm2, ymm2, ymm12
    vaddpd ymm3, ymm3, ymm13
    vaddpd ymm4, ymm4, ymm10
    dec rbx
    jnz step

9:  call energy
    call flush
    xor edi, edi
    mov eax, 231
    syscall

fail:
    mov edi, 1
    mov eax, 231
    syscall

# flush: the line(s) at OUT up to r12 written, and r12 back at OUT. Called
# from _start's frame.
flush:
    lea rsi, [rsp + 8 + OUT]
    mov rdx, r12
    sub rdx, rsi
1:  mov edi, 1
    mov eax, 1
    syscall
    cmp rax, -4
    je 1b
    test rax, rax
    jle fail
    add rsi, rax
    sub rdx, rax
    jnz 1b
    lea r12, [rsp + 8 + OUT]
    ret

# energy: run.jet's energy() of the state in ymm0-ymm9, as a line at r12
# (advanced). Called from _start's frame: its slots are rsp + 8 + X.
energy:
    vmovapd [rsp + 8 + P], ymm0
    vmovapd [rsp + 8 + P + 32], ymm1
    vmovapd [rsp + 8 + P + 64], ymm2
    vmovapd [rsp + 8 + P + 96], ymm3
    vmovapd [rsp + 8 + P + 128], ymm4
    vmovapd [rsp + 8 + V], ymm5
    vmovapd [rsp + 8 + V + 32], ymm6
    vmovapd [rsp + 8 + V + 64], ymm7
    vmovapd [rsp + 8 + V + 96], ymm8
    vmovapd [rsp + 8 + V + 128], ymm9
    vxorpd xmm10, xmm10, xmm10        # e
    xor r8d, r8d                      # i
1:  mov rax, r8
    shl rax, 5
    vmovsd xmm11, [rsp + 8 + V + rax]
    vmulsd xmm11, xmm11, xmm11
    vmovsd xmm12, [rsp + 8 + V + rax + 8]
    vmulsd xmm12, xmm12, xmm12
    vaddsd xmm11, xmm11, xmm12
    vmovsd xmm12, [rsp + 8 + V + rax + 16]
    vmulsd xmm12, xmm12, xmm12
    vaddsd xmm11, xmm11, xmm12        # v.dot(v)
    vmovsd xmm12, [rip + half]
    lea rcx, [rip + mass]
    vmulsd xmm12, xmm12, [rcx + rax]
    vmulsd xmm12, xmm12, xmm11
    vaddsd xmm10, xmm10, xmm12        # e += 0.5 * m * v.dot(v)
    lea r9, [r8 + 1]                  # j
2:  cmp r9, 5
    jae 3f
    mov rdx, r9
    shl rdx, 5
    vmovsd xmm11, [rsp + 8 + P + rax]
    vsubsd xmm11, xmm11, [rsp + 8 + P + rdx]
    vmulsd xmm11, xmm11, xmm11
    vmovsd xmm12, [rsp + 8 + P + rax + 8]
    vsubsd xmm12, xmm12, [rsp + 8 + P + rdx + 8]
    vmulsd xmm12, xmm12, xmm12
    vaddsd xmm11, xmm11, xmm12
    vmovsd xmm12, [rsp + 8 + P + rax + 16]
    vsubsd xmm12, xmm12, [rsp + 8 + P + rdx + 16]
    vmulsd xmm12, xmm12, xmm12
    vaddsd xmm11, xmm11, xmm12
    vsqrtsd xmm11, xmm11, xmm11       # (p_i - p_j).length()
    vmovsd xmm12, [rcx + rax]
    vmulsd xmm12, xmm12, [rcx + rdx]
    vdivsd xmm12, xmm12, xmm11
    vsubsd xmm10, xmm10, xmm12        # e -= m_i * m_j / length
    inc r9
    jmp 2b
3:  inc r8
    cmp r8, 5
    jb 1b

    # e as "[-]<int>.<9 digits>\n": q = |e| * 1e9 rounded to nearest, ties
    # to even, from the exact value M * 2^-k (k = 1075 - E).
    vmovq rax, xmm10
    test rax, rax
    jns 1f
    mov byte ptr [r12], '-'
    inc r12
1:  mov rcx, rax
    shr rcx, 52
    and ecx, 0x7ff                    # E
    mov rdx, rax
    shl rdx, 12
    shr rdx, 12                       # fraction
    test ecx, ecx
    jz 2f
    bts rdx, 52                       # M
    jmp 3f
2:  mov ecx, 1                        # subnormal: E = 1
3:  neg ecx
    add ecx, 1075                     # k
    xor r9d, r9d                      # q
    cmp ecx, 84
    jae 6f                            # M * 1e9 < 2^83 <= 2^(k-1): q = 0
    cmp ecx, 21
    jb fail                           # |e| >= 2^32: q would pass 63 bits
    mov rax, rdx
    mov r8d, 1000000000
    mul r8                            # rdx:rax = M * 1e9 < 2^83
    dec ecx                           # s = k - 1, 20..82
    cmp ecx, 64
    jae 4f
    bzhi r10, rax, rcx                # the bits below the half
    shrd rax, rdx, cl                 # P >> s (below 2^63)
    jmp 5f
4:  mov r10, rax
    sub ecx, 64
    bzhi r11, rdx, rcx
    or r10, r11
    shrx rax, rdx, rcx
5:  mov r9, rax
    shr r9, 1                         # q truncated
    test al, 1
    jz 6f                             # below a half
    test r10, r10
    jnz 7f                            # above a half
    test r9b, 1
    jz 6f                             # a tie to an even q
7:  inc r9
6:  mov rax, r9
    xor edx, edx
    mov r8d, 1000000000
    div r8                            # rax = integer part, rdx = fraction
    mov r11, rdx
    lea rdi, [rsp + 8 + TMP + 32]
    mov r8d, 10
1:  xor edx, edx
    div r8
    add dl, '0'
    dec rdi
    mov [rdi], dl
    test rax, rax
    jnz 1b
    lea rcx, [rsp + 8 + TMP + 32]
    sub rcx, rdi
    mov rsi, rdi
    mov rdi, r12
    rep movsb
    mov r12, rdi
    mov byte ptr [r12], '.'
    mov rax, r11
    mov ecx, 9
2:  xor edx, edx
    div r8
    add dl, '0'
    mov [r12 + rcx], dl
    dec ecx
    jnz 2b
    mov byte ptr [r12 + 10], 10
    add r12, 11
    ret

dt:
    .quad 0x3f847ae147ae147b             # 0.01
half:
    .quad 0x3fe0000000000000             # 0.5
two:                                     # the two unused pair slots' d
    .rept 4
    .quad 0x4000000000000000             # 2
    .endr
start_p:
    .quad 0x0000000000000000             # 0
    .quad 0x0000000000000000             # 0
    .quad 0x0000000000000000             # 0
    .quad 0x0000000000000000             # 0
    .quad 0x40135da0343cd92c             # 4.841431442464721
    .quad 0xbff290abc01fdb7c             # -1.1603200440274284
    .quad 0xbfba86f96c25ebf0             # -0.10362204447112311
    .quad 0x0000000000000000             # 0
    .quad 0x4020afcdc332ca67             # 8.34336671824458
    .quad 0x40107fcb31de01b0             # 4.124798564124305
    .quad 0xbfd9d353e1eb467c             # -0.4035234171143214
    .quad 0x0000000000000000             # 0
    .quad 0x4029c9eacea7d9cf             # 12.894369562139131
    .quad 0xc02e38e8d626667e             # -15.111151401698631
    .quad 0xbfcc9557be257da0             # -0.22330757889265573
    .quad 0x0000000000000000             # 0
    .quad 0x402ec267a905572a             # 15.379697114850917
    .quad 0xc039eb5833c8a220             # -25.919314609987964
    .quad 0x3fc6f1f393abe540             # 0.17925877295037118
    .quad 0x0000000000000000             # 0
start_v:
    .quad 0xbf3967e9a7e0d6f3             # -0.00038766340719874267
    .quad 0xbf6ad4ecfe5089fb             # -0.0032753590371765707
    .quad 0x3ef919331f0b8a71             # 0.000023935734080003
    .quad 0x0000000000000000             # 0
    .quad 0x3fe367069b93ccbc             # 0.606326392995832
    .quad 0x40067ef2f57d949b             # 2.81198684491626
    .quad 0xbf99d2d79a5a0715             # -0.02521836165988763
    .quad 0x0000000000000000             # 0
    .quad 0xbff02c21b8879442             # -1.0107743461787924
    .quad 0x3ffd35e9bf1f8f13             # 1.8256623712304119
    .quad 0x3f813c485f1123b4             # 0.008415761376584154
    .quad 0x0000000000000000             # 0
    .quad 0x3ff1531ca9911bef             # 1.0827910064415354
    .quad 0x3febcc7f3e54bbc5             # 0.8687130181696082
    .quad 0xbf862f6bfaf23e7c             # -0.010832637401363636
    .quad 0x0000000000000000             # 0
    .quad 0x3fef54b61659bc4a             # 0.979090732243898
    .quad 0x3fe307c631c4fba0             # 0.5946989986476758
    .quad 0xbfa1cb88587665f6             # -0.034755955504078104
    .quad 0x0000000000000000             # 0
# Each body's mass in all four lanes (the step multiplies a broadcast g by
# it; the energy reads lane 0).
mass:
    .rept 4
    .quad 0x4043bd3cc9be45de             # 39.47841760435743
    .endr
    .rept 4
    .quad 0x3fa34c95d9ab33d8             # 0.03769367487038949
    .endr
    .rept 4
    .quad 0x3f871d490d07c637             # 0.011286326131968767
    .endr
    .rept 4
    .quad 0x3f5c3dd29cf41eb3             # 0.0017237240570597112
    .endr
    .rept 4
    .quad 0x3f60a8f3531799ac             # 0.0020336868699246304
    .endr
