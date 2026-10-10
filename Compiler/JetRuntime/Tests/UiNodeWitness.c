// Link against the real native UiNode, metadata, and atomic SendFn providers.
// No replacement retain/release functions live in this witness.
#include <assert.h>
#include <math.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

extern uintptr_t jet_rt_alloc(int64_t, int64_t);
extern void jet_rt_free(uintptr_t, int64_t, int64_t);
extern uintptr_t jet_ui_raw_node_transfer(uintptr_t, double, double, int64_t,
    int64_t, const uintptr_t *, int64_t, uint8_t, uintptr_t, int64_t, uint8_t,
    int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, uintptr_t, uintptr_t);
extern uintptr_t jet_rt_handle_clone_jet_ui_node(uintptr_t);
extern void jet_rt_handle_drop_jet_ui_node(uintptr_t);
extern bool jet_rt_handle_eq_jet_ui_node(uintptr_t, uintptr_t);
extern void jet_rt_handle_field_jet_ui_node_label(uintptr_t, uintptr_t);
extern uintptr_t jet_rt_handle_field_jet_ui_node_width(uintptr_t);
extern uintptr_t jet_rt_handle_field_jet_ui_node_height(uintptr_t);
extern void jet_rt_native_closure_retain(uintptr_t);
extern bool jet_rt_native_closure_unref(uintptr_t);
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);

struct Text { const unsigned char *data; uint64_t len, cap; };
struct Closure {
    _Atomic uint64_t refs;
    void (*invoke)(struct Closure *, uintptr_t);
    void (*release)(struct Closure *);
    _Atomic unsigned *finalized;
    _Atomic unsigned *invocations;
};
_Static_assert(offsetof(struct Closure, invoke) == 8, "Closure24 invoke ABI");
_Static_assert(offsetof(struct Closure, release) == 16, "Closure24 release ABI");
_Static_assert(offsetof(struct Closure, finalized) == 24, "Closure24 capture ABI");
_Static_assert(sizeof(struct Closure) == 40, "witness capture allocation size");
_Static_assert(sizeof(struct Text) == 24, "E4 header ABI");
static _Atomic unsigned closure_allocations, closure_frees;

static void invoke(struct Closure *closure, uintptr_t payload) {
    assert(payload == 7);
    atomic_fetch_add_explicit(closure->invocations, 1, memory_order_relaxed);
}
static void release(struct Closure *closure) {
    atomic_fetch_add_explicit(closure->finalized, 1, memory_order_relaxed);
    jet_rt_free((uintptr_t)closure, sizeof(*closure), 8);
    atomic_fetch_add_explicit(&closure_frees, 1, memory_order_relaxed);
}
static struct Closure *closure(_Atomic unsigned *finalized,
                               _Atomic unsigned *invocations) {
    struct Closure *value = (void *)jet_rt_alloc(sizeof(*value), 8);
    atomic_fetch_add_explicit(&closure_allocations, 1, memory_order_relaxed);
    atomic_init(&value->refs, 1);
    value->invoke = invoke;
    value->release = release;
    value->finalized = finalized;
    value->invocations = invocations;
    return value;
}
static uint64_t word(uintptr_t node, size_t offset) {
    uint64_t value;
    memcpy(&value, (const unsigned char *)node + offset, sizeof(value));
    return value;
}
static double real(uintptr_t node, size_t offset) {
    double value;
    memcpy(&value, (const unsigned char *)node + offset, sizeof(value));
    return value;
}
static void text_equal(uintptr_t handle, const struct Text *expected) {
    const struct Text *actual = (const void *)handle;
    assert(actual->len == expected->len);
    assert(memcmp(actual->data, expected->data, expected->len) == 0);
}
static void label_getter(uintptr_t node, const struct Text *expected) {
    struct { uint64_t before; struct Text out; uint64_t after; } frame;
    memset(&frame, 0, sizeof(frame));
    frame.before = 0xfeed;
    frame.after = 0xbeef;
    jet_rt_handle_field_jet_ui_node_label(node, (uintptr_t)&frame.out);
    assert(frame.before == 0xfeed && frame.after == 0xbeef);
    text_equal((uintptr_t)&frame.out, expected);
    assert(frame.out.data == ((const struct Text *)word(node, 8))->data);
}

