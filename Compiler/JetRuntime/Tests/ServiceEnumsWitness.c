// Link actual native providers with -Wl,--wrap=jet_rt_free; no mocks.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct { int64_t tag; } Enum;
typedef struct { const char *data; int64_t len, capacity; } Text;
_Static_assert(sizeof(Enum) == 8, "canonical native enum8");
extern void jet_rt_alloc_init(void);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
extern void jet_rt_string_drop(Text *);
extern Enum *jet_rt_handle_clone_jet_service_restart(const Enum *);
extern void jet_rt_handle_drop_jet_service_restart(Enum *);
extern bool jet_rt_handle_eq_jet_service_restart(const Enum *, const Enum *);
extern Text *jet_rt_handle_show_jet_service_restart(const Enum *);
extern Text *jet_rt_handle_display_jet_service_restart(const Enum *);
extern Enum *jet_rt_handle_clone_jet_service_delivery(const Enum *);
extern void jet_rt_handle_drop_jet_service_delivery(Enum *);
extern bool jet_rt_handle_eq_jet_service_delivery(const Enum *, const Enum *);
extern Text *jet_rt_handle_show_jet_service_delivery(const Enum *);
extern Text *jet_rt_handle_display_jet_service_delivery(const Enum *);
static void *watched;
static unsigned frees, intercepted;
void __wrap_jet_rt_free(void *pointer, int64_t bytes, int64_t align) {
    ++intercepted;
    if (pointer == watched) { assert(bytes == 8 && align == 8); assert(++frees == 1); }
    __real_jet_rt_free(pointer, bytes, align);
}
static void text(Text *value, const char *expected) {
    assert(value->len == (int64_t)strlen(expected));
    assert(memcmp(value->data, expected, (size_t)value->len) == 0);
    jet_rt_string_drop(value);
}
static void family(int64_t count, const char *const *labels,
                   Enum *(*clone)(const Enum *), void (*drop)(Enum *),
                   bool (*eq)(const Enum *, const Enum *),
                   Text *(*show)(const Enum *), Text *(*display)(const Enum *)) {
    for (int64_t tag = 0; tag < count; ++tag) {
        Enum value = {tag};
        Enum *owned = clone(&value);
        assert(owned != &value && owned->tag == tag && eq(&value, owned));
        for (int64_t other = 0; other < count; ++other) {
            Enum different = {other};
            assert(eq(&value, &different) == (tag == other));
        }
        value.tag = (tag + 1) % count;
        assert(owned->tag == tag && !eq(&value, owned));
        Text *a = show(owned), *b = display(owned);
        assert(a != b);
        text(a, labels[tag]); text(b, labels[tag]);
        watched = owned; frees = 0;
        drop(owned);
        assert(frees == 1); watched = NULL;
    }
    drop(NULL);
}
int main(void) {
    static const char *const restart[] = {"OneForOne", "OneForAll", "RestForOne"};
    static const char *const delivery[] = {"AtMostOnce", "DurableAtLeastOnce"};
    jet_rt_alloc_init();
    family(3, restart, jet_rt_handle_clone_jet_service_restart, jet_rt_handle_drop_jet_service_restart,
           jet_rt_handle_eq_jet_service_restart, jet_rt_handle_show_jet_service_restart, jet_rt_handle_display_jet_service_restart);
    family(2, delivery, jet_rt_handle_clone_jet_service_delivery, jet_rt_handle_drop_jet_service_delivery,
           jet_rt_handle_eq_jet_service_delivery, jet_rt_handle_show_jet_service_delivery, jet_rt_handle_display_jet_service_delivery);
    assert(intercepted > 0);
    puts("ServiceEnums ownership/equality/render OK");
}
