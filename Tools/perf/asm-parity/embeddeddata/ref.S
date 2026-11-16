# embedded-data (gauntlet embedded.data): read the EDB1 telemetry file
# argv[1] ("EDB1", a u32le frame count, then 12-byte frames channel:u8
# flags:u8 sample:u16le tick:u32le energy:u16le load:u16le) and print the
# frame count, the valid frames (flags 0), their count per channel, their
# sample minimum, maximum and sum, their energy, load and tick sums, and
# the sum of every byte of the file, as
# Tools/gauntlet/entries/embedded-data defines them.
#
# Technique:
# - the file is mapped (MAP_POPULATE) into a reservation one page longer,
#   so that loads past its end read zeros: no tail loops;
# - 16 frames are 3 AVX-512 loads, and per frame dword (the first, the
#   tick, the third) one vpermi2d over the first two loads and one masked
#   vpermd from the third give it one lane per frame;
# - a channel above 3 is one vptestmd of the first dwords against 0xFC;
#   validity (flags 0) and the channel's bits 0 and 1 are masks, and every
#   aggregate is a masked add into 16 lane accumulators: valid frames,
#   bit 0, bit 1 and both (the four channel counts follow from those
#   four), sample sum, minimum and maximum, energy and load sums (16-bit
#   values in 32-bit lanes, widened to 64 bits every 65536 blocks), tick
#   sums mod 2^32 with their carries counted per lane;
# - the byte sum is vpsadbw over the same three loads, and over the header
#   and the bytes after the last whole block.
# Stops are the Jet program's, with its status and its stderr report byte
# for byte: a file that does not start with "EDB1" (status 1, `bad magic`)
# and a frame, among those the file holds, with a channel above 3 (status
# 1, `bad channel`). A file that cannot be read, has no whole count or
# holds fewer frames than its count ends the Jet program's run through a
# `?? return` that returns no error: status 0 and no output, here too.
# The input class: argv the one path, not starting with '-', and a file
# shorter than 32 GiB, so that no U64 of the Jet program's overflows;
# exit 2 outside it. Exit 1 when the reservation cannot be mapped or
# stdout cannot be written.
    .intel_syntax noprefix
    .text

    .set STAT, 0                 # struct stat (144 bytes)
    .set VALS, 160               # the printed numbers, 10 quadwords
    .set OUT, 256
    .set FRAME, 1024

# 16 frames in zmm14..16, the lanes to count in k4: their aggregates. A
# channel above 3 stops.
.macro FRAMES
    vmovdqa64 zmm17, zmm20
    vpermi2d zmm17, zmm14, zmm15
    vpermd zmm17{k5}, zmm21, zmm16         # the first dwords
    vptestmd k1{k4}, zmm17, zmm27
    kortestw k1, k1
    jnz badchannel
    vptestnmd k1{k4}, zmm17, zmm28         # valid: flags 0
    vptestmd k2{k1}, zmm17, dword ptr [rip + d_1]{1to16}   # channel bit 0
    vptestmd k3{k1}, zmm17, dword ptr [rip + d_2]{1to16}   # channel bit 1
    vpsubd zmm0{k1}, zmm0, zmm26
    vpsubd zmm1{k2}, zmm1, zmm26
    vpsubd zmm2{k3}, zmm2, zmm26
    kandw k2, k2, k3
    vpsubd zmm3{k2}, zmm3, zmm26
    vpsrld zmm17, zmm17, 16                # the samples
    vpaddd zmm4{k1}, zmm4, zmm17
    vpminud zmm5{k1}, zmm5, zmm17
    vpmaxud zmm6{k1}, zmm6, zmm17
    vmovdqa64 zmm17, zmm22
    vpermi2d zmm17, zmm14, zmm15
    vpermd zmm17{k6}, zmm23, zmm16         # the ticks
    vpaddd zmm9{k1}, zmm9, zmm17
    vpcmpud k2{k1}, zmm9, zmm17, 1         # below the tick: a carry out
    vpsubd zmm10{k2}, zmm10, zmm26
    vmovdqa64 zmm17, zmm24
    vpermi2d zmm17, zmm14, zmm15
    vpermd zmm17{k7}, zmm25, zmm16         # energy | load << 16
    vpandd zmm18, zmm17, zmm31
    vpaddd zmm7{k1}, zmm7, zmm18
    vpsrld zmm17, zmm17, 16
    vpaddd zmm8{k1}, zmm8, zmm17
