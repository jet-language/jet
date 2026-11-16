#include <stdio.h>

int driver_value(void);

int main(void) {
    const int value = driver_value() + 1;
    printf("%d\n", value);
    return value == 42 ? 0 : 1;
}
