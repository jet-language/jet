#include "guestimport.h"
#include <cstdint>
#include <cstdio>

extern "C" std::int64_t guest_host_add(std::int64_t value) {
    return value + 11;
}

int main() {
    std::printf("%lld\n", static_cast<long long>(call_host(31)));
    return 0;
}
