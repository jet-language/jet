// Link against the composed real Jet storage + MemRegistry providers.
// These callbacks are value destructors, not replacements for any provider.
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef uintptr_t Handle;
typedef uintptr_t (*Allocate)(Handle, int64_t, int64_t, uintptr_t);
typedef void (*Reset)(Handle);
extern Handle jet_arena_new(void), jet_arena_with_capacity(int64_t);
extern Handle jet_bump_new(void), jet_bump_with_capacity(int64_t);
extern Handle jet_pool_with_slots(int64_t), jet_rt_mem_pool_storage_new(void);
extern Handle jet_rt_mem_fixed_storage_over(uintptr_t, int64_t);
#define KIND(K, T) \
    extern uintptr_t jet_rt_mem_##K##_storage_alloc(Handle, int64_t, int64_t, uintptr_t); \
    extern uintptr_t jet_rt_mem_##K##_storage_try_alloc(Handle, int64_t, int64_t, uintptr_t); \
    extern void jet_rt_mem_##K##_storage_reset(Handle); \
    extern void jet_rt_handle_drop_##T(Handle)
KIND(arena, JetArena);
KIND(bump, JetBump);
KIND(pool, JetPool);
KIND(fixed, JetFixed);
#undef KIND
extern void jet_rt_mem_resources(uint64_t *);
extern void jet_rt_mem_allocator_memory(uint64_t *);
extern void jet_rt_mem_allocation_probe_reset(void);
extern void jet_rt_mem_allocation_probe_take(uint64_t *);
extern void jet_rt_fault_allocation_arm(int64_t, bool);
extern uint64_t jet_rt_fault_allocation_fact(int64_t);
extern void jet_rt_fault_allocation_clear(void);
extern void jet_rt_program_allocator_configure(int64_t, int64_t);
extern void jet_rt_program_allocator_restore(int64_t, int64_t);
extern uint64_t jet_rt_program_allocator_fact(int64_t);

