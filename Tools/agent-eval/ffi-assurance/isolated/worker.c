#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define FA_PROTOCOL_VERSION 1u
#define FA_HEADER_BYTES 40u
#define FA_MAX_PAYLOAD 4096u
#define FA_KIND_SUM_REQUEST 1u
#define FA_KIND_SUM_RESPONSE 2u
#define FA_KIND_ERROR 3u
#define FA_STATUS_OK 0u
#define FA_STATUS_BAD_MAGIC 1u
#define FA_STATUS_BAD_VERSION 2u
#define FA_STATUS_BAD_KIND 3u
#define FA_STATUS_BAD_LENGTH 4u
#define FA_STATUS_BAD_CHECKSUM 5u
#define FA_STATUS_BAD_REQUEST 6u
#define FA_STATUS_INTERNAL 7u

static const uint8_t FA_MAGIC[4] = {'F', 'A', 'I', 'S'};

static int read_exact(uint8_t *buffer, size_t length) {
    size_t offset = 0;
    while (offset < length) {
        const size_t got = fread(buffer + offset, 1, length - offset, stdin);
        if (got == 0) return offset == 0 && feof(stdin) ? 0 : -1;
        offset += got;
    }
    return 1;
}

static int write_exact(const uint8_t *buffer, size_t length) {
    return fwrite(buffer, 1, length, stdout) == length && fflush(stdout) == 0 ? 0 : -1;
}

static uint16_t get_u16(const uint8_t *bytes) {
    return (uint16_t)bytes[0] | ((uint16_t)bytes[1] << 8);
}

static uint32_t get_u32(const uint8_t *bytes) {
    return (uint32_t)bytes[0] |
           ((uint32_t)bytes[1] << 8) |
           ((uint32_t)bytes[2] << 16) |
           ((uint32_t)bytes[3] << 24);
}

static uint64_t get_u64(const uint8_t *bytes) {
    uint64_t value = 0;
    for (unsigned index = 0; index < 8; ++index) value |= (uint64_t)bytes[index] << (index * 8);
    return value;
}

static void put_u16(uint8_t *bytes, uint16_t value) {
    bytes[0] = (uint8_t)value;
    bytes[1] = (uint8_t)(value >> 8);
}

static void put_u32(uint8_t *bytes, uint32_t value) {
    for (unsigned index = 0; index < 4; ++index) bytes[index] = (uint8_t)(value >> (index * 8));
}

static void put_u64(uint8_t *bytes, uint64_t value) {
    for (unsigned index = 0; index < 8; ++index) bytes[index] = (uint8_t)(value >> (index * 8));
}

static uint64_t checksum(const uint8_t *payload, size_t length) {
    uint64_t value = UINT64_C(1469598103934665603);
    for (size_t index = 0; index < length; ++index) {
        value ^= payload[index];
        value *= UINT64_C(1099511628211);
    }
    return value;
}

static int send_message(uint16_t kind, uint64_t request_id, uint32_t status,
                        uint64_t value, uint64_t payload_checksum) {
    uint8_t header[FA_HEADER_BYTES] = {0};
    memcpy(header, FA_MAGIC, sizeof(FA_MAGIC));
    put_u16(header + 4, FA_PROTOCOL_VERSION);
    put_u16(header + 6, kind);
    put_u64(header + 8, request_id);
    put_u32(header + 16, 0);
    put_u32(header + 20, status);
    put_u64(header + 24, value);
    put_u64(header + 32, payload_checksum);
    return write_exact(header, sizeof(header));
}

static int handle_request(const uint8_t *header) {
    if (memcmp(header, FA_MAGIC, sizeof(FA_MAGIC)) != 0) {
        (void)send_message(FA_KIND_ERROR, 0, FA_STATUS_BAD_MAGIC, 0, 0);
        return 2;
    }
    const uint16_t version = get_u16(header + 4);
    const uint16_t kind = get_u16(header + 6);
    const uint64_t request_id = get_u64(header + 8);
    const uint32_t payload_length = get_u32(header + 16);
    const uint32_t status = get_u32(header + 20);
    const uint64_t request_value = get_u64(header + 24);
    const uint64_t expected_checksum = get_u64(header + 32);

    if (version != FA_PROTOCOL_VERSION) {
        (void)send_message(FA_KIND_ERROR, request_id, FA_STATUS_BAD_VERSION, 0, 0);
        return 2;
    }
    if (kind != FA_KIND_SUM_REQUEST) {
        (void)send_message(FA_KIND_ERROR, request_id, FA_STATUS_BAD_KIND, 0, 0);
        return 2;
    }
    if (request_id == 0 || status != FA_STATUS_OK || request_value != 0) {
        (void)send_message(FA_KIND_ERROR, request_id, FA_STATUS_BAD_REQUEST, 0, 0);
        return 2;
    }
    if (payload_length > FA_MAX_PAYLOAD) {
        (void)send_message(FA_KIND_ERROR, request_id, FA_STATUS_BAD_LENGTH, 0, 0);
        return 2;
    }

    uint8_t *payload = NULL;
    if (payload_length != 0) {
        payload = (uint8_t *)malloc(payload_length);
        if (payload == NULL) {
            (void)send_message(FA_KIND_ERROR, request_id, FA_STATUS_INTERNAL, 0, 0);
            return 2;
        }
        if (read_exact(payload, payload_length) != 1) {
            free(payload);
            (void)send_message(FA_KIND_ERROR, request_id, FA_STATUS_BAD_LENGTH, 0, 0);
            return 2;
        }
    }

    const uint64_t actual_checksum = checksum(payload, payload_length);
    if (actual_checksum != expected_checksum) {
        free(payload);
        (void)send_message(FA_KIND_ERROR, request_id, FA_STATUS_BAD_CHECKSUM, 0, actual_checksum);
        return 2;
    }

    uint64_t sum = 0;
    for (uint32_t index = 0; index < payload_length; ++index) sum += payload[index];
    free(payload);
    if (send_message(FA_KIND_SUM_RESPONSE, request_id, FA_STATUS_OK, sum, actual_checksum) != 0) return 2;
    return 0;
}

int main(void) {
    uint8_t header[FA_HEADER_BYTES];
    for (;;) {
        const int read_status = read_exact(header, sizeof(header));
        if (read_status == 0) return 0;
        if (read_status < 0) return 2;
        const int handled = handle_request(header);
        if (handled != 0) return handled;
    }
}
