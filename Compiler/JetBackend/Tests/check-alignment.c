// Linked-object witness: the linker must preserve the datum's align64 contract.
#include <stdint.h>
#include <stdlib.h>

void jet_rt_alignment(const void *address) {
    if ((uintptr_t)address % 64 != 0) abort();
}

int32_t jet_rt_main(void (*entry)(void)) {
    entry();
    return 0;
}
