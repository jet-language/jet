// Real Jet providers only. A free observer counts frees; it never replaces
// ownership.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

extern int64_t jet_rt_alloc_check_observe(int64_t, int64_t);
extern uintptr_t jet_rt_handle_clone_jet_ui_accessibility(uintptr_t);
extern void jet_rt_handle_drop_jet_ui_accessibility(uintptr_t);
extern bool jet_rt_handle_eq_jet_ui_accessibility(uintptr_t, uintptr_t);
extern uintptr_t jet_rt_handle_clone_jet_ui_shortcut(uintptr_t);
extern void jet_rt_handle_drop_jet_ui_shortcut(uintptr_t);
extern bool jet_rt_handle_eq_jet_ui_shortcut(uintptr_t, uintptr_t);
extern uintptr_t jet_ui_raw_node_transfer(uintptr_t, double, double, int64_t,
    int64_t, const uintptr_t *, int64_t, uint8_t, uintptr_t, int64_t, uint8_t,
    int64_t, int64_t, int64_t, int64_t, int64_t);
extern uintptr_t jet_ui_raw_accessibility_transfer(uintptr_t, uintptr_t, uint8_t);
extern uintptr_t jet_ui_raw_shortcut_transfer(uintptr_t, uintptr_t, uint8_t);
extern uintptr_t jet_rt_handle_clone_jet_ui_node(uintptr_t);
extern void jet_rt_handle_drop_jet_ui_node(uintptr_t);
extern bool jet_rt_handle_eq_jet_ui_node(uintptr_t, uintptr_t);
extern uintptr_t jet_rt_string_from_static(const void *, int64_t);
extern void jet_rt_string_drop(uintptr_t);

