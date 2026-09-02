#include "guestimport.h"
#include <stdint.h>
#include <stdio.h>

int64_t guest_host_add(int64_t value) {
    return value + 7;
}

int main(void) {
    printf("%lld\n", (long long)call_host(35));
    return 0;
}
