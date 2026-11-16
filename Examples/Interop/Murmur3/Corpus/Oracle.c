#include <stdint.h>
#include <stdio.h>

void MurmurHash3_x86_32(const uint8_t *data, int len, uint32_t seed, uint32_t *out);

struct vector {
    const uint8_t *data;
    int len;
    uint32_t seed;
    uint32_t expected;
};

int main(void) {
    static const uint8_t binary[] = {0, 1, 2, 3, 255};
    static const struct vector vectors[] = {
        {(const uint8_t *)"", 0, 0, 0},
        {(const uint8_t *)"a", 1, 0, 1009084850},
        {(const uint8_t *)"ab", 2, 0, 2613040991},
        {(const uint8_t *)"abc", 3, 0, 3017643002},
        {(const uint8_t *)"abcd", 4, 0, 1139631978},
        {(const uint8_t *)"hello", 5, 0, 613153351},
        {(const uint8_t *)"hello world", 11, 0, 1586663183},
        {(const uint8_t *)"hello", 5, 42, 3806057185},
        {binary, 5, 7, 3881383995},
    };

    for (size_t i = 0; i < sizeof(vectors) / sizeof(vectors[0]); i++) {
        uint32_t result = 0;
        MurmurHash3_x86_32(vectors[i].data, vectors[i].len, vectors[i].seed, &result);
        printf("%u\n", result);
        if (result != vectors[i].expected) return 1;
    }
    return 0;
}
