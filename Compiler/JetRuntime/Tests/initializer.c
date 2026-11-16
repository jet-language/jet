// Same ABI witness in an executable and an entryless dlopen library.
// The root is inspected BEFORE any exported function can initialize the heap.
#include <assert.h>
#include <stdint.h>
#include <string.h>
#ifdef JET_ENTRYLESS_LIBRARY
#include <dlfcn.h>
#else
extern int64_t runtime_fixture_root(void);
extern void jet_rt_alloc_init(void);
extern void runtime_fixture_tests(void);
#endif
int main(int argc, char **argv) {
#ifdef JET_ENTRYLESS_LIBRARY
    assert(argc == 2);
    void *library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    assert(library != 0);
    int64_t (*root)(void) = (int64_t (*)(void))dlsym(library, "runtime_fixture_root");
    void (*init)(void) = (void (*)(void))dlsym(library, "jet_rt_alloc_init");
    assert(root && init);
#else
    int64_t (*root)(void) = runtime_fixture_root;
    void (*init)(void) = jet_rt_alloc_init;
#endif
    const int64_t before = root();
    assert(before != 0); // .init_array ran at executable startup / dlopen.
    init();
    assert(root() == before);
    init();
    assert(root() == before);
#ifndef JET_ENTRYLESS_LIBRARY
    if (argc == 2 && strcmp(argv[1], "allocator") == 0) runtime_fixture_tests();
#else
    assert(dlclose(library) == 0);
#endif
    return 0;
}
