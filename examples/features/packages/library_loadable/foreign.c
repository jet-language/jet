/* Native host proof for `jet build --lib library.jet`. */
#include "loadable.h"
#include <stdio.h>

int main(void) {
    printf("%lld\n", (long long)on_tick(41));
    return 0;
}
