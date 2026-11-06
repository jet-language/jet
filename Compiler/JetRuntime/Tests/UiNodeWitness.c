// Link against the real native UiNode and transfer providers (UiABI.jet's
// Node160). The node holds no closures or refcounts: clone is a deep copy.
#include <assert.h>
#include <math.h>
#include <pthread.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

extern uintptr_t jet_ui_raw_node_transfer(uintptr_t, double, double, int64_t,
    int64_t, const uintptr_t *, int64_t, uint8_t, uintptr_t, int64_t, uint8_t,
    int64_t, int64_t, int64_t, int64_t, int64_t);
extern uintptr_t jet_ui_text_input(uintptr_t, int64_t);
extern uintptr_t jet_rt_handle_clone_jet_ui_node(uintptr_t);
extern void jet_rt_handle_drop_jet_ui_node(uintptr_t);
extern bool jet_rt_handle_eq_jet_ui_node(uintptr_t, uintptr_t);
extern void jet_rt_handle_field_jet_ui_node_label(uintptr_t, uintptr_t);
extern uintptr_t jet_rt_handle_field_jet_ui_node_width(uintptr_t);
extern uintptr_t jet_rt_handle_field_jet_ui_node_height(uintptr_t);
extern int64_t jet_rt_handle_field_jet_ui_node_ime(uintptr_t, uintptr_t);
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);

// UiABI.jet: the IME, click and drop option slots, all absent.
#define OPTION_MASK 26
#define SLOT_IME 2
struct Text { const unsigned char *data; uint64_t len, cap; };
_Static_assert(sizeof(struct Text) == 24, "E4 header ABI");

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
    assert(frame.out.data == ((const struct Text *)word(node, 0))->data);
}
static void no_ime(uintptr_t node) {
    uint64_t some = 0xfeed;
    assert(jet_rt_handle_field_jet_ui_node_ime(node, (uintptr_t)&some) == 0 && some == 0xfeed);
}

static uintptr_t absent(const struct Text *label) {
    // Poisoned ignored words prove that absent means unread: the color word
    // without color_present and the style words without style_present. Empty
    // List data is deliberately unmapped too.
    return jet_ui_raw_node_transfer((uintptr_t)label, -0.0, 1.5, -1, 0,
        (const uintptr_t *)1, 0, 0, 1, 63, 0, 9, 9, 9, 9, 9);
}
static void absent_golden(uintptr_t node, const struct Text *label) {
    text_equal(word(node, 0), label);
    assert(word(node, 8) == UINT64_C(0x8000000000000000));
    assert(word(node, 16) == UINT64_C(0x3ff8000000000000));
    assert(word(node, 24) == UINT64_MAX && word(node, 32) == 0);
    const uint64_t *children = (const void *)word(node, 40);
    assert(children[0] == 0 && children[1] == 0 && children[2] == 8);
    for (size_t offset = 48; offset <= 96; offset += 8)
        assert(word(node, offset) == 0);
    assert(word(node, 104) == OPTION_MASK);
    for (size_t offset = 112; offset < 160; offset += 8)
        assert(word(node, offset) == 0);
    label_getter(node, label);
    assert(jet_rt_handle_field_jet_ui_node_width(node) == node + 8);
    assert(jet_rt_handle_field_jet_ui_node_height(node) == node + 16);
    assert(signbit(real(node, 8)) && real(node, 16) == 1.5);
    no_ime(node);
}
static void populated_golden(uintptr_t node, uintptr_t child,
                             const struct Text *label, const struct Text *color) {
    text_equal(word(node, 0), label);
    assert(word(node, 8) == UINT64_C(0x4050000000000000)); // 64
    assert(word(node, 16) == UINT64_C(0x4038000000000000)); // 24
    assert(word(node, 24) == 0 && word(node, 32) == 3);
    const uint64_t *children = (const void *)word(node, 40);
    assert(children[0] == 1 && children[1] == 1 && children[2] != 0);
    uintptr_t kept_child = *(const uintptr_t *)children[2];
    assert(kept_child != child && jet_rt_handle_eq_jet_ui_node(kept_child, child));
    text_equal(word(node, 48), color);
    // style present; ANSI256 is masked to a byte, RGB to 24 bits; flags to 3.
    const uint64_t expected[] = {1, 3, 0x12abff, 2, 200, 7, OPTION_MASK};
    for (size_t i = 0; i < sizeof(expected) / sizeof(expected[0]); ++i)
        assert(word(node, 56 + i * 8) == expected[i]);
    for (size_t offset = 112; offset < 160; offset += 8)
        assert(word(node, offset) == 0);
    label_getter(node, label);
    no_ime(node);
}

