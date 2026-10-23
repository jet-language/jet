#include <stddef.h>
#include <stdint.h>

int64_t selfhost_sum_bytes(const int8_t *bytes, size_t count) {
    int64_t total = 0;
    for (size_t index = 0; index < count; index += 1) {
        total += bytes[index];
    }
    return total;
}
