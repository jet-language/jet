/* Function-boundary cell destructure-move (#4567): C peer. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int64_t *items;
    int64_t len;
} List;

typedef struct {
    List evens;
    List odds;
} Parts;

__attribute__((noinline)) static Parts partition(int64_t seed) {
    Parts out = {{malloc(64 * sizeof(int64_t)), 0}, {malloc(64 * sizeof(int64_t)), 0}};
    if (!out.evens.items || !out.odds.items) abort();
    for (int64_t i = 0; i < 64; i++) {
        int64_t v = seed + i;
        if (v % 2 == 0) out.evens.items[out.evens.len++] = v;
        else out.odds.items[out.odds.len++] = v;
    }
    return out;
}

int main(int argc, char **argv) {
    int64_t n = argc > 1 ? strtoll(argv[1], NULL, 10) : 0;
    int64_t acc = 0;
    for (int64_t i = 0; i < n; i++) {
        Parts p = partition(i);
        acc += p.evens.len + p.odds.items[0] + p.evens.items[31];
        free(p.evens.items);
        free(p.odds.items);
    }
    printf("%lld\n", (long long)acc);
    return 0;
}
