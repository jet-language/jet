# gameframe (gauntlet games.frame): render 1200 frames of the game scene on
# a 320x200 premultiplied RGBA8 canvas (clear, the scrolling tile map, 48
# sprites at alpha 180, the HUD bar at alpha 160, the "SCORE <n> FRAME <f>"
# line at scale 1 and the "JET RASTER" title at scale 2, alpha 200, in the
# 5x7 font, one rectangle per vertical run of a glyph column), then print
# "frames=1200 tiles=<n> sprites=<n> glyphs=<n>". With JET_FRAME_CAPTURE set
# and not empty, the final frame is written there as a P7 RGB_ALPHA PAM.
#
# Technique: every frame is drawn, as the Jet program draws it.
# - a rectangle clips to the canvas; an opaque one stores its color 8 pixels
#   at a time (AVX2), the row's last 8 overlapping, so that a row is
#   ceil(n / 8) + 1 stores at most;
# - a translucent one premultiplies its color once (the multiplier vector
#   (a, a, a, 255), so that alpha stays a) and blends 8 pixels per step in
#   16-bit lanes: d * (255 - a) + 127, divided by 255 exactly as
#   (t * 0x8081) >> 23 (vpmulhuw then a shift, exact for t <= 65152), packed
#   and added to the premultiplied source bytes, with 4- and 1-pixel tails;
# - a glyph column's vertical runs come from tzcnt over its bits, the run
#   then cleared as (bits + lowest bit) & bits;
# - the clear is 8000 32-byte stores of the sky color.
# A capture that cannot be written prints "write failed" on stderr, exit 1,
# as the Jet program's panic does.
    .intel_syntax noprefix
    .text

    .set W, 320
    .set H, 200
    .set PITCH, W * 4
    .set FRAMES, 1200
    .set CAPTURE, 0              # the capture path, or 0
    .set TILES, 8
    .set SPRITES, 16
    .set GLYPHS, 24
    .set HUD, 32                 # the HUD line (64 bytes)
    .set OUT, 96                 # the counts line (128 bytes)
    .set FRAME, 256

    # Colors as r | g << 8 | b << 16 | a << 24.
    .set SKY, 0xff201410
    .set BAR, 0xa0000000
    .set WHITE, 0xffffffff
    .set GOLD, 0xc878dcff

_start:
    # JET_FRAME_CAPTURE from the environment.
    mov rax, [rsp]               # argc
    lea rbx, [rsp + rax * 8 + 16] # envp
    sub rsp, FRAME
    mov qword ptr [rsp + CAPTURE], 0
1:  mov rsi, [rbx]
    test rsi, rsi
    jz 3f
    add rbx, 8
    lea rdi, [rip + s_capture]
    mov ecx, 18
    repe cmpsb
    jne 1b
    cmp byte ptr [rsi], 0
    je 3f
    mov [rsp + CAPTURE], rsi
3:  # The canvas.
    xor edi, edi
    mov esi, W * H * 4
    mov edx, 3                   # PROT_READ | PROT_WRITE
    mov r10d, 0x22               # MAP_PRIVATE | MAP_ANONYMOUS
    mov r8, -1
    xor r9d, r9d
    mov eax, 9                   # mmap
    syscall
    cmp rax, -4096
    ja fail
    mov r15, rax
    mov eax, 127
    vmovd xmm9, eax
    vpbroadcastw ymm9, xmm9      # 127 per word
    mov eax, 0x8081
    vmovd xmm10, eax
    vpbroadcastw ymm10, xmm10    # the divide-by-255 multiplier
    mov eax, 255
    vmovd xmm12, eax
    vpbroadcastw ymm12, xmm12    # 255 per word
    xor eax, eax
    mov [rsp + TILES], rax
    mov [rsp + SPRITES], rax
    mov [rsp + GLYPHS], rax
    xor r12d, r12d               # the frame

