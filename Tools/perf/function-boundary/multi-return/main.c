/* Function-boundary cell multi-return: C peer. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int64_t quotient;
    int64_t remainder;
} Parts;

__attribute__((noinline)) static Parts split(int64_t n, int64_t d) {
    Parts p = {n / d, n % d};
    return p;
}

int main(int argc, char **argv) {
    int64_t n = argc > 1 ? strtoll(argv[1], NULL, 10) : 0;
    int64_t acc = 0;
    for (int64_t i = 0; i < n; i++) {
        Parts p = split(i, i % 13 + 3);
        acc += p.quotient + p.remainder;
    }
    printf("%lld\n", (long long)acc);
    return 0;
}
