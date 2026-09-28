#include "sums.h"

uint64_t sum_bytes(const uint8_t *bytes, uint64_t count) {
    uint64_t total = 0;
    for (uint64_t index = 0; index < count; ++index) {
        total += bytes[index];
    }
    return total;
}
