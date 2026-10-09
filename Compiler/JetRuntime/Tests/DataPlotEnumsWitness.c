/* Link the REAL Jet provider object with -Wl,--wrap=jet_rt_free.
 * Private carrier layout comes from DataABI.jet, never Rust/MIR storage.
 * Rust derives Clone/PartialEq: DataPlot.rs:269-344,748-754,865-917,
 * 1024-1031,1144-1168,1255-1262,2357-2361. */
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>

extern void jet_rt_alloc_init(void);
extern int64_t jet_rt_alloc(int64_t bytes, int64_t alignment);
extern void __real_jet_rt_free(int64_t address, int64_t bytes, int64_t alignment);
#define DECLARE(T) \
    extern int64_t jet_rt_handle_clone_##T(int64_t value); \
    extern void jet_rt_handle_drop_##T(int64_t value); \
    extern bool jet_rt_handle_eq_##T(int64_t left, int64_t right)
DECLARE(JetDataPlotMark);
DECLARE(JetDataPlotChannel);
DECLARE(JetDataPlotAggregate);
DECLARE(JetDataPlotFilterOp);
DECLARE(JetDataPlotScaleKind);
DECLARE(JetDataPlotLegendPosition);
DECLARE(JetDataPlotFacetKind);
DECLARE(JetDataPlotInteraction);
DECLARE(JetDataPlotBackend);
DECLARE(JetDataPlotSupport);
DECLARE(JetDataPlotErrorKind);
DECLARE(JetDataPlotRenderFormat);

struct operations {
    int64_t (*clone)(int64_t);
    void (*drop)(int64_t);
    bool (*equal)(int64_t, int64_t);
    int64_t variants;
};
#define OPS(T, N) {jet_rt_handle_clone_##T, jet_rt_handle_drop_##T, jet_rt_handle_eq_##T, N}
static const struct operations families[] = {
    OPS(JetDataPlotMark, 3), OPS(JetDataPlotChannel, 6),
    OPS(JetDataPlotAggregate, 6), OPS(JetDataPlotFilterOp, 6),
    OPS(JetDataPlotScaleKind, 4), OPS(JetDataPlotLegendPosition, 4),
    OPS(JetDataPlotFacetKind, 2), OPS(JetDataPlotInteraction, 5),
    OPS(JetDataPlotBackend, 4), OPS(JetDataPlotSupport, 3),
    OPS(JetDataPlotErrorKind, 5), OPS(JetDataPlotRenderFormat, 2)
};
static int64_t expected[36];
static bool freed[36];
static size_t free_calls;
void __wrap_jet_rt_free(int64_t address, int64_t bytes, int64_t alignment) {
    assert(bytes == 8 && alignment == 8);
    size_t i = 0;
    while (i < 36 && expected[i] != address) ++i;
    assert(i < 36 && !freed[i]);
    freed[i] = true;
    ++free_calls;
    __real_jet_rt_free(address, bytes, alignment);
}
static void set_tag(int64_t value, int64_t tag) {
    *(int64_t *)(uintptr_t)value = tag;
}
int main(void) {
    jet_rt_alloc_init();
    const size_t count = sizeof families / sizeof families[0];
    assert(count == 12);
    for (size_t i = 0; i < count; ++i) {
        const struct operations *op = &families[i];
        int64_t original = jet_rt_alloc(8, 8);
        set_tag(original, 0);
        int64_t copy = op->clone(original);
        assert(copy != original && op->equal(original, copy));
        int64_t other = jet_rt_alloc(8, 8);
        set_tag(other, op->variants - 1);
        assert(!op->equal(original, other));
        for (int64_t tag = 0; tag < op->variants; ++tag) {
            set_tag(copy, tag);
            assert(op->equal(original, copy) == (tag == 0));
            assert(op->equal(copy, copy));
        }
        set_tag(copy, 1);
        assert(*(int64_t *)(uintptr_t)original == 0);
        expected[3 * i] = original;
        expected[3 * i + 1] = copy;
        expected[3 * i + 2] = other;
    }
    for (size_t i = 0; i < count; ++i) {
        const struct operations *op = &families[i];
        op->drop(expected[3 * i]);
        assert(op->equal(expected[3 * i + 1], expected[3 * i + 1]));
        op->drop(expected[3 * i + 1]);
        op->drop(expected[3 * i + 2]);
        op->drop(0);
    }
    assert(free_calls == 36);
    for (size_t i = 0; i < 36; ++i) assert(freed[i]);
    puts("DataPlot enum witness: 12 complete families, clone independence, 36 exact-once frees");
    return 0;
}
