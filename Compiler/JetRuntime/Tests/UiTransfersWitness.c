// Link against native UiNode/UiMetadata/UiTransfers; inputs remain borrowed.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <string.h>
typedef int64_t H;
typedef struct { const unsigned char *data; uint64_t len, cap; } Text;
typedef struct { uint64_t len, cap; H *data; } Sequence;
typedef struct { Text *name, *description; uint64_t len, cap; H *data; } Accessibility;
typedef struct { Text *key; H modifiers; } Shortcut;
typedef struct {
    Text *label; double width, height; H role, kind; Sequence *children;
    Text *color; H style, foreground_kind, foreground, background_kind, background;
    H flags, absent, ime, click, drop; Accessibility *accessibility;
    Shortcut *shortcut; Text *key;
} Node;
_Static_assert(sizeof(Node) == 160 && offsetof(Node, accessibility) == 136 && offsetof(Node, shortcut) == 144, "UiABI");
extern H jet_ui_raw_accessibility_transfer(H, H, uint8_t);
extern H jet_ui_raw_shortcut_transfer(H, H, uint8_t);
extern void jet_rt_handle_drop_jet_ui_node(H);
extern bool jet_rt_handle_eq_jet_ui_accessibility(H, H), jet_rt_handle_eq_jet_ui_shortcut(H, H);
static H handle(const void *p) { return (H)(uintptr_t)p; }
static Node *node(H h) { return (Node *)(uintptr_t)h; }
int main(void) {
    Text label = {(const unsigned char *)"node", 4, 0};
    Text old_name = {(const unsigned char *)"old", 3, 0};
    Text new_name = {(const unsigned char *)"new", 3, 0};
    Text old_key = {(const unsigned char *)"O", 1, 0};
    Text new_key = {(const unsigned char *)"N", 1, 0};
    Accessibility old_a = {&old_name, NULL, 0, 0, NULL};
    Accessibility new_a = {&new_name, NULL, 0, 0, NULL};
    Shortcut old_s = {&old_key, 3}, new_s = {&new_key, 4};
    Sequence children = {0, 0, NULL};
    Node input = {.label = &label, .width = 2, .height = 3, .role = -1,
        .children = &children, .absent = 26, .accessibility = &old_a, .shortcut = &old_s};
    Node saved = input;
    for (unsigned present = 0; present < 2; ++present) {
        // Absent replacement must not inspect the poisoned metadata address.
        H a = jet_ui_raw_accessibility_transfer(handle(&input), present ? handle(&new_a) : 1, present ? 255 : 0);
        Node *out = node(a);
        assert(out != &input && out->label == &label && out->children != &children);
        assert((out->accessibility != NULL) == (present != 0));
        if (present) {
            assert(out->accessibility != &new_a && out->accessibility->name == &new_name);
            assert(jet_rt_handle_eq_jet_ui_accessibility(handle(out->accessibility), handle(&new_a)));
        }
        assert(out->shortcut != &old_s && jet_rt_handle_eq_jet_ui_shortcut(handle(out->shortcut), handle(&old_s)));
        assert(memcmp(&input, &saved, sizeof input) == 0);
        jet_rt_handle_drop_jet_ui_node(a);
        H s = jet_ui_raw_shortcut_transfer(handle(&input), present ? handle(&new_s) : 1, present ? 255 : 0);
        out = node(s);
        assert((out->shortcut != NULL) == (present != 0));
        if (present) {
            assert(out->shortcut != &new_s && out->shortcut->key == &new_key);
            assert(jet_rt_handle_eq_jet_ui_shortcut(handle(out->shortcut), handle(&new_s)));
        }
        assert(out->accessibility != &old_a && jet_rt_handle_eq_jet_ui_accessibility(handle(out->accessibility), handle(&old_a)));
        assert(memcmp(&input, &saved, sizeof input) == 0);
        jet_rt_handle_drop_jet_ui_node(s);
    }
    return 0;
}
