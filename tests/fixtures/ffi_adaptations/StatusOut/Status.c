#include "status.h"

int32_t decode_status(const uint8_t *input, uint32_t input_len, uint8_t *output, uint32_t output_cap, uint32_t *written) {
    if (output_cap < input_len) return -1;
    for (uint32_t index = 0; index < input_len; ++index) output[index] = input[index] ^ 0xffu;
    *written = input_len;
    return 0;
}
