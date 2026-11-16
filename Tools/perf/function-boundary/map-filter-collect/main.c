/* Function-boundary cell map-filter-collect: C peer (one loop into a fresh array). */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define COUNT 100000

int main(int argc, char **argv) {
    int64_t n = argc > 1 ? strtoll(argv[1], NULL, 10) : 0;
    int64_t *xs = malloc(COUNT * sizeof(int64_t));
    if (!xs) abort();
    for (int64_t i = 0; i < COUNT; i++) xs[i] = i;
    int64_t acc = 0;
    for (int64_t r = 0; r < n; r++) {
        int64_t *ys = malloc(COUNT * sizeof(int64_t));
        if (!ys) abort();
        int64_t len = 0;
        for (int64_t i = 0; i < COUNT; i++) {
            int64_t x = xs[i] * 3 + r;
            if (x % 2 == 0) ys[len++] = x + 1;
        }
        for (int64_t i = 0; i < len; i++) acc += ys[i];
        free(ys);
    }
    free(xs);
    printf("%lld\n", (long long)acc);
    return 0;
}
