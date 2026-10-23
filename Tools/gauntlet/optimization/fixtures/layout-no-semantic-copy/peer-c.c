#include <stdint.h>
#include <stdio.h>

struct Pair {
    uint32_t left;
    uint32_t right;
};

int main(void) {
    const struct Pair pair = {1, 2};
    printf("%u\n", pair.left + pair.right);
    return 0;
}
