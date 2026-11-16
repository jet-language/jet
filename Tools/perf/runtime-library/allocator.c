// Shared driver for Jet runtime O0/O1 and Rust std::alloc: identical workload.
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>

extern void *jet_rt_alloc(int64_t size, int64_t align);
extern void jet_rt_free(void *pointer, int64_t size, int64_t align);

int main(void) {
    uint64_t checksum = 0;
    for (uint64_t iteration = 0; iteration < UINT64_C(1000000); ++iteration) {
        volatile uint64_t *words = jet_rt_alloc(64, 8);
        if (!words || ((uintptr_t)words & 7)) return 1;
        for (uint64_t word = 0; word < 8; ++word) words[word] = iteration + word;
        for (uint64_t word = 0; word < 8; ++word) checksum += words[word];
        jet_rt_free((void *)words, 64, 8);
    }
    printf("%" PRIu64 "\n", checksum);
    return 0;
}
