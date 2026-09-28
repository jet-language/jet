// Selected from Peter Scott's murmur3.c at revision dae94be0c0f54a399d23ea6cbe54bca5a4e93ce4.
// Upstream places MurmurHash3 in the public domain.

#include <stdint.h>

static inline uint32_t rotl32(uint32_t x, int8_t r) {
    return (x << r) | (x >> (32 - r));
}

static inline uint32_t fmix32(uint32_t h) {
    h ^= h >> 16;
    h *= 0x85ebca6b;
    h ^= h >> 13;
    h *= 0xc2b2ae35;
    h ^= h >> 16;
    return h;
}

void MurmurHash3_x86_32(const uint8_t *data, int len, uint32_t seed, uint32_t *out) {
    const int nblocks = len / 4;
    uint32_t h1 = seed;
    const uint32_t c1 = 0xcc9e2d51;
    const uint32_t c2 = 0x1b873593;

    for (int block = 0; block < nblocks; block++) {
        const int offset = block * 4;
        uint32_t k1 = (uint32_t)data[offset]
            | ((uint32_t)data[offset + 1] << 8)
            | ((uint32_t)data[offset + 2] << 16)
            | ((uint32_t)data[offset + 3] << 24);
        k1 *= c1;
        k1 = rotl32(k1, 15);
        k1 *= c2;
        h1 ^= k1;
        h1 = rotl32(h1, 13);
        h1 = h1 * 5 + 0xe6546b64;
    }

    const int tail = nblocks * 4;
    uint32_t k1 = 0;
    switch (len & 3) {
    case 3: k1 ^= (uint32_t)data[tail + 2] << 16; /* fall through */
    case 2: k1 ^= (uint32_t)data[tail + 1] << 8; /* fall through */
    case 1: k1 ^= (uint32_t)data[tail];
            k1 *= c1;
            k1 = rotl32(k1, 15);
            k1 *= c2;
            h1 ^= k1;
    }

    h1 ^= (uint32_t)len;
    *out = fmix32(h1);
}
