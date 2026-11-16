/* The witness counting route: jet_rt_alloc_check_observe reports every
 * checked-heap allocation and free, including the frees the runtime object
 * makes through its own section-relative calls, which a link-time
 * --wrap=jet_rt_free never sees. With no argument every block the run takes
 * is given back and the ledger balances (exit 0). `leak` keeps one string
 * byte buffer alive and must fail the balance assert (SIGABRT). */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { unsigned char *data; int64_t length, capacity; } Text;
extern void jet_rt_alloc_init(void);
extern int64_t jet_rt_alloc(int64_t, int64_t);
extern void jet_rt_free(int64_t, int64_t, int64_t);
extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern Text *jet_rt_string_from_static(const void *, int64_t);
extern Text *jet_rt_string_builder_new(void);
extern void jet_rt_string_builder_push_static(Text *, const void *, int64_t);
extern Text *jet_rt_string_builder_finish(Text *);
extern void jet_rt_string_drop(Text *);
static struct { int64_t address, bytes, align; bool live; } ledger[256];
static size_t allocations, frees;
static bool tracking;
static void observe_alloc(int64_t address, int64_t bytes, int64_t align) {
    if (!tracking) return;
    assert(address && bytes > 0 && allocations < 256 && (uint64_t)address % (uint64_t)align == 0);
    for (size_t i = 0; i < allocations; ++i) assert(!ledger[i].live || ledger[i].address != address);
    ledger[allocations].address = address; ledger[allocations].bytes = bytes;
    ledger[allocations].align = align; ledger[allocations++].live = true;
}
static void observe_free(int64_t address, int64_t bytes, int64_t align) {
    if (!tracking) return;
    for (size_t i = allocations; i > 0; --i) if (ledger[i - 1].address == address && ledger[i - 1].live) {
        assert(ledger[i - 1].bytes == bytes && ledger[i - 1].align == align);
        ledger[i - 1].live = false; ++frees; return;
    }
    assert(!"untracked or double free");
}
static void balanced(void) {
    size_t live = 0;
    for (size_t i = 0; i < allocations; ++i) live += ledger[i].live;
    if (live) fprintf(stderr, "alloc observer: %zu of %zu blocks still live\n", live, allocations);
    assert(live == 0 && allocations == frees);
}
int main(int argc, char **argv) {
    bool leak = argc == 2 && strcmp(argv[1], "leak") == 0;
    if (argc > 2 || (argc == 2 && !leak)) return 2;
    assert(jet_rt_alloc_check_observe((int64_t)(uintptr_t)observe_alloc, (int64_t)(uintptr_t)observe_free) == 1);
    jet_rt_alloc_init();
    tracking = true;
    // A C-side pair: seen by --wrap and by the observer alike.
    int64_t block = jet_rt_alloc(48, 16);
    jet_rt_free(block, 48, 16);
    assert(allocations == 1 && frees == 1);
    // Runtime-side frees: jet_rt_string_drop releases the header and buffer
    // through calls inside the runtime object.
    Text *owned = jet_rt_string_from_static("observer", 8);
    Text *built = jet_rt_string_builder_new();
    jet_rt_string_builder_push_static(built, "runtime frees ", 14);
    jet_rt_string_builder_push_static(built, "are visible", 11);
    built = jet_rt_string_builder_finish(built);
    assert(built->length == 25 && memcmp(built->data, "runtime frees are visible", 25) == 0);
    size_t before = frees;
    jet_rt_string_drop(owned);
    assert(frees > before);
    // `leak` swaps a copy in for the built string's byte buffer: the drop
    // releases the header and the copy, and exactly the original stays live.
    if (leak) {
        unsigned char *copy = (unsigned char *)(uintptr_t)jet_rt_alloc(built->capacity, 1);
        memcpy(copy, built->data, (size_t)built->length);
        built->data = copy;
    }
    jet_rt_string_drop(built);
    balanced();
    tracking = false;
    printf("alloc observer: %zu runtime allocations and frees balanced\n", allocations);
    return 0;
}
