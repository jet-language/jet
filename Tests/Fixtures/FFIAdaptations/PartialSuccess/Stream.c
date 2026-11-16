#include "stream.h"

int32_t read_chunk(const uint8_t *input, uint32_t input_len, uint8_t *output, uint32_t output_cap, uint32_t *read_len) {
    uint32_t amount = input_len < output_cap ? input_len : output_cap;
    for (uint32_t index = 0; index < amount; ++index) output[index] = input[index];
    *read_len = amount;
    return amount == input_len ? 0 : 1;
}
