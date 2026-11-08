// A C file runs with plain `jet run PlainC.c`. Jet tells a file's language by
// its extension: `.c` is C, and `.cc`, `.cpp` and `.cxx` are C++. It compiles
// and links the file with the C toolchain Jet programs compile their own C
// with, then runs it; the program's output and exit code are the command's.
// `jet build PlainC.c` leaves the program at `.jet/build/PlainC`, and
// `--release` builds it optimized.
#include <stdio.h>

static unsigned long fibonacci(int n) {
    unsigned long previous = 0, current = 1;
    for (int i = 0; i < n; i++) {
        unsigned long next = previous + current;
        previous = current;
        current = next;
    }
    return previous;
}

int main(int argc, char **argv) {
    printf("hello from C\n");
    for (int n = 0; n <= 10; n++) printf("%s%lu", n == 0 ? "" : " ", fibonacci(n));
    printf("\n%d argument(s)\n", argc - 1);
    for (int i = 1; i < argc; i++) printf("  %s\n", argv[i]);
    return 0;
}
