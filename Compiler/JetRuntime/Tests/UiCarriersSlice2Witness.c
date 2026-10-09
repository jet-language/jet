/* REAL Jet providers, --wrap=jet_rt_alloc/jet_rt_free. Private UiABI and
 * UiCarriersABI only. Stop modes require c39d89b6b + 9e344d0f0 composed:
 * exit101, empty stdout, exact stderr in UiCarriersSlice2Stops-*.stderr.
 * No C mocks, no Rust allocation. Syntax is not native execution proof. */
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>

typedef int64_t H;
typedef struct { unsigned char *data; uint64_t len, cap; } Text;
extern void jet_rt_alloc_init(void);
extern H jet_rt_alloc(H, H);
extern H __real_jet_rt_alloc(H, H);
extern void __real_jet_rt_free(H, H, H);
extern void jet_rt_free(H, H, H);
extern Text *jet_rt_string_from_static(const void *, H);
extern void jet_rt_string_drop(Text *);
#define DECLARE(T) \
    extern H jet_rt_handle_clone_##T(H); \
    extern void jet_rt_handle_drop_##T(H); \
    extern bool jet_rt_handle_eq_##T(H, H)
DECLARE(JetUiNode); DECLARE(JetUiAccessibility); DECLARE(JetUiShortcut);
DECLARE(JetTuiColor); DECLARE(JetTuiConstraint); DECLARE(JetTuiListState);
DECLARE(JetTuiStyle); DECLARE(JetTuiCapabilities); DECLARE(JetUiFsRights);
DECLARE(JetUiFsGrant);
extern void jet_rt_handle_field_JetUiNode_label(H, Text *);
extern H jet_rt_handle_field_JetUiNode_width(H);
extern H jet_rt_handle_field_JetUiNode_height(H);
extern void jet_rt_handle_field_JetUiShortcut_key(H, Text *);
extern H jet_rt_handle_field_JetTuiStyle_bold(H);
extern H jet_rt_handle_field_JetTuiStyle_dim(H);
extern H jet_rt_handle_field_JetTuiStyle_underline(H);
extern H jet_rt_handle_field_JetTuiCapabilities_color(H);
extern H jet_rt_handle_field_JetTuiCapabilities_unicode(H);
extern H jet_rt_handle_field_JetTuiCapabilities_mouse(H);
extern H jet_rt_handle_field_JetTuiCapabilities_resize(H);
extern H jet_rt_handle_field_JetTuiCapabilities_clipboard(H);
extern void jet_rt_handle_field_JetUiFsGrant_root(H, Text *);
extern H jet_rt_handle_field_JetUiFsGrant_rights(H);
extern H jet_rt_record_seq_List_Rec_JetUiNode_new(H);
extern H jet_rt_record_seq_List_Rec_JetUiNode_len(H);
extern H jet_rt_record_seq_List_Rec_JetUiNode_get(H, H);
extern void jet_rt_record_seq_List_Rec_JetUiNode_push(H, H);

