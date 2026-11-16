#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static int read_exact(FILE *file, void *buf, size_t size) {
    return fread(buf, 1, size, file) == size;
}

int main(int argc, char **argv) {
    FILE *file = fopen(argc > 1 ? argv[1] : "records.bin", "rb");
    if (!file) return 1;
    char magic[4]; uint32_t count;
    if (!read_exact(file, magic, 4) || magic[0] != 'J' || magic[1] != 'G' || magic[2] != 'B' || magic[3] != '1' || !read_exact(file, &count, 4)) return 1;
    double sum = 0.0; uint64_t hash = UINT64_C(0xcbf29ce484222325);
    for (uint32_t i = 0; i < count; ++i) {
        uint32_t id; double value; uint16_t name_len;
        if (!read_exact(file, &id, 4) || !read_exact(file, &value, 8) || !read_exact(file, &name_len, 2)) return 1;
        if (id % 7 == 0) sum += value;
        for (uint16_t j = 0; j < name_len; ++j) { unsigned char byte; if (!read_exact(file, &byte, 1)) return 1; hash = (hash ^ byte) * UINT64_C(0x100000001b3); }
    }
    fclose(file);
    printf("records %u\n", count);
    printf("sum7 %.6f\n", sum);
    printf("fnv %016llx\n", (unsigned long long)hash);
    return 0;
}