.endm

.macro TEXT s
    lea rsi, [rip + \s]
    mov ecx, \s\()_end - \s
    rep movsb
.endm

_start:
    cmp qword ptr [rsp], 2       # argc: the one path
    jne class
    mov rdi, [rsp + 16]          # argv[1]
    cmp byte ptr [rdi], '-'      # an option to the Jet program's CLI
    je class
    sub rsp, FRAME
    xor esi, esi                 # O_RDONLY
    mov eax, 2                   # open
    syscall
    test rax, rax
    js quiet
    mov rbx, rax                 # fd
    mov edi, eax
    lea rsi, [rsp + STAT]
    mov eax, 5                   # fstat
    syscall
    test rax, rax
    js quiet
    mov r13, [rsp + STAT + 48]   # st_size
    mov rax, 1 << 35
    cmp r13, rax
    jae class
    # Reserve size + one page readable, then map the file over its start.
    xor edi, edi
    lea rsi, [r13 + 4096]
    mov edx, 1                   # PROT_READ
    mov r10d, 0x22               # MAP_PRIVATE | MAP_ANONYMOUS
    mov r8, -1
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja fail
    mov r12, rax
    test r13, r13
    jz 1f
    mov rdi, r12
    mov rsi, r13
    mov edx, 1
    mov r10d, 0x8012             # MAP_PRIVATE | MAP_FIXED | MAP_POPULATE
    mov r8, rbx
    xor r9d, r9d
    mov eax, 9
    syscall
    cmp rax, -4096
    ja quiet                     # not a mappable file: not readable
1:  cmp dword ptr [r12], 0x31424445  # "EDB1"; a shorter file reads zeros
    jne nomagic
    cmp r13, 8
    jb quiet                     # no whole count
    mov r14d, [r12 + 4]          # the count
    lea rax, [r13 - 8]
    xor edx, edx
    mov ecx, 12
    div rcx                      # the whole frames in the file
    mov r15, r14
    cmp rax, r15
    cmovb r15, rax               # the frames read: min(count, whole)

    vmovdqu64 zmm20, [rip + perm_ab0]
    vmovdqu64 zmm21, [rip + perm_c0]
    vmovdqu64 zmm22, [rip + perm_ab1]
    vmovdqu64 zmm23, [rip + perm_c1]
    vmovdqu64 zmm24, [rip + perm_ab2]
    vmovdqu64 zmm25, [rip + perm_c2]
    mov eax, 0xf800              # the lanes the third load fills: frames
    kmovw k5, eax                # 11..15 for the first dword and the
    kmovw k6, eax                # tick,
    mov eax, 0xfc00              # 10..15 for the third dword
    kmovw k7, eax
    vpternlogd zmm26, zmm26, zmm26, 0xff   # -1
    vpxord zmm0, zmm0, zmm0      # valid frames
    vpxord zmm1, zmm1, zmm1      # channel bit 0
    vpxord zmm2, zmm2, zmm2      # channel bit 1
    vpxord zmm3, zmm3, zmm3      # both
    vpxord zmm4, zmm4, zmm4      # sample sum (32-bit, this stretch)
    vpbroadcastd zmm5, [rip + d_ffff]      # sample minimum
    vpxord zmm6, zmm6, zmm6      # sample maximum
    vpxord zmm7, zmm7, zmm7      # energy sum (32-bit, this stretch)
    vpxord zmm8, zmm8, zmm8      # load sum (32-bit, this stretch)
    vpxord zmm9, zmm9, zmm9      # tick sum mod 2^32
    vpxord zmm10, zmm10, zmm10   # tick carries
    vpxord zmm11, zmm11, zmm11   # byte sum (64-bit)
    vpxord zmm12, zmm12, zmm12   # sample sum (64-bit)
    vpxord zmm13, zmm13, zmm13   # energy sum (64-bit)
    vpxord zmm29, zmm29, zmm29   # load sum (64-bit)
    vpxord zmm19, zmm19, zmm19   # zero
    vpbroadcastd zmm27, [rip + d_fc]
    vpbroadcastd zmm28, [rip + d_ff00]
    vpbroadcastd zmm31, [rip + d_ffff]
    vpbroadcastq zmm30, [rip + q_lo]

    lea rsi, [r12 + 8]
    mov rcx, r15
    shr rcx, 4                   # whole blocks of 16 frames
    jz tail
    mov eax, 0xffff
    kmovw k4, eax