frame:
    # Clear.
    mov eax, SKY
    vmovd xmm0, eax
    vpbroadcastd ymm0, xmm0
    mov rdi, r15
    lea rcx, [r15 + W * H * 4]
1:  vmovdqu [rdi], ymm0
    vmovdqu [rdi + 32], ymm0
    vmovdqu [rdi + 64], ymm0
    vmovdqu [rdi + 96], ymm0
    sub rdi, -128
    cmp rdi, rcx
    jb 1b
    # Tiles: ((tx + f / 16) * 7 + ty * 5) % 4 picks the palette; 0 is empty.
    xor r13d, r13d               # ty
tiles:
    xor r14d, r14d               # tx
1:  mov eax, r12d
    shr eax, 4
    add eax, r14d
    imul eax, eax, 7
    lea ecx, [r13 + r13 * 4]
    add eax, ecx
    and eax, 3
    jz 2f
    lea rcx, [rip + palette]
    mov r8d, [rcx + rax * 4]
    mov edi, r14d
    shl edi, 4
    mov eax, r12d
    and eax, 15
    sub edi, eax
    mov esi, r13d
    shl esi, 4
    add esi, 8
    mov edx, 15
    mov ecx, 15
    call rect
    inc qword ptr [rsp + TILES]
2:  inc r14d
    cmp r14d, 21
    jb 1b
    inc r13d
    cmp r13d, 12
    jb tiles
    # Sprites.
    xor ebx, ebx                 # i
sprites:
    mov eax, ebx
    and eax, 3
    inc eax
    imul eax, r12d
    imul ecx, ebx, 41
    add eax, ecx
    xor edx, edx
    mov ecx, 344
    div ecx
    lea edi, [rdx - 12]          # x
    mov eax, ebx
    xor edx, edx
    mov ecx, 3
    div ecx
    lea eax, [rdx + 1]
    imul eax, r12d
    imul ecx, ebx, 29
    add eax, ecx
    xor edx, edx
    mov ecx, 224
    div ecx
    lea esi, [rdx - 12]          # y
    imul eax, ebx, 31            # blue
    add eax, r12d
    movzx eax, al
    shl eax, 16
    imul ecx, ebx, 97            # green
    movzx ecx, cl
    shl ecx, 8
    or eax, ecx
    imul ecx, ebx, 53            # red
    movzx ecx, cl
    or eax, ecx
    or eax, 180 << 24
    mov r8d, eax
    mov edx, 12
    mov ecx, 12
    call rect
    inc qword ptr [rsp + SPRITES]
    inc ebx
    cmp ebx, 48
    jb sprites
    # The HUD bar.
    xor edi, edi
    xor esi, esi
    mov edx, W
    mov ecx, 12
    mov r8d, BAR
    call rect
    # "SCORE <(f * 7) % 100000> FRAME <f>" at (4, 2), scale 1, white.
    lea rdi, [rsp + HUD]
    mov dword ptr [rdi], 0x524f4353  # "SCOR"
    mov word ptr [rdi + 4], 0x2045   # "E "
    add rdi, 6
    imul eax, r12d, 7
    xor edx, edx
    mov ecx, 100000
    div ecx
    mov eax, edx
    call put_uint
    mov dword ptr [rdi], 0x41524620  # " FRA"
    mov dword ptr [rdi + 3], 0x20454d41 # "AME "
    add rdi, 7
    mov eax, r12d
    call put_uint
    lea rsi, [rsp + HUD]
    mov rdx, rdi
    sub rdx, rsi
    add [rsp + GLYPHS], rdx
    add qword ptr [rsp + GLYPHS], 10
    mov edi, 4
    mov ecx, 2
    mov r8d, 1
    mov r9d, WHITE
    call text
    # "JET RASTER" at (196, 176), scale 2, gold.
    lea rsi, [rip + s_title]
    mov edx, 10
    mov edi, 196
    mov ecx, 176
    mov r8d, 2
    mov r9d, GOLD
    call text
    inc r12d
    cmp r12d, FRAMES
    jb frame

    # The capture.
    mov rdi, [rsp + CAPTURE]
    test rdi, rdi
    jz counts
    mov esi, 0x241               # O_WRONLY | O_CREAT | O_TRUNC
    mov edx, 0644
    mov eax, 2                   # open
    syscall
    test rax, rax
    js fail
    mov ebx, eax
    lea rsi, [rip + s_pam]
    mov edx, s_pam_end - s_pam
    call put_all
    mov rsi, r15
    mov edx, W * H * 4
    call put_all
    mov edi, ebx
    mov eax, 3                   # close
    syscall