static void *clone_on_thread(void *argument) {
    jet_rt_thread_attach();
    uintptr_t node = *(uintptr_t *)argument;
    for (unsigned i = 0; i < 1000; ++i) {
        uintptr_t copy = jet_rt_handle_clone_jet_ui_node(node);
        assert(copy != node && jet_rt_handle_eq_jet_ui_node(copy, node));
        assert(word(copy, 40) != word(node, 40));
        jet_rt_handle_drop_jet_ui_node(copy);
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
    memcpy((unsigned char *)copy + 8, &positive_zero, sizeof(positive_zero));
    assert(jet_rt_handle_eq_jet_ui_node(copy, child));
    memcpy((unsigned char *)copy + 8, &quiet_nan, sizeof(quiet_nan));
    assert(!jet_rt_handle_eq_jet_ui_node(copy, copy));
    jet_rt_handle_drop_jet_ui_node(copy);
    puts("absent: width=-0;height=1.5;role=-1;kind=0;children=0;color=none;style=none;mask=26;ime=none;click=none;drop=none;a11y=none;shortcut=none;key=none");

    uintptr_t node = jet_ui_raw_node_transfer((uintptr_t)&label, 64, 24, 0, 3,
        &child, 1, 1, (uintptr_t)&color, 63, 1, 3, 0x7f12abff, 2, 0x1c8, 15);
    populated_golden(node, child, &label, &color);
    copy = jet_rt_handle_clone_jet_ui_node(node);
    assert(copy != node && jet_rt_handle_eq_jet_ui_node(copy, node));
    populated_golden(copy, child, &label, &color);
    jet_rt_handle_drop_jet_ui_node(copy);
    assert(!jet_rt_handle_eq_jet_ui_node(node, child));
    puts("populated: width=64;height=24;role=0;kind=3;children=1;color=some-empty;style=3:12abff/2:200/7;mask=26");

    for (int64_t ime = 0; ime < 2; ++ime) {
        uintptr_t input = jet_ui_text_input((uintptr_t)&label, ime);
        assert(word(input, 104) == (OPTION_MASK & ~SLOT_IME) && word(input, 112) == (uint64_t)ime);
        uint64_t some = 0xfeed;
        assert(jet_rt_handle_field_jet_ui_node_ime(input, (uintptr_t)&some) == 1 && some == (uint64_t)ime);
        uintptr_t twin = jet_rt_handle_clone_jet_ui_node(input);
        assert(word(twin, 112) == (uint64_t)ime && jet_rt_handle_eq_jet_ui_node(twin, input));
        // The IME slot is part of equality: Native and Disabled differ.
        uint64_t other = (uint64_t)(1 - ime);
        memcpy((unsigned char *)twin + 112, &other, sizeof(other));
        assert(!jet_rt_handle_eq_jet_ui_node(twin, input));
        jet_rt_handle_drop_jet_ui_node(twin);
        jet_rt_handle_drop_jet_ui_node(input);
    }
    puts("ime: text input present Native/Disabled, cloned, compared");

    pthread_t threads[2];
    for (unsigned i = 0; i < 2; ++i)
        assert(pthread_create(&threads[i], NULL, clone_on_thread, &node) == 0);
    for (unsigned i = 0; i < 2; ++i)
        assert(pthread_join(threads[i], NULL) == 0);
    jet_rt_handle_drop_jet_ui_node(node);
    jet_rt_handle_drop_jet_ui_node(child);
    jet_rt_handle_drop_jet_ui_node(0);
    puts("UI Node160: absent/populated bytes, poisoned ignored words, borrowed getters, deep clone, IME presence, 2x1000 cross-thread clone/drop ok");
    return 0;
}