stretch:
    mov r8d, 65536               # blocks before the 32-bit sums widen
    cmp rcx, r8
    cmovb r8, rcx
    sub rcx, r8
    .p2align 5
block:
    vmovdqu64 zmm14, [rsi]
    vmovdqu64 zmm15, [rsi + 64]
    vmovdqu64 zmm16, [rsi + 128]
    vpsadbw zmm17, zmm14, zmm19
    vpaddq zmm11, zmm11, zmm17
    vpsadbw zmm17, zmm15, zmm19
    vpaddq zmm11, zmm11, zmm17
    vpsadbw zmm17, zmm16, zmm19
    vpaddq zmm11, zmm11, zmm17
    FRAMES
    add rsi, 192
    dec r8
    jnz block
    call widen
    test rcx, rcx
    jnz stretch

tail:
    # The last frames (fewer than 16): their lanes only, no byte sum.
    mov ecx, r15d
    and ecx, 15
    jz 1f
    vmovdqu64 zmm14, [rsi]
    vmovdqu64 zmm15, [rsi + 64]
    vmovdqu64 zmm16, [rsi + 128]
    mov eax, 1
    shl eax, cl
    dec eax
    kmovw k4, eax
    FRAMES
    call widen
1:  cmp r15, r14
    jb quiet                     # the count's frames are not all there
    # The bytes outside the blocks: the header, then from the last whole
    # block's end to the file's end.
    vmovq xmm17, [r12]
    vpsadbw xmm17, xmm17, xmm19
    vpaddq zmm11, zmm11, zmm17
    lea rdi, [r12 + r13]
2:  cmp rsi, rdi
    jae report
    vmovdqu64 zmm17, [rsi]       # zeros past the file's end
    vpsadbw zmm17, zmm17, zmm19
    vpaddq zmm11, zmm11, zmm17
    add rsi, 64
    jmp 2b

# The 32-bit sample, energy and load sums added into the 64-bit ones and
# cleared.
widen:
    vpsrlq zmm17, zmm4, 32
    vpaddq zmm12, zmm12, zmm17
    vpandq zmm17, zmm4, zmm30
    vpaddq zmm12, zmm12, zmm17
    vpsrlq zmm17, zmm7, 32
    vpaddq zmm13, zmm13, zmm17
    vpandq zmm17, zmm7, zmm30
    vpaddq zmm13, zmm13, zmm17
    vpsrlq zmm17, zmm8, 32
    vpaddq zmm29, zmm29, zmm17
    vpandq zmm17, zmm8, zmm30
    vpaddq zmm29, zmm29, zmm17
    vpxord zmm4, zmm4, zmm4
    vpxord zmm7, zmm7, zmm7
    vpxord zmm8, zmm8, zmm8
    ret

# rax = the sum of zmm17's dword lanes (hsumd) or quadword lanes (hsumq).
# Clobbers zmm17 and zmm18.
hsumd:
    vextracti64x4 ymm18, zmm17, 1
    vpmovzxdq zmm18, ymm18
    vpmovzxdq zmm17, ymm17
    vpaddq zmm17, zmm17, zmm18
hsumq:
    vextracti64x4 ymm18, zmm17, 1
    vpaddq ymm17, ymm17, ymm18
    vextracti32x4 xmm18, ymm17, 1
    vpaddq xmm17, xmm17, xmm18
    vpshufd xmm18, xmm17, 0x4e
    vpaddq xmm17, xmm17, xmm18
    vmovq rax, xmm17
    ret

