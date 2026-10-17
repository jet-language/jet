// Actual Jet runtime witnesses. C supplies only raw checked schema/payload
// carriers and a separately encoded hash; no clone/drop/Shared implementation.
#include "../SharedSource.h"
#include <assert.h>
#include <inttypes.h>
#include <stdio.h>
#include <string.h>
#include <stdatomic.h>

typedef struct { uintptr_t len, cap, data; } List;
typedef struct { uintptr_t data, len, cap; } E4;
typedef struct { E4 text; int64_t number; uintptr_t optional; } Row;
typedef struct { uint8_t flag; uint8_t padding[7]; List rows; } Root;
_Static_assert(sizeof(Row) == 40, "Lower placed record");
_Static_assert(sizeof(Root) == 32, "Lower placed root");
extern uintptr_t jet_rt_alloc(uintptr_t, uintptr_t);
extern void jet_rt_free(uintptr_t, uintptr_t, uintptr_t);
extern uintptr_t jet_rt_string_from_static(const char *, uintptr_t);
extern void jet_rt_string_drop(uintptr_t);
extern int64_t jet_rt_int_from_static(const char *, uintptr_t);
extern void jet_rt_int_drop(int64_t);
extern void jet_rt_thread_attach(void);
extern void jet_rt_thread_detach(void);
extern void jet_rt_shared_init(void);
extern uintptr_t jet_rt_shared_site_publish(_Atomic(uintptr_t) *, uintptr_t, int64_t);
extern void jet_rt_global_reset_registered(void);
extern int64_t jet_rt_source_fixture_live(void);

static uint64_t hash_word(uint64_t h, uint64_t word) {
    for (unsigned i = 0; i < 8; ++i) {
        h = (h ^ (word & 255)) * UINT64_C(1099511628211);
        word >>= 8;
    }
    return h;
}
/* Outer Source codec encoding, independent of pointer placement/native walk. */
static uint64_t source_codec_hash(const JetSourceSchema *s) {
    const uint64_t words[] = {s->nominal,s->size,s->align,s->kind,s->count,s->aux0,s->aux1};
    uint64_t h = UINT64_C(14695981039346656037);
    for (unsigned i = 0; i < 7; ++i) h = hash_word(h, words[i]);
    for (uint64_t i = 0; i < s->count; ++i) {
        h = hash_word(h, s->fields[i].offset);
        h = hash_word(h, source_codec_hash(s->fields[i].schema));
    }
    return hash_word(h, s->child ? source_codec_hash(s->child) : 0);
}
static void seal(JetSourceSchema *s) {
    s->hash = source_codec_hash(s);
    assert(s->hash == jet_rt_source_schema_hash(s));
}
static JetSourceSchema scalar(uint64_t id, uint64_t size, uint64_t align, uint64_t kind) {
    JetSourceSchema s = {.nominal=id,.size=size,.align=align,.kind=kind};
    seal(&s);
    return s;
}
static uint64_t refs(int64_t raw) {
    uintptr_t node = (uint64_t)raw & UINT64_C(0x3fffffffffffffff);
    return atomic_load((_Atomic(uint64_t) *)node);
}
static int64_t big(void) {
    const char text[] = "123456789012345678901234567890";
    return jet_rt_int_from_static(text, sizeof(text)-1);
}
static E4 inline_text(const char *text) {
    uintptr_t handle = jet_rt_string_from_static(text, strlen(text));
    E4 slot;
    memcpy(&slot, (void *)handle, sizeof(slot));
    /* Transfer bytes, not the separately allocated header. */
    jet_rt_free(handle, 24, 8);
    return slot;
}
static void scalar_witnesses(void) {
    int64_t baseline = jet_rt_source_fixture_live();
    JetSourceSchema word = scalar(UINT64_C(0x1122334455667788),8,8,JET_SOURCE_WORD);
    assert(word.hash == UINT64_C(0x66a2806c223fb6d5));
    JetSourceSchema relocated = word;
    assert(jet_rt_source_schema_hash(&relocated) == word.hash);
    JetSourceSchema byte = scalar(11,1,1,JET_SOURCE_WORD);
    uint8_t source[] = {0x71,0x72}, target[] = {0x33,0x44};
    jet_rt_source_data_clone(source,target,&byte);
    assert(target[0] == 0x71 && target[1] == 0x44); /* exact width */
    JetSourceSchema unit = scalar(12,0,1,JET_SOURCE_WORD);
    jet_rt_source_data_clone(source,target,&unit);
    assert(target[1] == 0x44);
    jet_rt_source_data_drop(target,&unit);
    JetSourceSchema integer = scalar(13,8,8,JET_SOURCE_INT);
    int64_t a = 42, b = 0;
    jet_rt_source_data_clone(&a,&b,&integer);
    assert(b == 42);
    jet_rt_source_data_drop(&b,&integer);
    a = big();
    assert(refs(a) == 1);
    jet_rt_source_data_clone(&a,&b,&integer);
    assert(a == b && refs(a) == 2);
    jet_rt_source_data_drop(&b,&integer);
    assert(refs(a) == 1);
    jet_rt_source_data_drop(&a,&integer);
    JetSourceSchema text = scalar(14,24,8,JET_SOURCE_E4);
    E4 e = inline_text("source"), copy = {0};
    jet_rt_source_data_clone(&e,&copy,&text);
    assert(e.data != copy.data && copy.len == 6 && memcmp((void *)copy.data,"source",6) == 0);
    jet_rt_source_data_drop(&e,&text);
    jet_rt_source_data_drop(&copy,&text);
    e = inline_text("");
    jet_rt_source_data_clone(&e,&copy,&text);
    assert(e.data == 1 && e.cap == 0 && copy.data == 1 && copy.cap == 0);
    jet_rt_source_data_drop(&e,&text);
    jet_rt_source_data_drop(&copy,&text);
    JetSourceSchema handle = scalar(14,8,8,JET_SOURCE_E4);
    uintptr_t h = jet_rt_string_from_static("handle",6), k = 0;
    jet_rt_source_data_clone(&h,&k,&handle);
    assert(h != k);
    jet_rt_source_data_drop(&h,&handle);
    jet_rt_source_data_drop(&k,&handle);
    assert(jet_rt_source_fixture_live() == baseline);
}

