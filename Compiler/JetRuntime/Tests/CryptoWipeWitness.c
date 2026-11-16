// Exercise the crypto.wipe kernel on real native [U8] headers [len, cap, data].
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
extern void *jet_rt_alloc(int64_t, int64_t);
extern void jet_rt_free(void *, int64_t, int64_t);
extern void jet_rt_block_retain(void *);
extern bool jet_rt_block_release(void *);
extern bool jet_rt_block_unique(void *);
extern void jet_rt_crypto_wipe(int64_t *);
extern int64_t jet_rt_wipe_fixture_stride(void);
static void all(const unsigned char *bytes, size_t count, unsigned char value) {
    for (size_t i = 0; i < count; ++i) assert(bytes[i] == value);
}
int main(void) {
    const int64_t stride = jet_rt_wipe_fixture_stride();
    assert(stride == 1 || stride == 8);
    const int64_t bytes = 16 * stride;
    // A buffer this List alone owns: every element and the spare capacity
    // are zeroed in place; the header is unchanged.
    unsigned char *data = jet_rt_alloc(bytes, 8);
    memset(data, 0xa5, (size_t)bytes);
    int64_t header[3] = {5, 16, (int64_t)(uintptr_t)data};
    jet_rt_crypto_wipe(header);
    assert(header[0] == 5 && header[1] == 16 && header[2] == (int64_t)(uintptr_t)data);
    all(data, (size_t)bytes, 0);
    // A buffer another List shares: the wiped List takes fresh zeroes of the
    // same capacity; the other keeps its bytes until its last release, whose
    // free zeroes them (a cached small block stays mapped).
    memset(data, 0x5a, (size_t)bytes);
    jet_rt_block_retain(data);
    jet_rt_crypto_wipe(header);
    unsigned char *fresh = (unsigned char *)(uintptr_t)header[2];
    assert(header[0] == 5 && header[1] == 16 && fresh != data);
    all(fresh, (size_t)bytes, 0);
    all(data, (size_t)bytes, 0x5a);
    assert(((const int64_t *)(const void *)data)[-3] == (INT64_C(1) << 62 | 1));
    assert(jet_rt_block_unique(data));
    assert(jet_rt_block_release(data));
    jet_rt_free(data, bytes, 8);
    all(data, (size_t)bytes, 0);
    jet_rt_free(fresh, bytes, 8);
    // Frozen bytes (capacity 0) cannot be written: the List takes fresh
    // zeroes of its length and the static bytes stay.
    static const unsigned char frozen[24] = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24};
    int64_t still[3] = {3, 0, (int64_t)(uintptr_t)frozen};
    jet_rt_crypto_wipe(still);
    unsigned char *own = (unsigned char *)(uintptr_t)still[2];
    assert(still[0] == 3 && still[1] == 3 && own != frozen);
    all(own, (size_t)(3 * stride), 0);
    assert(frozen[0] == 1 && frozen[2] == 3);
    jet_rt_free(own, 3 * stride, 8);
    // An empty List has nothing to wipe.
    int64_t empty[3] = {0, 0, 0};
    jet_rt_crypto_wipe(empty);
    assert(empty[0] == 0 && empty[1] == 0 && empty[2] == 0);
    puts("wipe unique=zero_in_place shared=fresh_zero_then_zero_at_free frozen=fresh_zero empty=unchanged");
    return 0;
}
