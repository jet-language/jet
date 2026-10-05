/* Function-boundary cell recursion-aggregate: C peer. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int64_t count;
    int64_t sum;
    int64_t low;
    int64_t high;
} Stats;

static Stats stats(int64_t lo, int64_t hi) {
    if (hi - lo <= 4) {
        Stats s = {0, 0, 10007, 0};
        for (int64_t i = lo; i < hi; i++) {
            int64_t v = (i * 7919) % 10007;
            s.count += 1;
            s.sum += v;
            if (v < s.low) s.low = v;
            if (v > s.high) s.high = v;
        }
        return s;
    }
    int64_t mid = lo + (hi - lo) / 2;
    Stats a = stats(lo, mid);
    Stats b = stats(mid, hi);
    Stats out = {a.count + b.count, a.sum + b.sum, a.low < b.low ? a.low : b.low, a.high > b.high ? a.high : b.high};
    return out;
}

int main(int argc, char **argv) {
    int64_t n = argc > 1 ? strtoll(argv[1], NULL, 10) : 0;
    int64_t acc = 0;
    for (int64_t r = 0; r < n; r++) {
        Stats s = stats(r, r + 100000);
        acc += s.count + s.sum + s.low + s.high;
    }
    printf("%lld\n", (long long)acc);
    return 0;
}
