/* Function-boundary cell list-fresh: C peer (fresh heap buffer per call). */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int64_t *items;
    int64_t len;
} List;

__attribute__((noinline)) static List build(int64_t seed, int64_t count) {
    List out = {malloc((size_t)count * sizeof(int64_t)), count};
    if (!out.items) abort();
    for (int64_t i = 0; i < count; i++) {
        out.items[i] = seed + i;
    }
    return out;
}

int main(int argc, char **argv) {
    int64_t n = argc > 1 ? strtoll(argv[1], NULL, 10) : 0;
    int64_t acc = 0;
    for (int64_t i = 0; i < n; i++) {
        List xs = build(i, 256);
        for (int64_t j = 0; j < xs.len; j++) {
            acc += xs.items[j];
        }
        free(xs.items);
    }
    printf("%lld\n", (long long)acc);
    return 0;
}
