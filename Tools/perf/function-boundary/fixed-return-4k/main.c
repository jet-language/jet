/* Function-boundary cell fixed-return-4k: C peer (4096-byte array return). */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define K 512

typedef struct {
    int64_t items[K];
} Block;

__attribute__((noinline)) static Block fill(int64_t seed) {
    Block out = {{0}};
    for (int64_t i = 0; i < K; i++) {
        out.items[i] = seed + i;
    }
    return out;
}

int main(int argc, char **argv) {
    int64_t n = argc > 1 ? strtoll(argv[1], NULL, 10) : 0;
    int64_t acc = 0;
    for (int64_t i = 0; i < n; i++) {
        Block block = fill(i);
        for (int64_t j = 0; j < K; j++) {
            acc += block.items[j];
        }
    }
    printf("%lld\n", (long long)acc);
    return 0;
}
