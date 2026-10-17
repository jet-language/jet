// Linker-wrapped real Jet allocations for the owning collector fixtures.
// No allocations are performed by the observer itself.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

extern int __real_main(int argc, char **argv);
extern int64_t __real_jet_rt_alloc(int64_t size, int64_t align);
extern void __real_jet_rt_free(int64_t pointer, int64_t size, int64_t align);

static struct { int64_t pointer, size, align; } live[4096];
static unsigned allocations, frees;
static int observing;

static void require(int ok, const char *message) {
    if (!ok) { fprintf(stderr, "collector-alloc: %s\n", message); abort(); }
}

int64_t __wrap_jet_rt_alloc(int64_t size, int64_t align) {
    int64_t pointer = __real_jet_rt_alloc(size, align);
    if (observing && pointer != 0) {
        unsigned i;
        for (i = 0; i < 4096 && live[i].pointer != 0; ++i) {}
        require(i < 4096, "allocation observer capacity exceeded");
        live[i].pointer = pointer;
        live[i].size = size;
        live[i].align = align;
        ++allocations;
    }
    return pointer;
}

void __wrap_jet_rt_free(int64_t pointer, int64_t size, int64_t align) {
    if (observing && pointer != 0) {
        unsigned i;
        for (i = 0; i < 4096 && live[i].pointer != pointer; ++i) {}
        require(i < 4096, "unmatched or duplicate free");
        require(live[i].size == size && live[i].align == align, "free layout mismatch");
        live[i].pointer = 0;
        ++frees;
    }
    __real_jet_rt_free(pointer, size, align);
}

int __wrap_main(int argc, char **argv) {
    observing = 1;
    int result = __real_main(argc, argv);
    observing = 0;
    require(result == 0, "fixture entry failed");
    require(allocations > 0, "no real Jet allocation observed");
    require(allocations == frees, "allocations were not balanced");
    for (unsigned i = 0; i < 4096; ++i) require(live[i].pointer == 0, "live allocation after fixture cleanup");
    fprintf(stderr, "collector-alloc: balanced live=0\n");
    return result;
}