/* Observer reused from UiCarriersBasicWitness; forwards every real call. */
static struct { H address, bytes, alignment; bool live; } ledger[16384];
static size_t allocations, frees;
static bool tracking;
H __wrap_jet_rt_alloc(H bytes, H alignment) {
    H address = __real_jet_rt_alloc(bytes, alignment);
    if (tracking) {
        assert(allocations < 16384 && address && bytes > 0);
        assert((uint64_t)address % (uint64_t)alignment == 0);
        for (size_t i = 0; i < allocations; ++i)
            assert(!ledger[i].live || ledger[i].address != address);
        ledger[allocations].address = address;
        ledger[allocations].bytes = bytes;
        ledger[allocations].alignment = alignment;
        ledger[allocations++].live = true;
    }
    return address;
}
void __wrap_jet_rt_free(H address, H bytes, H alignment) {
    if (tracking) {
        for (size_t i = allocations; i > 0; --i) {
            if (ledger[i-1].address == address && ledger[i-1].live) {
                assert(ledger[i-1].bytes == bytes && ledger[i-1].alignment == alignment);
                ledger[i-1].live = false;
                ++frees;
                __real_jet_rt_free(address, bytes, alignment);
                return;
            }
        }
        assert(!"untracked or double free");
    }
    __real_jet_rt_free(address, bytes, alignment);
}
static uint64_t *words(H h) { return (uint64_t *)(uintptr_t)h; }
static H boxed(size_t n) {
    H h = jet_rt_alloc((H)n, 8);
    memset((void *)(uintptr_t)h, 0, n);
    return h;
}
static H text(const char *s) { return (H)(uintptr_t)jet_rt_string_from_static(s, (H)strlen(s)); }
static void header_is(Text *view, H owner) {
    Text *source = (void *)(uintptr_t)owner;
    assert(view->data == source->data && view->len == source->len && view->cap == source->cap);
}
static uint64_t bits(double f) { uint64_t b; memcpy(&b, &f, 8); return b; }
static H leaf(const char *s) {
    H h = boxed(160);
    words(h)[0] = (uint64_t)text(s);
    words(h)[3] = UINT64_MAX; /* role None */
    words(h)[5] = (uint64_t)jet_rt_record_seq_List_Rec_JetUiNode_new(0);
    words(h)[13] = 26; /* IME/click/drop None */
    return h;
}
static void node_cases(void) {
    /* Every independent option presence combination, including Some(0). */
    for (unsigned mask = 0; mask < 512; ++mask) {
        H a = leaf("label");
        if (mask & 1) words(a)[3] = 0;
        if (mask & 2) { H meta = boxed(40); words(a)[17] = (uint64_t)meta; }
        if (mask & 4) words(a)[13] &= ~UINT64_C(2);
        if (mask & 8) words(a)[6] = (uint64_t)text("");
        if (mask & 16) words(a)[7] = 1;
        if (mask & 32) words(a)[19] = (uint64_t)text("");
        if (mask & 64) words(a)[13] &= ~UINT64_C(8);
        if (mask & 128) words(a)[13] &= ~UINT64_C(16);
        if (mask & 256) { H shortcut = boxed(16); words(shortcut)[0] = (uint64_t)text(""); words(a)[18] = (uint64_t)shortcut; }
        H b = jet_rt_handle_clone_JetUiNode(a);
        assert(a != b && jet_rt_handle_eq_JetUiNode(a, b));
        for (unsigned slot = 2; slot <= 16; slot *= 2) {
            if (slot == 4) continue;
            words(b)[13] ^= slot;
            assert(!jet_rt_handle_eq_JetUiNode(a, b));
            words(b)[13] ^= slot;
        }
        uint64_t saved = words(b)[19];
        words(b)[19] = saved ? 0 : (uint64_t)text("");
        assert(!jet_rt_handle_eq_JetUiNode(a, b));
        if (!saved) jet_rt_string_drop((Text *)(uintptr_t)words(b)[19]);
        words(b)[19] = saved;
        /* Optional-style absent payload never contributes to equality. */
        if (!(mask & 16)) { words(b)[8] = UINT64_MAX; words(b)[9] = UINT64_MAX; assert(jet_rt_handle_eq_JetUiNode(a,b)); }
        jet_rt_handle_drop_JetUiNode(a); jet_rt_handle_drop_JetUiNode(b);
    }
    H a = leaf("parent"), children = (H)words(a)[5];
    for (unsigned i = 0; i < 11; ++i) jet_rt_record_seq_List_Rec_JetUiNode_push(children, leaf(i & 1 ? "odd" : "even"));
    assert(jet_rt_record_seq_List_Rec_JetUiNode_len(children) == 11 && words(children)[1] >= 11);
    H meta = boxed(40), states = boxed(7*16), shortcut = boxed(16);
    words(meta)[0] = (uint64_t)text("name"); words(meta)[1] = (uint64_t)text("");
    words(meta)[2] = words(meta)[3] = 7; words(meta)[4] = (uint64_t)states;
    for (unsigned i = 0; i < 7; ++i) { words(states)[i*2] = i; words(states)[i*2+1] = UINT64_C(0xdeadbeef00000001); }
    words(states)[13] = (uint64_t)text("value");
    words(shortcut)[0] = (uint64_t)text("key"); words(shortcut)[1] = UINT64_C(0xabcdef001f);
    words(a)[17] = (uint64_t)meta; words(a)[18] = (uint64_t)shortcut;
    words(a)[19] = (uint64_t)text("author-key"); words(a)[6] = (uint64_t)text("#abcdef");
    words(a)[7] = UINT64_C(0xbeef000001); words(a)[8] = 3; words(a)[9] = UINT64_C(0xabcd123456);
    words(a)[10] = 1; words(a)[11] = UINT64_C(0xbeef000055); words(a)[12] = UINT64_MAX;
    words(a)[13] = 0; words(a)[14] = 1; words(a)[15] = UINT64_MAX; words(a)[16] = 0;
    H b = jet_rt_handle_clone_JetUiNode(a), bc = (H)words(b)[5];
    assert(jet_rt_handle_eq_JetUiNode(a,b));
    assert(words(b)[0] != words(a)[0] && words(b)[19] != words(a)[19]);
    assert(words(b)[17] != words(a)[17] && words(b)[18] != words(a)[18]);
    assert(words((H)words(b)[17])[4] != (uint64_t)states);
    assert(words((H)words(b)[18])[0] != words(shortcut)[0]);
    assert(words(b)[7] == 1 && words(b)[9] == 0x123456 && words(b)[11] == 0x55 && words(b)[12] == 7);
    H child = jet_rt_record_seq_List_Rec_JetUiNode_get(bc,0);
    assert(child != jet_rt_record_seq_List_Rec_JetUiNode_get(children,0));
    uint64_t *slots = words((H)words(bc)[2]), saved = slots[0]; slots[0] = slots[1]; slots[1] = saved;
    assert(!jet_rt_handle_eq_JetUiNode(a,b)); saved = slots[0]; slots[0] = slots[1]; slots[1] = saved;
    words(child)[1] = UINT64_C(0x7ff8000000000001);
    assert(!jet_rt_handle_eq_JetUiNode(b,b)); words(child)[1] = 0;
    words(b)[1] = bits(-0.0); assert(jet_rt_handle_eq_JetUiNode(a,b));
    words(b)[2] = UINT64_C(0x7ff8000000000001); assert(!jet_rt_handle_eq_JetUiNode(b,b)); words(b)[2] = 0;
    Text view; jet_rt_handle_field_JetUiNode_label(b,&view); header_is(&view,(H)words(b)[0]);
    assert(jet_rt_handle_field_JetUiNode_width(b) == b+8 && jet_rt_handle_field_JetUiNode_height(b) == b+16);
    jet_rt_handle_field_JetUiShortcut_key((H)words(b)[18],&view); header_is(&view,(H)words((H)words(b)[18])[0]);
    H bs = (H)words((H)words(b)[17])[4];
    for (unsigned i=2;i<=4;++i) { words(bs)[2*i+1] ^= 1; assert(!jet_rt_handle_eq_JetUiNode(a,b)); words(bs)[2*i+1] ^= 1; }
    words(bs)[0] = 1; assert(!jet_rt_handle_eq_JetUiNode(a,b)); words(bs)[0] = 0;
    words(bs)[1] = 0; assert(jet_rt_handle_eq_JetUiNode(a,b));
    words((H)words(b)[18])[1] ^= 1; assert(!jet_rt_handle_eq_JetUiNode(a,b)); words((H)words(b)[18])[1] ^= 1;
    jet_rt_handle_drop_JetUiNode(a);
    assert(view.len == 3 && memcmp(view.data,"key",3) == 0);
    jet_rt_handle_drop_JetUiNode(b);
    jet_rt_handle_drop_JetUiNode(0); jet_rt_handle_drop_JetUiAccessibility(0); jet_rt_handle_drop_JetUiShortcut(0);
}
static void scalar_cases(void) {
    for (unsigned tag=0;tag<3;++tag) {
        H a=boxed(16); words(a)[0]=tag; words(a)[1]=UINT64_C(0xabcdef123456);
        H b=jet_rt_handle_clone_JetTuiColor(a);
        assert(jet_rt_handle_eq_JetTuiColor(a,b));
        assert(words(b)[1] == (tag==2 ? 0x123456 : 0x56));
        words(b)[1]^=1; assert(!jet_rt_handle_eq_JetTuiColor(a,b));
        jet_rt_handle_drop_JetTuiColor(a); jet_rt_handle_drop_JetTuiColor(b);
    }
    const uint64_t floats[]={0,UINT64_C(0x8000000000000000),UINT64_C(0x7ff8000000000001),UINT64_C(0x3ff0000000000000),UINT64_C(0x7ff0000000000000)};
    for (unsigned tag=0;tag<5;++tag) {
        for (size_t i=0;i<5;++i) for (size_t j=0;j<5;++j) {
            H a=boxed(16),b=boxed(16); words(a)[0]=words(b)[0]=tag; words(a)[1]=floats[i]; words(b)[1]=floats[j];
            bool expected;
            if(tag==4) expected=(floats[i]&65535)==(floats[j]&65535);
            else { double x,y; memcpy(&x,&floats[i],8); memcpy(&y,&floats[j],8); expected=x==y; }
            assert(jet_rt_handle_eq_JetTuiConstraint(a,b)==expected);
            H c=jet_rt_handle_clone_JetTuiConstraint(a);
            assert(words(c)[1]==(tag==4 ? floats[i]&65535 : floats[i]));
            words(b)[0]=(tag+1)%5; assert(!jet_rt_handle_eq_JetTuiConstraint(a,b));
            jet_rt_handle_drop_JetTuiConstraint(a); jet_rt_handle_drop_JetTuiConstraint(b); jet_rt_handle_drop_JetTuiConstraint(c);
        }
    }
    H state=boxed(16); words(state)[0]=UINT64_MAX; words(state)[1]=UINT64_C(0x8000000000000000);
    H copy=jet_rt_handle_clone_JetTuiListState(state); assert(jet_rt_handle_eq_JetTuiListState(state,copy));
    words(copy)[1]^=1; assert(!jet_rt_handle_eq_JetTuiListState(state,copy));
    jet_rt_handle_drop_JetTuiListState(state); jet_rt_handle_drop_JetTuiListState(copy);
    for(unsigned opts=0;opts<4;++opts) {
        H a=boxed(72); words(a)[0]=opts&1; words(a)[3]=(opts>>1)&1;
        words(a)[1]=2; words(a)[2]=UINT64_MAX; words(a)[4]=1; words(a)[5]=UINT64_MAX;
        words(a)[6]=UINT64_C(0xaaaa000001); words(a)[7]=UINT64_C(0xaaaa000000); words(a)[8]=1;
        H b=jet_rt_handle_clone_JetTuiStyle(a); assert(jet_rt_handle_eq_JetTuiStyle(a,b));
        if(!(opts&1)) assert(words(b)[1]==0&&words(b)[2]==0);
        if(!(opts&2)) assert(words(b)[4]==0&&words(b)[5]==0);
        H (*fields[])(H)={jet_rt_handle_field_JetTuiStyle_bold,jet_rt_handle_field_JetTuiStyle_dim,jet_rt_handle_field_JetTuiStyle_underline};
        for(unsigned i=0;i<3;++i){ assert(fields[i](b)==b+48+8*i); *(unsigned char *)(uintptr_t)fields[i](b)^=1; assert(!jet_rt_handle_eq_JetTuiStyle(a,b)); *(unsigned char *)(uintptr_t)fields[i](b)^=1; }
        jet_rt_handle_drop_JetTuiStyle(a); jet_rt_handle_drop_JetTuiStyle(b);
    }
    H caps=boxed(64); words(caps)[0]=2;
    for(unsigned i=1;i<6;++i) words(caps)[i]=UINT64_C(0xbeef000000)|(i&1);
    words(caps)[6]=UINT64_MAX; words(caps)[7]=UINT64_C(0x8000000000000000);
    copy=jet_rt_handle_clone_JetTuiCapabilities(caps); assert(jet_rt_handle_eq_JetTuiCapabilities(caps,copy));
    H (*fields[])(H)={jet_rt_handle_field_JetTuiCapabilities_color,jet_rt_handle_field_JetTuiCapabilities_unicode,jet_rt_handle_field_JetTuiCapabilities_mouse,jet_rt_handle_field_JetTuiCapabilities_resize,jet_rt_handle_field_JetTuiCapabilities_clipboard};
    for(unsigned i=0;i<5;++i) { assert(fields[i](copy)==copy+8+8*i); *(unsigned char *)(uintptr_t)fields[i](copy)^=1; assert(!jet_rt_handle_eq_JetTuiCapabilities(caps,copy)); *(unsigned char *)(uintptr_t)fields[i](copy)^=1; }
    words(copy)[6]^=1; assert(!jet_rt_handle_eq_JetTuiCapabilities(caps,copy)); words(copy)[6]^=1;
    jet_rt_handle_drop_JetTuiCapabilities(caps); jet_rt_handle_drop_JetTuiCapabilities(copy);
    H rights=boxed(8); words(rights)[0]=UINT64_C(0xabcdefff);
    copy=jet_rt_handle_clone_JetUiFsRights(rights); assert(words(copy)[0]==255&&jet_rt_handle_eq_JetUiFsRights(rights,copy));
    words(copy)[0]^=128; assert(!jet_rt_handle_eq_JetUiFsRights(rights,copy));
    jet_rt_handle_drop_JetUiFsRights(rights); jet_rt_handle_drop_JetUiFsRights(copy);
    H grant=boxed(16); words(grant)[0]=(uint64_t)text("root"); words(grant)[1]=UINT64_C(0xabcdef00fe);
    copy=jet_rt_handle_clone_JetUiFsGrant(grant); assert(jet_rt_handle_eq_JetUiFsGrant(grant,copy));
    assert(words(copy)[0]!=words(grant)[0] && jet_rt_handle_field_JetUiFsGrant_rights(copy)==copy+8);
    Text view; jet_rt_handle_field_JetUiFsGrant_root(copy,&view); header_is(&view,(H)words(copy)[0]);
    jet_rt_handle_drop_JetUiFsGrant(grant); assert(view.len==4&&memcmp(view.data,"root",4)==0);
    jet_rt_handle_drop_JetUiFsGrant(copy);
    jet_rt_handle_drop_JetTuiColor(0); jet_rt_handle_drop_JetTuiConstraint(0); jet_rt_handle_drop_JetTuiListState(0);
    jet_rt_handle_drop_JetTuiStyle(0); jet_rt_handle_drop_JetTuiCapabilities(0); jet_rt_handle_drop_JetUiFsRights(0); jet_rt_handle_drop_JetUiFsGrant(0);
}
int main(int argc,char **argv) {
    jet_rt_alloc_init();
    if(argc==2) {
        if(!strcmp(argv[1],"negative-capacity")) jet_rt_record_seq_List_Rec_JetUiNode_new(-1);
        else if(!strcmp(argv[1],"oversize-capacity")) jet_rt_record_seq_List_Rec_JetUiNode_new(INT64_MAX);
        else if(!strcmp(argv[1],"index-zero")) jet_rt_record_seq_List_Rec_JetUiNode_get(jet_rt_record_seq_List_Rec_JetUiNode_new(0),0);
        else if(!strcmp(argv[1],"index-negative")) jet_rt_record_seq_List_Rec_JetUiNode_get(jet_rt_record_seq_List_Rec_JetUiNode_new(0),-1);
        else return 2;
        return 99;
    }
    tracking=true; node_cases(); scalar_cases();
    assert(allocations==frees);
    for(size_t i=0;i<allocations;++i) assert(!ledger[i].live);
    puts("UI slice2: option presence, raw callbacks, deep ordered ownership, borrowed fields, bit masks, IEEE constraints and exact-once frees ok");
    return 0;
}
