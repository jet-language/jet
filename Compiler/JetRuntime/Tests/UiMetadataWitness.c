// Real Jet providers only. Link with -Wl,--wrap=jet_rt_free to count frees;
// the observer always forwards to the real allocator, never replaces ownership.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

extern void __real_jet_rt_free(uintptr_t, int64_t, int64_t);
extern uintptr_t jet_rt_handle_clone_jet_ui_accessibility(uintptr_t);
extern void jet_rt_handle_drop_jet_ui_accessibility(uintptr_t);
extern bool jet_rt_handle_eq_jet_ui_accessibility(uintptr_t, uintptr_t);
extern uintptr_t jet_rt_handle_clone_jet_ui_shortcut(uintptr_t);
extern void jet_rt_handle_drop_jet_ui_shortcut(uintptr_t);
extern bool jet_rt_handle_eq_jet_ui_shortcut(uintptr_t, uintptr_t);
extern uintptr_t jet_ui_raw_node_transfer(uintptr_t, double, double, int64_t,
    int64_t, const uintptr_t *, int64_t, uint8_t, uintptr_t, int64_t, uint8_t,
    int64_t, int64_t, int64_t, int64_t, int64_t, int64_t, uintptr_t, uintptr_t);
extern uintptr_t jet_ui_raw_accessibility_transfer(uintptr_t, uintptr_t, uint8_t);
extern uintptr_t jet_ui_raw_shortcut_transfer(uintptr_t, uintptr_t, uint8_t);
extern uintptr_t jet_rt_handle_clone_jet_ui_node(uintptr_t);
extern void jet_rt_handle_drop_jet_ui_node(uintptr_t);
extern bool jet_rt_handle_eq_jet_ui_node(uintptr_t, uintptr_t);

