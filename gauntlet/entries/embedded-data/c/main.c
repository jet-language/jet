#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int read_u8(const unsigned char *data, size_t size, size_t *at, uint8_t *value) {
    if (*at >= size) return 0;
    *value = data[*at];
    *at += 1;
    return 1;
}

static int read_u16(const unsigned char *data, size_t size, size_t *at, uint16_t *value) {
    uint8_t low, high;
    if (!read_u8(data, size, at, &low) || !read_u8(data, size, at, &high)) return 0;
    *value = (uint16_t)low | ((uint16_t)high << 8);
    return 1;
}

static int read_u32(const unsigned char *data, size_t size, size_t *at, uint32_t *value) {
    uint8_t bytes[4];
    for (size_t index = 0; index < 4; ++index) {
        if (!read_u8(data, size, at, &bytes[index])) return 0;
    }
    *value = (uint32_t)bytes[0] |
        ((uint32_t)bytes[1] << 8) |
        ((uint32_t)bytes[2] << 16) |
        ((uint32_t)bytes[3] << 24);
    return 1;
}

int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : "telemetry.bin";
    FILE *file = fopen(path, "rb");
    if (!file) return 1;
    if (fseek(file, 0, SEEK_END) != 0) return 1;
    long length = ftell(file);
    if (length < 0 || fseek(file, 0, SEEK_SET) != 0) return 1;
    size_t size = (size_t)length;
    unsigned char *data = malloc(size);
    if (!data || fread(data, 1, size, file) != size) return 1;
    fclose(file);

    if (size < 8 || memcmp(data, "EDB1", 4) != 0) return 1;
    size_t at = 4;
    uint32_t count;
    if (!read_u32(data, size, &at, &count)) return 1;
    uint64_t checksum = 0;
    for (size_t index = 0; index < size; ++index) checksum += data[index];

    uint64_t valid = 0;
    uint64_t channels[4] = {0, 0, 0, 0};
    uint64_t sample_min = UINT16_MAX;
    uint64_t sample_max = 0;
    uint64_t sample_sum = 0;
    uint64_t energy_sum = 0;
    uint64_t load_sum = 0;
    uint64_t tick_sum = 0;

    for (uint32_t index = 0; index < count; ++index) {
        uint8_t channel, flags;
        uint16_t sample, energy, load;
        uint32_t tick;
        if (!read_u8(data, size, &at, &channel) ||
            !read_u8(data, size, &at, &flags) ||
            !read_u16(data, size, &at, &sample) ||
            !read_u32(data, size, &at, &tick) ||
            !read_u16(data, size, &at, &energy) ||
            !read_u16(data, size, &at, &load)) return 1;
        if (channel >= 4) return 1;
        if (flags == 0) {
            ++valid;
            ++channels[channel];
            if (sample < sample_min) sample_min = sample;
            if (sample > sample_max) sample_max = sample;
            sample_sum += sample;
            energy_sum += energy;
            load_sum += load;
            tick_sum += tick;
        }
    }

    printf("frames %u\n", count);
    printf("valid %llu\n", (unsigned long long)valid);
    printf("channels %llu %llu %llu %llu\n",
        (unsigned long long)channels[0], (unsigned long long)channels[1],
        (unsigned long long)channels[2], (unsigned long long)channels[3]);
    printf("sample %llu %llu %llu\n",
        (unsigned long long)sample_min, (unsigned long long)sample_max,
        (unsigned long long)sample_sum);
    printf("energy %llu\n", (unsigned long long)energy_sum);
    printf("load %llu\n", (unsigned long long)load_sum);
    printf("ticks %llu\n", (unsigned long long)tick_sum);
    printf("checksum %llu\n", (unsigned long long)checksum);
    free(data);
    return 0;
}
