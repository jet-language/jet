// Real native Condition48 + allocator; link -Wl,--wrap=jet_rt_free.
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
typedef struct { int64_t refs, mutex, head, tail, epoch, pending; } Condition;
_Static_assert(sizeof(Condition) == 48, "canonical native Condition48");
extern void jet_rt_alloc_init(void);
extern Condition *jet_rt_native_condition_new(void);
extern void jet_rt_native_condition_notify_one(Condition *);
extern void jet_rt_native_condition_notify_all(Condition *);
extern Condition *jet_rt_handle_clone_jet_condition(Condition *);
extern void jet_rt_handle_drop_jet_condition(Condition *);
extern void __real_jet_rt_free(void *, int64_t, int64_t);
static Condition *watched;
static unsigned frees;
void __wrap_jet_rt_free(void *pointer, int64_t bytes, int64_t align) {
    if (pointer == watched) { assert(bytes == 48 && align == 8); assert(++frees == 1); }
    __real_jet_rt_free(pointer, bytes, align);
}
int main(void) {
    jet_rt_alloc_init();
    Condition *owner = jet_rt_native_condition_new();
    watched = owner;
    assert(owner->refs == 1 && owner->pending == 0 && owner->epoch == 0);
    Condition *a = jet_rt_handle_clone_jet_condition(owner);
    Condition *b = jet_rt_handle_clone_jet_condition(a);
    assert(a == owner && b == owner && owner->refs == 3);
    jet_rt_native_condition_notify_one(a);
    assert(b->pending == 1 && owner->epoch == 1);
    jet_rt_handle_drop_jet_condition(owner);
    assert(frees == 0 && b->refs == 2 && b->pending == 1);
    jet_rt_native_condition_notify_all(b);
    assert(a->pending == 2 && a->epoch == 2);
    jet_rt_handle_drop_jet_condition(a);
    assert(frees == 0 && b->refs == 1 && b->pending == 2);
    jet_rt_handle_drop_jet_condition(NULL);
    assert(frees == 0);
    jet_rt_handle_drop_jet_condition(b);
    assert(frees == 1); watched = NULL;
    puts("Condition retained identity/notify/drop OK");
}