struct Text { const unsigned char *data; uint64_t len, cap; };
struct State { uint64_t tag, payload; };
struct Accessibility { uintptr_t name, description; uint64_t len, cap; struct State *data; };
struct Shortcut { uintptr_t key; uint64_t modifiers; };
_Static_assert(sizeof(struct Accessibility) == 40, "Accessibility40 ABI");
_Static_assert(sizeof(struct State) == 16, "State16 ABI");
_Static_assert(sizeof(struct Shortcut) == 16, "Shortcut16 ABI");
static uintptr_t watched[128];
static unsigned releases[128], watched_count;
void __wrap_jet_rt_free(uintptr_t pointer, int64_t size, int64_t align) {
    for (unsigned i = 0; i < watched_count; ++i)
        if (watched[i] == pointer) assert(++releases[i] == 1);
    __real_jet_rt_free(pointer, size, align);
}
static void watch(uintptr_t pointer) {
    assert(pointer && watched_count < 128);
    for (unsigned i = 0; i < watched_count; ++i) assert(watched[i] != pointer);
    watched[watched_count++] = pointer;
}
static void watch_text(uintptr_t pointer) {
    if (!pointer) return;
    const struct Text *text = (const void *)pointer;
    watch(pointer);
    if (text->cap) watch((uintptr_t)text->data);
}
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
    const struct Text name = {(const unsigned char *)"name", 4, 0};
    const struct Text description = {(const unsigned char *)"desc", 4, 0};
    const struct Text value = {(const unsigned char *)"\xce\xbb", 2, 0};
    const struct Text mismatch = {(const unsigned char *)"other", 5, 0};
    struct State states[] = {{0, UINT64_MAX}, {1, UINT64_MAX}, {2, 1},
        {3, 0}, {4, 1}, {5, UINT64_MAX}, {6, (uintptr_t)&value}, {2, 0}};
    struct Accessibility source = {(uintptr_t)&name, (uintptr_t)&description,
        8, 8, states};
    uintptr_t a = jet_rt_handle_clone_jet_ui_accessibility((uintptr_t)&source);
    uintptr_t b = jet_rt_handle_clone_jet_ui_accessibility(a);
    struct Accessibility *copy = (void *)a, *other = (void *)b;
    assert(a != b && copy->data != other->data && copy->data != states);
    assert(copy->name != source.name && copy->name != other->name);
    assert(copy->data[6].payload != states[6].payload);
    assert(copy->data[6].payload != other->data[6].payload);
    assert(jet_rt_handle_eq_jet_ui_accessibility(a, b));
    for (unsigned i = 2; i <= 4; ++i) {
        other->data[i].payload ^= 1;
        assert(!jet_rt_handle_eq_jet_ui_accessibility(a, b));
        other->data[i].payload ^= 1;
    }
    uintptr_t saved = other->data[6].payload;
    other->data[6].payload = (uintptr_t)&mismatch;
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
    other->description = (uintptr_t)&mismatch;
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
    struct Shortcut shortcut = {(uintptr_t)&name, 31};
    uintptr_t s = jet_rt_handle_clone_jet_ui_shortcut((uintptr_t)&shortcut);
    uintptr_t t = jet_rt_handle_clone_jet_ui_shortcut(s);
    assert(s != t && ((struct Shortcut *)s)->key != ((struct Shortcut *)t)->key);
    assert(jet_rt_handle_eq_jet_ui_shortcut(s, t));
    ((struct Shortcut *)t)->modifiers ^= 1;
    assert(!jet_rt_handle_eq_jet_ui_shortcut(s, t));
    ((struct Shortcut *)t)->modifiers ^= 1;
    saved = ((struct Shortcut *)t)->key;
    ((struct Shortcut *)t)->key = (uintptr_t)&mismatch;
    assert(!jet_rt_handle_eq_jet_ui_shortcut(s, t));
    ((struct Shortcut *)t)->key = saved;
    uintptr_t node = jet_ui_raw_node_transfer((uintptr_t)&name, 0, 0, -1, 0,
        NULL, 0, 0, 1, 63, 0, 0, 0, 0, 0, 0, INT64_MIN, UINTPTR_MAX, UINTPTR_MAX);
    uintptr_t attached = jet_ui_raw_accessibility_transfer(node, a, 1);
    uintptr_t both = jet_ui_raw_shortcut_transfer(attached, s, 1);
    uintptr_t clone = jet_rt_handle_clone_jet_ui_node(both);
    assert(word(both, 112) == 30 && jet_rt_handle_eq_jet_ui_node(both, clone));
    assert(word(both, 144) != word(clone, 144));
    assert(word(both, 152) != word(clone, 152));
    uintptr_t cleared = jet_ui_raw_accessibility_transfer(both, UINTPTR_MAX, 0);
    assert(word(cleared, 144) == 0 && word(cleared, 112) == 31);
    uintptr_t absent = jet_ui_raw_shortcut_transfer(cleared, UINTPTR_MAX, 0);
    assert(word(absent, 152) == 0 && word(absent, 112) == 63);
    watch_accessibility(a); watch_accessibility(b); watch_accessibility(e);
    watch_shortcut(s); watch_shortcut(t);
    watch_accessibility(word(attached, 144));
    watch_accessibility(word(both, 144)); watch_shortcut(word(both, 152));
    watch_accessibility(word(clone, 144)); watch_shortcut(word(clone, 152));
    watch_shortcut(word(cleared, 152));
    jet_rt_handle_drop_jet_ui_accessibility(a);
    jet_rt_handle_drop_jet_ui_accessibility(b);
    jet_rt_handle_drop_jet_ui_accessibility(e);
    jet_rt_handle_drop_jet_ui_shortcut(s); jet_rt_handle_drop_jet_ui_shortcut(t);
    jet_rt_handle_drop_jet_ui_node(node); jet_rt_handle_drop_jet_ui_node(attached);
    jet_rt_handle_drop_jet_ui_node(both); jet_rt_handle_drop_jet_ui_node(clone);
    jet_rt_handle_drop_jet_ui_node(cleared); jet_rt_handle_drop_jet_ui_node(absent);
    jet_rt_handle_drop_jet_ui_accessibility(0); jet_rt_handle_drop_jet_ui_shortcut(0);
    for (unsigned i = 0; i < watched_count; ++i) assert(releases[i] == 1);
    puts("UI metadata: all variants, ordered equality, optional text, independent clones, transfers, exactly-once frees ok");
    return 0;
}