static uintptr_t absent(const struct Text *label) {
    // Poison appended words prove that absent means unread, not an Int decode
    // or pointer dereference. Empty List data is deliberately unmapped too.
    return jet_ui_raw_node_transfer((uintptr_t)label, -0.0, 1.5, -1, 0,
        (const uintptr_t *)1, 0, 0, 1, 63, 0, 0, 0, 0, 0, 0,
        INT64_MIN, UINTPTR_MAX, UINTPTR_MAX);
}
static void absent_golden(uintptr_t node, const struct Text *label) {
    assert(word(node, 0) == 1);
    text_equal(word(node, 8), label);
    assert(word(node, 16) == UINT64_C(0x8000000000000000));
    assert(word(node, 24) == UINT64_C(0x3ff8000000000000));
    assert(word(node, 32) == UINT64_MAX && word(node, 40) == 0);
    const uint64_t *children = (const void *)word(node, 48);
    assert(children[0] == 0 && children[1] == 0 && children[2] == 0);
    for (size_t offset = 56; offset <= 104; offset += 8)
        assert(word(node, offset) == 0);
    assert(word(node, 112) == 63);
    for (size_t offset = 120; offset < 160; offset += 8)
        assert(word(node, offset) == 0);
    label_getter(node, label);
    assert(jet_rt_handle_field_jet_ui_node_width(node) == node + 16);
    assert(jet_rt_handle_field_jet_ui_node_height(node) == node + 24);
    assert(signbit(real(node, 16)) && real(node, 24) == 1.5);
}
static void populated_golden(uintptr_t node, uintptr_t child,
                             const struct Text *label, const struct Text *color,
                             struct Closure *click, struct Closure *drop) {
    assert(word(node, 0) == 1);
    text_equal(word(node, 8), label);
    assert(word(node, 16) == UINT64_C(0x4050000000000000)); // 64
    assert(word(node, 24) == UINT64_C(0x4038000000000000)); // 24
    assert(word(node, 32) == 0 && word(node, 40) == 3);
    const uint64_t *children = (const void *)word(node, 48);
    assert(children[0] == 1 && children[1] == 1 && children[2] != 0);
    uintptr_t kept_child = *(const uintptr_t *)children[2];
    assert(kept_child != child && jet_rt_handle_eq_jet_ui_node(kept_child, child));
    text_equal(word(node, 56), color);
    const uint64_t expected[] = {1, 3, 0x12abff, 2, 200, 7, 37, 1};
    for (size_t i = 0; i < sizeof(expected) / sizeof(expected[0]); ++i)
        assert(word(node, 64 + i * 8) == expected[i]);
    assert(word(node, 128) == (uintptr_t)click);
    assert(word(node, 136) == (uintptr_t)drop);
    assert(word(node, 144) == 0 && word(node, 152) == 0);
    label_getter(node, label);
}

