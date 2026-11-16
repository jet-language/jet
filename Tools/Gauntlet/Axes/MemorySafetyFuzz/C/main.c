#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>

enum { RECORD_BYTES = 64, METADATA_BYTES = 4, PAYLOAD_BYTES = 60, KIND_COUNT = 5 };

int main(void) {
    FILE *file = fopen("fuzz-input.bin", "rb");
    if (file == NULL) return 1;
    if (fseek(file, 0, SEEK_END) != 0) {
        fclose(file);
        return 1;
    }
    long size = ftell(file);
    if (size <= 0 || size % RECORD_BYTES != 0) {
        fclose(file);
        return 2;
    }
    rewind(file);
    size_t bytes = (size_t)size;
    unsigned char *data = malloc(bytes);
    if (data == NULL) {
        fclose(file);
        return 1;
    }
    if (fread(data, 1, bytes, file) != bytes) {
        free(data);
        fclose(file);
        return 1;
    }
    fclose(file);

    size_t counts[KIND_COUNT] = {0};
    uint32_t checksum = 0;
    uint32_t semantic = 0;
    for (size_t offset = 0; offset < bytes; offset += RECORD_BYTES) {
        const unsigned int kind = data[offset];
        if (kind >= KIND_COUNT) {
            free(data);
            return 2;
        }
        const size_t declared_length = data[offset + 1];
        const size_t requested_index = data[offset + 2];
        const size_t bounded_length = declared_length < PAYLOAD_BYTES ? declared_length : PAYLOAD_BYTES;
        const size_t safe_index = requested_index < PAYLOAD_BYTES ? requested_index : PAYLOAD_BYTES;
        uint32_t value = 0;
        counts[kind] += 1;
        for (size_t index = 0; index < RECORD_BYTES; index += 1)
            checksum += data[offset + index];
        if (kind == 0 || kind == 2) {
            for (size_t index = 0; index < bounded_length; index += 1)
                value += data[offset + METADATA_BYTES + index];
        } else if (requested_index < PAYLOAD_BYTES) {
            unsigned char selected = data[offset + METADATA_BYTES + requested_index];
            if (kind == 3) {
                unsigned char copied = selected;
                selected = copied;
            }
            value = selected;
            if (kind == 4) value ^= 0xa5u;
        }
        semantic += value;
        semantic += (uint32_t)((kind + 1u) * 257u + bounded_length + safe_index);
    }

    printf(
        "cases %zu valid %zu boundary %zu oob %zu use_after_free %zu wrong_output %zu bytes %zu checksum %u semantic %u\n",
        bytes / RECORD_BYTES,
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        counts[4],
        bytes,
        checksum,
        semantic
    );
    free(data);
    return 0;
}
