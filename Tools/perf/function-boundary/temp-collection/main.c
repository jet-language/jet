/* Function-boundary cell temp-collection: C peer (local stack array). */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

__attribute__((noinline)) static int64_t score(int64_t seed) {
    int64_t tmp[16];
    for (int64_t i = 0; i < 16; i++) tmp[i] = (seed * 7 + i * 13) % 101;
    int64_t best = 0;
    for (int64_t i = 0; i < 16; i++) {
        if (tmp[i] > best) best = tmp[i];
    }
    return best + tmp[seed % 16];
}

int main(int argc, char **argv) {
    int64_t n = argc > 1 ? strtoll(argv[1], NULL, 10) : 0;
    int64_t acc = 0;
    for (int64_t i = 0; i < n; i++) acc += score(i);
    printf("%lld\n", (long long)acc);
    return 0;
}