counts:
    lea rdi, [rsp + OUT]
    lea rsi, [rip + s_frames]
    mov ecx, 18
    rep movsb
    mov rax, [rsp + TILES]
    call put_uint
    lea rsi, [rip + s_sprites]
    mov ecx, 9
    rep movsb
    mov rax, [rsp + SPRITES]
    call put_uint
    lea rsi, [rip + s_glyphs]
    mov ecx, 8
    rep movsb
    mov rax, [rsp + GLYPHS]
    call put_uint
    mov byte ptr [rdi], 10
    inc rdi
    lea rsi, [rsp + OUT]
    mov rdx, rdi
    sub rdx, rsi
    mov ebx, 1
    call put_all
    xor edi, edi
    mov eax, 231                 # exit_group
    syscall

# put_all: rdx bytes at rsi to the descriptor ebx, EINTR retried and short
# writes advanced.
put_all:
    mov edi, ebx
    mov eax, 1                   # write
    syscall
    cmp rax, -4
    je put_all
    test rax, rax
    js fail
    add rsi, rax
    sub rdx, rax
    jnz put_all
    ret

# rect: the rectangle (edi x, esi y, edx width, ecx height) in r8d's color
# (straight), clipped and drawn over the canvas at r15. Clobbers rax, rcx,
# rdx, rsi, rdi, r8 through r11, ymm0, ymm1, ymm8, ymm11.
rect:
    mov eax, r8d
    shr eax, 24                  # alpha
    jz 9f
    lea r9d, [rdi + rdx]         # x1
    lea r10d, [rsi + rcx]        # y1
    xor r11d, r11d
    test edi, edi
    cmovs edi, r11d
    test esi, esi
    cmovs esi, r11d
    mov r11d, W
    cmp r9d, r11d
    cmovg r9d, r11d
    mov r11d, H
    cmp r10d, r11d
    cmovg r10d, r11d
    sub r9d, edi                 # pixels per row
    jle 9f
    sub r10d, esi                # rows
    jle 9f
    imul esi, esi, W
    add esi, edi
    lea rdi, [r15 + rsi * 4]
    cmp eax, 255
    je opaque
    # Premultiply: (c * a + 127) / 255 per channel, alpha times 255.
    vmovd xmm0, r8d
    vpmovzxbw xmm0, xmm0
    vmovd xmm1, eax
    vpbroadcastw ymm1, xmm1
    vpsubw ymm8, ymm12, ymm1     # 255 - a
    vpblendw xmm1, xmm1, xmm12, 0x08
    vpmullw xmm0, xmm0, xmm1
    vpaddw xmm0, xmm0, xmm9
    vpmulhuw xmm0, xmm0, xmm10
    vpsrlw xmm0, xmm0, 7
    vpackuswb xmm0, xmm0, xmm0
    vpbroadcastd ymm11, xmm0     # the source, premultiplied
1:  mov rsi, rdi
    mov ecx, r9d
