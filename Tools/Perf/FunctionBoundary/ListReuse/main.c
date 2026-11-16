/* Function-boundary cell list-reuse: C peer (caller-reused buffer). */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int64_t *items;
    int64_t len;
    int64_t cap;
} List;

__attribute__((noinline)) static void build_into(List *out, int64_t seed, int64_t count) {
    out->len = 0;
    if (out->cap < count) {
        out->items = realloc(out->items, (size_t)count * sizeof(int64_t));
        if (!out->items) abort();
        out->cap = count;
    }
    for (int64_t i = 0; i < count; i++) {
        out->items[out->len++] = seed + i;
    }
}

int main(int argc, char **argv) {
    int64_t n = argc > 1 ? strtoll(argv[1], NULL, 10) : 0;
    int64_t acc = 0;
    List xs = {NULL, 0, 0};
    for (int64_t i = 0; i < n; i++) {
        build_into(&xs, i, 256);
        for (int64_t j = 0; j < xs.len; j++) {
            acc += xs.items[j];
        }
    }
    free(xs.items);
    printf("%lld\n", (long long)acc);
    return 0;
}
