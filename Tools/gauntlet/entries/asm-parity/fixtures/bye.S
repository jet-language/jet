// Not the hello cell's output: write(1, "bye\n", 4), then exit(0).
    .globl _start
    .text
_start:
    mov $1, %eax
    mov $1, %edi
    lea msg(%rip), %rsi
    mov $4, %edx
    syscall
    mov $60, %eax
    xor %edi, %edi
    syscall
    .section .rodata
msg:
    .ascii "bye\n"