typedef struct {
    JetSourceSchema word, text, integer, ints, option, row, rows, root;
    JetSourceField row_fields[3], root_fields[2];
} Graph;
static void graph(Graph *g) {
    memset(g,0,sizeof(*g));
    g->word = scalar(11,1,1,JET_SOURCE_WORD);
    g->text = scalar(14,24,8,JET_SOURCE_E4);
    g->integer = scalar(13,8,8,JET_SOURCE_INT);
    g->ints = (JetSourceSchema){.nominal=20,.size=24,.align=8,.kind=JET_SOURCE_LIST,.child=&g->integer,.aux0=8};
    seal(&g->ints);
    g->option = (JetSourceSchema){.nominal=21,.size=8,.align=8,.kind=JET_SOURCE_OPTION,.child=&g->ints,.aux1=24};
    seal(&g->option);
    g->row_fields[0] = (JetSourceField){0,&g->text};
    g->row_fields[1] = (JetSourceField){24,&g->integer};
    g->row_fields[2] = (JetSourceField){32,&g->option};
    g->row = (JetSourceSchema){.nominal=22,.size=40,.align=8,.kind=JET_SOURCE_RECORD,.count=3,.fields=g->row_fields};
    seal(&g->row);
    g->rows = (JetSourceSchema){.nominal=23,.size=24,.align=8,.kind=JET_SOURCE_LIST,.child=&g->row,.aux0=40};
    seal(&g->rows);
    g->root_fields[0] = (JetSourceField){0,&g->word};
    g->root_fields[1] = (JetSourceField){8,&g->rows};
    g->root = (JetSourceSchema){.nominal=24,.size=32,.align=8,.kind=JET_SOURCE_RECORD,.count=2,.fields=g->root_fields};
    seal(&g->root);
}
static Root payload(void) {
    Row *rows = (Row *)jet_rt_alloc(80,8);
    memset(rows,0,80);
    rows[0].text = inline_text("nested");
    rows[0].number = big();
    List *optional = (List *)jet_rt_alloc(24,8);
    int64_t *numbers = (int64_t *)jet_rt_alloc(16,8);
    numbers[0] = big(); numbers[1] = 17;
    *optional = (List){2,2,(uintptr_t)numbers};
    rows[0].optional = (uintptr_t)optional;
    rows[1].text = inline_text(""); rows[1].number = -19; /* None */
    return (Root){.flag=1,.rows={2,2,(uintptr_t)rows}};
}
static uintptr_t make_root(void) {
    Graph factory_schema;
    graph(&factory_schema);
    Root state = payload();
    return jet_rt_native_shared_source_new(&state,&factory_schema.root);
    /* Both factory schema and slot cease to exist after return. */
}
static void nested_witnesses(void) {
    int64_t baseline = jet_rt_source_fixture_live();
    Graph g; graph(&g);
    Root a = payload(), b;
    Row *original = (Row *)a.rows.data;
    int64_t n = original[0].number;
    jet_rt_source_data_clone(&a,&b,&g.root);
    Row *cloned = (Row *)b.rows.data;
    assert(b.flag == 1 && b.rows.data != a.rows.data && b.rows.len == 2);
    assert(cloned[0].text.data != original[0].text.data);
    assert(cloned[0].optional != original[0].optional && cloned[1].optional == 0);
    assert(refs(n) == 2);
    List *x = (List *)original[0].optional, *y = (List *)cloned[0].optional;
    assert(x->data != y->data && refs(((int64_t *)x->data)[0]) == 2);
    jet_rt_source_data_drop(&b,&g.root);
    assert(refs(n) == 1 && refs(((int64_t *)x->data)[0]) == 1);
    jet_rt_source_data_drop(&a,&g.root);
    assert(jet_rt_source_fixture_live() == baseline);
}
static _Atomic(uintptr_t) process_site, world_site;
static void projection_witnesses(void) {
    Graph factory_a, factory_b;
    graph(&factory_a); graph(&factory_b);
    uintptr_t cell = make_root();
    assert(jet_rt_shared_site_publish(&process_site,cell,0) == cell);
    JetSourceBorrow view = {0};
    assert(jet_rt_source_shared_borrow_begin(cell,&factory_b.root,0,&view) == JET_SOURCE_ALIAS);
    uintptr_t alias = view.payload;
    Root bytes;
    memcpy(&bytes,(void *)alias,sizeof(bytes));
    JetSourceBorrow refused = {0x1234,0x5678};
    assert(jet_rt_source_shared_edit_begin(cell,&factory_b.root,0,&refused) == JET_SOURCE_BORROW_REENTRANT);
    assert(jet_rt_source_shared_borrow_begin(cell,&factory_b.root,0,&refused) == JET_SOURCE_BORROW_REENTRANT);
    assert(refused.token == 0x1234 && refused.payload == 0x5678);
    /* Publication of a loser during a read borrow is non-writing and legal. */
    assert(jet_rt_shared_site_publish(&process_site,make_root(),0) == cell);
    assert(memcmp(&bytes,(void *)alias,sizeof(bytes)) == 0);
    jet_rt_source_shared_borrow_end(view.token);
    assert(jet_rt_source_shared_edit_begin(cell,&factory_a.root,0,&view) == JET_SOURCE_ALIAS);
    assert(view.payload == alias); /* reprojection never copies the table */
    ((Root *)view.payload)->flag = 0;
    jet_rt_source_shared_borrow_end(view.token);
    assert(jet_rt_source_shared_borrow_begin(cell,&factory_b.root,0,&view) == JET_SOURCE_ALIAS);
    assert(view.payload == alias && ((Root *)view.payload)->flag == 0);
    memcpy(&bytes,(void *)alias,sizeof(bytes));
    jet_rt_source_shared_borrow_end(view.token);
    factory_b.root.hash ^= 1;
    assert(jet_rt_source_shared_borrow_begin(cell,&factory_b.root,0,&refused) == JET_SOURCE_SCHEMA_PROCESS_MISMATCH);
    assert(refused.token == 0x1234 && refused.payload == 0x5678);
    assert(atomic_load(&process_site) == cell && memcmp(&bytes,(void *)alias,sizeof(bytes)) == 0);
    factory_b.root = factory_a.root;
    factory_b.root.nominal ^= 1;
    assert(jet_rt_source_shared_borrow_begin(cell,&factory_b.root,0,&refused) == JET_SOURCE_SCHEMA_PROCESS_MISMATCH);
    uintptr_t world = make_root();
    assert(jet_rt_shared_site_publish(&world_site,world,1) == world);
    assert(jet_rt_source_shared_borrow_begin(world,&factory_a.root,1,&view) == JET_SOURCE_ALIAS);
    uintptr_t retained = jet_rt_native_shared_clone(world);
    jet_rt_source_shared_borrow_end(view.token);
    assert(jet_rt_source_shared_borrow_begin(world,&factory_b.root,1,&refused) == JET_SOURCE_SCHEMA_WORLD_LEASE_BOUNDARY);
    assert(atomic_load(&world_site) == world); /* mismatch is NOT a reset */
    jet_rt_global_reset_registered(); /* explicit quiescent lease boundary */
    assert(atomic_load(&world_site) == 0 && atomic_load(&process_site) == cell);
    assert(jet_rt_source_shared_borrow_begin(retained,&factory_a.root,1,&view) == JET_SOURCE_ALIAS);
    assert(((Root *)view.payload)->rows.len == 2); /* aliases survive site release */
    jet_rt_source_shared_borrow_end(view.token);
    jet_rt_native_shared_drop(retained);
    factory_b.root = factory_a.root;
    factory_b.root.nominal += 100; seal(&factory_b.root);
    Root next = payload();
    uintptr_t fresh = jet_rt_native_shared_source_new(&next,&factory_b.root);
    assert(jet_rt_shared_site_publish(&world_site,fresh,1) == fresh);
    assert(jet_rt_source_shared_borrow_begin(fresh,&factory_b.root,1,&view) == JET_SOURCE_ALIAS);
    jet_rt_source_shared_borrow_end(view.token);
    jet_rt_global_reset_registered();
    /* Explicit final process teardown, never a Process lease reset. */
    jet_rt_native_shared_drop(atomic_exchange(&process_site,0));
}
int main(void) {
    jet_rt_thread_attach();
    jet_rt_shared_init(); jet_rt_source_shared_init();
    /* Prime the pinned registry node, which intentionally survives leases. */
    Graph g; graph(&g); Root prime = payload();
    uintptr_t root = jet_rt_native_shared_source_new(&prime,&g.root);
    assert(jet_rt_shared_site_publish(&world_site,root,1) == root);
    jet_rt_global_reset_registered();
    int64_t baseline = jet_rt_source_fixture_live();
    scalar_witnesses(); nested_witnesses(); projection_witnesses();
    assert(jet_rt_source_fixture_live() == baseline);
    jet_rt_thread_detach();
    puts("shared source: hash, kinds, nested ownership, alias, Process refusal, World boundary, reentry: ok");
    return 0;
}