# eax = the minimum (hmin) or maximum (hmax) of zmm17's dword lanes.
# Clobbers zmm17 and zmm18.
hmin:
    vextracti64x4 ymm18, zmm17, 1
    vpminud ymm17, ymm17, ymm18
    vextracti32x4 xmm18, ymm17, 1
    vpminud xmm17, xmm17, xmm18
    vpshufd xmm18, xmm17, 0x4e
    vpminud xmm17, xmm17, xmm18
    vpshufd xmm18, xmm17, 0xb1
    vpminud xmm17, xmm17, xmm18
    vmovd eax, xmm17
    ret
hmax:
    vextracti64x4 ymm18, zmm17, 1
    vpmaxud ymm17, ymm17, ymm18
    vextracti32x4 xmm18, ymm17, 1
    vpmaxud xmm17, xmm17, xmm18
    vpshufd xmm18, xmm17, 0x4e
    vpmaxud xmm17, xmm17, xmm18
    vpshufd xmm18, xmm17, 0xb1
    vpmaxud xmm17, xmm17, xmm18
    vmovd eax, xmm17
    ret

    .set V_VALID, VALS
    .set V_C0, VALS + 8
    .set V_C1, VALS + 16
    .set V_C2, VALS + 24
    .set V_C3, VALS + 32
    .set V_MIN, VALS + 40
    .set V_MAX, VALS + 48
    .set V_TICKS, VALS + 56

report:
    vmovdqa64 zmm17, zmm0
    call hsumd
    mov r8, rax                  # valid
    vmovdqa64 zmm17, zmm1
    call hsumd
    mov r9, rax                  # channel bit 0
    vmovdqa64 zmm17, zmm2
    call hsumd
    mov r10, rax                 # channel bit 1
    vmovdqa64 zmm17, zmm3
    call hsumd                   # both: channel 3
    mov [rsp + V_VALID], r8
    mov [rsp + V_C3], rax
    sub r9, rax
    mov [rsp + V_C1], r9
    sub r10, rax
    mov [rsp + V_C2], r10
    sub r8, r9
    sub r8, r10
    sub r8, rax
    mov [rsp + V_C0], r8
    vmovdqa64 zmm17, zmm5
    call hmin
    mov [rsp + V_MIN], rax
    vmovdqa64 zmm17, zmm6
    call hmax
    mov [rsp + V_MAX], rax
    vmovdqa64 zmm17, zmm10
    call hsumd
    shl rax, 32
    mov r8, rax
    vmovdqa64 zmm17, zmm9
    call hsumd
    add rax, r8
    mov [rsp + V_TICKS], rax

    lea rdi, [rsp + OUT]
    TEXT s_frames
    mov rax, r14
    call putnum
    TEXT s_valid
    mov rax, [rsp + V_VALID]
    call putnum
    TEXT s_channels
    mov rax, [rsp + V_C0]
    call putnum
    mov byte ptr [rdi], ' '
    inc rdi
    mov rax, [rsp + V_C1]
    call putnum
    mov byte ptr [rdi], ' '
    inc rdi
    mov rax, [rsp + V_C2]
    call putnum
    mov byte ptr [rdi], ' '
    inc rdi
    mov rax, [rsp + V_C3]
    call putnum
    TEXT s_sample
    mov rax, [rsp + V_MIN]
    call putnum
    mov byte ptr [rdi], ' '
    inc rdi
    mov rax, [rsp + V_MAX]
    call putnum
    mov byte ptr [rdi], ' '
    inc rdi
    vmovdqa64 zmm17, zmm12
    call hsumq
    call putnum
    TEXT s_energy
    vmovdqa64 zmm17, zmm13
    call hsumq
    call putnum
    TEXT s_load
    vmovdqa64 zmm17, zmm29
    call hsumq
    call putnum
    TEXT s_ticks
    mov rax, [rsp + V_TICKS]
    call putnum
    TEXT s_checksum
    vmovdqa64 zmm17, zmm11
    call hsumq
    call putnum
    mov byte ptr [rdi], 10
    inc rdi
    lea rsi, [rsp + OUT]
    mov rdx, rdi
    sub rdx, rsi
    mov edi, 1
    call write_all
    xor edi, edi
    mov eax, 231
    syscall

