/* Function-boundary cell split-join: C peer (piece table, then join). */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    const char *start;
    size_t len;
} Piece;

int main(int argc, char **argv) {
    int64_t n = argc > 1 ? strtoll(argv[1], NULL, 10) : 0;
    size_t cap = 20000 * 8, len = 0;
    char *text = malloc(cap);
    if (!text) abort();
    for (int i = 0; i < 20000; i++) {
        len += (size_t)snprintf(text + len, cap - len, i ? ",w%d" : "w%d", i % 1000);
    }
    int64_t acc = 0;
    for (int64_t r = 0; r < n; r++) {
        size_t count = 1;
        for (size_t i = 0; i < len; i++) count += text[i] == ',';
        Piece *pieces = malloc(count * sizeof(Piece));
        if (!pieces) abort();
        size_t k = 0, start = 0, total = 0;
        for (size_t i = 0; i <= len; i++) {
            if (i == len || text[i] == ',') {
                pieces[k].start = text + start;
                pieces[k].len = i - start;
                total += i - start;
                k++;
                start = i + 1;
            }
        }
        size_t out_len = total + (count - 1);
        char *joined = malloc(out_len + 1);
        if (!joined) abort();
        size_t at = 0;
        for (size_t p = 0; p < count; p++) {
            if (p) joined[at++] = ';';
            memcpy(joined + at, pieces[p].start, pieces[p].len);
            at += pieces[p].len;
        }
        acc += (int64_t)at + r;
        free(joined);
        free(pieces);
    }
    free(text);
    printf("%lld\n", (long long)acc);
    return 0;
}
