#include "guestimport.h"
#include <stdint.h>
#include <stdio.h>

extern "C" int64_t guest_host_add(int64_t value) {
    return value + 11;
}

int main() {
    printf("%lld\n", static_cast<long long>(call_host(31)));
    return 0;
}