2:  cmp ecx, 8
    jb 3f
    vpmovzxbw ymm0, [rsi]
    vpmovzxbw ymm1, [rsi + 16]
    vpmullw ymm0, ymm0, ymm8
    vpmullw ymm1, ymm1, ymm8
    vpaddw ymm0, ymm0, ymm9
    vpaddw ymm1, ymm1, ymm9
    vpmulhuw ymm0, ymm0, ymm10
    vpmulhuw ymm1, ymm1, ymm10
    vpsrlw ymm0, ymm0, 7
    vpsrlw ymm1, ymm1, 7
    vpackuswb ymm0, ymm0, ymm1
    vpermq ymm0, ymm0, 0xd8
    vpaddb ymm0, ymm0, ymm11
    vmovdqu [rsi], ymm0
    add rsi, 32
    sub ecx, 8
    jmp 2b
3:  cmp ecx, 4
    jb 4f
    vpmovzxbw ymm0, [rsi]
    vpmullw ymm0, ymm0, ymm8
    vpaddw ymm0, ymm0, ymm9
    vpmulhuw ymm0, ymm0, ymm10
    vpsrlw ymm0, ymm0, 7
    vextracti128 xmm1, ymm0, 1
    vpackuswb xmm0, xmm0, xmm1
    vpaddb xmm0, xmm0, xmm11
    vmovdqu [rsi], xmm0
    add rsi, 16
    sub ecx, 4
4:  test ecx, ecx
    jz 5f
    vmovd xmm0, [rsi]
    vpmovzxbw xmm0, xmm0
    vpmullw xmm0, xmm0, xmm8
    vpaddw xmm0, xmm0, xmm9
    vpmulhuw xmm0, xmm0, xmm10
    vpsrlw xmm0, xmm0, 7
    vpackuswb xmm0, xmm0, xmm0
    vpaddb xmm0, xmm0, xmm11
    vmovd [rsi], xmm0
    add rsi, 4
    dec ecx
    jmp 4b
5:  add rdi, PITCH
    dec r10d
    jnz 1b
9:  ret

opaque:
    vmovd xmm0, r8d
    vpbroadcastd ymm0, xmm0
    cmp r9d, 8
    jb 4f
    lea r11, [r9 * 4 - 32]       # the row's last 8 pixels
1:  xor ecx, ecx
2:  vmovdqu [rdi + rcx], ymm0
    add rcx, 32
    cmp rcx, r11
    jb 2b
    vmovdqu [rdi + r11], ymm0
    add rdi, PITCH
    dec r10d
    jnz 1b
    ret
4:  cmp r9d, 4
    jb 6f
    lea r11, [r9 * 4 - 16]
5:  vmovdqu [rdi], xmm0
    vmovdqu [rdi + r11], xmm0
    add rdi, PITCH
    dec r10d
    jnz 5b
    ret
6:  xor ecx, ecx
7:  mov [rdi + rcx * 4], r8d
    inc ecx
    cmp ecx, r9d
    jb 7b
    add rdi, PITCH
    dec r10d
    jnz 6b
    ret

# text: rdx bytes of ASCII at rsi in the 5x7 font, at (edi, ecx), scale
# r8d, color r9d: per glyph column, one rectangle per vertical run of set
# bits, top first; a 6 * scale advance. A byte outside the font draws "?".
text:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    sub rsp, 16
    mov [rsp], r9d               # color
    mov [rsp + 4], r8d           # scale
    mov [rsp + 8], ecx           # y
    mov rbx, rsi
    lea rbp, [rsi + rdx]
    mov r12d, edi                # pen
1:  cmp rbx, rbp
    jae 9f
    movzx eax, byte ptr [rbx]
    sub eax, 32
    cmp eax, 94
    jbe 2f
    mov eax, 31
2:  lea eax, [rax + rax * 4]
    lea r13, [rip + font]
    add r13, rax                 # the glyph's columns
    xor r14d, r14d               # the column
3:  movzx eax, byte ptr [r13 + r14]
    mov [rsp + 12], eax          # its bits left
