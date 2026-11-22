#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    static char in_buf[65536];
    static char out_buf[65536];
    setvbuf(stdin, in_buf, _IOFBF, sizeof(in_buf));
    setvbuf(stdout, out_buf, _IOFBF, sizeof(out_buf));

    char *line = NULL;
    size_t cap = 0;
    ssize_t n;
    while ((n = getline(&line, &cap, stdin)) != -1) {
        if (n > 0 && line[n - 1] == '\n') n--;
        if (n > 0 && line[n - 1] == '\r') n--;
        if (n > 0 && fwrite(line, 1, (size_t)n, stdout) != (size_t)n) return 1;
        if (fputc('\n', stdout) == EOF) return 1;
    }
    free(line);
    if (ferror(stdin) || fflush(stdout) != 0) return 1;
    return 0;
}