struct ThreadCase { uintptr_t node; struct Closure *click, *drop; };
static void *clone_on_thread(void *argument) {
    jet_rt_thread_attach();
    struct ThreadCase *test = argument;
    uintptr_t copy = jet_rt_handle_clone_jet_ui_node(test->node);
    assert(copy != test->node && jet_rt_handle_eq_jet_ui_node(copy, test->node));
    assert(atomic_load_explicit(&test->click->refs, memory_order_relaxed) == 2);
    assert(atomic_load_explicit(&test->drop->refs, memory_order_relaxed) == 2);
    ((struct Closure *)word(copy, 128))->invoke(test->click, 7);
    ((struct Closure *)word(copy, 136))->invoke(test->drop, 7);
    jet_rt_handle_drop_jet_ui_node(copy);
    jet_rt_thread_detach();
    return NULL;
}
static void *contend_references(void *argument) {
    jet_rt_thread_attach();
    struct Closure *value = argument;
    for (unsigned i = 0; i < 100000; ++i) {
        jet_rt_native_closure_retain((uintptr_t)value);
        // The node's original reference stays live throughout both threads.
        assert(!jet_rt_native_closure_unref((uintptr_t)value));
    }
    jet_rt_thread_detach();
    return NULL;
}
int main(void) {
    // Native init_array must initialize the allocator before this entry.
    const unsigned char bytes[] = "A\n\xce\xbb";
    const struct Text label = {bytes, sizeof(bytes) - 1, 0};
    const struct Text color = {(const unsigned char *)"", 0, 0};
    uintptr_t child = absent(&label);
    absent_golden(child, &label);
    uintptr_t copy = jet_rt_handle_clone_jet_ui_node(child);
    assert(copy != child && jet_rt_handle_eq_jet_ui_node(copy, child));
    absent_golden(copy, &label);
    // Float PartialEq is semantic, not bitwise: signed zero equals zero,
    // while even a node compared to itself is unequal when it contains NaN.
    uint64_t positive_zero = 0, quiet_nan = UINT64_C(0x7ff8000000000001);
    memcpy((unsigned char *)copy + 16, &positive_zero, sizeof(positive_zero));
    assert(jet_rt_handle_eq_jet_ui_node(copy, child));
    memcpy((unsigned char *)copy + 16, &quiet_nan, sizeof(quiet_nan));
    assert(!jet_rt_handle_eq_jet_ui_node(copy, copy));
    jet_rt_handle_drop_jet_ui_node(copy);
    puts("absent: refs=1;width=-0;height=1.5;role=-1;kind=0;children=0;color=none;style=none;mask=63;ime=none;click=none;drop=none;a11y=none;shortcut=none");

    _Atomic unsigned click_finalized = 0, drop_finalized = 0;
    _Atomic unsigned click_invocations = 0, drop_invocations = 0;
    struct Closure *click = closure(&click_finalized, &click_invocations);
    struct Closure *drop = closure(&drop_finalized, &drop_invocations);
    uintptr_t node = jet_ui_raw_node_transfer((uintptr_t)&label, 64, 24, 0, 3,
        &child, 1, 1, (uintptr_t)&color, 37, 1, 3, 0x12abff, 2, 200, 7, 1,
        (uintptr_t)click, (uintptr_t)drop);
    populated_golden(node, child, &label, &color, click, drop);
    assert(atomic_load_explicit(&click->refs, memory_order_relaxed) == 1);
    assert(atomic_load_explicit(&drop->refs, memory_order_relaxed) == 1);
    puts("populated: refs=1;width=64;height=24;role=0;kind=3;children=1;color=some-empty;style=3:12abff/2:200/7;mask=37;ime=1;click=present;drop=present;a11y=none;shortcut=none");

    for (unsigned present = 0; present < 8; ++present) {
        bool ime_present = (present & 1) != 0;
        bool click_present = (present & 2) != 0;
        bool drop_present = (present & 4) != 0;
        int64_t mask = 63 ^ (ime_present ? 2 : 0) ^
                          (click_present ? 8 : 0) ^ (drop_present ? 16 : 0);
        if (click_present) jet_rt_native_closure_retain((uintptr_t)click);
        if (drop_present) jet_rt_native_closure_retain((uintptr_t)drop);
        uintptr_t combination = jet_ui_raw_node_transfer((uintptr_t)&label,
            -0.0, 1.5, -1, 0, NULL, 0, 0, 1, mask, 0, 0, 0, 0, 0, 0,
            ime_present ? 0 : INT64_MIN,
            click_present ? (uintptr_t)click : UINTPTR_MAX,
            drop_present ? (uintptr_t)drop : UINTPTR_MAX);
        assert(word(combination, 112) == (uint64_t)mask);
        assert(word(combination, 120) == 0); // present Native vs absent via mask
        assert(word(combination, 128) == (click_present ? (uintptr_t)click : 0));
        assert(word(combination, 136) == (drop_present ? (uintptr_t)drop : 0));
        jet_rt_handle_drop_jet_ui_node(combination);
    }

    struct ThreadCase test = {node, click, drop};
    pthread_t thread;
    assert(pthread_create(&thread, NULL, clone_on_thread, &test) == 0);
    assert(pthread_join(thread, NULL) == 0);
    pthread_t contenders[2];
    for (unsigned i = 0; i < 2; ++i)
        assert(pthread_create(&contenders[i], NULL, contend_references, click) == 0);
    for (unsigned i = 0; i < 2; ++i)
        assert(pthread_join(contenders[i], NULL) == 0);
    assert(atomic_load_explicit(&click->refs, memory_order_relaxed) == 1);
    assert(atomic_load_explicit(&drop->refs, memory_order_relaxed) == 1);
    assert(atomic_load_explicit(&click_finalized, memory_order_relaxed) == 0);
    assert(atomic_load_explicit(&drop_finalized, memory_order_relaxed) == 0);
    jet_rt_handle_drop_jet_ui_node(node);
    jet_rt_handle_drop_jet_ui_node(child);
    jet_rt_handle_drop_jet_ui_node(0);
    assert(atomic_load_explicit(&click_finalized, memory_order_relaxed) == 1);
    assert(atomic_load_explicit(&drop_finalized, memory_order_relaxed) == 1);
    assert(atomic_load_explicit(&click_invocations, memory_order_relaxed) == 1);
    assert(atomic_load_explicit(&drop_invocations, memory_order_relaxed) == 1);
    assert(atomic_load_explicit(&closure_allocations, memory_order_relaxed) == 2);
    assert(atomic_load_explicit(&closure_frees, memory_order_relaxed) == 2);
    puts("UI Node160: absent/populated bytes, all presence masks, callback retain/release, cross-thread clone/drop, 2x100000 atomic pairs, counted closure frees ok");
    return 0;
}