struct Text { const unsigned char *data; uint64_t len, cap; };
struct State { uint64_t tag, payload; };
struct Accessibility { uintptr_t name, description; uint64_t len, cap; struct State *data; };
struct Shortcut { uintptr_t key; uint64_t modifiers; };
_Static_assert(sizeof(struct Accessibility) == 40, "Accessibility40 ABI");
_Static_assert(sizeof(struct State) == 16, "State16 ABI");
_Static_assert(sizeof(struct Shortcut) == 16, "Shortcut16 ABI");
static uintptr_t watched[128];
static unsigned releases[128], watched_count;
// The checked heap reports every free it accepts here, the runtime's own
// included (jet_rt_alloc_check_observe), before it reclaims the block.
static void observe_free(uintptr_t pointer, int64_t size, int64_t align) {
    (void)size; (void)align;
    for (unsigned i = 0; i < watched_count; ++i)
        if (watched[i] == pointer) assert(++releases[i] == 1);
}
// A block watched again before it is freed is another owner of the same
// refcounted text: it stays one entry that must be released exactly once.
static void watch(uintptr_t pointer) {
    assert(pointer && watched_count < 128);
    for (unsigned i = 0; i < watched_count; ++i)
        if (watched[i] == pointer) { assert(releases[i] == 0); return; }
    watched[watched_count++] = pointer;
}
// A heap text is one counted block at t-8 plus its byte buffer.
static void watch_text(uintptr_t pointer) {
    if (!pointer) return;
    const struct Text *text = (const void *)pointer;
    watch(pointer - 8);
    if (text->cap) watch((uintptr_t)text->data);
}
// Heap text is [count][data,len,cap,cache]; the owner count is the word before.
static int64_t owners(uintptr_t text) {
    int64_t word;
    memcpy(&word, (const unsigned char *)text - 8, sizeof(word));
    return word & INT64_C(0x3fffffffffffffff);
}
static uintptr_t heap(const char *bytes) { return jet_rt_string_from_static(bytes, (int64_t)strlen(bytes)); }
static void watch_accessibility(uintptr_t pointer) {
    const struct Accessibility *value = (const void *)pointer;
    watch(pointer);
    watch_text(value->name);
    watch_text(value->description);
    if (value->cap) watch((uintptr_t)value->data);
    for (uint64_t i = 0; i < value->len; ++i)
        if (value->data[i].tag == 6) watch_text(value->data[i].payload);
}
static void watch_shortcut(uintptr_t pointer) {
    watch(pointer);
    watch_text(((const struct Shortcut *)pointer)->key);
}
static uintptr_t word(uintptr_t pointer, unsigned offset) {
    uintptr_t result;
    memcpy(&result, (const unsigned char *)pointer + offset, sizeof(result));
    return result;
}
int main(void) {
    assert(jet_rt_alloc_check_observe(0, (int64_t)(uintptr_t)observe_free) == 1);
    // Inputs are heap texts: the runtime keeps owners of what it stores.
    const uintptr_t name = heap("name");
    const uintptr_t description = heap("desc");
    const uintptr_t value = heap("\xce\xbb");
    const uintptr_t mismatch = heap("other");
    struct State states[] = {{0, UINT64_MAX}, {1, UINT64_MAX}, {2, 1},
        {3, 0}, {4, 1}, {5, UINT64_MAX}, {6, value}, {2, 0}};
    struct Accessibility source = {name, description, 8, 8, states};
    uintptr_t a = jet_rt_handle_clone_jet_ui_accessibility((uintptr_t)&source);
    uintptr_t b = jet_rt_handle_clone_jet_ui_accessibility(a);
    struct Accessibility *copy = (void *)a, *other = (void *)b;
    // Records and state lists are reallocated; the texts are shared by owner count.
    assert(a != b && copy->data != other->data && copy->data != states);
    assert(copy->name == source.name && other->name == source.name && owners(name) == 3);
    assert(copy->description == description && other->description == description && owners(description) == 3);
    assert(copy->data[6].payload == states[6].payload && other->data[6].payload == value && owners(value) == 3);
    assert(jet_rt_handle_eq_jet_ui_accessibility(a, b));
    for (unsigned i = 2; i <= 4; ++i) {
        other->data[i].payload ^= 1;
        assert(!jet_rt_handle_eq_jet_ui_accessibility(a, b));
        other->data[i].payload ^= 1;
    }
    uintptr_t saved = other->data[6].payload;
    other->data[6].payload = mismatch;
    assert(!jet_rt_handle_eq_jet_ui_accessibility(a, b));
    other->data[6].payload = saved;
    other->data[0].payload = UINT64_MAX;
    assert(jet_rt_handle_eq_jet_ui_accessibility(a, b));
    other->data[0].tag = 1;
    assert(!jet_rt_handle_eq_jet_ui_accessibility(a, b));
    other->data[0].tag = 0;
    --other->len;
    assert(!jet_rt_handle_eq_jet_ui_accessibility(a, b));
    ++other->len;
    saved = other->name;
    other->name = 0;
    assert(!jet_rt_handle_eq_jet_ui_accessibility(a, b));
    other->name = saved;
    saved = other->description;
    other->description = mismatch;
    assert(!jet_rt_handle_eq_jet_ui_accessibility(a, b));
    other->description = saved;
    struct State swap = other->data[2];
    other->data[2] = other->data[3];
    other->data[3] = swap;
    assert(!jet_rt_handle_eq_jet_ui_accessibility(a, b));
    swap = other->data[2]; other->data[2] = other->data[3]; other->data[3] = swap;
    struct Accessibility empty = {0, 0, 0, 0, (void *)1};
    uintptr_t e = jet_rt_handle_clone_jet_ui_accessibility((uintptr_t)&empty);
    assert(jet_rt_handle_eq_jet_ui_accessibility(e, (uintptr_t)&empty));
    struct Shortcut shortcut = {name, 31};
    uintptr_t s = jet_rt_handle_clone_jet_ui_shortcut((uintptr_t)&shortcut);
    uintptr_t t = jet_rt_handle_clone_jet_ui_shortcut(s);
    assert(s != t && ((struct Shortcut *)s)->key == name && ((struct Shortcut *)t)->key == name && owners(name) == 5);
    assert(jet_rt_handle_eq_jet_ui_shortcut(s, t));
    ((struct Shortcut *)t)->modifiers ^= 1;
    assert(!jet_rt_handle_eq_jet_ui_shortcut(s, t));
    ((struct Shortcut *)t)->modifiers ^= 1;
    saved = ((struct Shortcut *)t)->key;
    ((struct Shortcut *)t)->key = mismatch;
    assert(!jet_rt_handle_eq_jet_ui_shortcut(s, t));
    ((struct Shortcut *)t)->key = saved;
    uintptr_t node = jet_ui_raw_node_transfer(name, 0, 0, -1, 0,
        NULL, 0, 0, 1, 63, 0, 0, 0, 0, 0, 0);
    uintptr_t attached = jet_ui_raw_accessibility_transfer(node, a, 1);
    uintptr_t both = jet_ui_raw_shortcut_transfer(attached, s, 1);
    uintptr_t clone = jet_rt_handle_clone_jet_ui_node(both);
    // Node160: accessibility and shortcut are nullable owners outside the
    // absent mask, which tracks only the IME, click and drop slots.
    enum { NODE_ABSENT_MASK = 104, NODE_ACCESSIBILITY = 136, NODE_SHORTCUT = 144, NODE_OPTION_MASK = 26 };
    assert(word(node, NODE_ACCESSIBILITY) == 0 && word(node, NODE_SHORTCUT) == 0);
    assert(word(both, NODE_ABSENT_MASK) == NODE_OPTION_MASK && jet_rt_handle_eq_jet_ui_node(both, clone));
    assert(word(both, NODE_ACCESSIBILITY) && word(both, NODE_ACCESSIBILITY) != a && word(both, NODE_SHORTCUT) && word(both, NODE_SHORTCUT) != s);
    assert(word(both, NODE_ACCESSIBILITY) != word(clone, NODE_ACCESSIBILITY));
    assert(word(both, NODE_SHORTCUT) != word(clone, NODE_SHORTCUT));
    uintptr_t cleared = jet_ui_raw_accessibility_transfer(both, UINTPTR_MAX, 0);
    assert(word(cleared, NODE_ACCESSIBILITY) == 0 && word(cleared, NODE_SHORTCUT) && word(cleared, NODE_ABSENT_MASK) == NODE_OPTION_MASK);
    uintptr_t absent = jet_ui_raw_shortcut_transfer(cleared, UINTPTR_MAX, 0);
    assert(word(absent, NODE_SHORTCUT) == 0 && word(absent, NODE_ABSENT_MASK) == NODE_OPTION_MASK);
    watch_accessibility(a); watch_accessibility(b); watch_accessibility(e);
    watch_shortcut(s); watch_shortcut(t);
    watch_accessibility(word(attached, NODE_ACCESSIBILITY));
    watch_accessibility(word(both, NODE_ACCESSIBILITY)); watch_shortcut(word(both, NODE_SHORTCUT));
    watch_accessibility(word(clone, NODE_ACCESSIBILITY)); watch_shortcut(word(clone, NODE_SHORTCUT));
    watch_shortcut(word(cleared, NODE_SHORTCUT));
    jet_rt_handle_drop_jet_ui_accessibility(a);
    jet_rt_handle_drop_jet_ui_accessibility(b);
    jet_rt_handle_drop_jet_ui_accessibility(e);
    jet_rt_handle_drop_jet_ui_shortcut(s); jet_rt_handle_drop_jet_ui_shortcut(t);
    jet_rt_handle_drop_jet_ui_node(node); jet_rt_handle_drop_jet_ui_node(attached);
    jet_rt_handle_drop_jet_ui_node(both); jet_rt_handle_drop_jet_ui_node(clone);
    jet_rt_handle_drop_jet_ui_node(cleared); jet_rt_handle_drop_jet_ui_node(absent);
    jet_rt_handle_drop_jet_ui_accessibility(0); jet_rt_handle_drop_jet_ui_shortcut(0);
    // Only the C owners remain; their releases free each text exactly once.
    assert(owners(name) == 1 && owners(description) == 1 && owners(value) == 1 && owners(mismatch) == 1);
    watch_text(mismatch);
    jet_rt_string_drop(name); jet_rt_string_drop(description);
    jet_rt_string_drop(value); jet_rt_string_drop(mismatch);
    for (unsigned i = 0; i < watched_count; ++i) assert(releases[i] == 1);
    puts("UI metadata: all variants, ordered equality, optional text, independent clones, transfers, exactly-once frees ok");
    return 0;
}
