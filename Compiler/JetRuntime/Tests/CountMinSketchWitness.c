// Real CMS producer/methods and allocator only; link -pthread -Wl,--wrap=jet_rt_free.
#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { const char *data; int64_t len, capacity; } Text;
typedef struct { int64_t refs, mutex; uint32_t rows[4][256]; } CMS;
_Static_assert(sizeof(CMS) == 4112, "canonical shared CMS4112");
extern void jet_rt_alloc_init(void);
extern CMS *jet_count_min_sketch_new(void);
extern CMS *jet_rt_handle_clone_jet_count_min_sketch(CMS *);
extern void jet_rt_handle_drop_jet_count_min_sketch(CMS *);
extern Text *jet_rt_handle_show_jet_count_min_sketch(const CMS *);
extern void jet_rt_native_cms_add(CMS *, const Text *);
extern int64_t jet_rt_native_cms_count(CMS *, const Text *);
extern int64_t jet_int_owned_to_i64(int64_t);
extern void jet_rt_int_drop(int64_t);
extern void jet_rt_string_drop(Text *);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
static CMS *watched;
static unsigned frees;
void __wrap_jet_rt_free(void *pointer, int64_t bytes, int64_t align) {
    if (pointer == watched) { assert(bytes == 4112 && align == 8); assert(++frees == 1); }
    __real_jet_rt_free(pointer, bytes, align);
}
static void count(CMS *value, const Text *key, int64_t expected) {
    int64_t result = jet_rt_native_cms_count(value, key);
    assert(jet_int_owned_to_i64(result) == expected); jet_rt_int_drop(result);
}
static void *adds(void *pointer) {
    Text shared = {"shared", 6, 0};
    for (unsigned i = 0; i < 1000; ++i) jet_rt_native_cms_add(pointer, &shared);
    return NULL;
}
int main(void) {
    // Frozen low-byte indices from Sketch.rs's two unsigned wrapping FNV hashes.
    // No alternative CMS implementation is linked into this client.
    static const unsigned indices[][4] = {{57,77,97,117},{115,187,3,75},{173,179,185,191},{205,226,247,12}};
    static const char snow[] = "snow\0\xe9\x9b\xaa";
    static const Text keys[] = {{"",0,0},{"alpha",5,5},{"beta",4,4},{snow,sizeof(snow)-1,sizeof(snow)-1}};
    jet_rt_alloc_init();
    CMS *owner = jet_count_min_sketch_new(); watched = owner;
    assert(owner->refs == 1 && owner->mutex == 0);
    for (unsigned r = 0; r < 4; ++r) for (unsigned c = 0; c < 256; ++c) assert(owner->rows[r][c] == 0);
    CMS *a = jet_rt_handle_clone_jet_count_min_sketch(owner), *b = jet_rt_handle_clone_jet_count_min_sketch(a);
    assert(owner == a && a == b && owner->refs == 3);
    for (unsigned i = 0; i < 4; ++i) {
        count(owner, &keys[i], 0);
        for (unsigned n = 0; n <= i; ++n) jet_rt_native_cms_add(a, &keys[i]);
        count(b, &keys[i], (int64_t)i + 1);
    }
    for (unsigned r = 0; r < 4; ++r) for (unsigned c = 0; c < 256; ++c) {
        uint32_t expected = 0;
        for (unsigned i = 0; i < 4; ++i) if (indices[i][r] == c) expected += i + 1;
        assert(owner->rows[r][c] == expected);
    }
    // Between operations and with no concurrent users, set real counters to exercise saturation/min.
    for (unsigned r = 0; r < 4; ++r) owner->rows[r][indices[1][r]] = UINT32_MAX;
    jet_rt_native_cms_add(a, &keys[1]); count(b, &keys[1], UINT32_MAX);
    for (unsigned r = 0; r < 4; ++r) assert(owner->rows[r][indices[1][r]] == UINT32_MAX);
    owner->rows[2][indices[1][2]] = 17; count(owner, &keys[1], 17);
    jet_rt_native_cms_add(b, &keys[1]); count(a, &keys[1], 18);
    Text *shown = jet_rt_handle_show_jet_count_min_sketch(owner);
    assert(shown->len == 14 && memcmp(shown->data, "CountMinSketch", 14) == 0); jet_rt_string_drop(shown);
    jet_rt_handle_drop_jet_count_min_sketch(owner);
    assert(frees == 0 && a->refs == 2);
    pthread_t threads[3];
    for (unsigned i = 0; i < 3; ++i) assert(pthread_create(&threads[i], NULL, adds, a) == 0);
    for (unsigned i = 0; i < 3; ++i) assert(pthread_join(threads[i], NULL) == 0);
    Text shared = {"shared",6,0}; count(b, &shared, 3000);
    jet_rt_handle_drop_jet_count_min_sketch(a); assert(frees == 0 && b->refs == 1);
    jet_rt_handle_drop_jet_count_min_sketch(NULL); assert(frees == 0);
    jet_rt_handle_drop_jet_count_min_sketch(b); assert(frees == 1); watched = NULL;
    puts("CMS shared mutation/rows/saturation/drop OK");
}
