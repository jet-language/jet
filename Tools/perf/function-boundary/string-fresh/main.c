/* Function-boundary cell string-fresh: C peer (fresh growable text per call). */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char *bytes;
    size_t len;
    size_t cap;
} Text;

static void append(Text *t, const char *piece, size_t n) {
    if (t->len + n > t->cap) {
        size_t cap = t->cap ? t->cap * 2 : 8;
        while (cap < t->len + n) cap *= 2;
        t->bytes = realloc(t->bytes, cap);
        if (!t->bytes) abort();
        t->cap = cap;
    }
    memcpy(t->bytes + t->len, piece, n);
    t->len += n;
}

__attribute__((noinline)) static Text render(int64_t seed) {
    Text out = {NULL, 0, 0};
    for (int64_t i = 0; i < 32 + seed % 32; i++) {
        if ((seed + i) % 2 == 0) append(&out, "ab", 2);
        else append(&out, "xyz", 3);
    }
    return out;
}

int main(int argc, char **argv) {
    int64_t n = argc > 1 ? strtoll(argv[1], NULL, 10) : 0;
    int64_t acc = 0;
    for (int64_t i = 0; i < n; i++) {
        Text text = render(i);
        acc += (int64_t)text.len;
        free(text.bytes);
    }
    printf("%lld\n", (long long)acc);
    return 0;
}