# rax in decimal at rdi, rdi advanced. Digits are made in the red zone.
# Clobbers rax, rcx, rdx, rsi and r8.
putnum:
    lea rsi, [rsp - 8]
    mov r8d, 10
1:  xor edx, edx
    div r8
    add dl, '0'
    mov [rsi], dl
    dec rsi
    test rax, rax
    jnz 1b
    lea rcx, [rsp - 8]
    sub rcx, rsi
    inc rsi
    rep movsb
    ret

# write(edi, rsi, rdx) until done, EINTR retried, short writes advanced;
# a failed write exits 1 for stdout and ends the write for stderr.
write_all:
    test rdx, rdx
    jz 2f
    mov eax, 1
    syscall
    cmp rax, -4
    je write_all
    test rax, rax
    jle 1f
    add rsi, rax
    sub rdx, rax
    jmp write_all
1:  cmp edi, 1
    je fail
2:  ret

# The Jet program's stops: the error run returns, reported on stderr with
# status 1.
nomagic:
    lea rsi, [rip + e_magic]
    mov edx, e_magic_end - e_magic
    jmp 1f
badchannel:
    lea rsi, [rip + e_chan]
    mov edx, e_chan_end - e_chan
1:  mov edi, 2
    call write_all
fail:
    mov edi, 1
    mov eax, 231
    syscall

# run returns through a `?? return` with no error: status 0, no output.
quiet:
    xor edi, edi
    mov eax, 231
    syscall

class:
    mov edi, 2
    mov eax, 231
    syscall

    .p2align 6
# Frame f's dword j is dword 3f + j of the 48 in zmm14..16. The vpermi2d
# takes those below 32 (frames up to 10, or 9 for j = 2) from zmm14..15,
# the masked vpermd the rest from zmm16 (its dword 3f + j - 32).
perm_ab0:
    .long 0, 3, 6, 9, 12, 15, 18, 21, 24, 27, 30, 0, 0, 0, 0, 0
perm_c0:
    .long 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 4, 7, 10, 13
perm_ab1:
    .long 1, 4, 7, 10, 13, 16, 19, 22, 25, 28, 31, 0, 0, 0, 0, 0
perm_c1:
    .long 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 5, 8, 11, 14
perm_ab2:
    .long 2, 5, 8, 11, 14, 17, 20, 23, 26, 29, 0, 0, 0, 0, 0, 0
perm_c2:
    .long 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 6, 9, 12, 15
q_lo:
    .quad 0xffffffff
d_fc:
    .long 0xfc
d_ff00:
    .long 0xff00
d_1:
    .long 1
d_2:
    .long 2
d_ffff:
    .long 0xffff
s_frames:
    .ascii "frames "
s_frames_end:
s_valid:
    .ascii "\nvalid "
s_valid_end:
s_channels:
    .ascii "\nchannels "
s_channels_end:
s_sample:
    .ascii "\nsample "
s_sample_end:
s_energy:
    .ascii "\nenergy "
s_energy_end:
s_load:
    .ascii "\nload "
s_load_end:
s_ticks:
    .ascii "\nticks "
s_ticks_end:
s_checksum:
    .ascii "\nchecksum "
s_checksum_end:
e_magic:
    .ascii "Error: bad magic\n"
    .ascii "  failed here: run (run.jet:15)\n"
    .ascii "      reader.take_pattern([U8]{\"EDB1\"}) ?? return Err(\"bad magic\")\n"
e_magic_end:
e_chan:
    .ascii "Error: bad channel\n"
    .ascii "  failed here: run (run.jet:30)\n"
    .ascii "      if channel >= channels.len() -> return Err(\"bad channel\")\n"
e_chan_end:
