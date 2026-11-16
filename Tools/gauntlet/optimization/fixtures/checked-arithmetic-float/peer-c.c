#include <stdio.h>

int main(void) {
    const double left = (1.0e16 + -1.0e16) + 1.0;
    puts(left == 1.0 ? "true" : "false");
    puts("123");
    return 0;
}