static unsigned order[128], count;
static uint64_t live_during_drop, bytes_during_drop;
static void resources(uint64_t arenas, uint64_t allocations, uint64_t bytes) {
    uint64_t out[3];
    jet_rt_mem_resources(out);
    assert(out[0] == arenas && out[1] == allocations && out[2] == bytes);
}
static uint64_t retained(void) {
    uint64_t out[2];
    jet_rt_mem_allocator_memory(out);
    return out[0];
}
static void remember(unsigned id) {
    uint64_t out[3];
    jet_rt_mem_resources(out);
    assert(out[1] == live_during_drop && out[2] == bytes_during_drop);
    assert(count < sizeof(order) / sizeof(order[0]));
    order[count++] = id;
}
static void drop(uintptr_t value) { remember((unsigned)*(const uint64_t *)value); }
static void zero_drop(uintptr_t value) { assert(value == 128); remember(30); }
static uintptr_t put(Allocate allocate, Handle h, int64_t bytes, int64_t align, unsigned id) {
    uintptr_t p = allocate(h, bytes, align, (uintptr_t)drop);
    assert(p && p % (uintptr_t)align == 0 && bytes >= 8);
    *(uint64_t *)p = id; // Lower's immediate initialized typed store.
    return p;
}
static void reverse(Reset reset, Handle h, unsigned first, unsigned second,
                    unsigned third, uint64_t allocations, uint64_t bytes) {
    count = 0;
    live_during_drop = allocations;
    bytes_during_drop = bytes;
    reset(h);
    assert(count == (third ? 3u : 2u));
    assert(order[0] == first && order[1] == second);
    if (third) assert(order[2] == third);
}
static void probe(uint64_t allocations, uint64_t bytes) {
    uint64_t out[2];
    jet_rt_mem_allocation_probe_take(out);
    assert(out[0] == allocations && out[1] == bytes);
}
static void arena(void) {
    jet_rt_mem_allocation_probe_reset();
    Handle h = jet_arena_with_capacity(16);
    resources(1, 0, 0);
    assert(retained() == 16);
    uintptr_t a = put(jet_rt_mem_arena_storage_alloc, h, 8, 8, 1);
    uintptr_t b = put(jet_rt_mem_arena_storage_alloc, h, 8, 8, 2);
    assert(b == a + 8);
    uintptr_t c = put(jet_rt_mem_arena_storage_alloc, h, 8, 64, 3);
    assert(c != a && retained() == 88); // max(next32, size8+align64)=72
    resources(1, 3, 24);
    probe(3, 24);
    reverse(jet_rt_mem_arena_storage_reset, h, 3, 2, 1, 3, 24);
    resources(1, 0, 0);
    assert(retained() == 88);
    uintptr_t reused = jet_rt_mem_arena_storage_alloc(h, 8, 8, 0);
    assert(reused == a);
    uintptr_t zero = jet_rt_mem_arena_storage_alloc(h, 0, 8, 0);
    assert(zero == a + 8);
    resources(1, 2, 8);
    probe(2, 8);
    jet_rt_handle_drop_jet_arena(h);
    resources(0, 0, 0);
    assert(retained() == 0);
    puts("Arena: align/grow/reuse/ZST/reverse-drop ok");
}
static void bump(void) {
    Handle h = jet_bump_with_capacity(32);
    uintptr_t a = put(jet_rt_mem_bump_storage_alloc, h, 8, 8, 10);
    uintptr_t b = put(jet_rt_mem_bump_storage_alloc, h, 8, 16, 20);
    assert(b == a + 16);
    uintptr_t zero = jet_rt_mem_bump_storage_alloc(h, 0, 8, 0);
    assert(zero == a + 24);
    resources(1, 3, 16);
    assert(jet_rt_mem_bump_storage_try_alloc(h, 16, 8, 0) == 0);
    resources(1, 3, 16);
    reverse(jet_rt_mem_bump_storage_reset, h, 20, 10, 0, 3, 16);
    assert(jet_rt_mem_bump_storage_alloc(h, 8, 8, 0) == a);
    jet_rt_handle_drop_jet_bump(h);
    resources(0, 0, 0);
    assert(retained() == 0);
    puts("Bump: align/capacity/reuse/reverse-drop ok");
}
static void pool(void) {
    Handle h = jet_pool_with_slots(2);
    uintptr_t a = put(jet_rt_mem_pool_storage_alloc, h, 8, 8, 1);
    uintptr_t b = put(jet_rt_mem_pool_storage_alloc, h, 32, 32, 2);
    assert(retained() == 40);
    assert(jet_rt_mem_pool_storage_try_alloc(h, 8, 8, 0) == 0);
    reverse(jet_rt_mem_pool_storage_reset, h, 2, 1, 0, 2, 40);
    assert(put(jet_rt_mem_pool_storage_alloc, h, 16, 8, 3) == b);
    assert(put(jet_rt_mem_pool_storage_alloc, h, 8, 8, 4) == a);
    reverse(jet_rt_mem_pool_storage_reset, h, 4, 3, 0, 2, 24);
    uintptr_t replacement = jet_rt_mem_pool_storage_alloc(h, 64, 64, 0);
    assert(replacement != a && retained() == 96); // slot0 class replaced
    jet_rt_handle_drop_jet_pool(h);
    resources(0, 0, 0);
    assert(retained() == 0);
    h = jet_pool_with_slots(0);
    assert(jet_rt_mem_pool_storage_try_alloc(h, 8, 8, 0) == 0);
    resources(1, 0, 0);
    jet_rt_handle_drop_jet_pool(h);
    puts("Pool: class priority/replacement/zero-slots/reuse/reverse-drop ok");
}
static void fixed(void) {
    _Alignas(64) unsigned char bytes[258];
    memset(bytes, 0xa5, sizeof(bytes));
    Handle h = jet_rt_mem_fixed_storage_over((uintptr_t)(bytes + 1), 256);
    uintptr_t a = put(jet_rt_mem_fixed_storage_alloc, h, 8, 64, 10);
    uintptr_t b = put(jet_rt_mem_fixed_storage_alloc, h, 8, 8, 20);
    assert(a == (uintptr_t)(bytes + 64) && b == a + 8);
    assert(jet_rt_mem_fixed_storage_alloc(h, 0, 128, (uintptr_t)zero_drop) == 128);
    resources(1, 3, 16);
    assert(retained() == 256);
    assert(jet_rt_mem_fixed_storage_try_alloc(h, 256, 8, 0) == 0);
    reverse(jet_rt_mem_fixed_storage_reset, h, 30, 20, 10, 3, 16);
    assert(jet_rt_mem_fixed_storage_alloc(h, 8, 64, 0) == a);
    jet_rt_handle_drop_jet_fixed(h);
    assert(bytes[0] == 0xa5 && bytes[257] == 0xa5); // neither guard touched
    bytes[1] = 0x5a; // borrowed backing is still live, never freed by Fixed.
    resources(0, 0, 0);
    assert(retained() == 0);
    _Alignas(8) unsigned char tiny[40];
    h = jet_rt_mem_fixed_storage_over((uintptr_t)tiny, sizeof(tiny));
    assert(jet_rt_mem_fixed_storage_alloc(h, 0, 8, 0) == 8);
    assert(jet_rt_mem_fixed_storage_try_alloc(h, 0, 8, 0) == 0);
    jet_rt_handle_drop_jet_fixed(h);
    puts("Fixed: in-buffer headers/unaligned backing/ZST dangling/reuse/reverse-drop/borrowed backing ok");
}
static void tries(void) {
    Handle h = jet_arena_with_capacity(1);
    jet_rt_fault_allocation_arm(2, true);
    uintptr_t p = jet_rt_mem_arena_storage_try_alloc(h, 16, 8, 0);
    assert(p && jet_rt_fault_allocation_fact(0) == 1);
    assert(jet_rt_fault_allocation_fact(1) == 0 && retained() == 25);
    assert(jet_rt_mem_arena_storage_try_alloc(h, 1, 1, 0) == 0);
    assert(jet_rt_fault_allocation_fact(0) == 2 && jet_rt_fault_allocation_fact(1) == 1);
    resources(1, 1, 16);
    jet_rt_fault_allocation_arm(2, true);
    uintptr_t reuse = jet_rt_mem_arena_storage_try_alloc(h, 1, 1, 0);
    assert(reuse == p + 16 && jet_rt_fault_allocation_fact(0) == 1);
    assert(jet_rt_fault_allocation_fact(1) == 0);
    jet_rt_mem_arena_storage_reset(h);
    jet_rt_fault_allocation_arm(1, true);
    assert(jet_rt_mem_arena_storage_try_alloc(h, 128, 8, 0) == 0);
    assert(jet_rt_fault_allocation_fact(0) == 1 && retained() == 25);
    resources(1, 0, 0);
    jet_rt_fault_allocation_arm(0, false);
    jet_rt_program_allocator_configure(1, 0);
    int64_t huge = INT64_C(4611686018427387904);
    assert(jet_rt_mem_arena_storage_try_alloc(h, huge, 8, 0) == 0);
    assert(jet_rt_fault_allocation_fact(0) == 1);
    assert(jet_rt_fault_allocation_fact(1) == 0);
    assert(jet_rt_program_allocator_fact(0) == 1);
    assert(jet_rt_program_allocator_fact(1) == (uint64_t)huge + 8);
    assert(jet_rt_program_allocator_fact(2) == 0);
    jet_rt_program_allocator_restore(0, 0);
    resources(1, 0, 0);
    assert(retained() == 25);
    jet_rt_fault_allocation_clear();
    jet_rt_handle_drop_jet_arena(h);
    puts("try: inject once/grow/reuse/OOM rollback/no double probe ok");
}
static void defaults(void) {
    Handle a = jet_arena_new();
    assert(retained() == 4096);
    Handle b = jet_bump_new();
    assert(retained() == 4096 + 65536);
    Handle p = jet_rt_mem_pool_storage_new();
    resources(3, 0, 0);
    uintptr_t first = jet_rt_mem_pool_storage_alloc(p, 0, 1, 0);
    assert(first);
    for (unsigned i = 1; i < 64; ++i)
        assert(jet_rt_mem_pool_storage_alloc(p, 0, 1, 0));
    assert(jet_rt_mem_pool_storage_try_alloc(p, 0, 1, 0) == 0);
    jet_rt_handle_drop_jet_pool(p);
    jet_rt_handle_drop_jet_bump(b);
    jet_rt_handle_drop_jet_arena(a);
    jet_rt_handle_drop_jet_arena(0);
    jet_rt_handle_drop_jet_bump(0);
    jet_rt_handle_drop_jet_pool(0);
    jet_rt_handle_drop_jet_fixed(0);
    puts("defaults: Arena=4096 Bump=65536 Pool=64 ok");
}
int main(void) {
    // Real runtime init_array owns allocator/TCB/fault/Sentry/MemRegistry init.
    resources(0, 0, 0);
    assert(retained() == 0);
    arena(); bump(); pool(); fixed(); tries(); defaults();
    resources(0, 0, 0);
    assert(retained() == 0);
    puts("MemStorage: resources and retained blocks balanced");
    return 0;
}