4:  mov eax, [rsp + 12]
    test eax, eax
    jz 5f
    tzcnt ecx, eax               # the run's first row
    mov edx, eax
    shr edx, cl
    not edx
    tzcnt edx, edx               # its length
    blsi r8d, eax
    add r8d, eax
    and r8d, eax
    mov [rsp + 12], r8d
    mov r9d, [rsp + 4]
    mov edi, r14d
    imul edi, r9d
    add edi, r12d
    mov esi, ecx
    imul esi, r9d
    add esi, [rsp + 8]
    mov ecx, edx
    imul ecx, r9d
    mov edx, r9d
    mov r8d, [rsp]
    call rect
    jmp 4b
5:  inc r14d
    cmp r14d, 5
    jb 3b
    imul eax, [rsp + 4], 6
    add r12d, eax
    inc rbx
    jmp 1b
9:  add rsp, 16
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
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

fail:
    mov edi, 2
    lea rsi, [rip + s_write]
    mov edx, 13
    mov eax, 1                   # write
    syscall
    mov edi, 1
    mov eax, 231
    syscall

    .p2align 2
palette:   .long 0, 0xffa46534, 0xff069a4e, 0xff00a0c4
# The 5x7 font for ASCII 0x20..0x7E: five column bytes per glyph, bit 0 the
# top row.
font:
    .byte 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x5F, 0x00, 0x00, 0x00, 0x07, 0x00, 0x07, 0x00, 0x14, 0x7F, 0x14, 0x7F, 0x14, 0x24, 0x2A, 0x7F, 0x2A, 0x12
    .byte 0x23, 0x13, 0x08, 0x64, 0x62, 0x36, 0x49, 0x55, 0x22, 0x50, 0x00, 0x05, 0x03, 0x00, 0x00, 0x00, 0x1C, 0x22, 0x41, 0x00, 0x00, 0x41, 0x22, 0x1C, 0x00
    .byte 0x08, 0x2A, 0x1C, 0x2A, 0x08, 0x08, 0x08, 0x3E, 0x08, 0x08, 0x00, 0x50, 0x30, 0x00, 0x00, 0x08, 0x08, 0x08, 0x08, 0x08, 0x00, 0x60, 0x60, 0x00, 0x00
    .byte 0x20, 0x10, 0x08, 0x04, 0x02, 0x3E, 0x51, 0x49, 0x45, 0x3E, 0x00, 0x42, 0x7F, 0x40, 0x00, 0x42, 0x61, 0x51, 0x49, 0x46, 0x21, 0x41, 0x45, 0x4B, 0x31
    .byte 0x18, 0x14, 0x12, 0x7F, 0x10, 0x27, 0x45, 0x45, 0x45, 0x39, 0x3C, 0x4A, 0x49, 0x49, 0x30, 0x01, 0x71, 0x09, 0x05, 0x03, 0x36, 0x49, 0x49, 0x49, 0x36
    .byte 0x06, 0x49, 0x49, 0x29, 0x1E, 0x00, 0x36, 0x36, 0x00, 0x00, 0x00, 0x56, 0x36, 0x00, 0x00, 0x08, 0x14, 0x22, 0x41, 0x00, 0x14, 0x14, 0x14, 0x14, 0x14
    .byte 0x00, 0x41, 0x22, 0x14, 0x08, 0x02, 0x01, 0x51, 0x09, 0x06, 0x32, 0x49, 0x79, 0x41, 0x3E, 0x7E, 0x11, 0x11, 0x11, 0x7E, 0x7F, 0x49, 0x49, 0x49, 0x36
    .byte 0x3E, 0x41, 0x41, 0x41, 0x22, 0x7F, 0x41, 0x41, 0x22, 0x1C, 0x7F, 0x49, 0x49, 0x49, 0x41, 0x7F, 0x09, 0x09, 0x09, 0x01, 0x3E, 0x41, 0x49, 0x49, 0x7A
    .byte 0x7F, 0x08, 0x08, 0x08, 0x7F, 0x00, 0x41, 0x7F, 0x41, 0x00, 0x20, 0x40, 0x41, 0x3F, 0x01, 0x7F, 0x08, 0x14, 0x22, 0x41, 0x7F, 0x40, 0x40, 0x40, 0x40
    .byte 0x7F, 0x02, 0x0C, 0x02, 0x7F, 0x7F, 0x04, 0x08, 0x10, 0x7F, 0x3E, 0x41, 0x41, 0x41, 0x3E, 0x7F, 0x09, 0x09, 0x09, 0x06, 0x3E, 0x41, 0x51, 0x21, 0x5E
    .byte 0x7F, 0x09, 0x19, 0x29, 0x46, 0x46, 0x49, 0x49, 0x49, 0x31, 0x01, 0x01, 0x7F, 0x01, 0x01, 0x3F, 0x40, 0x40, 0x40, 0x3F, 0x1F, 0x20, 0x40, 0x20, 0x1F
    .byte 0x3F, 0x40, 0x38, 0x40, 0x3F, 0x63, 0x14, 0x08, 0x14, 0x63, 0x07, 0x08, 0x70, 0x08, 0x07, 0x61, 0x51, 0x49, 0x45, 0x43, 0x00, 0x7F, 0x41, 0x41, 0x00
    .byte 0x02, 0x04, 0x08, 0x10, 0x20, 0x00, 0x41, 0x41, 0x7F, 0x00, 0x04, 0x02, 0x01, 0x02, 0x04, 0x40, 0x40, 0x40, 0x40, 0x40, 0x00, 0x01, 0x02, 0x04, 0x00
    .byte 0x20, 0x54, 0x54, 0x54, 0x78, 0x7F, 0x48, 0x44, 0x44, 0x38, 0x38, 0x44, 0x44, 0x44, 0x20, 0x38, 0x44, 0x44, 0x48, 0x7F, 0x38, 0x54, 0x54, 0x54, 0x18
    .byte 0x08, 0x7E, 0x09, 0x01, 0x02, 0x0C, 0x52, 0x52, 0x52, 0x3E, 0x7F, 0x08, 0x04, 0x04, 0x78, 0x00, 0x44, 0x7D, 0x40, 0x00, 0x20, 0x40, 0x44, 0x3D, 0x00
    .byte 0x7F, 0x10, 0x28, 0x44, 0x00, 0x00, 0x41, 0x7F, 0x40, 0x00, 0x7C, 0x04, 0x18, 0x04, 0x78, 0x7C, 0x08, 0x04, 0x04, 0x78, 0x38, 0x44, 0x44, 0x44, 0x38
    .byte 0x7C, 0x14, 0x14, 0x14, 0x08, 0x08, 0x14, 0x14, 0x18, 0x7C, 0x7C, 0x08, 0x04, 0x04, 0x08, 0x48, 0x54, 0x54, 0x54, 0x20, 0x04, 0x3F, 0x44, 0x40, 0x20
    .byte 0x3C, 0x40, 0x40, 0x20, 0x7C, 0x1C, 0x20, 0x40, 0x20, 0x1C, 0x3C, 0x40, 0x30, 0x40, 0x3C, 0x44, 0x28, 0x10, 0x28, 0x44, 0x0C, 0x50, 0x50, 0x50, 0x3C
    .byte 0x44, 0x64, 0x54, 0x4C, 0x44, 0x00, 0x08, 0x36, 0x41, 0x00, 0x00, 0x00, 0x7F, 0x00, 0x00, 0x00, 0x41, 0x36, 0x08, 0x00, 0x08, 0x04, 0x08, 0x10, 0x08
s_capture: .ascii "JET_FRAME_CAPTURE="
s_title:   .ascii "JET RASTER"
s_pam:     .ascii "P7\nWIDTH 320\nHEIGHT 200\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n"
s_pam_end:
s_frames:  .ascii "frames=1200 tiles="
s_sprites: .ascii " sprites="
s_glyphs:  .ascii " glyphs="
s_write:   .ascii "write failed\n"
