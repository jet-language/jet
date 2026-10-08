// Exercise the real borrowed zeroize ABI and observe the SAME storage.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
extern void jet_crypto_zeroize(const uint64_t *, int64_t);
extern int64_t jet_rt_zeroize_fixture_stride(void);
int main(void) {
    const int64_t stride = jet_rt_zeroize_fixture_stride();
    assert(stride == 1 || stride == 8);
    struct { uint64_t before; unsigned char payload[56]; uint64_t after; } buffer;
    memset(&buffer, 0xa5, sizeof buffer);
    const size_t count = 7 * (size_t)stride;
    jet_crypto_zeroize((const uint64_t *)(const void *)buffer.payload, 7);
    for (size_t i = 0; i < count; ++i) assert(buffer.payload[i] == 0);
    for (size_t i = count; i < sizeof buffer.payload; ++i) assert(buffer.payload[i] == 0xa5);
    assert(buffer.before == UINT64_C(0xa5a5a5a5a5a5a5a5));
    assert(buffer.after == UINT64_C(0xa5a5a5a5a5a5a5a5));
    memset(buffer.payload, 0x5a, sizeof buffer.payload);
    jet_crypto_zeroize((const uint64_t *)(const void *)buffer.payload, 0);
    for (size_t i = 0; i < sizeof buffer.payload; ++i) assert(buffer.payload[i] == 0x5a);
    jet_crypto_zeroize(NULL, 0);
    // Borrowed payload remains owned/live here; the leaf did not free it.
    buffer.payload[0] = 0x42;
    assert(buffer.payload[0] == 0x42);
    puts("zeroize actual_span=zero len0=unchanged null0=ok");
    return 0;
}
