// Hello world for x86-64 Linux with no C library: two system calls.
// This is the floor for the hello-compile benchmark (bench.py): the least
// code a program can run to print "hello" and exit. Build:
//   gcc -nostdlib -static -o hello hello.S
// For the smallest file, also pass -s -Wl,-n,--build-id=none,-z,norelro.
    .intel_syntax noprefix
    .section .rodata
message:
    .ascii "hello\n"
    .set message_length, . - message

    .text
    .globl _start
_start:
    mov eax, 1                  // write(
    mov edi, 1                  //   stdout,
    lea rsi, [rip + message]    //   message,
    mov edx, message_length     //   length)
    syscall
    mov eax, 60                 // exit(
    xor edi, edi                //   0)
    syscall

    .section .note.GNU-stack, "", @progbits
