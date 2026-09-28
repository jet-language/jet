#include <stdio.h>

int main(void) {
    const int values[] = {1, 2, 3, 4};
    int total = 0;
    for (size_t i = 0; i < 4; ++i) total += values[i] * 2;
    printf("%d\n", total);
    return 0;
}
